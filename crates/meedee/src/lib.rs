//! Shared Rust MIDI toolkit. No Python interpreter or PyO3 dependency.

pub mod ci;
mod ci_browser;
mod ci_session;
pub mod codecs;
pub mod controls;
pub mod gm;
pub mod messages;
pub mod metrics;
pub mod mpe;
pub mod notes;
pub mod routing;
pub mod testing;
pub mod timecode;
pub mod topology;
pub mod transfer;
pub mod transport;
pub mod tuning;
mod tuning_query;
pub mod ump;
pub mod universal;
mod velocity;

pub mod archive;
pub mod clip;
mod comparison;
mod event_views;
pub mod events;
pub mod file_io;
mod file_timing;
pub mod inspection;
mod inspection_timing;
mod live;
mod playback;
mod recording;
pub mod smf;
pub mod syx;

mod sequence;

mod input;
pub mod midi;
mod output;

#[doc(hidden)]
pub mod drivers;
pub mod transfer_input;
pub mod transfer_output;
pub mod tuning_live;

pub mod ci_live;
pub mod discovery;
