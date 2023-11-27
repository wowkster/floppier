use core::fmt::Debug;

use cortex_m::delay::Delay;
use defmt::Format;
use embedded_hal::digital::v2::{OutputPin, PinState};
use rp_pico::hal::gpio::{
    new_pin, DynBankId, DynFunction, DynPinId, DynPullType, DynSioConfig, FunctionSio, Pin,
    SioOutput,
};

use crate::note::Note;

use super::Instrument;

pub type FloppyDrivePin = Pin<DynPinId, FunctionSio<SioOutput>, DynPullType>;

/// Floppy drive specification: http://www.bitsavers.org/pdf/mitsubishi/floppy/MF355/UGD-0489A_MF355B_Specifications_Sep86.pdf
pub struct FloppyDrive {
    step_pin: FloppyDrivePin,
    direction_pin: FloppyDrivePin,
    current_note: Option<Note>,
    current_tick: u32,
    current_position: u8,
    current_state: PinState,
    current_direction: Direction,
}

impl Debug for FloppyDrive {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FloppyDrive")
            .field("step_pin", &self.step_pin.id().num)
            .field("direction_pin", &self.direction_pin.id().num)
            .field("current_note", &self.current_note)
            .field("current_tick", &self.current_tick)
            .field("current_position", &self.current_position)
            .field("current_state", &self.current_state)
            .field("current_direction", &self.current_direction)
            .finish()
    }
}

#[derive(Debug, Format)]
enum Direction {
    Forward,
    Reverse,
}

impl FloppyDrive {
    pub const NUM_TRACKS: u8 = 80;
    pub const MAX_POSITION: u8 = 158;
    pub const MIN_POSITION: u8 = 0;

    pub unsafe fn new(port: u8) -> Self {
        let (step_pin, direction_pin) = Self::floppy_port_to_pins(port);

        let step_pin = Self::create_pin(step_pin);
        let direction_pin = Self::create_pin(direction_pin);

        Self::from_pins(step_pin, direction_pin)
    }

    pub fn from_pins(step_pin: FloppyDrivePin, direction_pin: FloppyDrivePin) -> Self {
        Self {
            step_pin,
            direction_pin,
            current_note: None,
            current_tick: 0,
            current_position: 0,
            current_state: PinState::Low,
            current_direction: Direction::Forward,
        }
    }

    fn toggle_pin(&mut self) {
        if self.current_position >= Self::MAX_POSITION {
            self.current_direction = Direction::Reverse;
            self.direction_pin.set_high().unwrap();
        } else if self.current_position == Self::MIN_POSITION {
            self.current_direction = Direction::Forward;
            self.direction_pin.set_low().unwrap();
        }

        match self.current_direction {
            Direction::Forward => self.current_position += 1,
            Direction::Reverse => self.current_position -= 1,
        }

        self.step_pin.set_state(self.current_state).unwrap();
        self.current_state = !self.current_state;
    }

    pub unsafe fn create_pin(pin: u8) -> FloppyDrivePin {
        let mut pin = new_pin(DynPinId {
            bank: DynBankId::Bank0,
            num: pin,
        });

        pin.set_pull_type(DynPullType::Up);
        pin.try_set_function(DynFunction::Sio(DynSioConfig::Output))
            .ok()
            .unwrap();

        pin.try_into_function().ok().unwrap()
    }

    /// Convert a floppy port number to the pins it uses
    /// ex: port 0 uses pins 0 and 1, port 1 uses pins 2 and 3, etc.
    ///
    /// Returns a tuple of (step_pin, direction_pin)
    fn floppy_port_to_pins(port: u8) -> (u8, u8) {
        (port * 2, port * 2 + 1)
    }
}

impl Instrument for FloppyDrive {
    fn set_note(&mut self, note: Option<Note>) {
        self.current_note = note;
        self.current_tick = 0;
    }

    fn tick(&mut self) {
        let Some(note) = self.current_note else {
            return;
        };

        if !note.is_playable() {
            return;
        }

        self.current_tick += 1;

        if self.current_tick >= note.half_ticks() {
            self.toggle_pin();
            self.current_tick = 0;
        }
    }

    fn reset(&mut self, delay: &mut Delay) {
        self.current_note = None;
        self.direction_pin.set_high().unwrap();

        delay.delay_ms(1);

        for _ in 0..Self::NUM_TRACKS {
            self.step_pin.set_high().unwrap();
            delay.delay_ms(3);
            self.step_pin.set_low().unwrap();
            delay.delay_ms(3);
        }

        self.direction_pin.set_low().unwrap();
        self.current_position = 0;
        self.current_state = PinState::Low;
        self.current_direction = Direction::Forward;
    }
}
