use core::fmt::Debug;
use defmt::Format;

use crate::note::Note;

/// Floppy drive specification: http://www.bitsavers.org/pdf/mitsubishi/floppy/MF355/UGD-0489A_MF355B_Specifications_Sep86.pdf
#[derive(Debug, Format)]
pub struct FloppyDrive {
    current_note: Option<Note>,
    current_note_tick: u32,
    current_state: bool,
    current_period_tick: u32,
    current_position: u8,
    current_direction: Direction,
    current_direction_tick: u32,
    movement: bool,
}

impl FloppyDrive {
    pub const NUM_TRACKS: u8 = 80;
    pub const MAX_POSITION_MOVEMENT: u8 = 156;
    pub const MIN_POSITION_MOVEMENT: u8 = 2;
    pub const MAX_POSITION_STILL: u8 = 81;
    pub const MIN_POSITION_STILL: u8 = 79;

    pub fn new(movement: bool) -> Self {
        Self {
            current_note: None,
            current_note_tick: 0,
            current_period_tick: 0,
            current_position: 0,
            current_state: false,
            current_direction: Direction::Forward,
            current_direction_tick: 0,
            movement,
        }
    }

    pub fn set_note(&mut self, note: Option<Note>) {
        self.current_note = note.filter(|note| note.is_playable());
        self.current_period_tick = 0;
        self.current_note_tick = 0;
        self.current_direction_tick = 0;

        if !self.current_state {
            self.toggle_step();
        }

        assert!(self.current_state);
    }

    pub fn tick(&mut self) -> DriveState {
        let Some(note) = self.current_note else {
            return DriveState {
                drive_select: false,
                step: self.current_state,
                direction: self.current_direction,
            };
        };

        self.current_note_tick += 1;
        self.current_direction_tick += 1;
        let drive_select = self.current_note_tick > 1;

        if drive_select {
            self.current_period_tick += 1;

            if self.current_period_tick >= note.half_ticks() {
                self.toggle_step();
                self.current_period_tick = 0;
            }
        }

        let direction = if self.current_direction_tick > 2 {
            self.current_direction
        } else {
            self.current_direction.inverse()
        };

        DriveState {
            drive_select,
            step: self.current_state,
            direction,
        }
    }

    fn toggle_step(&mut self) {
        let (min_position, max_position) = if self.movement {
            (Self::MIN_POSITION_MOVEMENT, Self::MAX_POSITION_MOVEMENT)
        } else {
            (Self::MIN_POSITION_STILL, Self::MAX_POSITION_STILL)
        };

        if self.current_position >= max_position {
            self.current_direction = Direction::Reverse;
            self.current_direction_tick = 0;
        } else if self.current_position == min_position {
            self.current_direction = Direction::Forward;
            self.current_direction_tick = 0;
        }

        match self.current_direction {
            Direction::Forward => self.current_position += 1,
            Direction::Reverse => self.current_position -= 1,
        }

        self.current_state = !self.current_state;
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Format)]
pub enum Direction {
    #[default]
    Forward,
    Reverse,
}

impl Direction {
    pub const fn inverse(self) -> Self {
        match self {
            Direction::Forward => Self::Reverse,
            Direction::Reverse => Self::Forward,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, defmt::Format)]
pub struct DriveState {
    pub drive_select: bool,
    pub step: bool,
    pub direction: Direction,
}

impl From<DriveState> for u8 {
    fn from(value: DriveState) -> Self {
        let mut byte = 0;

        if !value.drive_select {
            byte |= 0x1;
        }

        if !value.step {
            byte |= 0x2;
        }

        if value.direction == Direction::Reverse {
            byte |= 0x4;
        }

        byte
    }
}
