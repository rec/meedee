//! Shared Rust MIDI toolkit. No Python interpreter or PyO3 dependency.

pub mod ci;
pub mod ci_browser;
pub mod ci_session;
pub mod codecs;
pub mod controls;
pub mod gm;
pub mod messages;
pub mod mpe;
pub mod notes;
pub mod routing;
pub mod testing;
pub mod timecode;
pub mod topology;
pub mod transfer;
pub mod transport;
pub mod tuning;
pub mod ump;
pub mod universal;
pub mod velocity;

pub mod archive;
pub mod clip;
pub mod comparison;
pub mod event_views;
pub mod events;
pub mod file_io;
pub mod file_timing;
pub mod inspection;
pub mod inspection_timing;
pub mod live;
pub mod playback;
pub mod recording;
pub mod smf;
pub mod syx;
