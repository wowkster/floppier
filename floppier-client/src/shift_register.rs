use embedded_hal::digital::OutputPin;
use rp_pico::hal::gpio::{
    bank0::{Gpio2, Gpio3, Gpio4, Gpio5},
    FunctionSio, Pin, PullDown, SioOutput,
};

use crate::macros::delay_cycles;

type SerialInputPin = Pin<Gpio2, FunctionSio<SioOutput>, PullDown>;
type SerialClockPin = Pin<Gpio3, FunctionSio<SioOutput>, PullDown>;
type StorageClockPin = Pin<Gpio4, FunctionSio<SioOutput>, PullDown>;
type OutputEnablePin = Pin<Gpio5, FunctionSio<SioOutput>, PullDown>;

/// https://www.ti.com/lit/ds/symlink/sn74hc595.pdf
pub struct SN74HC595 {
    serial_input: SerialInputPin,
    serial_clock: SerialClockPin,
    storage_clock: StorageClockPin,
    output_enable: OutputEnablePin,
}

impl SN74HC595 {
    pub fn new(
        mut serial_input: SerialInputPin,
        mut serial_clock: SerialClockPin,
        mut storage_clock: StorageClockPin,
        mut output_enable: OutputEnablePin,
    ) -> Self {
        serial_input.set_low().unwrap();
        serial_clock.set_low().unwrap();
        storage_clock.set_low().unwrap();
        output_enable.set_high().unwrap();

        Self {
            serial_input,
            serial_clock,
            storage_clock,
            output_enable,
        }
    }

    #[inline]
    fn pulse_serial_clock(&mut self) {
        self.serial_clock.set_high().unwrap();
        delay_cycles!(1);
        self.serial_clock.set_low().unwrap();
        delay_cycles!(1);
    }

    #[inline]
    pub fn pulse_storage_clock(&mut self) {
        self.storage_clock.set_high().unwrap();
        delay_cycles!(2);
        self.storage_clock.set_low().unwrap();
        delay_cycles!(2);
    }

    #[inline]
    pub fn set_output_enabled(&mut self, enabled: bool) {
        // Output is active low
        self.output_enable.set_state((!enabled).into()).unwrap();
    }

    #[inline]
    pub fn write_byte(&mut self, mut byte: u8) {
        for _ in 0..8 {
            let bit = (byte >> 7) == 1;
            byte <<= 1;

            self.serial_input.set_state(bit.into()).unwrap();
            delay_cycles!(2);

            self.pulse_serial_clock();
        }
    }

    // pub fn write_bytes(&mut self, data: &[u8]) {
    //     for byte in data {
    //         self.write_byte(*byte);
    //     }
    // }
}
