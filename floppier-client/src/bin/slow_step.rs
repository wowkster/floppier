#![no_std]
#![no_main]

use cortex_m::delay::Delay;
use defmt::info;
use defmt_rtt as _;
use embedded_alloc::LlffHeap as Heap;
use embedded_hal::digital::OutputPin;
use panic_probe as _;
use rp_pico::{entry, hal};

use hal::{
    clocks::{init_clocks_and_plls, Clock},
    pac,
    watchdog::Watchdog,
    Sio,
};

use floppier_client::{
    floppy_drive::{Direction, DriveState, FloppyDrive},
    shift_register::SN74HC595,
};

#[global_allocator]
static HEAP: Heap = Heap::empty();

#[entry]
fn main() -> ! {
    info!("Floppier Can Node v{}", env!("CARGO_PKG_VERSION"));

    {
        use core::mem::MaybeUninit;

        const HEAP_SIZE: usize = 1024 * 16;
        static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
        unsafe { HEAP.init(HEAP_MEM.as_ptr() as usize, HEAP_SIZE) }
    }

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

    let mut delay = Delay::new(core.SYST, clocks.system_clock.freq().to_Hz());

    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    let mut led_pin = pins.gpio25.into_push_pull_output();
    led_pin.set_high().unwrap();

    let mut shift_register = SN74HC595::new(
        pins.gpio2.reconfigure(),
        pins.gpio3.reconfigure(),
        pins.gpio4.reconfigure(),
        pins.gpio5.reconfigure(),
    );

    shift_register.set_output_enabled(true);

    let mut state = DriveState {
        drive_select: true,
        step: false,
        direction: Direction::Reverse,
    };

    const NUM_DRIVES: usize = 3;

    #[allow(clippy::empty_loop)]
    loop {
        for _ in 0..FloppyDrive::NUM_TRACKS {
            state.step = true;

            for _ in 0..NUM_DRIVES {
                shift_register.write_byte(state.into());
            }
            shift_register.pulse_storage_clock();

            delay.delay_ms(1);

            state.step = false;

            for _ in 0..NUM_DRIVES {
                shift_register.write_byte(state.into());
            }
            shift_register.pulse_storage_clock();

            delay.delay_ms(250);
        }

        state.direction = match state.direction {
            Direction::Forward => Direction::Reverse,
            Direction::Reverse => Direction::Forward,
        };

        delay.delay_ms(200);
    }
}
