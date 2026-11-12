//! Shared Rust MIDI toolkit. No Python interpreter or PyO3 dependency.

pub mod ci;
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
pub mod translation;
pub mod transport;
pub mod tuning;
pub mod ump;
pub mod universal;
pub mod universe;
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
pub mod sysex;

mod sequence;

mod input;
pub mod midi;
mod output;

#[doc(hidden)]
pub mod drivers;

pub mod discovery;

// Common values and I/O; specialist operations remain in their public modules.
pub use archive::{Recorder, RecordingReport};
pub use codecs::Decoder;
pub use controls::{
    ControlInterpreter, Controller, ParameterFamily, StepDirection, VelocityInterpreter,
};
pub use events::{
    Cancellation, Clock, ClockMapping, PacketDecoder, PlaybackPlan, PlaybackReport, Rational,
    ReceivedPacket, SourceSpan, StreamDiagnostic, Timestamp,
};
pub use file_io::{Reader, Writer};
pub use gm::{BankProgram, DrumKit, Gm1On, Gm2On, GmOff, Percussion, Program};
pub use inspection::{ComparisonReport, Monitor, MonitorReport};
pub use messages::{
    ActiveSensing, AnyCommand, AnyMessage, Beats, ChannelPressure, Command, Continue,
    ControlChange, ControllerFamily, Message, Note, NoteOff, NoteOn, ParameterSet, ParameterStep,
    PitchBend, PolyPressure, PrimitiveMessage, ProgramChange, QuarterFrame, Rest, SongPosition,
    SongSelect, Start, Stop, SysEx, SystemReset, TimingClock, TuneRequest,
};
pub use metrics::{Distribution, Histogram};
pub use midi::{
    Backend, Encoding, Error, Input, Output, OutputSource, OutputValue, PlaybackOptions, Port,
    Recording, RecordingOptions, SubprocessOptions, TimestampMode,
};
pub use mpe::ZoneConfiguration;
pub use routing::{Route, Router};
pub use timecode::FrameRate;
pub use transport::Packet;
pub use tuning::{NoteTuning, Pitch, ScaleOffsets, TuningTable, TuningValue};
pub use ump::{SysEx7Packet, SysEx8Packet};
pub use universal::{
    IdentityReply, IdentityRequest, MasterCoarseTuning, MasterFineTuning, MasterVolume,
};
