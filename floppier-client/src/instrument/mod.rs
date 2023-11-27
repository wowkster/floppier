use cortex_m::delay::Delay;

use crate::note::Note;

pub mod floppy_drive;

/// Represents a musical instrument controlled by the floppier hardware
pub trait Instrument {
    /// Sets the note to be played by the instrument
    fn set_note(&mut self, note: Option<Note>);

    /// Called on every tick of the sequencer (controlled by timer resolution)
    fn tick(&mut self);

    /// Resets the instrument to its initial state.
    /// Called when the sequencer is reset.
    fn reset(&mut self, delay: &mut Delay);
}
