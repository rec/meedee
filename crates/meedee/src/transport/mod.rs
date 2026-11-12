//! Low-level native byte/UMP transport. Rust use never initializes Python.
//!
//! Each client owns a native thread, including destruction. Native calls can
//! block indefinitely in-process. new_subprocess selects a shared deadline-bounded
//! worker instead. Callbacks must return promptly and must not call transport APIs.
//! Input is raw transport data; byte packets can contain partial SysEx and need
//! a bounded stream decoder before being treated as complete musical messages.

mod ffi;
mod native;
pub mod subprocess;
mod worker;

use std::error::Error as StdError;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use native::{Native, Receiver};
use worker::Worker;
pub(crate) use worker::{ensure_control_thread, CallbackScope};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Capability(String),
    Invalid(String),
    Native(String),
    Busy,
    Closed,
    CallbackThread,
    Worker(String),
    Timeout(String),
    Resource(String),
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
            Self::Native(text) | Self::Worker(text) | Self::Timeout(text) => f.write_str(text),
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
pub enum Encoding {
    #[default]
    Bytes,
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
    pub fn encoding(self) -> Encoding {
        match self {
            Self::CoreMidi | Self::AlsaSeq | Self::AlsaRaw | Self::WinMm => Encoding::Bytes,
            _ => Encoding::Ump,
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
    pub encoding: Encoding,
    pub backend: Option<Backend>,
    pub timestamps: TimestampMode,
    /// Bounds library-owned receive bytes, including running callbacks.
    /// Application-retained data and native buffers are excluded.
    pub receive_byte_limit: usize,
}

impl Default for InputOptions {
    fn default() -> Self {
        Self {
            encoding: Encoding::Bytes,
            backend: None,
            timestamps: TimestampMode::Absolute,
            receive_byte_limit: 8 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OutputOptions {
    pub encoding: Encoding,
    pub backend: Option<Backend>,
}

fn resolve_backend(encoding: Encoding, selected: Option<Backend>) -> Result<Backend> {
    let backend = selected.unwrap_or_else(|| default_backend(encoding));
    if backend.encoding() != encoding {
        return Err(Error::Capability(
            "backend and encoding do not match".into(),
        ));
    }
    if !compiled_backends()?.contains(&backend) {
        return Err(Error::Capability(
            "requested native backend is not compiled; no fallback was selected".into(),
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

#[derive(Default)]
pub(super) struct FailureState {
    pub error: Mutex<Option<Error>>,
    pub operation: Mutex<String>,
}
pub struct Input {
    failure: Arc<FailureState>,
    worker: Worker<Native>,
    clock: Clock,
    limit: usize,
}
pub struct Output {
    failure: Arc<FailureState>,
    worker: Worker<Native>,
    encoding: Encoding,
}

/// Owns the client until close returns it. Drop retires delivery immediately;
/// native teardown is performed by the client thread without joining it here.
pub struct InputConnection {
    failure: Arc<FailureState>,
    worker: Option<Worker<Native>>,
    receiver: Arc<Receiver>,
    clock: Clock,
    limit: usize,
}
pub struct OutputConnection {
    failure: Arc<FailureState>,
    worker: Worker<Native>,
    encoding: Encoding,
}

/// Read-only worker failure information, retained independently of native handles.
#[derive(Clone)]
pub struct WorkerDiagnostics {
    failure: Arc<FailureState>,
}
impl WorkerDiagnostics {
    pub fn operation(&self) -> String {
        self.failure
            .operation
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    pub fn failure(&self) -> Option<Error> {
        self.failure
            .error
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
}
/// Receive diagnostics remain readable after a connection is closed.
#[derive(Clone)]
pub struct InputDiagnostics {
    failure: WorkerDiagnostics,
    receiver: Arc<Receiver>,
}
impl InputDiagnostics {
    pub fn worker_failure(&self) -> Option<Error> {
        self.failure.failure()
    }
    pub fn dropped_packets(&self) -> u64 {
        self.receiver
            .dropped
            .load(Ordering::Relaxed)
            .saturating_add(self.receiver.transport_dropped.load(Ordering::Relaxed))
    }
    pub fn callback_panicked(&self) -> bool {
        self.receiver.panicked.load(Ordering::Acquire)
    }
}

impl Input {
    pub fn worker_diagnostics(&self) -> WorkerDiagnostics {
        WorkerDiagnostics {
            failure: Arc::clone(&self.failure),
        }
    }

    pub fn worker_failure(&self) -> Option<Error> {
        self.failure
            .error
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn new(options: InputOptions) -> Result<Self> {
        if options.receive_byte_limit == 0 {
            return Err(Error::Invalid("receive_byte_limit must be positive".into()));
        }
        let backend = resolve_backend(options.encoding, options.backend)?;
        let clock = Clock {
            id: 0,
            backend,
            mode: options.timestamps,
        };
        let worker = Worker::spawn(move || Native::new(true, backend), Native::initialize)?;
        Ok(Self {
            failure: Arc::new(FailureState::default()),
            worker,
            clock,
            limit: options.receive_byte_limit,
        })
    }

    pub fn new_subprocess(
        options: InputOptions,
        subprocess: subprocess::SubprocessOptions,
    ) -> Result<Self> {
        if options.receive_byte_limit == 0 {
            return Err(Error::Invalid("receive_byte_limit must be positive".into()));
        }
        let backend = options
            .backend
            .unwrap_or_else(|| default_backend(options.encoding));
        if backend.encoding() != options.encoding {
            return Err(Error::Capability(
                "backend and encoding do not match".into(),
            ));
        }
        let failure = Arc::new(FailureState::default());
        let shared = Arc::clone(&failure);
        let worker = Worker::spawn_subprocess(
            move || {
                Native::subprocess(
                    true,
                    backend,
                    options.timestamps,
                    options.receive_byte_limit,
                    subprocess,
                    shared,
                )
            },
            Native::initialize,
        )?;
        Ok(Self {
            failure,
            worker,
            clock: Clock {
                id: 0,
                backend,
                mode: options.timestamps,
            },
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
            crate::live::Decoder::new(stream.clone(), self.clock.backend.encoding(), limit)
                .map_err(|_| {
                    Error::Invalid("source must be nonempty and decoder limit positive".into())
                })?;
        let enabled = self.clock.mode == TimestampMode::Absolute;
        let clock = crate::events::Clock::new(
            crate::events::fresh_identity("native")
                .map_err(|_| Error::Resource("clock identity space exhausted".into()))?,
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
            failure: self.failure,
            worker: Some(self.worker),
            receiver,
            clock,
            limit: self.limit,
        })
    }
}

impl Output {
    pub fn worker_diagnostics(&self) -> WorkerDiagnostics {
        WorkerDiagnostics {
            failure: Arc::clone(&self.failure),
        }
    }

    pub fn worker_failure(&self) -> Option<Error> {
        self.failure
            .error
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn new(options: OutputOptions) -> Result<Self> {
        let backend = resolve_backend(options.encoding, options.backend)?;
        let worker = Worker::spawn(move || Native::new(false, backend), Native::initialize)?;
        Ok(Self {
            failure: Arc::new(FailureState::default()),
            worker,
            encoding: options.encoding,
        })
    }
    pub fn new_subprocess(
        options: OutputOptions,
        subprocess: subprocess::SubprocessOptions,
    ) -> Result<Self> {
        let backend = options
            .backend
            .unwrap_or_else(|| default_backend(options.encoding));
        if backend.encoding() != options.encoding {
            return Err(Error::Capability(
                "backend and encoding do not match".into(),
            ));
        }
        let failure = Arc::new(FailureState::default());
        let shared = Arc::clone(&failure);
        let worker = Worker::spawn_subprocess(
            move || {
                Native::subprocess(
                    false,
                    backend,
                    TimestampMode::Off,
                    8_388_608,
                    subprocess,
                    shared,
                )
            },
            Native::initialize,
        )?;
        Ok(Self {
            failure,
            worker,
            encoding: options.encoding,
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
            failure: self.failure,
            worker: self.worker,
            encoding: self.encoding,
        })
    }
}

impl InputConnection {
    pub fn worker_diagnostics(&self) -> WorkerDiagnostics {
        WorkerDiagnostics {
            failure: Arc::clone(&self.failure),
        }
    }

    pub fn worker_failure(&self) -> Option<Error> {
        self.failure
            .error
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub(crate) fn check_health(&self) -> Result<()> {
        self.worker
            .as_ref()
            .ok_or(Error::Closed)?
            .call(|native| native.check_health())
    }
    pub fn diagnostics(&self) -> InputDiagnostics {
        InputDiagnostics {
            failure: WorkerDiagnostics {
                failure: Arc::clone(&self.failure),
            },
            receiver: Arc::clone(&self.receiver),
        }
    }
    pub fn clock(&self) -> Clock {
        self.clock
    }
    pub fn dropped_packets(&self) -> u64 {
        self.receiver
            .dropped
            .load(Ordering::Relaxed)
            .saturating_add(self.receiver.transport_dropped.load(Ordering::Relaxed))
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
            failure: Arc::clone(&self.failure),
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
    pub fn worker_diagnostics(&self) -> WorkerDiagnostics {
        WorkerDiagnostics {
            failure: Arc::clone(&self.failure),
        }
    }

    pub fn worker_failure(&self) -> Option<Error> {
        self.failure
            .error
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Send a complete command in one owner-thread operation. No other send or
    /// close on this connection can run between its primitive messages. The
    /// failed message may have effects even when its backend returns an error.
    pub fn send_command(&self, command: &(impl crate::messages::Command + ?Sized)) -> Result<()> {
        if self.encoding == Encoding::Ump {
            use crate::ump::UmpValue;
            let mut words = Vec::new();
            for message in command
                .expand()
                .map_err(|e| Error::Invalid(e.to_string()))?
            {
                words.extend(
                    message
                        .encode_words()
                        .map_err(|e| Error::Invalid(e.to_string()))?,
                );
            }
            return self.send(&Packet::Ump(words));
        }
        let packets: Vec<Packet> = crate::codecs::expand(command)
            .map_err(|_| Error::Invalid("command channel is unassigned".into()))?
            .iter()
            .map(|message| crate::codecs::encode(message).map(Packet::Bytes))
            .collect::<std::result::Result<_, _>>()
            .map_err(|_| Error::Invalid("message channel is unassigned".into()))?;
        let total_messages = packets.len();
        self.worker
            .call(move |native| native.send_packets(&packets))
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
        validate_packet(self.encoding, packet)?;
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
            failure: self.failure,
            worker: self.worker,
            encoding: self.encoding,
        })
    }
}

/// Validate a raw send before consuming binding ownership or entering a driver.
pub fn validate_packet(encoding: Encoding, packet: &Packet) -> Result<()> {
    match (encoding, packet) {
        (Encoding::Bytes, Packet::Bytes(bytes))
            if !bytes.is_empty() && bytes.len() <= i32::MAX as usize =>
        {
            Ok(())
        }
        (Encoding::Ump, Packet::Ump(words)) => validate_ump(words),
        _ => Err(Error::Invalid(
            "packet encoding does not match this connection, or the packet is empty".into(),
        )),
    }
}

fn default_backend(encoding: Encoding) -> Backend {
    #[cfg(target_os = "macos")]
    {
        if encoding == Encoding::Bytes {
            Backend::CoreMidi
        } else {
            Backend::CoreMidiUmp
        }
    }
    #[cfg(target_os = "linux")]
    {
        if encoding == Encoding::Bytes {
            Backend::AlsaSeq
        } else {
            Backend::AlsaSeqUmp
        }
    }
    #[cfg(target_os = "windows")]
    {
        if encoding == Encoding::Bytes {
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
        return Err(Error::Invalid(
            "UMP word count exceeds the native limit".into(),
        ));
    }
    if words.is_empty() {
        return Err(Error::Invalid("UMP stream must not be empty".into()));
    }
    let mut offset = 0;
    while offset < words.len() {
        offset += UMP_WORDS[(words[offset] >> 28) as usize];
        if offset > words.len() {
            return Err(Error::Invalid("UMP stream ends inside a packet".into()));
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
    fn convenience_selection_requires_a_unique_pattern_match() {
        let owner = Arc::new(());
        let ports: Vec<_> = ["Keyboard", "FLOW 8", "Café1", "Duplicate", "Duplicate"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| Port {
                owner: Arc::downgrade(&owner),
                generation: 1,
                index,
                backend: Backend::CoreMidi,
                backend_id: index as u64,
                name: name.into(),
            })
            .collect();
        let select = crate::output::select_port;
        assert_eq!(select(&ports, None).unwrap(), &ports[0]);
        assert_eq!(select(&ports, Some("FLOW")).unwrap(), &ports[1]);
        assert_eq!(select(&ports, Some("*é?")).unwrap(), &ports[2]);
        assert_eq!(
            select(&ports, Some("Duplicate")).unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert_eq!(
            select(&ports, Some("*")).unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert_eq!(
            select(&ports, Some("flow")).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
        assert_eq!(
            select(&[], None).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
    }

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
            resolve_backend(Encoding::Ump, Some(Backend::CoreMidi)),
            Err(Error::Capability(_))
        ));
    }
}
