#![no_std]

pub mod floppy_drive;
pub mod macros;
pub mod note;
pub mod shift_register;

pub const TIMER_RESOLUTION_US: u64 = 30;
pub const NOTE_DURATION_US: u32 = 250_000;
