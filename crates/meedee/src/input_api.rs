//! Typed input and serial callback delivery over the shared input owner.
use super::{invalid, report, Core, Recording};
use crate::{
    events::{Clock, Payload},
    live::Item,
    midi,
    transport::{self, Port, ReceivedPacket},
};
use std::{
    io,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

type Callback<T> = Box<dyn FnMut(midi::Result<Item<T>>) + Send>;
type Receive<T> = fn(&mut Core, Option<Instant>) -> Option<io::Result<Item<T>>>;

/// A lazy MIDI input. Ordinary input yields decoded messages. `raw()` selects
/// unchanged received packets. Configure callbacks before opening a connection.
///
/// ```no_run
/// use meedee::midi::Input;
/// for packet in Input::new().raw().unwrap() {
///     println!("{:?}", packet.packet);
/// }
/// ```
///
/// Raw input has no channel filter or decoded recording methods.
/// ```compile_fail
/// use meedee::midi::Input;
/// Input::new().raw().unwrap().with_channel(1);
/// ```
/// ```compile_fail
/// use meedee::midi::Input;
/// Input::new().raw().unwrap().record(std::time::Duration::ZERO);
/// ```
pub struct Input<T: Send + 'static = Payload> {
    core: Option<Core>,
    next: Receive<T>,
    callback: Option<Callback<T>>,
    callback_mode: bool,
    dispatch: Option<Dispatch>,
    status: Arc<Mutex<Status>>,
    stream: Option<String>,
    clock: Option<Clock>,
}
struct Dispatch {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<(Core, Option<transport::Error>)>,
}
#[derive(Default)]
struct Status {
    dropped: u64,
    failure: Option<transport::Error>,
}
/// Input iteration including stream diagnostics and terminal errors.
pub struct InputErrors<'a, T: Send + 'static = Payload> {
    input: &'a mut Input<T>,
    finished: bool,
}
impl Default for Input<Payload> {
    fn default() -> Self {
        Self::new()
    }
}
impl Input<Payload> {
    pub fn new() -> Self {
        Self::from_core(Core::new(), Core::next_item)
    }
    pub fn raw(mut self) -> midi::Result<Input<ReceivedPacket>> {
        let core = self.core_mut()?;
        if core.decoder.is_some() || core.closed || self.callback.is_some() {
            return Err(invalid("select raw mode before opening or configuring callbacks").into());
        }
        let mut core = self.core.take().expect("unopened input");
        if core.channels.is_some() {
            return Err(invalid("channel filtering requires decoded input").into());
        }
        core.raw = true;
        Ok(Input::from_core(core, Core::next_raw))
    }
    pub fn with_channel(mut self, channel: i64) -> midi::Result<Self> {
        self.core = Some(self.take_core()?.with_channel(channel)?);
        Ok(self)
    }
    pub fn with_channels(mut self, channels: impl IntoIterator<Item = i64>) -> midi::Result<Self> {
        self.core = Some(self.take_core()?.with_channels(channels)?);
        Ok(self)
    }
    pub fn record(mut self, duration: Duration) -> midi::Result<Recording> {
        transport::ensure_control_thread().map_err(midi::Error::Transport)?;
        if self.callback_mode {
            return Err(invalid("recording is unavailable in callback mode").into());
        }
        self.take_core()?.record(duration)
    }
    pub fn record_to(
        mut self,
        duration: Duration,
        path: impl AsRef<Path>,
        overwrite: bool,
    ) -> midi::Result<Recording> {
        transport::ensure_control_thread().map_err(midi::Error::Transport)?;
        if self.callback_mode {
            return Err(invalid("recording is unavailable in callback mode").into());
        }
        self.take_core()?.record_to(duration, path, overwrite)
    }
}
impl<T: Send + 'static> Input<T> {
    fn from_core(core: Core, next: Receive<T>) -> Self {
        Self {
            core: Some(core),
            next,
            callback: None,
            callback_mode: false,
            dispatch: None,
            status: Arc::new(Mutex::new(Status::default())),
            stream: None,
            clock: None,
        }
    }
    fn core_mut(&mut self) -> midi::Result<&mut Core> {
        self.core
            .as_mut()
            .ok_or_else(|| invalid("operation is unavailable in callback mode").into())
    }
    fn take_core(&mut self) -> midi::Result<Core> {
        self.core
            .take()
            .ok_or_else(|| invalid("operation is unavailable in callback mode").into())
    }
    pub fn with_port(mut self, pattern: impl Into<String>) -> midi::Result<Self> {
        self.core = Some(self.take_core()?.with_port(pattern)?);
        Ok(self)
    }
    pub fn with_transport(mut self, options: transport::InputOptions) -> midi::Result<Self> {
        self.core = Some(self.take_core()?.with_transport(options)?);
        Ok(self)
    }
    pub fn with_receive_limits(
        mut self,
        packets: usize,
        bytes: usize,
        sysex: usize,
    ) -> midi::Result<Self> {
        self.core = Some(
            self.take_core()?
                .with_receive_limits(packets, bytes, sysex)?,
        );
        Ok(self)
    }
    /// Deliver data, diagnostics and terminal errors serially on a dispatch thread.
    /// The callback must return promptly. Blocking MIDI operations are rejected there.
    pub fn with_callback(
        mut self,
        callback: impl FnMut(midi::Result<Item<T>>) + Send + 'static,
    ) -> midi::Result<Self> {
        let core = self.core_mut()?;
        if core.decoder.is_some() || core.closed || self.callback.is_some() {
            return Err(invalid("configure one callback before opening input").into());
        }
        self.callback_mode = true;
        self.callback = Some(Box::new(callback));
        Ok(self)
    }
    pub fn ports(&mut self) -> midi::Result<Vec<Port>> {
        transport::ensure_control_thread().map_err(midi::Error::Transport)?;
        self.core_mut()?.ports()
    }
    pub fn connect(mut self, port: &Port) -> midi::Result<Self> {
        transport::ensure_control_thread().map_err(midi::Error::Transport)?;
        self.core_mut()?.open_connection(Some(port), "")?;
        self.start_dispatch()?;
        Ok(self)
    }
    pub fn create_virtual(mut self, name: &str) -> midi::Result<Self> {
        transport::ensure_control_thread().map_err(midi::Error::Transport)?;
        self.core_mut()?.open_connection(None, name)?;
        self.start_dispatch()?;
        Ok(self)
    }
    pub fn stream(&mut self) -> midi::Result<&str> {
        transport::ensure_control_thread().map_err(midi::Error::Transport)?;
        if self.dispatch.is_none() {
            self.core_mut()?.stream()?;
            self.start_dispatch()?;
        }
        if self.dispatch.is_some() {
            Ok(self.stream.as_deref().expect("opened callback stream"))
        } else {
            self.core_mut()?.stream()
        }
    }
    pub fn clock(&mut self) -> midi::Result<&Clock> {
        self.stream()?;
        if self.dispatch.is_some() {
            Ok(self.clock.as_ref().expect("opened callback clock"))
        } else {
            self.core_mut()?.clock()
        }
    }
    pub fn dropped_packets(&self) -> u64 {
        self.core.as_ref().map_or_else(
            || {
                self.status
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .dropped
            },
            Core::dropped_packets,
        )
    }
    pub fn check_health(&self) -> midi::Result<()> {
        if let Some(error) = self
            .status
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .failure
            .clone()
        {
            return Err(midi::Error::Transport(error));
        }
        if let Some(core) = &self.core {
            core.check_health()
        } else {
            Ok(())
        }
    }
    pub fn receive(&mut self, timeout: Option<Duration>) -> midi::Result<Option<Item<T>>> {
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
        let next = self.next;
        let core = self.core_mut()?;
        let value = next(core, deadline).transpose();
        if value.is_err() {
            let _ = core.close();
        }
        value.map_err(midi::Error::from)
    }
    pub fn with_errors(&mut self) -> InputErrors<'_, T> {
        InputErrors {
            input: self,
            finished: false,
        }
    }
    /// Stop delivery and wait for the current callback and accepted input to drain.
    pub fn close(&mut self) -> midi::Result<()> {
        transport::ensure_control_thread().map_err(midi::Error::Transport)?;
        if let Some(dispatch) = self.dispatch.take() {
            dispatch.stop.store(true, Ordering::Release);
            let (core, close_error) = dispatch.thread.join().map_err(|_| {
                midi::Error::Transport(transport::Error::Worker(
                    "input dispatch thread panicked".into(),
                ))
            })?;
            self.core = Some(core);
            if let Some(error) = close_error {
                return Err(midi::Error::Transport(error));
            }
        }
        self.core_mut()?.close()
    }
    fn start_dispatch(&mut self) -> midi::Result<()> {
        if self.callback.is_none() {
            return Ok(());
        }
        let mut core = self.take_core()?;
        self.stream = core.stream.clone();
        self.clock = core.clock.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let status = self.status.clone();
        let next = self.next;
        let mut callback = self.callback.take().expect("configured callback");
        let thread = thread::Builder::new()
            .name("meedee input dispatch".into())
            .spawn(move || {
                let mut failed = false;
                let mut close_error = None;
                loop {
                    if stopped.load(Ordering::Acquire) && !core.closed {
                        if let Err(error) = core.close() {
                            let failure = transport_failure(&error);
                            status
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .failure
                                .get_or_insert(failure.clone());
                            close_error = Some(failure);
                            if core.raw {
                                core.raw_pending.push_back(Err(error.into()));
                            } else {
                                core.pending.push_back(Err(error.into()));
                            }
                        }
                    }
                    let item = next(&mut core, Some(Instant::now() + Duration::from_millis(50)));
                    status.lock().unwrap_or_else(|p| p.into_inner()).dropped =
                        core.dropped_packets();
                    if let Some(item) = item {
                        let terminal = item.is_err();
                        if let Err(error) = &item {
                            status.lock().unwrap_or_else(|p| p.into_inner()).failure =
                                Some(input_failure(error));
                        }
                        let delivered =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                let _scope = transport::CallbackScope::enter();
                                callback(item.map_err(midi::Error::from));
                            }));
                        if delivered.is_err() {
                            status.lock().unwrap_or_else(|p| p.into_inner()).failure =
                                Some(transport::Error::Worker("input callback panicked".into()));
                            failed = true;
                            break;
                        }
                        if terminal {
                            failed = true;
                            break;
                        }
                    } else if core.closed {
                        break;
                    }
                }
                if let Err(error) = core.close() {
                    let failure = transport_failure(&error);
                    status
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .failure
                        .get_or_insert(failure.clone());
                    close_error.get_or_insert(failure);
                }
                if failed {
                    core.pending.clear();
                    core.raw_pending.clear();
                }
                status.lock().unwrap_or_else(|p| p.into_inner()).dropped = core.dropped_packets();
                (core, close_error)
            })
            .map_err(midi::Error::Io)?;
        self.dispatch = Some(Dispatch { stop, thread });
        Ok(())
    }
}
fn input_failure(error: &io::Error) -> transport::Error {
    if let Some(cause) = error.get_ref() {
        if let Some(cause) = cause.downcast_ref::<midi::Error>() {
            return transport_failure(cause);
        }
        if let Some(cause) = cause.downcast_ref::<transport::Error>() {
            return cause.clone();
        }
    }
    transport::Error::Worker(error.to_string())
}
fn transport_failure(error: &midi::Error) -> transport::Error {
    match error {
        midi::Error::Transport(e)
        | midi::Error::Connect(e)
        | midi::Error::Init(e)
        | midi::Error::Send(e) => e.clone(),
        _ => transport::Error::Worker(error.to_string()),
    }
}
impl<T: Send + 'static> Iterator for Input<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        loop {
            match self.receive(None) {
                Ok(Some(Item::Message(value))) => return Some(*value),
                Ok(Some(Item::Diagnostic(d))) => report(&format!("{d:?}")),
                Ok(None) => return None,
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
        match self.input.receive(None) {
            Ok(Some(item)) => Some(Ok(item)),
            Ok(None) => {
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
        if self.core.is_some() || self.dispatch.is_some() {
            if let Err(error) = self.close() {
                report(&error);
            }
        }
    }
}
