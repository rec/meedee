//! Typed input, background recording and serial callback delivery.
use super::{invalid, report, Core};
use crate::{
    archive::{Record, Recorder, RecordingReport},
    events::{Clock, Payload},
    live::Item,
    midi,
    playback::Cancellation,
    transport::{self, Port, ReceivedPacket},
};
use std::{
    collections::VecDeque,
    io,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

type Callback<T> = Box<dyn FnMut(midi::Result<Item<T>>) + Send>;
type Receive<T> = fn(&mut Core, Option<Instant>) -> Option<io::Result<Item<T>>>;

/// A lazy input yielding decoded messages; raw() selects unchanged packets.
/// Callbacks and attached recording use the same bounded background dispatcher.
/// Raw input has no decoded recording or channel-filtering methods.
/// ```no_run
/// use middy::midi::Input;
/// for packet in Input::new().raw().unwrap() {
///     println!("{:?}", packet.packet);
/// }
/// ```
/// ```compile_fail
/// use middy::midi::Input;
/// Input::new().raw().unwrap().with_channel(1);
/// ```
/// ```compile_fail
/// use middy::midi::Input;
/// Input::new().raw().unwrap().record(std::time::Duration::ZERO);
/// ```
/// ```compile_fail
/// use middy::midi::Input;
/// Input::new().raw().unwrap().start(Some("rec.middy"), &Default::default());
/// ```
pub struct Input<T: Send + 'static = Payload> {
    core: Arc<Mutex<Core>>,
    next: Receive<T>,
    size: fn(&Item<T>) -> usize,
    callback: Option<Callback<T>>,
    pub(super) callback_mode: bool,
    dispatch: Option<Dispatch>,
    delivery: Arc<Delivery<T>>,
    stream: Option<String>,
    clock: Option<Clock>,
}
struct Dispatch {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<Option<Arc<midi::Error>>>,
}
struct Delivery<T> {
    state: Mutex<State<T>>,
    changed: Condvar,
}
struct State<T> {
    pending: VecDeque<(Item<T>, usize)>,
    bytes: usize,
    closed: bool,
    failure: Option<Arc<midi::Error>>,
}
impl<T> Default for State<T> {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            bytes: 0,
            closed: false,
            failure: None,
        }
    }
}
/// Borrowed iteration including diagnostics and one terminal failure.
pub struct InputErrors<'a, T: Send + 'static = Payload> {
    input: &'a mut Input<T>,
    finished: bool,
}
/// Attached recording controls, corresponding to Python start keywords.
#[derive(Clone, Debug)]
pub struct RecordingOptions {
    pub label: String,
    pub best_effort: bool,
    pub payload_limit: usize,
    pub record_limit: usize,
    pub byte_limit: usize,
    pub metrics: bool,
}
impl Default for RecordingOptions {
    fn default() -> Self {
        Self {
            label: String::new(),
            best_effort: false,
            payload_limit: 1_048_576,
            record_limit: 128,
            byte_limit: 8_388_608,
            metrics: false,
        }
    }
}
impl Default for Input<Payload> {
    fn default() -> Self {
        Self::new()
    }
}
impl Input<Payload> {
    pub fn new() -> Self {
        Self::decoded(Core::new())
    }
    pub(super) fn decoded(core: Core) -> Self {
        Self::from_core(core, Core::next_item, |item| match item {
            Item::Diagnostic(d) => d.reason.len(),
            Item::Message(p) => match p.as_ref() {
                Payload::Message(p) => p.encoded_len(),
                Payload::Ump(p) => {
                    use crate::ump::UmpValue;
                    p.encode_words().map_or(0, |w| w.len() * 4)
                }
                Payload::Command(_) => 64,
            },
        })
    }
    pub fn raw(mut self) -> midi::Result<Input<ReceivedPacket>> {
        let core = self.take_core()?;
        if core.decoder.is_some() || core.closed || self.callback.is_some() {
            return Err(invalid("select raw mode before opening or configuring callbacks").into());
        }
        if core.channels.is_some() {
            return Err(invalid("channel filtering requires decoded input").into());
        }
        let mut core = core;
        core.raw = true;
        Ok(Input::from_core(core, Core::next_raw, |item| match item {
            Item::Diagnostic(d) => d.reason.len(),
            Item::Message(p) => match &p.packet {
                transport::Packet::Bytes(p) => p.len(),
                transport::Packet::Ump(p) => p.len() * 4,
            },
        }))
    }
    pub fn with_channel(mut self, channel: i64) -> midi::Result<Self> {
        let core = self.take_core()?.with_channel(channel)?;
        self.core = Arc::new(Mutex::new(core));
        Ok(self)
    }
    pub fn with_channels(mut self, channels: impl IntoIterator<Item = i64>) -> midi::Result<Self> {
        let core = self.take_core()?.with_channels(channels)?;
        self.core = Arc::new(Mutex::new(core));
        Ok(self)
    }
    /// Record independently of receive calls. None retains unbounded memory records;
    /// Some(path) uses a bounded background archive writer. Stop through the input
    /// to finalize diagnostics and detach while keeping the connection open.
    pub fn start(
        &mut self,
        path: Option<impl AsRef<Path>>,
        options: &RecordingOptions,
    ) -> midi::Result<Arc<Recorder>> {
        transport::ensure_control_thread().map_err(midi::Error::Transport)?;
        if self.closed() {
            return Err(midi::Error::Closed);
        }
        self.stream()?;
        let (clock, stream) = {
            let core = self.core.lock().unwrap_or_else(|p| p.into_inner());
            if core.closed {
                return Err(midi::Error::Closed);
            }
            if core.recording.is_some() {
                return Err(invalid("recording is already active").into());
            }
            (
                core.clock.clone().expect("opened clock"),
                core.stream.clone().expect("opened stream"),
            )
        };
        let descriptors = vec![
            Record::Clock(clock.clone()),
            Record::Stream {
                id: stream,
                label: options.label.clone(),
                clock_identity: path.as_ref().map(|_| clock.identity().into()),
            },
        ];
        let recorder = Arc::new(if let Some(path) = path {
            Recorder::new(
                std::fs::File::create(path)?,
                descriptors,
                options.payload_limit,
                options.record_limit,
                options.byte_limit,
                options.best_effort,
                options.metrics,
            )?
        } else {
            Recorder::memory(descriptors, options.metrics)?
        });
        let attached = {
            let mut core = self.core.lock().unwrap_or_else(|p| p.into_inner());
            let attached = core.attach_recording(recorder.clone());
            if attached.is_ok() && recorder.is_memory() {
                let delivery = self
                    .delivery
                    .state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                for (item, _) in &delivery.pending {
                    core.record_item(item);
                }
            }
            attached
        };
        if let Err(error) = attached {
            recorder.fail(error.to_string());
            if let Err(cleanup) = recorder.stop(Duration::from_secs(5)) {
                report(&cleanup);
            }
            return Err(error);
        }
        if let Err(error) = self.start_dispatch() {
            recorder.fail(error.to_string());
            if let Err(cleanup) = finish_recording(&self.core, Duration::from_secs(5)) {
                report(&cleanup);
            }
            return Err(error);
        }
        Ok(recorder)
    }
}
impl<T: Send + 'static> Input<T> {
    fn from_core(core: Core, next: Receive<T>, size: fn(&Item<T>) -> usize) -> Self {
        Self {
            core: Arc::new(Mutex::new(core)),
            next,
            size,
            callback: None,
            callback_mode: false,
            dispatch: None,
            delivery: Arc::new(Delivery {
                state: Mutex::new(State::default()),
                changed: Condvar::new(),
            }),
            stream: None,
            clock: None,
        }
    }
    fn take_core(&mut self) -> midi::Result<Core> {
        if self.dispatch.is_some() {
            return Err(invalid("input is already dispatching").into());
        }
        Ok(std::mem::take(
            &mut *self.core.lock().unwrap_or_else(|p| p.into_inner()),
        ))
    }
    pub fn with_port(mut self, pattern: impl Into<String>) -> midi::Result<Self> {
        let core = self.take_core()?.with_port(pattern)?;
        self.core = Arc::new(Mutex::new(core));
        Ok(self)
    }
    pub fn with_run_in_subprocess(mut self, enabled: bool) -> midi::Result<Self> {
        let core = self.take_core()?.with_run_in_subprocess(enabled)?;
        self.core = Arc::new(Mutex::new(core));
        Ok(self)
    }
    pub fn with_subprocess_options(
        mut self,
        options: transport::subprocess::SubprocessOptions,
    ) -> midi::Result<Self> {
        let core = self.take_core()?.with_subprocess_options(options)?;
        self.core = Arc::new(Mutex::new(core));
        Ok(self)
    }
    pub fn with_transport(mut self, options: transport::InputOptions) -> midi::Result<Self> {
        let core = self.take_core()?.with_transport(options)?;
        self.core = Arc::new(Mutex::new(core));
        Ok(self)
    }
    pub fn with_receive_limits(
        mut self,
        packets: usize,
        bytes: usize,
        sysex: usize,
    ) -> midi::Result<Self> {
        let core = self
            .take_core()?
            .with_receive_limits(packets, bytes, sysex)?;
        self.core = Arc::new(Mutex::new(core));
        Ok(self)
    }
    /// Configure a serial callback before opening. Panic stops input permanently.
    pub fn with_callback(
        mut self,
        callback: impl FnMut(midi::Result<Item<T>>) + Send + 'static,
    ) -> midi::Result<Self> {
        {
            let core = self.core.lock().unwrap_or_else(|p| p.into_inner());
            if core.decoder.is_some() || core.closed || self.callback.is_some() {
                return Err(invalid("configure one callback before opening input").into());
            }
        }
        self.callback_mode = true;
        self.callback = Some(Box::new(callback));
        Ok(self)
    }
    pub fn ports(&mut self) -> midi::Result<Vec<Port>> {
        transport::ensure_control_thread().map_err(midi::Error::Transport)?;
        self.core.lock().unwrap_or_else(|p| p.into_inner()).ports()
    }
    pub fn connect(mut self, port: &Port) -> midi::Result<Self> {
        transport::ensure_control_thread().map_err(midi::Error::Transport)?;
        self.core
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .open_connection(Some(port), "")?;
        self.cache_metadata();
        self.start_dispatch()?;
        Ok(self)
    }
    pub fn create_virtual(mut self, name: &str) -> midi::Result<Self> {
        transport::ensure_control_thread().map_err(midi::Error::Transport)?;
        self.core
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .open_connection(None, name)?;
        self.cache_metadata();
        self.start_dispatch()?;
        Ok(self)
    }
    fn cache_metadata(&mut self) {
        let core = self.core.lock().unwrap_or_else(|p| p.into_inner());
        self.stream = core.stream.clone();
        self.clock = core.clock.clone();
    }
    pub fn closed(&self) -> bool {
        let closed = self
            .delivery
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .closed;
        closed || self.core.lock().unwrap_or_else(|p| p.into_inner()).closed
    }
    pub fn failure(&self) -> Option<Arc<midi::Error>> {
        self.delivery
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .failure
            .clone()
    }
    pub fn stream(&mut self) -> midi::Result<&str> {
        if self.stream.is_none() {
            {
                let mut core = self.core.lock().unwrap_or_else(|p| p.into_inner());
                if core.stream.is_none() {
                    transport::ensure_control_thread().map_err(midi::Error::Transport)?;
                    core.stream()?;
                }
            }
            self.cache_metadata();
            if !self.closed() {
                self.start_dispatch()?;
            }
        }
        Ok(self.stream.as_deref().expect("opened stream"))
    }
    pub fn clock(&mut self) -> midi::Result<&Clock> {
        self.stream()?;
        Ok(self.clock.as_ref().expect("opened clock"))
    }
    pub fn dropped_packets(&self) -> u64 {
        self.core
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .dropped_packets()
    }
    /// Host ingress activity, independent of channel filters and consumers.
    pub fn last_message_time(&self) -> Option<Instant> {
        let core = self.core.lock().unwrap_or_else(|p| p.into_inner());
        let value = core
            .queue
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .last_message_time;
        value
    }
    pub fn recording_report(&self) -> Option<RecordingReport> {
        self.core
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .recording_report
            .clone()
    }
    /// Timeout retains the stopping handle; retry to obtain the final report.
    pub fn stop(&mut self, timeout: Duration) -> midi::Result<RecordingReport> {
        transport::ensure_control_thread().map_err(midi::Error::Transport)?;
        finish_recording(&self.core, timeout)?
            .ok_or_else(|| invalid("no recording is attached").into())
    }
    pub fn check_health(&self) -> midi::Result<()> {
        if let Some(error) = self.failure() {
            return Err(midi::Error::Shared(error));
        }
        if self.closed() {
            return Err(midi::Error::Closed);
        }
        self.core
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .check_health()
    }
    /// None denotes timeout only. Cancellation consumes nothing and leaves input usable.
    pub fn receive(
        &mut self,
        timeout: Option<Duration>,
        cancel: Option<&Cancellation>,
    ) -> midi::Result<Option<Item<T>>> {
        transport::ensure_control_thread().map_err(midi::Error::Transport)?;
        if self.callback_mode {
            return Err(invalid("receive and iteration are unavailable in callback mode").into());
        }
        let deadline = timeout
            .map(|t| {
                Instant::now()
                    .checked_add(t)
                    .ok_or_else(|| invalid("receive timeout exceeds clock range"))
            })
            .transpose()?;
        loop {
            if cancel.is_some_and(Cancellation::cancelled) {
                return Err(midi::Error::Cancelled);
            }
            let mut state = self
                .delivery
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some((item, size)) = state.pending.pop_front() {
                state.bytes -= size;
                return Ok(Some(item));
            }
            if let Some(error) = &state.failure {
                return Err(midi::Error::Shared(error.clone()));
            }
            if self.dispatch.is_some() {
                if state.closed {
                    return Err(midi::Error::Closed);
                }
                if deadline.is_some_and(|d| Instant::now() >= d) {
                    return Ok(None);
                }
                let wait = deadline.map_or(Duration::from_millis(50), |d| {
                    d.saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(50))
                });
                drop(
                    self.delivery
                        .changed
                        .wait_timeout(state, wait)
                        .unwrap_or_else(|p| p.into_inner()),
                );
            } else {
                drop(state);
                let poll = deadline.map_or(Instant::now() + Duration::from_millis(50), |d| {
                    d.min(Instant::now() + Duration::from_millis(50))
                });
                let result = {
                    let mut core = self.core.lock().unwrap_or_else(|p| p.into_inner());
                    (self.next)(&mut core, Some(poll)).transpose()
                };
                // Recheck cancellation before consuming: put an observed item back.
                if cancel.is_some_and(Cancellation::cancelled) {
                    if let Ok(Some(item)) = result {
                        let size = (self.size)(&item);
                        let mut state = self
                            .delivery
                            .state
                            .lock()
                            .unwrap_or_else(|p| p.into_inner());
                        state.bytes += size;
                        state.pending.push_back((item, size));
                    } else if let Err(error) = result {
                        let _ = self.core.lock().unwrap_or_else(|p| p.into_inner()).close();
                        self.retain(error.into());
                    }
                    return Err(midi::Error::Cancelled);
                }
                // A cancelled direct poll may have retained an item on an earlier call.
                match result {
                    Ok(Some(item)) => return Ok(Some(item)),
                    Err(error) => {
                        let _ = self.core.lock().unwrap_or_else(|p| p.into_inner()).close();
                        let error = self.retain(error.into());
                        return Err(midi::Error::Shared(error));
                    }
                    Ok(None) => {
                        if let Some(error) = self.failure() {
                            return Err(midi::Error::Shared(error));
                        }
                        if self.closed() {
                            return Err(midi::Error::Closed);
                        }
                        if deadline.is_some_and(|d| Instant::now() >= d) {
                            return Ok(None);
                        }
                    }
                }
            }
        }
    }
    fn retain(&self, error: midi::Error) -> Arc<midi::Error> {
        let error = retained(error);
        let mut state = self
            .delivery
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        state.closed = true;
        state.failure.get_or_insert_with(|| error.clone());
        self.delivery.changed.notify_all();
        error
    }
    pub fn with_errors(&mut self) -> InputErrors<'_, T> {
        InputErrors {
            input: self,
            finished: false,
        }
    }
    pub fn close(&mut self) -> midi::Result<()> {
        transport::ensure_control_thread().map_err(midi::Error::Transport)?;
        self.delivery
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .closed = true;
        let mut failure = None;
        let memory_recording = self
            .core
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .recording
            .as_ref()
            .is_some_and(|a| a.recorder.is_memory());
        let mut finalized = false;
        if let Some(dispatch) = self.dispatch.take() {
            dispatch.stop.store(true, Ordering::Release);
            match dispatch.thread.join() {
                Ok(error) => {
                    failure = error;
                    finalized = true;
                }
                Err(_) => {
                    let error = self.retain(midi::Error::Transport(transport::Error::Worker(
                        "input dispatch thread panicked".into(),
                    )));
                    if let Some(attachment) = &self
                        .core
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .recording
                    {
                        attachment.recorder.fail(error.to_string());
                    }
                    failure = Some(error);
                }
            }
        }
        let closed = self.core.lock().unwrap_or_else(|p| p.into_inner()).close();
        if let Err(error) = closed {
            if let Some(attachment) = &self
                .core
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .recording
            {
                attachment.recorder.fail(error.to_string());
            }
            failure.get_or_insert_with(|| self.retain(error));
        }
        if !finalized {
            match finish_recording(&self.core, Duration::from_secs(5)) {
                Ok(Some(report)) if !report.complete && !memory_recording => {
                    failure.get_or_insert_with(|| {
                        self.retain(io::Error::other(report.failure.unwrap_or_default()).into())
                    });
                }
                Err(error) => {
                    failure.get_or_insert_with(|| self.retain(error));
                }
                _ => {}
            }
        }
        self.delivery.changed.notify_all();
        if let Some(error) = failure {
            return Err(midi::Error::Shared(error));
        }
        Ok(())
    }
    fn start_dispatch(&mut self) -> midi::Result<()> {
        if self.dispatch.is_some() {
            return Ok(());
        }
        let (recording, packets, bytes) = {
            let core = self.core.lock().unwrap_or_else(|p| p.into_inner());
            (
                core.recording.is_some(),
                core.queue.packet_limit,
                core.queue.byte_limit,
            )
        };
        if self.callback.is_none() && !recording {
            return Ok(());
        }
        let core = self.core.clone();
        let delivery = self.delivery.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let next = self.next;
        let size_of = self.size;
        let mut callback = self.callback.take();
        let run = move || {
            let mut close_error = None;
            let mut callback_failed = false;
            let mut terminal_failure = None;
            loop {
                let item = {
                    let mut core = core.lock().unwrap_or_else(|p| p.into_inner());
                    if stopped.load(Ordering::Acquire) && !core.closed {
                        if let Err(error) = core.close() {
                            close_error = Some(retained(error));
                        }
                    }
                    next(&mut core, Some(Instant::now()))
                };
                if let Some(item) = item {
                    let item = item.map_err(|e| midi::Error::Shared(retained(e.into())));
                    let terminal = item.as_ref().err().and_then(|e| {
                        if let midi::Error::Shared(e) = e {
                            Some(e.clone())
                        } else {
                            None
                        }
                    });
                    if let Some(error) = &terminal {
                        terminal_failure = Some(error.clone());
                        let mut state = delivery.state.lock().unwrap_or_else(|p| p.into_inner());
                        state.failure.get_or_insert_with(|| error.clone());
                        state.closed = true;
                    }
                    if let Some(callback) = &mut callback {
                        let delivered =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                let _scope = transport::CallbackScope::enter();
                                callback(item);
                            }));
                        if delivered.is_err() {
                            let error = Arc::new(midi::Error::Transport(transport::Error::Worker(
                                "input callback panicked".into(),
                            )));
                            report(&error);
                            let mut state =
                                delivery.state.lock().unwrap_or_else(|p| p.into_inner());
                            state.closed = true;
                            state.failure.get_or_insert(error.clone());
                            terminal_failure = Some(error);
                            callback_failed = true;
                            break;
                        }
                    } else if let Ok(item) = item {
                        let size = size_of(&item);
                        let mut state = delivery.state.lock().unwrap_or_else(|p| p.into_inner());
                        if state.failure.is_none() {
                            if state.pending.len() >= packets
                                || size > bytes.saturating_sub(state.bytes)
                            {
                                state.closed = true;
                                state.failure = Some(Arc::new(midi::Error::Transport(transport::Error::Resource(
                                    "input receive queue overflowed; recording has independent admission".into()))));
                            } else {
                                state.bytes += size;
                                state.pending.push_back((item, size));
                            }
                        }
                    }
                    delivery.changed.notify_all();
                    if terminal.is_some() {
                        break;
                    }
                } else {
                    let queue = {
                        let core = core.lock().unwrap_or_else(|p| p.into_inner());
                        if core.closed {
                            break;
                        }
                        core.queue.clone()
                    };
                    let state = queue.state.lock().unwrap_or_else(|p| p.into_inner());
                    if state.packets.is_empty() {
                        drop(
                            queue
                                .changed
                                .wait_timeout(state, Duration::from_millis(20))
                                .unwrap_or_else(|p| p.into_inner()),
                        );
                    }
                }
            }
            {
                let mut core = core.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(failure) = &terminal_failure {
                    if let Some(a) = &core.recording {
                        a.recorder.fail(failure.to_string());
                    }
                }
                if let Err(error) = core.close() {
                    close_error.get_or_insert_with(|| retained(error));
                }
            }
            if let Some(error) = &close_error {
                if let Some(callback) = callback.as_mut().filter(|_| !callback_failed) {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let _scope = transport::CallbackScope::enter();
                        callback(Err(midi::Error::Shared(error.clone())));
                    }));
                }
            }
            let memory_recording = core
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .recording
                .as_ref()
                .is_some_and(|a| a.recorder.is_memory());
            if let Some(error) = &close_error {
                if let Some(attachment) = &core.lock().unwrap_or_else(|p| p.into_inner()).recording
                {
                    attachment.recorder.fail(error.to_string());
                }
            }
            match finish_recording(&core, Duration::from_secs(5)) {
                Ok(Some(report)) if !report.complete && !memory_recording => {
                    close_error.get_or_insert_with(|| {
                        Arc::new(midi::Error::Io(io::Error::other(
                            report.failure.unwrap_or_default(),
                        )))
                    });
                }
                Err(error) => {
                    close_error.get_or_insert_with(|| retained(error));
                }
                _ => {}
            }
            let mut state = delivery.state.lock().unwrap_or_else(|p| p.into_inner());
            state.closed = true;
            if let Some(error) = &close_error {
                state.failure.get_or_insert_with(|| error.clone());
            }
            delivery.changed.notify_all();
            close_error
        };
        let thread = thread::Builder::new()
            .name("middy input dispatch".into())
            .spawn(run)
            .map_err(midi::Error::Io)?;
        self.dispatch = Some(Dispatch { stop, thread });
        Ok(())
    }
}
fn finish_recording(
    core: &Arc<Mutex<Core>>,
    timeout: Duration,
) -> midi::Result<Option<RecordingReport>> {
    if timeout.is_zero() || Instant::now().checked_add(timeout).is_none() {
        return Err(invalid("timeout must be positive and within clock range").into());
    }
    let recorder = {
        let mut core = core.lock().unwrap_or_else(|p| p.into_inner());
        core.stop_recording_admission()
    };
    let Some(recorder) = recorder else {
        return Ok(None);
    };
    let report = recorder.stop(timeout)?;
    let mut core = core.lock().unwrap_or_else(|p| p.into_inner());
    core.recording_report = Some(report.clone());
    core.recording = None;
    Ok(Some(report))
}
fn retained(error: midi::Error) -> Arc<midi::Error> {
    match error {
        midi::Error::Shared(error) => error,
        error => Arc::new(error),
    }
}
impl<T: Send + 'static> Iterator for Input<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        loop {
            match self.receive(None, None) {
                Ok(Some(Item::Message(value))) => return Some(*value),
                Ok(Some(Item::Diagnostic(d))) => report(&format!("{d:?}")),
                Ok(None) => continue,
                Err(midi::Error::Closed) => return None,
                Err(e) => {
                    report(&e);
                    return None;
                }
            }
        }
    }
}
impl<T: Send + 'static> Iterator for InputErrors<'_, T> {
    type Item = midi::Result<Item<T>>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        match self.input.receive(None, None) {
            Ok(Some(item)) => Some(Ok(item)),
            Ok(None) => None,
            Err(midi::Error::Closed) => {
                self.finished = true;
                None
            }
            Err(e) => {
                self.finished = true;
                Some(Err(e))
            }
        }
    }
}
impl<T: Send + 'static> Drop for Input<T> {
    fn drop(&mut self) {
        if let Some(dispatch) = &self.dispatch {
            dispatch.stop.store(true, Ordering::Release);
        }
        if let Err(error) = self.close() {
            report(&error);
        }
    }
}
