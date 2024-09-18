#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum FloppierS2CMessage {
    Hello,
    SetConfig(SetConfig),
    MidiEvent(MidiEvent),
    End,
}

#[derive(Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum FloppierC2SMessage {
    HelloAck,
    SetConfigAck,
    Ready,
    MidiEventAck,
    EndAck,
    Error(#[cfg_attr(feature = "defmt", defmt(Debug2Format))] String),
}

#[derive(Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct SetConfig {
    /// Strategy to use to resolve parallel notes
    pub parallel_mode: ParallelMode,

    /// Whether or not to move the drive heads while playing
    pub movement: bool,

    /// The number of drives in the stack (used for bit timing)
    pub drive_count: u8,

    /// Map of track numbers to tracks which map channel numbers to ports
    #[cfg_attr(feature = "defmt", defmt(Debug2Format))]
    pub tracks: BTreeMap<u16, BTreeMap<u8, Vec<u8>>>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[serde(rename_all = "lowercase")]
pub enum ParallelMode {
    /// Use only the first note in the chord
    #[default]
    Collapse,

    /// Synthesize a chord by combining the notes and sampling the composed sinusoid
    Synthesize,

    /// Distribute the notes across the available drives
    Distribute,
}

/// An event sent to the client with midi data
#[derive(Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct MidiEvent {
    pub track: u16,
    pub channel: u8,
    pub message: LimitedMidiMessage,
}

/// A limited set of MIDI messages that can be sent to the client
#[derive(Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum LimitedMidiMessage {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8, velocity: u8 },
    ProgramChange { program: u8 },
    ControlChange { control: u8, value: u8 },
    PitchBend { value: i16 },
}
