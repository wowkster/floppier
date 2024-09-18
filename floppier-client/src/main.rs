#![no_std]
#![no_main]

extern crate alloc;

use core::cell::{Cell, RefCell};

use alloc::{collections::BTreeMap, string::ToString};
use critical_section::Mutex;
use defmt_rtt as _;
use embedded_hal::delay::DelayNs;
use floppier_proto::{
    FloppierC2SMessage, FloppierS2CMessage, LimitedMidiMessage, MidiEvent, SetConfig,
};

use embedded_alloc::LlffHeap as Heap;
use heapless::Vec;
use panic_probe as _;
use rp_pico::{
    entry,
    hal::{
        self,
        clocks::UsbClock,
        fugit::{ ExtU32, ExtU64},
        timer::{Alarm, Alarm0},
        Timer,
    },
    pac::{RESETS, USBCTRL_DPRAM, USBCTRL_REGS},
};
use usb_device::{class_prelude::*, prelude::*};
use usbd_serial::SerialPort;

use hal::{
    clocks::init_clocks_and_plls,
    pac::{self, interrupt},
    watchdog::Watchdog,
    Sio,
};

mod io;

use crate::io::{get_received_message, send_message, update_read_buffer};
use floppier_client::{
    floppy_drive::{Direction, DriveState, FloppyDrive},
    note::Note,
    shift_register::SN74HC595, TIMER_RESOLUTION_US,
};

#[global_allocator]
static HEAP: Heap = Heap::empty();

// This can be static mut because it gets set once and only ever gets cloned
static mut TIMER: Option<Timer> = None;

// These can be static mut because they're set once and only ever accessed in
// the usb interrupt
static mut USB_DEVICE: Option<UsbDevice<hal::usb::UsbBus>> = None;
static mut USB_BUS: Option<UsbBusAllocator<hal::usb::UsbBus>> = None;
static mut USB_SERIAL: Option<SerialPort<hal::usb::UsbBus>> = None;

// These can be static mut because they're set once and only ever accessed in
// the timer interrupt
static mut ALARM0: Option<Alarm0> = None;
static mut SHIFT_REGISTER: Option<SN74HC595> = None;

/* State */

static CLIENT_STATE: Mutex<Cell<ClientState>> = Mutex::new(Cell::new(ClientState::WaitingForHello));

const MAX_DRIVE_COUNT: usize = 8;

type TrackMap = BTreeMap<u16, ChannelMap>;
type ChannelMap = BTreeMap<u8, Vec<usize, MAX_DRIVE_COUNT>>;

static TRACK_MAP: Mutex<RefCell<Option<TrackMap>>> = Mutex::new(RefCell::new(None));

type FloppyDriveStack = Vec<FloppyDrive, MAX_DRIVE_COUNT>;

static FLOPPY_DRIVES: Mutex<RefCell<FloppyDriveStack>> = Mutex::new(RefCell::new(Vec::new()));

#[derive(Debug, Clone, Copy, defmt::Format, PartialEq)]
enum ClientState {
    WaitingForHello,
    WaitingForSetConfig,
    PlayingMidiStream,
}

#[entry]
fn main() -> ! {
    defmt::info!("Floppier Client v{}", env!("CARGO_PKG_VERSION"));

    init_heap();

    let mut pac = pac::Peripherals::take().unwrap();
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

    /* Set up the timer */

    let mut timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);
    unsafe {
        TIMER = Some(timer);
    }

    /* Set up the USB device */

    init_usb_device(
        &mut pac.RESETS,
        pac.USBCTRL_REGS,
        pac.USBCTRL_DPRAM,
        clocks.usb_clock,
    );
    unsafe {
        pac::NVIC::unmask(hal::pac::Interrupt::USBCTRL_IRQ);
    };

    /* Set up the shift register */

    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    let mut shift_register = SN74HC595::new(
        pins.gpio2.reconfigure(),
        pins.gpio3.reconfigure(),
        pins.gpio4.reconfigure(),
        pins.gpio5.reconfigure(),
    );

    shift_register.set_output_enabled(true);

    unsafe {
        SHIFT_REGISTER = Some(shift_register);
    }

    /* Set up the tick alarm */

    let mut alarm0 = timer.alarm_0().unwrap();

    alarm0.schedule(0u32.micros()).unwrap();
    alarm0.enable_interrupt();

    unsafe {
        ALARM0 = Some(alarm0);
    };

    /* Do nothing on the main thread */

    loop {
        cortex_m::asm::wfi();
    }
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
        .device_class(2) // from: https://www.usb.org/defined-class-codes
        .strings(&[StringDescriptors::new(LangID::EN_US)
            .manufacturer("Adrian Wowk")
            .product("Floppier Client")
            .serial_number("FLOP")])
        .unwrap()
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
    let Some(message) = get_received_message() else {
        return;
    };

    critical_section::with(|cs| {
        match message {
            FloppierS2CMessage::Hello => {

                if !is_state(ClientState::WaitingForHello) {
                    defmt::warn!("Resetting state due to new hello packet!");

                    pac::NVIC::mask(hal::pac::Interrupt::TIMER_IRQ_0);

                    let mut floppy_drives = FLOPPY_DRIVES.borrow(cs).borrow_mut();
    
                    for drive in floppy_drives.iter_mut() {
                        drive.set_note(None);
                    }
    
                }
               
                defmt::info!("Connected to server!");

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

                /* Set configuration */

                set_config(config);

                defmt::info!("Configured successfully!");

                let _ = send_message(serial, FloppierC2SMessage::SetConfigAck);

                /* Reset drives */

                defmt::info!("Resetting drives...");

                reset_drives();

                /* Transition to ready  */

                defmt::info!("Drives reset!");

                set_state(ClientState::PlayingMidiStream);
                let _ = send_message(serial, FloppierC2SMessage::Ready);

                pac::NVIC::unmask(hal::pac::Interrupt::TIMER_IRQ_0);
                
                defmt::info!("Started timer interrupt!")
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

                let track_map = TRACK_MAP.borrow(cs).borrow();
                let track_map = track_map.as_ref().unwrap();
                let mut floppy_drives = FLOPPY_DRIVES.borrow(cs).borrow_mut();

                if let Some(drives) = track_map.get(&track).and_then(|track| track.get(&channel)) {
                    match message {
                        LimitedMidiMessage::NoteOn { note, velocity } => {
                            for i in drives {
                                if velocity > 0 {
                                    floppy_drives[*i].set_note(Some(Note::try_from(note).unwrap()));
                                } else {
                                    floppy_drives[*i].set_note(None);
                                }
                            }
                        }
                        LimitedMidiMessage::NoteOff { .. } => {
                            for i in drives {
                                floppy_drives[*i].set_note(None)
                            }
                        }
                        LimitedMidiMessage::ProgramChange { .. } => todo!(),
                        LimitedMidiMessage::ControlChange { .. } => todo!(),
                        LimitedMidiMessage::PitchBend { .. } => todo!(),
                    }
                } else {
                    defmt::warn!(
                        "No drives found for track {} and channel {}",
                        track, channel
                    );
                }

                let _ = send_message(serial, FloppierC2SMessage::MidiEventAck);
            }
            FloppierS2CMessage::End => {
                if !is_state(ClientState::PlayingMidiStream) {
                    let _ = send_message(
                        serial,
                        FloppierC2SMessage::Error("Unexpected end packet!".to_string()),
                    );
                    panic!("Unexpected end packet!");
                }

                pac::NVIC::mask(hal::pac::Interrupt::TIMER_IRQ_0);

                let mut floppy_drives = FLOPPY_DRIVES.borrow(cs).borrow_mut();

                for drive in floppy_drives.iter_mut() {
                    drive.set_note(None);
                }

                let _ = send_message(serial, FloppierC2SMessage::EndAck);
                set_state(ClientState::WaitingForHello);
            }
        }
    });
}

fn is_state(state: ClientState) -> bool {
    critical_section::with(|cs| CLIENT_STATE.borrow(cs).get() == state)
}

fn set_state(state: ClientState) {
    critical_section::with(|cs| CLIENT_STATE.borrow(cs).set(state))
}

fn set_config(config: SetConfig) {
    let track_map = config
        .tracks
        .into_iter()
        .map(|(track_number, track)| {
            let channels = track
                .into_iter()
                .map(|(channel_number, drives)| {
                    (
                        channel_number,
                        Vec::from_iter(drives.into_iter().map(|drive_index| {
                            assert!(
                                drive_index < config.drive_count,
                                "Supplied drive index exceeded drive count!"
                            );

                            drive_index as usize
                        })),
                    )
                })
                .collect::<ChannelMap>();

            (track_number, channels)
        })
        .collect::<TrackMap>();

    let floppy_drives: FloppyDriveStack =
        Vec::from_iter((0..config.drive_count).map(|_| FloppyDrive::new(config.movement)));

    critical_section::with(|cs| {
        TRACK_MAP.borrow(cs).replace(Some(track_map));
        *FLOPPY_DRIVES.borrow(cs).borrow_mut() = floppy_drives;
    });
}

fn reset_drives() {
    critical_section::with(|cs| {
        let floppy_drives = FLOPPY_DRIVES.borrow(cs).borrow();
        let shift_register = unsafe { SHIFT_REGISTER.as_mut().unwrap() };
        let mut timer = unsafe { TIMER }.unwrap();

        let mut state = DriveState {
            drive_select: true,
            step: false,
            direction: Direction::Reverse,
        };

        for _ in 0..3 {
            for _ in 0..FloppyDrive::NUM_TRACKS {
                state.step = true;

                for _ in 0..floppy_drives.len() {
                    shift_register.write_byte(state.into());
                }
                shift_register.pulse_storage_clock();

                timer.delay_ms(3);

                state.step = false;

                for _ in 0..floppy_drives.len() {
                    shift_register.write_byte(state.into());
                }
                shift_register.pulse_storage_clock();

                timer.delay_ms(3);
            }

            state.direction = match state.direction {
                Direction::Forward => Direction::Reverse,
                Direction::Reverse => Direction::Forward,
            };

            timer.delay_ms(200);
        }
    })
}

#[interrupt]
fn TIMER_IRQ_0() {
    let alarm = unsafe { ALARM0.as_mut().unwrap() };
    let timer = unsafe { TIMER }.unwrap();
    let shift_register = unsafe { SHIFT_REGISTER.as_mut().unwrap() };

    let start_time = timer.get_counter();
    
    critical_section::with(|cs| {
        /* Tick all the drives and write their values to the shift registers */

        let mut floppy_drives = FLOPPY_DRIVES.borrow(cs).borrow_mut();

        for drive in floppy_drives.iter_mut() {
            shift_register.write_byte(drive.tick().into())
        }
     
        shift_register.pulse_storage_clock();

        /* Schedule the next alarm */

        let end_time = timer.get_counter();

        let elapsed_time = end_time - start_time;

        let time_to_next = TIMER_RESOLUTION_US
            .micros()
            .checked_sub(elapsed_time)
            .unwrap_or(0u64.micros());

        if time_to_next.is_zero() {
            let overrun_us = elapsed_time
                .checked_sub(TIMER_RESOLUTION_US.micros::<1, 1_000_000>())
                .unwrap()
                .to_micros();
            defmt::error!(
                "TIMER_IRQ_0 overran alotted time (TIMER_RESOLUTION_US) by {}µs! (total elapsed = {}µs)",
                overrun_us, 
                elapsed_time.to_micros(),
            );
        }

        alarm.clear_interrupt();
        alarm.schedule(time_to_next.try_into().unwrap()).unwrap();
        alarm.enable_interrupt();
    });
}
