//! Shared Rust MIDI toolkit. No Python interpreter or PyO3 dependency.

pub mod codecs;
pub mod controls;
pub mod gm;
pub mod messages;
pub mod mpe;
pub mod timecode;
pub mod transport;
pub mod tuning;
pub mod ump;
pub mod universal;
pub mod velocity;

pub mod clip;
pub mod file_io;
pub mod smf;
pub mod syx;
