use embedded_hal::digital::OutputPin;
use pio::ProgramWithDefines;
use rp_pico::{
    hal::{
        gpio::{
            bank0::{Gpio2, Gpio3, Gpio4, Gpio5},
            FunctionPio0, FunctionSio, Pin, PullDown, SioOutput,
        },
        pio::{PIOBuilder, PinDir, Tx, UninitStateMachine, PIO, SM0},
    },
    pac::PIO0,
};

type SerialInputPin = Pin<Gpio2, FunctionPio0, PullDown>;
type SerialClockPin = Pin<Gpio3, FunctionPio0, PullDown>;
type StorageClockPin = Pin<Gpio4, FunctionPio0, PullDown>;
type PIOPins = (SerialInputPin, SerialClockPin, StorageClockPin);

type Pio = PIO<PIO0>;
type PioUninitStateMachine = UninitStateMachine<(PIO0, SM0)>;
type PioTx = Tx<(PIO0, SM0)>;

type OutputEnablePin = Pin<Gpio5, FunctionSio<SioOutput>, PullDown>;

/// https://www.ti.com/lit/ds/symlink/sn74hc595.pdf
pub struct SN74HC595 {
    output_enable: OutputEnablePin,
    tx: PioTx,
}

impl SN74HC595 {
    pub fn new(
        mut pio: Pio,
        uninit_sm: PioUninitStateMachine,
        (serial_input, serial_clock, storage_clock): PIOPins,
        mut output_enable: OutputEnablePin,
    ) -> Self {
        output_enable.set_high().unwrap();

        let (serial_input_id, serial_clock_id, storage_clock_id) = (
            serial_input.id().num,
            serial_clock.id().num,
            storage_clock.id().num,
        );

        let ProgramWithDefines { program, .. } = pio_proc::pio_file!("src/sn74hc595.pio");

        let installed = pio.install(&program).unwrap();
        let (mut sm, _, tx) = PIOBuilder::from_installed_program(installed)
            .out_pins(serial_input_id, 1)
            .set_pins(serial_clock_id, 2)
            .clock_divisor_fixed_point(1, 0)
            .autopull(true)
            .build(uninit_sm);

        sm.set_pindirs([
            (serial_input_id, PinDir::Output),
            (serial_clock_id, PinDir::Output),
            (storage_clock_id, PinDir::Output),
        ]);
        sm.start();

        Self { output_enable, tx }
    }

    #[inline]
    pub fn set_output_enabled(&mut self, enabled: bool) {
        // Output is active low
        self.output_enable.set_state((!enabled).into()).unwrap();
    }

    pub fn write_byte_to_all(&mut self, data: u8) {
        self.tx.write_u8_replicated(data.reverse_bits());
        self.tx.write_u8_replicated(data.reverse_bits());
    }

    pub fn write_bytes(&mut self, data: &[u8; 8]) {
        self.tx.write(u32::from_le_bytes([
            data[0].reverse_bits(),
            data[1].reverse_bits(),
            data[2].reverse_bits(),
            data[3].reverse_bits(),
        ]));
        self.tx.write(u32::from_le_bytes([
            data[4].reverse_bits(),
            data[5].reverse_bits(),
            data[6].reverse_bits(),
            data[7].reverse_bits(),
        ]));
    }
}
