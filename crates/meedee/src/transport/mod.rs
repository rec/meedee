//! Low-level native byte/UMP transport. Rust use never initializes Python.
//!
//! Each client owns a native thread, including destruction. Native calls can
//! block indefinitely: this in-process Rust API does not provide process
//! isolation. Callbacks must return promptly and must not call transport APIs.
//! Input is raw transport data; byte packets can contain partial SysEx and need
//! a bounded stream decoder before being treated as complete musical messages.

mod ffi;
mod native;
mod worker;

use std::error::Error as StdError;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use native::{Native, Receiver};
use worker::Worker;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Capability(&'static str),
    Invalid(&'static str),
    Native(String),
    Busy,
    Closed,
    CallbackThread,
    Worker(String),
    Resource(&'static str),
    CompoundSend {
        completed_messages: Option<usize>,
        total_messages: usize,
        cause: Box<Error>,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Capability(text) | Self::Invalid(text) | Self::Resource(text) => {
                f.write_str(text)
            }
            Self::Native(text) | Self::Worker(text) => f.write_str(text),
            Self::Busy => f.write_str("native client command queue is full"),
            Self::Closed => f.write_str("native client thread has stopped"),
            Self::CallbackThread => {
                f.write_str("transport operations are forbidden inside MIDI callbacks")
            }
            Self::CompoundSend {
                completed_messages,
                total_messages,
                cause,
            } => {
                write!(f, "compound MIDI send failed (completed {completed_messages:?} of {total_messages} messages): {cause}")
            }
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::CompoundSend { cause, .. } => Some(cause.as_ref()),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Protocol {
    #[default]
    Midi1,
    Ump,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Backend {
    CoreMidi = 1,
    AlsaSeq = 2,
    AlsaRaw = 3,
    WinMm = 5,
    AlsaRawUmp = 0x1000,
    AlsaSeqUmp = 0x1001,
    CoreMidiUmp = 0x1002,
    WindowsMidiServices = 0x1003,
}

impl Backend {
    pub fn protocol(self) -> Protocol {
        match self {
            Self::CoreMidi | Self::AlsaSeq | Self::AlsaRaw | Self::WinMm => Protocol::Midi1,
            _ => Protocol::Ump,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TimestampMode {
    #[default]
    Absolute,
    Off,
}

/// Compiled support is not proof of a working driver, service or endpoint.
pub fn compiled_backends() -> Result<Vec<Backend>> {
    let mut mask = 0;
    // SAFETY: the bridge writes one u32 and does not retain its address.
    native::status(unsafe { ffi::md_available(&mut mask) })?;
    Ok(BACKENDS
        .iter()
        .enumerate()
        .filter_map(|(index, backend)| (mask & (1 << index) != 0).then_some(*backend))
        .collect())
}

#[derive(Clone, Copy, Debug)]
pub struct InputOptions {
    pub protocol: Protocol,
    pub backend: Option<Backend>,
    pub timestamps: TimestampMode,
    /// Bounds library-owned receive bytes, including running callbacks.
    /// Application-retained data and native buffers are excluded.
    pub receive_byte_limit: usize,
}

impl Default for InputOptions {
    fn default() -> Self {
        Self {
            protocol: Protocol::Midi1,
            backend: None,
            timestamps: TimestampMode::Absolute,
            receive_byte_limit: 8 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OutputOptions {
    pub protocol: Protocol,
    pub backend: Option<Backend>,
}

fn resolve_backend(protocol: Protocol, selected: Option<Backend>) -> Result<Backend> {
    let backend = selected.unwrap_or_else(|| default_backend(protocol));
    if backend.protocol() != protocol {
        return Err(Error::Capability("backend and protocol do not match"));
    }
    if !compiled_backends()?.contains(&backend) {
        return Err(Error::Capability(
            "requested native backend is not compiled; no fallback was selected",
        ));
    }
    Ok(backend)
}

/// Immutable discovery metadata. Does not keep its originating client alive.
#[derive(Clone, Debug)]
pub struct Port {
    owner: Weak<()>,
    generation: u64,
    index: usize,
    backend: Backend,
    backend_id: u64,
    name: String,
}

impl Port {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn backend(&self) -> Backend {
        self.backend
    }
    /// Backend-supplied number, which is not necessarily unique or persistent.
    pub fn backend_id(&self) -> u64 {
        self.backend_id
    }
}

impl PartialEq for Port {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.owner, &other.owner)
            && self.generation == other.generation
            && self.index == other.index
    }
}
impl Eq for Port {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Packet {
    Bytes(Vec<u8>),
    Ump(Vec<u32>),
}

/// Backend Absolute nanoseconds, including any backend-defined host fallback.
/// This is not UTC. No equivalence across connections or backends is asserted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceivedPacket {
    /// Admission order, including packets subsequently dropped by a bounded stage.
    pub sequence: u64,
    pub timestamp_ns: Option<i64>,
    pub clock: Clock,
    pub packet: Packet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    id: u64,
    pub backend: Backend,
    pub mode: TimestampMode,
}

pub struct Input {
    worker: Worker<Native>,
    clock: Clock,
    limit: usize,
}
pub struct Output {
    worker: Worker<Native>,
    protocol: Protocol,
}

/// Owns the client until close returns it. Drop retires delivery immediately;
/// native teardown is performed by the client thread without joining it here.
pub struct InputConnection {
    worker: Option<Worker<Native>>,
    receiver: Arc<Receiver>,
    clock: Clock,
    limit: usize,
}
pub struct OutputConnection {
    worker: Worker<Native>,
    protocol: Protocol,
}

/// Receive diagnostics remain readable after a connection is closed.
#[derive(Clone)]
pub struct InputDiagnostics {
    receiver: Arc<Receiver>,
}
impl InputDiagnostics {
    pub fn dropped_packets(&self) -> u64 {
        self.receiver.dropped.load(Ordering::Relaxed)
    }
    pub fn callback_panicked(&self) -> bool {
        self.receiver.panicked.load(Ordering::Acquire)
    }
}

impl Input {
    pub fn new(options: InputOptions) -> Result<Self> {
        if options.receive_byte_limit == 0 {
            return Err(Error::Invalid("receive_byte_limit must be positive"));
        }
        let backend = resolve_backend(options.protocol, options.backend)?;
        let clock = Clock {
            id: 0,
            backend,
            mode: options.timestamps,
        };
        let worker = Worker::spawn(move || Native::new(true, backend), Native::initialize)?;
        Ok(Self {
            worker,
            clock,
            limit: options.receive_byte_limit,
        })
    }

    /// Refreshing discovery invalidates older port snapshots for this client.
    pub fn ports(&self) -> Result<Vec<Port>> {
        self.worker.call(Native::ports)
    }

    pub fn connect(
        self,
        port: &Port,
        callback: impl FnMut(ReceivedPacket) + Send + 'static,
    ) -> Result<InputConnection> {
        self.open(Some(port.clone()), "", callback)
    }

    pub fn create_virtual(
        self,
        name: &str,
        callback: impl FnMut(ReceivedPacket) + Send + 'static,
    ) -> Result<InputConnection> {
        self.open(None, name, callback)
    }

    pub fn connect_typed(
        self,
        port: &Port,
        stream: String,
        sysex_limit: usize,
        callback: impl FnMut(crate::live::Item) + Send + 'static,
    ) -> Result<InputConnection> {
        self.open_typed(Some(port.clone()), "", stream, sysex_limit, callback)
    }
    pub fn create_virtual_typed(
        self,
        name: &str,
        stream: String,
        sysex_limit: usize,
        callback: impl FnMut(crate::live::Item) + Send + 'static,
    ) -> Result<InputConnection> {
        self.open_typed(None, name, stream, sysex_limit, callback)
    }
    fn open_typed(
        self,
        port: Option<Port>,
        name: &str,
        stream: String,
        limit: usize,
        mut callback: impl FnMut(crate::live::Item) + Send + 'static,
    ) -> Result<InputConnection> {
        let mut decoder =
            crate::live::Decoder::new(stream.clone(), self.clock.backend.protocol(), limit)
                .map_err(|_| {
                    Error::Invalid("source must be nonempty and decoder limit positive")
                })?;
        let enabled = self.clock.mode == TimestampMode::Absolute;
        let clock = crate::events::Clock::new(
            crate::events::fresh_identity("native")
                .map_err(|_| Error::Resource("clock identity space exhausted"))?,
            1,
            1_000_000_000,
            if enabled {
                "backend_absolute"
            } else {
                "disabled"
            }
            .into(),
            None,
        )
        .expect("validated native clock")
        .with_transport(self.clock.backend as i32, enabled);
        self.open(port, name, move |packet| {
            match decoder.feed(
                &packet.packet,
                &clock,
                packet.timestamp_ns,
                Some(packet.sequence),
            ) {
                Ok(items) => {
                    for item in items {
                        callback(item);
                    }
                }
                Err(e) => callback(crate::live::Item::Diagnostic(crate::live::Diagnostic {
                    stream: stream.clone(),
                    packet: packet.sequence,
                    kind: crate::live::DiagnosticKind::Malformed,
                    lost: 0,
                    reason: e.to_string(),
                })),
            }
        })
    }

    fn open(
        self,
        port: Option<Port>,
        name: &str,
        callback: impl FnMut(ReceivedPacket) + Send + 'static,
    ) -> Result<InputConnection> {
        let name = native::port_name(name)?;
        let clock = Clock {
            id: NEXT_CLOCK.fetch_add(1, Ordering::Relaxed),
            ..self.clock
        };
        let receiver = Arc::new(Receiver::new(clock, self.limit, callback));
        let context = Arc::clone(&receiver);
        self.worker
            .call(move |native| native.open_input(port.as_ref(), &name, context))?;
        Ok(InputConnection {
            worker: Some(self.worker),
            receiver,
            clock,
            limit: self.limit,
        })
    }
}

impl Output {
    pub fn new(options: OutputOptions) -> Result<Self> {
        let backend = resolve_backend(options.protocol, options.backend)?;
        let worker = Worker::spawn(move || Native::new(false, backend), Native::initialize)?;
        Ok(Self {
            worker,
            protocol: options.protocol,
        })
    }
    pub fn ports(&self) -> Result<Vec<Port>> {
        self.worker.call(Native::ports)
    }
    pub fn connect(self, port: &Port) -> Result<OutputConnection> {
        self.open(Some(port.clone()), "")
    }
    pub fn create_virtual(self, name: &str) -> Result<OutputConnection> {
        self.open(None, name)
    }
    fn open(self, port: Option<Port>, name: &str) -> Result<OutputConnection> {
        let name = native::port_name(name)?;
        self.worker
            .call(move |native| native.open_output(port.as_ref(), &name))?;
        Ok(OutputConnection {
            worker: self.worker,
            protocol: self.protocol,
        })
    }
}

impl InputConnection {
    pub fn diagnostics(&self) -> InputDiagnostics {
        InputDiagnostics {
            receiver: Arc::clone(&self.receiver),
        }
    }
    pub fn clock(&self) -> Clock {
        self.clock
    }
    pub fn dropped_packets(&self) -> u64 {
        self.receiver.dropped.load(Ordering::Relaxed)
    }
    pub fn callback_panicked(&self) -> bool {
        self.receiver.panicked.load(Ordering::Acquire)
    }
    pub fn close(mut self) -> Result<Input> {
        self.receiver.retired.store(true, Ordering::SeqCst);
        let worker = self.worker.take().expect("live connection worker");
        worker.call(|native| {
            native.close_input()?;
            native.check_health()
        })?;
        Ok(Input {
            worker,
            clock: self.clock,
            limit: self.limit,
        })
    }
}

impl Drop for InputConnection {
    fn drop(&mut self) {
        self.receiver.retired.store(true, Ordering::SeqCst);
    }
}

impl OutputConnection {
    /// Send a complete command in one owner-thread operation. No other send or
    /// close on this connection can run between its primitive messages. The
    /// failed message may have effects even when its backend returns an error.
    pub fn send_command(&self, command: &(impl crate::messages::Command + ?Sized)) -> Result<()> {
        if self.protocol != Protocol::Midi1 {
            return Err(Error::Capability(
                "compound commands require MIDI 1.0 byte transport",
            ));
        }
        let packets: Vec<Packet> = crate::codecs::expand(command)
            .iter()
            .map(|message| Packet::Bytes(crate::codecs::encode(message)))
            .collect();
        let total_messages = packets.len();
        self.worker
            .call(move |native| {
                for (completed_messages, packet) in packets.iter().enumerate() {
                    native.send(packet).map_err(|cause| Error::CompoundSend {
                        completed_messages: Some(completed_messages),
                        total_messages,
                        cause: Box::new(cause),
                    })?;
                }
                Ok(())
            })
            .map_err(|cause| {
                if matches!(cause, Error::CompoundSend { .. }) {
                    return cause;
                }
                let completed_messages =
                    matches!(cause, Error::Busy | Error::CallbackThread).then_some(0);
                Error::CompoundSend {
                    completed_messages,
                    total_messages,
                    cause: Box::new(cause),
                }
            })
    }

    /// Backend acceptance, not physical delivery. A multi-packet UMP send can
    /// fail after accepting a prefix; its error reports that word count. No retry.
    pub fn send(&self, packet: &Packet) -> Result<()> {
        validate_packet(self.protocol, packet)?;
        let owned = match packet {
            Packet::Bytes(bytes) => Packet::Bytes(native::copy(bytes)?),
            Packet::Ump(words) => Packet::Ump(native::copy(words)?),
        };
        self.worker.call(move |native| native.send(&owned))
    }
    pub fn close(self) -> Result<Output> {
        self.worker.call(|native| {
            native.close_output()?;
            native.check_health()
        })?;
        Ok(Output {
            worker: self.worker,
            protocol: self.protocol,
        })
    }
}

/// Validate a raw send before consuming binding ownership or entering a driver.
pub fn validate_packet(protocol: Protocol, packet: &Packet) -> Result<()> {
    match (protocol, packet) {
        (Protocol::Midi1, Packet::Bytes(bytes))
            if !bytes.is_empty() && bytes.len() <= i32::MAX as usize =>
        {
            Ok(())
        }
        (Protocol::Ump, Packet::Ump(words)) => validate_ump(words),
        _ => Err(Error::Invalid(
            "packet encoding does not match this connection, or the packet is empty",
        )),
    }
}

fn default_backend(protocol: Protocol) -> Backend {
    #[cfg(target_os = "macos")]
    {
        if protocol == Protocol::Midi1 {
            Backend::CoreMidi
        } else {
            Backend::CoreMidiUmp
        }
    }
    #[cfg(target_os = "linux")]
    {
        if protocol == Protocol::Midi1 {
            Backend::AlsaSeq
        } else {
            Backend::AlsaSeqUmp
        }
    }
    #[cfg(target_os = "windows")]
    {
        if protocol == Protocol::Midi1 {
            Backend::WinMm
        } else {
            Backend::WindowsMidiServices
        }
    }
}

// Packet framing only, not a MIDI 2.0 semantic codec. Reserved types retain
// their UMP-assigned widths so a stream cannot end halfway through a packet.
fn validate_ump(words: &[u32]) -> Result<()> {
    if words.len() > i32::MAX as usize {
        return Err(Error::Invalid("UMP word count exceeds the native limit"));
    }
    if words.is_empty() {
        return Err(Error::Invalid("UMP stream must not be empty"));
    }
    let mut offset = 0;
    while offset < words.len() {
        offset += UMP_WORDS[(words[offset] >> 28) as usize];
        if offset > words.len() {
            return Err(Error::Invalid("UMP stream ends inside a packet"));
        }
    }
    Ok(())
}

static NEXT_CLOCK: AtomicU64 = AtomicU64::new(1);

const BACKENDS: [Backend; 8] = [
    Backend::CoreMidi,
    Backend::CoreMidiUmp,
    Backend::AlsaSeq,
    Backend::AlsaSeqUmp,
    Backend::AlsaRaw,
    Backend::AlsaRawUmp,
    Backend::WinMm,
    Backend::WindowsMidiServices,
];
const UMP_WORDS: [usize; 16] = [1, 1, 1, 2, 2, 4, 1, 1, 2, 2, 2, 3, 3, 4, 4, 4];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ump_framing_preserves_wide_packets_and_rejects_truncation() {
        assert!(validate_ump(&[0x20903c64, 0x40903c00, 0xc0000000]).is_ok());
        assert!(validate_ump(&[0x40903c00]).is_err());
        assert!(validate_ump(&[0x50000000, 0, 0]).is_err());
        assert!(validate_ump(&[]).is_err());
    }

    #[test]
    fn protocol_mismatch_is_rejected_without_opening_a_device() {
        assert!(matches!(
            resolve_backend(Protocol::Ump, Some(Backend::CoreMidi)),
            Err(Error::Capability(_))
        ));
    }
}
