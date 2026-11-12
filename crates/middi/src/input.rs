//! Lazy MIDI input, caller-thread decoding and scoped native ownership.
use crate::events::{self, Clock, Payload};
use crate::live::{Decoder, DiagnosticKind, Item};
use crate::output::{select_port, transport_error};
use crate::transport::{self, Packet, Port};
use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
#[path = "input_recording.rs"]
mod recording;
pub use recording::Recording;
#[path = "input_api.rs"]
mod api;
pub use api::{Input, InputErrors, RecordingOptions};

/// Iterate over messages from the first input, reporting diagnostics to stderr.
/// No native client is created until discovery or receiving. Drop closes input.
/// Use `with_errors` to receive diagnostics and terminal errors explicitly.
///
/// ```no_run
/// use middi::midi::Input;
/// for message in Input::new() {
///     println!("{message:?}");
/// }
/// ```
pub(super) struct Core {
    options: transport::InputOptions,
    run_in_subprocess: bool,
    subprocess_options: transport::subprocess::SubprocessOptions,
    port: Option<String>,
    channels: Option<u16>,
    sysex_limit: usize,
    queue: Arc<Queue>,
    client: Option<transport::Input>,
    connection: Option<transport::InputConnection>,
    decoder: Option<Decoder>,
    clock: Option<Clock>,
    stream: Option<String>,
    pending: VecDeque<io::Result<Item>>,
    reported_loss: u64,
    expected: u64,
    closed: bool,
    raw: bool,
    native_dropped: u64,
    raw_pending: VecDeque<io::Result<Item<transport::ReceivedPacket>>>,
    native_clock: Option<transport::Clock>,
    recording: Option<recording::Attachment>,
    recording_report: Option<crate::archive::RecordingReport>,
}

/// Borrowed input iteration that retains decoding diagnostics and terminal errors.
#[cfg(test)]
struct CoreErrors<'a> {
    input: &'a mut Core,
}
impl Default for Core {
    fn default() -> Self {
        Self::new()
    }
}
impl Core {
    pub fn new() -> Self {
        Self {
            options: transport::InputOptions::default(),
            run_in_subprocess: false,
            subprocess_options: Default::default(),
            port: None,
            channels: None,
            sysex_limit: crate::codecs::Decoder::DEFAULT_SYSEX_LIMIT,
            queue: Arc::new(Queue::new(128, 8 * 1024 * 1024)),
            client: None,
            connection: None,
            decoder: None,
            clock: None,
            stream: None,
            pending: VecDeque::new(),
            reported_loss: 0,
            expected: 0,
            closed: false,
            raw: false,
            native_dropped: 0,
            raw_pending: VecDeque::new(),
            native_clock: None,
            recording: None,
            recording_report: None,
        }
    }
    pub fn with_channel(self, channel: i64) -> crate::midi::Result<Self> {
        self.with_channels([channel])
    }
    /// An empty selection excludes all channel messages. System messages and
    /// diagnostics always pass through, including on UMP input.
    pub fn with_channels(
        mut self,
        channels: impl IntoIterator<Item = i64>,
    ) -> crate::midi::Result<Self> {
        let mut mask = 0;
        for channel in channels {
            if !(1..=16).contains(&channel) {
                return Err(invalid("channel must be in 1..=16").into());
            }
            mask |= 1 << (channel - 1);
        }
        self.channels = Some(mask);
        Ok(self)
    }
    /// Select a unique case-sensitive substring or shell name pattern, including brackets.
    pub fn with_port(mut self, pattern: impl Into<String>) -> crate::midi::Result<Self> {
        if self.connection.is_some() || self.closed {
            return Err(invalid("input is connected or closed").into());
        }
        let pattern = pattern.into();
        if pattern.is_empty() {
            return Err(invalid("port pattern must not be empty").into());
        }
        self.port = Some(pattern);
        Ok(self)
    }
    pub fn with_run_in_subprocess(mut self, enabled: bool) -> crate::midi::Result<Self> {
        if self.client.is_some() || self.connection.is_some() || self.closed {
            return Err(invalid("configure subprocess before discovery or connection").into());
        }
        self.run_in_subprocess = enabled;
        Ok(self)
    }
    pub fn with_subprocess_options(
        mut self,
        options: transport::subprocess::SubprocessOptions,
    ) -> crate::midi::Result<Self> {
        if self.client.is_some() || self.connection.is_some() || self.closed {
            return Err(invalid("configure subprocess before discovery or connection").into());
        }
        if options.timeout.is_zero()
            || std::time::Instant::now()
                .checked_add(options.timeout)
                .is_none()
        {
            return Err(invalid("worker timeout must be positive and representable").into());
        }
        self.subprocess_options = options;
        Ok(self)
    }
    pub fn with_transport(mut self, options: transport::InputOptions) -> crate::midi::Result<Self> {
        if self.client.is_some() || self.connection.is_some() || self.closed {
            return Err(invalid("configure transport before discovery or connection").into());
        }
        self.options = options;
        Ok(self)
    }
    /// Bound queued raw packets/bytes and assembled SysEx bytes. Native transport
    /// admission has its separate InputOptions receive byte limit.
    pub fn with_receive_limits(
        mut self,
        packets: usize,
        bytes: usize,
        sysex: usize,
    ) -> crate::midi::Result<Self> {
        if self.connection.is_some() || self.closed {
            return Err(invalid("configure limits before connection").into());
        }
        if packets == 0 || bytes == 0 || sysex == 0 {
            return Err(invalid("receive limits must be positive").into());
        }
        self.queue = Arc::new(Queue::new(packets, bytes));
        self.sysex_limit = sysex;
        Ok(self)
    }
    pub fn ports(&mut self) -> crate::midi::Result<Vec<Port>> {
        self.client()?
            .ports()
            .map_err(crate::midi::Error::Transport)
    }
    pub fn stream(&mut self) -> crate::midi::Result<&str> {
        if self.closed {
            return Err(invalid("input is closed").into());
        }
        self.open()?;
        Ok(self.stream.as_deref().expect("opened input stream"))
    }
    pub fn dropped_packets(&self) -> u64 {
        let queued = self
            .queue
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .lost;
        queued.saturating_add(
            self.connection
                .as_ref()
                .map_or(self.native_dropped, |c| c.diagnostics().dropped_packets()),
        )
    }
    pub fn check_health(&self) -> crate::midi::Result<()> {
        if self.closed {
            return Err(invalid("input is closed").into());
        }
        if let Some(connection) = &self.connection {
            connection
                .check_health()
                .map_err(crate::midi::Error::Transport)?;
        }
        Ok(())
    }
    #[cfg(test)]
    fn with_errors(&mut self) -> CoreErrors<'_> {
        CoreErrors { input: self }
    }
    /// Stop native delivery, drain accepted packets and finish partial decoding.
    /// Remaining messages/diagnostics can still be drained by either iterator.
    pub fn close(&mut self) -> crate::midi::Result<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let mut lost = 0;
        let result = if let Some(connection) = self.connection.take() {
            let diagnostics = connection.diagnostics();
            let result = connection.close().map(drop).map_err(transport_error);
            lost = diagnostics.dropped_packets();
            self.native_dropped = lost;
            result
        } else {
            Ok(())
        };
        self.client = None;
        let packets = {
            let mut state = self.queue.state.lock().unwrap_or_else(|p| p.into_inner());
            state.closed = true;
            lost = lost.saturating_add(state.lost);
            state.bytes = 0;
            std::mem::take(&mut state.packets)
        };
        self.queue.changed.notify_all();
        for packet in packets {
            if self.raw {
                self.raw_packet(packet);
            } else {
                self.decode(packet);
            }
        }
        if self.raw {
            if lost > self.reported_loss {
                let diagnostic = self.raw_loss(
                    lost - self.reported_loss,
                    "packets dropped before input closed",
                );
                self.raw_pending.push_back(Ok(Item::Diagnostic(diagnostic)));
                self.reported_loss = lost;
            }
            return result.map_err(crate::midi::Error::from);
        }
        let mut final_items = Vec::new();
        if let Some(decoder) = self.decoder.as_mut() {
            if lost > self.reported_loss {
                final_items.push(decoder.loss(
                    lost - self.reported_loss,
                    "packets dropped before input closed".into(),
                ));
                self.reported_loss = lost;
            }
            final_items.extend(decoder.finish());
        }
        for item in final_items {
            self.accept_item(item);
        }
        result.map_err(crate::midi::Error::from)
    }
    fn client(&mut self) -> io::Result<&transport::Input> {
        if self.closed || self.connection.is_some() {
            return Err(invalid("input is connected or closed"));
        }
        if self.client.is_none() {
            self.client = Some(
                if self.run_in_subprocess {
                    transport::Input::new_subprocess(self.options, self.subprocess_options.clone())
                } else {
                    transport::Input::new(self.options)
                }
                .map_err(|e| io::Error::from(crate::midi::Error::Init(e)))?,
            );
        }
        Ok(self.client.as_ref().expect("initialized input"))
    }
    fn open_connection(&mut self, port: Option<&Port>, name: &str) -> io::Result<()> {
        self.client()?;
        let stream = events::fresh_identity("input")?;
        let decoder = Decoder::new(stream.clone(), self.options.encoding, self.sysex_limit)?;
        let queue = self.queue.clone();
        let client = self.client.take().expect("initialized input");
        let callback = move |packet: transport::ReceivedPacket| {
            queue.push(Ingress {
                packet: packet.packet,
                sequence: packet.sequence,
                timestamp: packet.timestamp_ns,
            })
        };
        let connection = match port {
            Some(port) => client.connect(port, callback),
            None => client.create_virtual(name, callback),
        }
        .map_err(|e| io::Error::from(crate::midi::Error::Connect(e)))?;
        let native_clock = connection.clock();
        self.native_clock = Some(native_clock);
        let enabled = native_clock.mode == transport::TimestampMode::Absolute;
        self.clock = Some(
            Clock::new(
                events::fresh_identity("input-clock")?,
                1,
                1_000_000_000,
                if enabled {
                    "backend_absolute"
                } else {
                    "disabled"
                }
                .into(),
                None,
            )?
            .with_transport(native_clock.backend as i32, enabled),
        );
        self.decoder = Some(decoder);
        self.stream = Some(stream);
        self.connection = Some(connection);
        Ok(())
    }
    fn open(&mut self) -> io::Result<()> {
        if self.decoder.is_none() && !self.closed {
            let ports = self.ports()?;
            let port = select_port(&ports, self.port.as_deref())?;
            self.open_connection(Some(port), "")?;
        }
        Ok(())
    }
    fn decode(&mut self, packet: Ingress) {
        self.expected = packet.sequence.saturating_add(1);
        let result = self.decoder.as_mut().expect("connected decoder").feed(
            &packet.packet,
            self.clock.as_ref().expect("connected clock"),
            packet.timestamp,
            Some(packet.sequence),
        );
        match result {
            Ok(items) => {
                for item in items {
                    if let Item::Diagnostic(d) = &item {
                        if d.kind == DiagnosticKind::Loss {
                            self.reported_loss = self.reported_loss.saturating_add(d.lost);
                        }
                    }
                    self.accept_item(item);
                }
            }
            Err(error) => {
                if let Some(attachment) = &self.recording {
                    attachment.recorder.fail(error.to_string());
                }
                self.pending.push_back(Err(error));
            }
        }
    }
    fn accept_item(&mut self, item: Item) {
        if let (Some(mask), Item::Message(message)) = (self.channels, &item) {
            match crate::routing::classify(message) {
                Ok(info) if info.channel.is_some_and(|c| mask & (1 << (c - 1)) == 0) => return,
                Err(error) => {
                    self.pending.push_back(Err(error));
                    return;
                }
                _ => {}
            }
        }
        self.record_item(&item);
        self.pending.push_back(Ok(item));
    }
    fn raw_loss(&self, lost: u64, reason: &str) -> crate::live::Diagnostic {
        crate::live::Diagnostic {
            stream: self.stream.clone().expect("opened stream"),
            packet: self.expected,
            kind: DiagnosticKind::Loss,
            lost,
            reason: reason.into(),
        }
    }
    fn raw_packet(&mut self, packet: Ingress) {
        if packet.sequence > self.expected {
            let lost = packet.sequence - self.expected;
            self.reported_loss = self.reported_loss.saturating_add(lost);
            let diagnostic = self.raw_loss(lost, "receive queue dropped packets");
            self.raw_pending.push_back(Ok(Item::Diagnostic(diagnostic)));
        }
        self.expected = packet.sequence.saturating_add(1);
        self.raw_pending
            .push_back(Ok(Item::Message(Box::new(transport::ReceivedPacket {
                packet: packet.packet,
                sequence: packet.sequence,
                timestamp_ns: packet.timestamp,
                clock: self.native_clock.expect("opened native clock"),
            }))));
    }
    fn next_raw(
        &mut self,
        deadline: Option<Instant>,
    ) -> Option<io::Result<Item<transport::ReceivedPacket>>> {
        if let Err(error) = self.open() {
            return Some(Err(error));
        }
        loop {
            if let Some(item) = self.raw_pending.pop_front() {
                return Some(item);
            }
            match self.next_ingress(deadline)? {
                Ok(IngressItem::Packet(packet)) => self.raw_packet(packet),
                Ok(IngressItem::Loss(diagnostic)) => return Some(Ok(Item::Diagnostic(diagnostic))),
                Err(error) => return Some(Err(error)),
            }
        }
    }
    fn next_item(&mut self, deadline: Option<Instant>) -> Option<io::Result<Item>> {
        if let Err(error) = self.open() {
            if let Err(close_error) = self.close() {
                report(&close_error);
            }
            return Some(Err(error));
        }
        loop {
            if let Some(item) = self.pending.pop_front() {
                if item.is_err() {
                    if let Err(close_error) = self.close() {
                        report(&close_error);
                    }
                    self.pending.clear();
                    return Some(item);
                }
                return Some(item);
            }
            match self.next_ingress(deadline)? {
                Ok(IngressItem::Packet(packet)) => self.decode(packet),
                Ok(IngressItem::Loss(diagnostic)) => {
                    let decoder = self.decoder.as_mut().expect("opened decoder");
                    decoder.loss(diagnostic.lost, diagnostic.reason.clone());
                    decoder.begin_at(self.expected);
                    let item = Item::Diagnostic(diagnostic);
                    self.record_item(&item);
                    return Some(Ok(item));
                }
                Err(error) => {
                    if let Err(close_error) = self.close() {
                        report(&close_error);
                    }
                    self.pending.push_back(Err(error));
                }
            }
        }
    }
    fn next_ingress(&mut self, deadline: Option<Instant>) -> Option<io::Result<IngressItem>> {
        loop {
            if self.closed {
                return None;
            }
            let mut state = self.queue.state.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(packet) = state.packets.pop_front() {
                state.bytes -= packet.size();
                return Some(Ok(IngressItem::Packet(packet)));
            }
            if state.next_sequence > self.expected {
                let next = state.next_sequence;
                drop(state);
                let diagnostic =
                    self.raw_loss(next - self.expected, "receive queue dropped packets");
                self.reported_loss = self.reported_loss.saturating_add(diagnostic.lost);
                self.expected = next;
                return Some(Ok(IngressItem::Loss(diagnostic)));
            }
            drop(state);
            let total = self.dropped_packets();
            if let Err(error) = self.check_health() {
                return Some(Err(error.into()));
            }
            let state = self.queue.state.lock().unwrap_or_else(|p| p.into_inner());
            if !state.packets.is_empty() {
                continue;
            }
            if total > self.reported_loss {
                drop(state);
                let diagnostic =
                    self.raw_loss(total - self.reported_loss, "native receive dropped packets");
                self.expected = self.expected.saturating_add(diagnostic.lost);
                self.reported_loss = total;
                return Some(Ok(IngressItem::Loss(diagnostic)));
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return None;
            }
            let wait = deadline.map_or(Duration::from_millis(50), |d| {
                d.saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(50))
            });
            drop(
                self.queue
                    .changed
                    .wait_timeout(state, wait)
                    .unwrap_or_else(|p| p.into_inner()),
            );
        }
    }
}
impl Iterator for Core {
    type Item = Payload;
    fn next(&mut self) -> Option<Payload> {
        loop {
            match self.next_item(None)? {
                Ok(Item::Message(message)) => return Some(*message),
                Ok(Item::Diagnostic(diagnostic)) => {
                    let _ = writeln!(io::stderr().lock(), "middi: {diagnostic:?}");
                }
                Err(error) => {
                    report(&error);
                    return None;
                }
            }
        }
    }
}
#[cfg(test)]
impl Iterator for CoreErrors<'_> {
    type Item = crate::midi::Result<Item>;
    fn next(&mut self) -> Option<Self::Item> {
        self.input
            .next_item(None)
            .map(|r| r.map_err(crate::midi::Error::from))
    }
}
impl Drop for Core {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            report(&error);
        }
        for item in self.pending.drain(..) {
            match item {
                Ok(Item::Diagnostic(diagnostic)) => {
                    let _ = writeln!(io::stderr().lock(), "middi: {diagnostic:?}");
                }
                Err(error) => report(&error),
                _ => {}
            }
        }
    }
}
enum IngressItem {
    Packet(Ingress),
    Loss(crate::live::Diagnostic),
}
struct Ingress {
    packet: Packet,
    sequence: u64,
    timestamp: Option<i64>,
}
impl Ingress {
    fn size(&self) -> usize {
        match &self.packet {
            Packet::Bytes(v) => v.len(),
            Packet::Ump(v) => v.len() * 4,
        }
    }
}
struct Queue {
    state: Mutex<QueueState>,
    changed: Condvar,
    packet_limit: usize,
    byte_limit: usize,
}
#[derive(Default)]
struct QueueState {
    packets: VecDeque<Ingress>,
    bytes: usize,
    lost: u64,
    closed: bool,
    next_sequence: u64,
    last_message_time: Option<Instant>,
}
impl Queue {
    fn new(packet_limit: usize, byte_limit: usize) -> Self {
        Self {
            state: Mutex::new(QueueState::default()),
            changed: Condvar::new(),
            packet_limit,
            byte_limit,
        }
    }
    fn push(&self, packet: Ingress) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.closed {
            return;
        }
        state.last_message_time = Some(Instant::now());
        state.next_sequence = packet.sequence.saturating_add(1);
        let size = packet.size();
        if state.packets.len() == self.packet_limit || size > self.byte_limit - state.bytes {
            state.lost = state.lost.saturating_add(1);
        } else {
            state.bytes += size;
            state.packets.push_back(packet);
        }
        self.changed.notify_one();
    }
}
fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason)
}
fn report(error: &impl std::fmt::Display) {
    let _ = writeln!(io::stderr().lock(), "middi: {error}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{AnyMessage, Message};

    fn input(encoding: transport::Encoding) -> Core {
        let mut input = Core::new();
        input.options.encoding = encoding;
        input.decoder = Some(Decoder::new("keyboard".into(), encoding, 1024).unwrap());
        input.stream = Some("keyboard".into());
        input.clock =
            Some(Clock::new("test-clock".into(), 1, 1_000_000_000, "test".into(), None).unwrap());
        input
    }
    fn packet(sequence: u64, bytes: &[u8]) -> Ingress {
        Ingress {
            packet: Packet::Bytes(bytes.to_vec()),
            sequence,
            timestamp: Some(sequence as i64 * 100),
        }
    }
    #[test]
    fn cancellation_preserves_queued_input_and_closure_is_distinct_from_timeout() {
        let core = input(transport::Encoding::Bytes).with_channel(2).unwrap();
        let queue = core.queue.clone();
        let start = Instant::now();
        queue.push(packet(0, &[0x90, 60, 90, 0x91, 62, 90]));
        let mut input = Input::decoded(core);
        let cancel = crate::events::Cancellation::default();
        cancel.cancel();
        assert!(matches!(
            input.receive(None, Some(&cancel)),
            Err(crate::midi::Error::Cancelled)
        ));
        assert!(!input.closed() && input.failure().is_none());
        let activity = input.last_message_time().unwrap();
        assert!(activity >= start);
        let Some(Item::Message(message)) = input.receive(Some(Duration::ZERO), None).unwrap()
        else {
            panic!("selected message");
        };
        assert!(
            matches!(message.as_ref(), Payload::Message(AnyMessage::NoteOn(m)) if m.note() == 62)
        );
        assert!(input.receive(Some(Duration::ZERO), None).unwrap().is_none());
        input.close().unwrap();
        assert_eq!(input.last_message_time(), Some(activity));
        assert_eq!(input.stream().unwrap(), "keyboard");
        assert_eq!(input.clock().unwrap().identity(), "test-clock");
        assert!(matches!(
            input.receive(Some(Duration::ZERO), None),
            Err(crate::midi::Error::Closed)
        ));
        assert!(input.with_errors().next().is_none());
        assert!(input.failure().is_none());
    }

    #[test]
    fn attached_recording_runs_without_receiving_and_has_independent_capacity() {
        let core = input(transport::Encoding::Bytes)
            .with_receive_limits(1, 8192, 1024)
            .unwrap();
        let queue = core.queue.clone();
        let mut input = Input::decoded(core);
        queue.push(packet(0, &[0x90, 50, 90, 0x90, 52, 90]));
        assert!(matches!(
            input.receive(Some(Duration::ZERO), None).unwrap(),
            Some(Item::Message(_))
        ));
        let path = std::env::temp_dir().join(format!(
            "middi-background-{}-{:?}.middi",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let recorder = input
            .start(
                Some(&path),
                &RecordingOptions {
                    metrics: true,
                    ..Default::default()
                },
            )
            .unwrap();
        queue.push(packet(1, &[0x90, 60, 90, 0x90, 62, 90, 0x90, 64, 90]));
        let deadline = Instant::now() + Duration::from_secs(2);
        while recorder.pressure().unwrap().accepted_records < 3 {
            assert!(Instant::now() < deadline, "background recorder did not run");
            std::thread::yield_now();
        }
        let report = input.stop(Duration::from_secs(1)).unwrap();
        assert!(report.complete && report.records == 5);
        assert!(input.failure().is_some()); // receive capacity is independent
        let records = crate::archive::Reader::open(&path)
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            records
                .iter()
                .filter(|r| matches!(r, crate::archive::Record::Event(_)))
                .count(),
            3
        );
        input.close().unwrap();
        assert_eq!(input.recording_report(), Some(report));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn recording_stop_accounts_for_a_lost_tail_once() {
        let core = input(transport::Encoding::Bytes)
            .with_receive_limits(2, 8192, 1024)
            .unwrap();
        let queue = core.queue.clone();
        for sequence in 0..3 {
            queue.push(packet(sequence, &[0x90, 60, 90]));
        }
        let path = std::env::temp_dir().join(format!(
            "middi-stop-loss-{}-{}.middi",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut input = Input::decoded(core);
        input
            .start(
                Some(&path),
                &RecordingOptions {
                    best_effort: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let report = input.stop(Duration::from_secs(1)).unwrap();
        assert!(report.complete);
        assert_eq!(report.gap_records, 1);
        input.close().unwrap();
        assert_eq!(input.dropped_packets(), 1);
        assert_eq!(input.recording_report(), Some(report));
        let records = crate::archive::Reader::open(&path)
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            records
                .iter()
                .filter_map(|r| if let crate::archive::Record::Gap { lost, .. } = r {
                    Some(lost)
                } else {
                    None
                })
                .sum::<u64>(),
            1
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn input_iterates_messages_with_provenance_and_channel_filtering() {
        let mut input = input(transport::Encoding::Bytes)
            .with_channels([1, 3])
            .unwrap();
        input
            .queue
            .push(packet(0, &[0x90, 60, 90, 0x91, 62, 70, 0x92, 64, 80, 0xf8]));
        input.close().unwrap();
        let messages: Vec<_> = input.collect();
        assert_eq!(messages.len(), 3);
        assert!(matches!(&messages[0],Payload::Message(AnyMessage::NoteOn(m)) if m.note()==60));
        assert!(matches!(&messages[1],Payload::Message(AnyMessage::NoteOn(m)) if m.note()==64));
        assert!(matches!(
            &messages[2],
            Payload::Message(AnyMessage::TimingClock(_))
        ));
        for message in messages {
            assert_eq!(message.stream(), Some("keyboard"));
            assert_eq!(message.timestamp().unwrap().ticks, 0);
            assert_eq!(message.span().unwrap().first_packet, 0);
        }
    }
    #[test]
    fn ump_channels_filter_without_dropping_system_messages() {
        let mut input = input(transport::Encoding::Ump).with_channel(3).unwrap();
        for (sequence, words) in [vec![0x2090_3c5a], vec![0x2192_3e46], vec![0x10f8_0000]]
            .into_iter()
            .enumerate()
        {
            input.queue.push(Ingress {
                packet: Packet::Ump(words),
                sequence: sequence as u64,
                timestamp: Some(100),
            });
        }
        input.close().unwrap();
        let messages: Vec<_> = input.collect();
        assert_eq!(messages.len(), 2);
        assert!(matches!(&messages[0],Payload::Message(m) if m.group()==Some(2)));
    }
    #[test]
    fn queue_loss_is_reported_during_silence_without_double_counting_next_packet() {
        let mut input = input(transport::Encoding::Bytes)
            .with_receive_limits(1, 3, 1024)
            .unwrap();
        input.queue.push(packet(0, &[0x90, 60, 90]));
        input.queue.push(packet(1, &[0x90, 62, 90]));
        assert!(matches!(
            input.with_errors().next().unwrap().unwrap(),
            Item::Message(_)
        ));
        assert!(
            matches!(input.with_errors().next().unwrap().unwrap(), Item::Diagnostic(d) if d.lost==1)
        );
        input.queue.push(packet(2, &[0x90, 64, 90]));
        assert!(matches!(
            input.with_errors().next().unwrap().unwrap(),
            Item::Message(_)
        ));
        input.close().unwrap();
        assert!(input.with_errors().next().is_none());
    }

    #[test]
    fn bounded_queue_reports_tail_loss_after_accepted_prefix() {
        let mut input = input(transport::Encoding::Bytes)
            .with_receive_limits(1, 3, 1024)
            .unwrap();
        input.queue.push(packet(0, &[0x90, 60, 90]));
        input.queue.push(packet(1, &[0x90, 62, 90]));
        input.close().unwrap();
        let items: Vec<_> = input
            .with_errors()
            .collect::<crate::midi::Result<_>>()
            .unwrap();
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0], Item::Message(_)));
        assert!(
            matches!(&items[1],Item::Diagnostic(d) if d.kind==DiagnosticKind::Loss && d.lost==1)
        );
        assert!(input.next().is_none());
    }
    #[test]
    fn gaps_reset_partial_messages_and_are_not_reported_twice_on_close() {
        let mut input = input(transport::Encoding::Bytes).with_channel(2).unwrap();
        input.queue.push(packet(0, &[0x90, 60]));
        input.queue.push(packet(2, &[90, 0x91, 64, 70]));
        input.queue.state.lock().unwrap().lost = 1;
        input.close().unwrap();
        let items: Vec<_> = input
            .with_errors()
            .collect::<crate::midi::Result<_>>()
            .unwrap();
        assert_eq!(
            items
                .iter()
                .filter(|i| matches!(i,Item::Diagnostic(d) if d.kind==DiagnosticKind::Loss))
                .count(),
            1
        );
        let notes: Vec<_> = items
            .iter()
            .filter_map(|i| match i {
                Item::Message(m) => match m.as_ref() {
                    Payload::Message(AnyMessage::NoteOn(n)) => Some(n.note()),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(notes, [64]);
    }
    #[test]
    fn errors_iterator_retains_malformed_and_truncated_diagnostics() {
        let mut input = input(transport::Encoding::Bytes).with_channels([]).unwrap();
        input.queue.push(packet(0, &[1, 0x90, 60]));
        input.close().unwrap();
        let items: Vec<_> = input
            .with_errors()
            .collect::<crate::midi::Result<_>>()
            .unwrap();
        assert!(items
            .iter()
            .any(|i| matches!(i,Item::Diagnostic(d) if d.kind==DiagnosticKind::Malformed)));
        assert!(items
            .iter()
            .any(|i| matches!(i,Item::Diagnostic(d) if d.kind==DiagnosticKind::Truncated)));
    }
    #[test]
    fn drop_retires_queue_without_ownership_cycle() {
        let input = input(transport::Encoding::Bytes);
        let weak = Arc::downgrade(&input.queue);
        drop(input);
        assert!(weak.upgrade().is_none());
        let mut input = Core::new();
        input.close().unwrap();
        assert!(input.with_errors().next().is_none());
        assert!(input.client.is_none());
    }
    #[test]
    fn default_iteration_continues_after_malformed_input_from_callback_queue() {
        let mut input = input(transport::Encoding::Bytes);
        let queue = input.queue.clone();
        let producer = std::thread::spawn(move || queue.push(packet(0, &[1, 0x90, 60, 90])));
        assert!(
            matches!(input.next(), Some(Payload::Message(AnyMessage::NoteOn(m))) if m.note()==60)
        );
        producer.join().unwrap();
        input.close().unwrap();
        assert!(input.next().is_none());
    }

    #[test]
    fn memory_recording_stops_without_closing_input() {
        let core = input(transport::Encoding::Bytes);
        core.queue.push(packet(0, &[0x90, 60, 90]));
        let queue = core.queue.clone();
        let mut input = Input::decoded(core);
        let recorder = input
            .start(None::<&std::path::Path>, &Default::default())
            .unwrap();
        let report = input.stop(Duration::from_secs(1)).unwrap();
        assert!(report.complete);
        assert!(!input.closed());
        assert_eq!(input.recording_report(), Some(report));
        let records = recorder.records().unwrap();
        assert_eq!(records.len(), 3);
        queue.push(packet(1, &[0x90, 62, 90]));
        for note in [60, 62] {
            let item = input
                .receive(Some(Duration::from_secs(1)), None)
                .unwrap()
                .unwrap();
            assert!(
                matches!(item, Item::Message(m) if matches!(*m, Payload::Message(AnyMessage::NoteOn(ref m)) if m.note() == note))
            );
        }
        assert_eq!(recorder.records().unwrap(), records);
        input.close().unwrap();
    }

    #[test]
    fn recording_preserves_messages_loss_and_finalization_diagnostics() {
        use crate::archive::Record;
        let input = input(transport::Encoding::Bytes);
        input.queue.push(packet(0, &[0x90, 60, 90]));
        input.queue.push(packet(2, &[0x90, 62]));
        let recording = Input::decoded(input).record(Duration::ZERO).unwrap();
        assert!(matches!(&recording.records[0], Record::Clock(_)));
        assert!(matches!(&recording.records[1], Record::Stream { id, .. } if id=="keyboard"));
        assert!(
            matches!(&recording.records[2], Record::Event(m) if m.timestamp().unwrap().ticks==0)
        );
        assert!(matches!(&recording.records[3], Record::Gap { lost: 1, .. }));
        assert!(matches!(&recording.records[4], Record::Gap { lost: 0, .. }));
        assert_eq!(
            recording
                .clone()
                .into_iter()
                .collect::<io::Result<Vec<_>>>()
                .unwrap(),
            recording.records
        );
    }

    #[test]
    fn recording_retains_terminal_receive_failure_as_a_discontinuity() {
        let input = input(transport::Encoding::Bytes);
        input.queue.push(packet(u64::MAX, &[0x90, 60, 90]));
        let recording = Input::decoded(input).record(Duration::ZERO).unwrap();
        assert!(
            matches!(recording.records.last().unwrap(), crate::archive::Record::Discontinuity { reason, .. } if reason.contains("sequence exhausted"))
        );
        assert_eq!(recording.records.len(), 3);
        assert!(Input::new()
            .record(Duration::MAX)
            .unwrap_err()
            .to_string()
            .contains("clock range"));
    }

    #[test]
    fn recording_times_out_on_silence_and_releases_the_receiver() {
        let input = input(transport::Encoding::Bytes);
        let queue = Arc::downgrade(&input.queue);
        let recording = Input::decoded(input)
            .record(Duration::from_millis(1))
            .unwrap();
        assert_eq!(recording.records.len(), 2);
        assert!(queue.upgrade().is_none());
    }

    #[test]
    fn file_recording_roundtrips_and_requires_explicit_overwrite() {
        let path = std::env::temp_dir().join(format!(
            "middi-record-{}-{}.middi",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = input(transport::Encoding::Bytes);
        first.queue.push(packet(0, &[0x90, 60, 90, 0x80, 60, 0]));
        let recording = Input::decoded(first)
            .record_to(Duration::ZERO, &path, false)
            .unwrap();
        let original = std::fs::read(&path).unwrap();
        assert_eq!(
            crate::archive::Reader::open(&path)
                .unwrap()
                .collect::<io::Result<Vec<_>>>()
                .unwrap(),
            recording.records
        );
        assert!(
            matches!(Input::decoded(input(transport::Encoding::Bytes)).record_to(Duration::ZERO, &path, false).unwrap_err(), crate::midi::Error::Io(e) if e.kind() == io::ErrorKind::AlreadyExists)
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let second = Input::decoded(input(transport::Encoding::Bytes))
            .record_to(Duration::ZERO, &path, true)
            .unwrap();
        assert_eq!(
            crate::archive::Reader::open(&path)
                .unwrap()
                .collect::<io::Result<Vec<_>>>()
                .unwrap(),
            second.records
        );
        assert!(std::fs::metadata(&path).unwrap().len() < original.len() as u64);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn terminal_decode_error_stops_iteration() {
        let mut input = input(transport::Encoding::Bytes);
        input.queue.push(packet(u64::MAX, &[0x90, 60, 90]));
        assert!(input.with_errors().next().unwrap().is_err());
        assert!(input.with_errors().next().is_none());
        assert!(input.closed);
    }
}
