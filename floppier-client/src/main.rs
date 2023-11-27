#![no_std]
#![no_main]

extern crate alloc;

use alloc::{collections::BTreeMap, string::ToString, vec::Vec};
use cortex_m::delay::Delay;
use defmt::{info, todo, warn, Format};
use defmt_rtt as _;
use floppier_proto::{
    FloppierC2SMessage, FloppierS2CMessage, LimitedMidiMessage, MidiEvent, SetConfig,
};

use io::{receive_message, send_message, update_read_buffer};
use panic_probe as _;

use embedded_alloc::Heap;
use embedded_hal::digital::v2::OutputPin;
use rp_pico::{
    entry,
    hal::{
        self,
        clocks::UsbClock,
        fugit::ExtU32,
        timer::{Alarm, Alarm0},
        Timer,
    },
    pac::{RESETS, USBCTRL_DPRAM, USBCTRL_REGS},
};
use usb_device::{class_prelude::*, prelude::*};
use usbd_serial::SerialPort;

use hal::{
    clocks::{init_clocks_and_plls, Clock},
    pac::{self, interrupt},
    watchdog::Watchdog,
    Sio,
};

use crate::{
    instrument::{FloppyDrive, Instrument},
    note::Note,
};

mod instrument;
mod io;
mod note;

pub const TIMER_RESOLUTION_US: u32 = 20;
pub const NOTE_DURATION_US: u32 = 250_000;

#[global_allocator]
static HEAP: Heap = Heap::empty();

static mut ALARM0: Option<Alarm0> = None;
static mut TIMER: Option<Timer> = None;
static mut DELAY: Option<Delay> = None;

static mut USB_DEVICE: Option<UsbDevice<hal::usb::UsbBus>> = None;
static mut USB_BUS: Option<UsbBusAllocator<hal::usb::UsbBus>> = None;
static mut USB_SERIAL: Option<SerialPort<hal::usb::UsbBus>> = None;

static mut CLIENT_STATE: ClientState = ClientState::WaitingForHello;
static mut FLOPPY_DRIVES: Option<BTreeMap<u16, BTreeMap<u8, Vec<FloppyDrive>>>> = None;

#[derive(Debug, Format, PartialEq)]
enum ClientState {
    WaitingForHello,
    WaitingForSetConfig,
    WaitingForReset,
    PlayingMidiStream,
}

#[entry]
fn main() -> ! {
    info!("Floppier Client v{}", env!("CARGO_PKG_VERSION"));

    init_heap();

    let mut pac = pac::Peripherals::take().unwrap();
    let core = pac::CorePeripherals::take().unwrap();
    let mut watchdog = Watchdog::new(pac.WATCHDOG);
    let sio = Sio::new(pac.SIO);

    let clocks = init_clocks_and_plls(
        rp_pico::XOSC_CRYSTAL_FREQ,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();

    // Set up the timer
    let mut timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);
    let alarm0 = timer.alarm_0().unwrap();

    unsafe {
        TIMER = Some(timer);
        ALARM0 = Some(alarm0);
    }

    // Set up the Floppy Drives
    unsafe {
        FLOPPY_DRIVES = Some(BTreeMap::new());
    }

    // Set up the delay
    unsafe {
        DELAY = Some(Delay::new(core.SYST, clocks.system_clock.freq().to_Hz()));
    }

    // Set up the USB device
    init_usb_device(
        &mut pac.RESETS,
        pac.USBCTRL_REGS,
        pac.USBCTRL_DPRAM,
        clocks.usb_clock,
    );

    // Enable the USB interrupt
    unsafe {
        pac::NVIC::unmask(hal::pac::Interrupt::USBCTRL_IRQ);
    };

    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    let mut led_pin = pins.gpio25.into_push_pull_output();
    led_pin.set_high().unwrap();

    unsafe {
        let alarm0 = ALARM0.as_mut().unwrap();

        alarm0.schedule(TIMER_RESOLUTION_US.micros()).unwrap();
        alarm0.enable_interrupt();

        pac::NVIC::unmask(hal::pac::Interrupt::TIMER_IRQ_0);
    };

    #[allow(clippy::empty_loop)]
    loop {}
}

fn init_heap() {
    use core::mem::MaybeUninit;
    const HEAP_SIZE: usize = 1024 * 16;
    static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
    unsafe { HEAP.init(HEAP_MEM.as_ptr() as usize, HEAP_SIZE) }
}

fn init_usb_device(
    resets: &mut RESETS,
    usbctrl_regs: USBCTRL_REGS,
    usbctrl_dpram: USBCTRL_DPRAM,
    usb_clock: UsbClock,
) {
    // Set up the USB driver
    let usb_bus = UsbBusAllocator::new(hal::usb::UsbBus::new(
        usbctrl_regs,
        usbctrl_dpram,
        usb_clock,
        true,
        resets,
    ));
    unsafe {
        // Note (safety): This is safe as interrupts haven't been started yet
        USB_BUS = Some(usb_bus);
    }

    // Grab a reference to the USB Bus allocator. We are promising to the
    // compiler not to take mutable access to this global variable whilst this
    // reference exists!
    let bus_ref = unsafe { USB_BUS.as_ref().unwrap() };

    // Set up the USB Communications Class Device driver
    let serial = SerialPort::new(bus_ref);

    unsafe {
        USB_SERIAL = Some(serial);
    }

    // Create a USB device with a fake VID and PID
    let usb_dev = UsbDeviceBuilder::new(bus_ref, UsbVidPid(0x16c0, 0x27dd))
        .manufacturer("Adrian Wowk")
        .product("Floppier Client")
        .serial_number("FLOP")
        .device_class(2) // from: https://www.usb.org/defined-class-codes
        .build();

    unsafe {
        // Note (safety): This is safe as interrupts haven't been started yet
        USB_DEVICE = Some(usb_dev);
    }
}

/// This function is called whenever the USB Hardware generates an Interrupt
/// Request.
///
/// We do all our USB work under interrupt, so the main thread can continue on
/// knowing nothing about USB.
#[allow(non_snake_case)]
#[interrupt]
unsafe fn USBCTRL_IRQ() {
    // Grab the global objects. This is OK as we only access them under interrupt.
    let usb_dev = USB_DEVICE.as_mut().unwrap();
    let serial = USB_SERIAL.as_mut().unwrap();

    // Poll the USB driver with all of our supported USB Classes
    if !usb_dev.poll(&mut [serial]) {
        return;
    }

    // If we get here, we have a USB event to handle
    update_read_buffer(serial);

    // Check if we have received a full message
    let Some(message) = receive_message() else {
        return;
    };

    // debug!("Received message: {:?}", message);
    // debug!("State: {:?}", CLIENT_STATE);

    cortex_m::interrupt::free(|_| match message {
        FloppierS2CMessage::Hello => {
            if !is_state(ClientState::WaitingForHello) {
                let _ = send_message(
                    serial,
                    FloppierC2SMessage::Error("Unexpected hello packet!".to_string()),
                );
                panic!("Unexpected hello packet!");
            }

            info!("Connected to server!");

            let _ = send_message(serial, FloppierC2SMessage::HelloAck);
            set_state(ClientState::WaitingForSetConfig);
        }
        FloppierS2CMessage::SetConfig(config) => {
            if !is_state(ClientState::WaitingForSetConfig) {
                let _ = send_message(
                    serial,
                    FloppierC2SMessage::Error("Unexpected set config packet!".to_string()),
                );
                panic!("Unexpected set config packet!");
            }

            set_config(config);

            info!("Configured successfully!");

            let _ = send_message(serial, FloppierC2SMessage::SetConfigAck);
            set_state(ClientState::WaitingForReset);
        }
        FloppierS2CMessage::MidiEvent(event) => {
            if !is_state(ClientState::PlayingMidiStream) {
                let _ = send_message(
                    serial,
                    FloppierC2SMessage::Error("Unexpected midi event packet!".to_string()),
                );
                panic!("Unexpected midi event packet!");
            }

            let MidiEvent {
                track,
                channel,
                message,
            } = event;

            let floppy_drives = unsafe { FLOPPY_DRIVES.as_mut().unwrap() };

            if let Some(drives) = floppy_drives
                .get_mut(&track)
                .and_then(|track| track.get_mut(&channel))
            {
                match message {
                    LimitedMidiMessage::NoteOn { note, velocity } => {
                        drives.iter_mut().for_each(|drive| {
                            if velocity > 0 {
                                drive.set_note(Some(Note::try_from(note).unwrap()));
                            } else {
                                drive.set_note(None);
                            }
                        })
                    }
                    LimitedMidiMessage::NoteOff { .. } => {
                        drives.iter_mut().for_each(|drive| drive.set_note(None))
                    }
                    LimitedMidiMessage::ProgramChange { .. } => todo!(),
                    LimitedMidiMessage::ControlChange { .. } => todo!(),
                    LimitedMidiMessage::PitchBend { .. } => todo!(),
                }
            } else {
                warn!(
                    "No drives found for track {} and channel {}",
                    track, channel
                );
            }

            let _ = send_message(serial, FloppierC2SMessage::MidiEventAck);
        }
    });

    if is_state(ClientState::WaitingForReset) {
        let floppy_drives = FLOPPY_DRIVES.as_mut().unwrap();
        let delay = DELAY.as_mut().unwrap();

        info!("Resetting drives...");

        floppy_drives
            .iter_mut()
            .flat_map(|(_, track)| track.iter_mut())
            .flat_map(|(_, channel)| channel.iter_mut())
            .for_each(|drive| drive.reset(delay));

        info!("Drives reset!");

        let _ = send_message(serial, FloppierC2SMessage::Ready);
        set_state(ClientState::PlayingMidiStream);
    }
}

fn is_state(state: ClientState) -> bool {
    unsafe { CLIENT_STATE == state }
}

fn set_state(state: ClientState) {
    unsafe {
        CLIENT_STATE = state;
    }
}

fn set_config(config: SetConfig) {
    let floppy_drives = unsafe { FLOPPY_DRIVES.as_mut().unwrap() };

    *floppy_drives = config
        .tracks
        .into_iter()
        .map(|(track_number, track)| {
            let channels = track
                .into_iter()
                .map(|(channel_number, ports)| {
                    let drives = ports
                        .into_iter()
                        .map(|port| unsafe { FloppyDrive::new(port) })
                        .collect();

                    (channel_number, drives)
                })
                .collect();

            (track_number, channels)
        })
        .collect();
}

#[interrupt]
fn TIMER_IRQ_0() {
    let alarm = unsafe { ALARM0.as_mut().unwrap() };
    let timer = unsafe { TIMER.as_mut().unwrap() };
    let floppy_drives = unsafe { FLOPPY_DRIVES.as_mut().unwrap() };

    cortex_m::interrupt::free(|_| {
        let start_time = timer.get_counter();

        floppy_drives
            .iter_mut()
            .flat_map(|(_, track)| track.iter_mut())
            .flat_map(|(_, channel)| channel.iter_mut())
            .for_each(|drive| drive.tick());

        let end_time = timer.get_counter();

        let elapsed_time = end_time - start_time;

        alarm.clear_interrupt();
        alarm
            .schedule((TIMER_RESOLUTION_US - elapsed_time.to_micros() as u32).micros())
            .unwrap();
        alarm.enable_interrupt();
    });
}
