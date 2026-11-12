//! Shared subprocess transport. The standalone worker never initializes Python.
mod wire;

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

/// Deployment settings for a subprocess worker, independent of the application runtime.
/// Configure these before discovery or connection; enabling remains explicit.
#[derive(Clone, Debug)]
pub struct SubprocessOptions {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub timeout: Duration,
}
impl Default for SubprocessOptions {
    fn default() -> Self {
        Self {
            program: PathBuf::from("middy-worker"),
            args: Vec::new(),
            timeout: Duration::from_secs(5),
        }
    }
}

mod queue;
mod server;
pub use server::serve;

use super::{Backend, Error, Packet, Receiver, Result, TimestampMode};
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc, Mutex, Weak,
};
use std::thread;
use std::time::Instant;
use wire::Value;

type Failure = Arc<super::FailureState>;

fn worker_error(error: impl std::fmt::Display) -> Error {
    Error::Worker(error.to_string())
}
fn bad() -> Error {
    Error::Worker("invalid worker protocol value".into())
}
fn tuple(value: Value) -> Result<Vec<Value>> {
    match value {
        Value::Tuple(v) | Value::List(v) => Ok(v),
        _ => Err(bad()),
    }
}
fn string(value: Value) -> Result<String> {
    match value {
        Value::String(v) => Ok(v),
        _ => Err(bad()),
    }
}
fn number(value: Value) -> Result<i128> {
    match value {
        Value::Integer(v) => Ok(v),
        _ => Err(bad()),
    }
}
fn unsigned(value: Value) -> Result<u64> {
    u64::try_from(number(value)?).map_err(worker_error)
}
fn boolean(value: Value) -> Result<bool> {
    match value {
        Value::Bool(v) => Ok(v),
        _ => Err(bad()),
    }
}
fn values<const N: usize>(value: Value) -> Result<[Value; N]> {
    tuple(value)?.try_into().map_err(|_| bad())
}
fn text(value: impl Into<String>) -> Value {
    Value::String(value.into())
}
fn integer(value: impl Into<i128>) -> Value {
    Value::Integer(value.into())
}
fn packet_value(packet: &Packet) -> Value {
    match packet {
        Packet::Bytes(v) => Value::Bytes(v.clone()),
        Packet::Ump(v) => Value::Words(v.clone()),
    }
}
fn packet(value: Value) -> Result<Packet> {
    match value {
        Value::Bytes(v) => Ok(Packet::Bytes(v)),
        Value::Words(v) => Ok(Packet::Ump(v)),
        _ => Err(bad()),
    }
}
fn packet_size(packet: &Packet) -> usize {
    match packet {
        Packet::Bytes(v) => v.len(),
        Packet::Ump(v) => v.len().saturating_mul(4),
    }
}
fn error_value(error: &Error) -> Value {
    let (kind, detail) = match error {
        Error::Capability(v) => ("capability", text(v)),
        Error::Invalid(v) => ("invalid", text(v)),
        Error::Native(v) => ("native", text(v)),
        Error::Worker(v) => ("worker", text(v)),
        Error::Timeout(v) => ("timeout", text(v)),
        Error::Resource(v) => ("resource", text(v)),
        Error::Busy => ("busy", Value::Null),
        Error::Closed => ("closed", Value::Null),
        Error::CallbackThread => ("callback", Value::Null),
        Error::CompoundSend {
            completed_messages,
            total_messages,
            cause,
        } => (
            "compound",
            Value::Tuple(vec![
                completed_messages.map_or(Value::Null, |v| integer(v as u64)),
                integer(*total_messages as u64),
                error_value(cause),
            ]),
        ),
    };
    Value::Tuple(vec![text(kind), detail])
}
fn decode_error(value: Value) -> Result<Error> {
    let [kind, detail] = values(value)?;
    Ok(match string(kind)?.as_str() {
        "capability" => Error::Capability(string(detail)?),
        "invalid" => Error::Invalid(string(detail)?),
        "native" => Error::Native(string(detail)?),
        "worker" => Error::Worker(string(detail)?),
        "timeout" => Error::Timeout(string(detail)?),
        "resource" => Error::Resource(string(detail)?),
        "busy" => Error::Busy,
        "closed" => Error::Closed,
        "callback" => Error::CallbackThread,
        "compound" => {
            let [completed, total, cause] = values(detail)?;
            Error::CompoundSend {
                completed_messages: if completed == Value::Null {
                    None
                } else {
                    Some(usize::try_from(unsigned(completed)?).map_err(worker_error)?)
                },
                total_messages: usize::try_from(unsigned(total)?).map_err(worker_error)?,
                cause: Box::new(decode_error(cause)?),
            }
        }
        _ => return Err(bad()),
    })
}

struct Process {
    reserved: AtomicBool,
    child: Mutex<Child>,
    failure: Failure,
    delivery: Mutex<Option<Arc<queue::Delivery>>>,
}
impl Process {
    fn release(&self) {
        if self.reserved.swap(false, Ordering::AcqRel) {
            WORKERS.fetch_sub(1, Ordering::AcqRel);
        }
    }
    fn reap(&self) {
        loop {
            match self
                .child
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .try_wait()
            {
                Ok(Some(_)) => {
                    self.release();
                    return;
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(_) => return,
            }
        }
    }
    fn fail(&self, error: Error) {
        let mut failure = self.failure.error.lock().unwrap_or_else(|p| p.into_inner());
        if failure.is_none() {
            *failure = Some(error);
        }
        drop(failure);
        if let Some(delivery) = self
            .delivery
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            delivery.retire();
        }
        let _ = self.child.lock().unwrap_or_else(|p| p.into_inner()).kill();
    }
    fn error(&self) -> Option<Error> {
        self.failure
            .error
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let child = self.child.get_mut().unwrap_or_else(|p| p.into_inner());
        let _ = child.kill();
        // The process slot stays reserved until the child has been reaped.
        let _ = child.wait();
        if *self.reserved.get_mut() {
            WORKERS.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

pub(super) struct Session {
    process: Arc<Process>,
    requests: Option<mpsc::SyncSender<Value>>,
    replies: mpsc::Receiver<Result<Value>>,
    timeout: Duration,
    generation: u64,
}
impl Session {
    pub(super) fn start(
        options: SubprocessOptions,
        input: bool,
        backend: Backend,
        mode: TimestampMode,
        limit: usize,
        failure: Failure,
    ) -> Result<Self> {
        if options.timeout.is_zero() || Instant::now().checked_add(options.timeout).is_none() {
            return Err(Error::Invalid(
                "worker timeout must be positive and representable".into(),
            ));
        }
        if WORKERS.fetch_add(1, Ordering::AcqRel) >= 32 {
            WORKERS.fetch_sub(1, Ordering::AcqRel);
            return Err(Error::Resource(
                "subprocess worker limit reached (32 per parent process)".into(),
            ));
        }
        let child = Command::new(&options.program)
            .args(&options.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(v) => v,
            Err(e) => {
                WORKERS.fetch_sub(1, Ordering::AcqRel);
                return Err(worker_error(format!(
                    "cannot start worker {:?}: {e}",
                    options.program
                )));
            }
        };
        let stdin = child.stdin.take().ok_or_else(bad)?;
        let stdout = child.stdout.take().ok_or_else(bad)?;
        let process = Arc::new(Process {
            reserved: AtomicBool::new(true),
            child: Mutex::new(child),
            failure,
            delivery: Mutex::new(None),
        });
        let (requests, receiver) = mpsc::sync_channel(1);
        let weak = Arc::downgrade(&process);
        thread::Builder::new()
            .name("middy worker requests".into())
            .spawn(move || write_requests(stdin, receiver, weak))
            .map_err(worker_error)?;
        let (reply, replies) = mpsc::sync_channel(1);
        let weak = Arc::downgrade(&process);
        thread::Builder::new()
            .name("middy worker results".into())
            .spawn(move || read_results(stdout, reply, weak))
            .map_err(worker_error)?;
        let session = Self {
            process,
            requests: Some(requests),
            replies,
            timeout: options.timeout,
            generation: 0,
        };
        session.request(
            "init",
            vec![
                Value::Bool(input),
                integer(backend as i32),
                Value::Bool(mode == TimestampMode::Absolute),
                integer(limit as u64),
            ],
        )?;
        session.check()?;
        Ok(session)
    }
    fn request(&self, operation: &str, args: Vec<Value>) -> Result<Value> {
        if let Some(error) = self.process.error() {
            return Err(error);
        }
        *self
            .process
            .failure
            .operation
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = operation.to_owned();
        let deadline = Instant::now() + self.timeout;
        let request = Value::Tuple(vec![text(operation), Value::List(args)]);
        match self
            .requests
            .as_ref()
            .ok_or(Error::Closed)?
            .try_send(request)
        {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => return Err(Error::Busy),
            Err(mpsc::TrySendError::Disconnected(_)) => {
                let error = self
                    .process
                    .error()
                    .unwrap_or_else(|| worker_error("worker request thread stopped"));
                self.process.fail(error.clone());
                return Err(error);
            }
        }
        let result = self
            .replies
            .recv_timeout(deadline.saturating_duration_since(Instant::now()));
        match result {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let error = self.process.error().unwrap_or_else(|| {
                    Error::Timeout(format!(
                        "MIDI {operation} exceeded {:?}; create a new client",
                        self.timeout
                    ))
                });
                self.process.fail(error.clone());
                Err(error)
            }
            Err(_) => {
                let error = self
                    .process
                    .error()
                    .unwrap_or_else(|| worker_error("MIDI worker exited"));
                self.process.fail(error.clone());
                Err(error)
            }
        }
    }
    pub(super) fn ports(&self) -> Result<Vec<(String, u64)>> {
        let result = self.request("ports", vec![])?;
        let parsed = (|| {
            tuple(result)?
                .into_iter()
                .map(|value| {
                    let [name, id] = values(value)?;
                    Ok((string(name)?, unsigned(id)?))
                })
                .collect::<Result<Vec<_>>>()
        })();
        if let Err(error) = &parsed {
            self.process.fail(error.clone());
        }
        self.check()?;
        parsed
    }
    pub(super) fn open(
        &mut self,
        index: Option<usize>,
        name: &str,
        receiver: Option<Arc<Receiver>>,
        limit: usize,
    ) -> Result<()> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| Error::Resource("worker generation exhausted".into()))?;
        let delivery = receiver
            .map(|receiver| queue::Delivery::parent(receiver, limit, self.generation))
            .transpose()?;
        *self
            .process
            .delivery
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = delivery;
        let result = self
            .request(
                "open",
                vec![
                    index.map_or(Value::Null, |v| integer(v as u64)),
                    text(name),
                    integer(self.generation),
                ],
            )
            .and_then(|_| self.check());
        if let Err(error) = &result {
            self.process.fail(error.clone());
        }
        result
    }
    pub(super) fn send(&self, packets: &[Packet], compound: bool) -> Result<()> {
        self.request(
            if compound { "send_command" } else { "send" },
            vec![Value::List(packets.iter().map(packet_value).collect())],
        )?;
        if let Some(error) = self.process.error() {
            return Err(if compound {
                Error::CompoundSend {
                    completed_messages: Some(packets.len()),
                    total_messages: packets.len(),
                    cause: Box::new(error),
                }
            } else {
                error
            });
        }
        Ok(())
    }
    pub(super) fn close(&self) -> Result<()> {
        if let Some(delivery) = self
            .process
            .delivery
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            delivery.retire();
        }
        let result = self.request("close", vec![]).map(|_| ());
        if let Some(delivery) = self
            .process
            .delivery
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            delivery.drain();
        }
        result
    }
    pub(super) fn invalidate(&self, operation: &str, error: Error) {
        if self.process.error().is_none() {
            *self
                .process
                .failure
                .operation
                .lock()
                .unwrap_or_else(|p| p.into_inner()) = operation.to_owned();
        }
        self.process.fail(error);
    }
    pub(super) fn check(&self) -> Result<()> {
        self.process.error().map_or(Ok(()), Err)
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if self.process.error().is_none() {
            let _ = self.request("shutdown", vec![]);
        }
        self.requests = None;
        if let Some(delivery) = self
            .process
            .delivery
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            delivery.retire();
        }
    }
}
fn write_requests(stream: impl Write, requests: mpsc::Receiver<Value>, process: Weak<Process>) {
    let mut stream = std::io::BufWriter::new(stream);
    let result = (|| -> std::io::Result<()> {
        stream.write_all(wire::MAGIC)?;
        for request in requests {
            wire::write(&mut stream, &request)?;
            stream.flush()?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        if let Some(process) = process.upgrade() {
            process.fail(worker_error(error));
        }
    }
}
fn read_results(
    stream: impl Read,
    replies: mpsc::SyncSender<Result<Value>>,
    process: Weak<Process>,
) {
    let mut stream = std::io::BufReader::new(stream);
    let result = (|| -> Result<()> {
        let mut magic = [0; 8];
        stream.read_exact(&mut magic).map_err(worker_error)?;
        if &magic != wire::MAGIC {
            return Err(worker_error("worker protocol version mismatch"));
        }
        loop {
            let value = wire::read(&mut stream).map_err(worker_error)?;
            let Some(process) = process.upgrade() else {
                return Ok(());
            };
            let [tag, payload] = values(value)?;
            let tag_name = string(tag)?;
            match tag_name.as_str() {
                "message" => {
                    let [generation, sequence, timestamp, data, dropped] = values(payload)?;
                    if let Some(delivery) = process
                        .delivery
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .as_ref()
                    {
                        if delivery.generation == unsigned(generation)? {
                            delivery.update(unsigned(dropped)?);
                            delivery.push(
                                unsigned(sequence)?,
                                if timestamp == Value::Null {
                                    None
                                } else {
                                    Some(i64::try_from(number(timestamp)?).map_err(worker_error)?)
                                },
                                packet(data)?,
                            );
                        }
                    }
                }
                "dropped" => {
                    let [generation, dropped] = values(payload)?;
                    if let Some(delivery) = process
                        .delivery
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .as_ref()
                    {
                        if delivery.generation == unsigned(generation)? {
                            delivery.update(unsigned(dropped)?);
                        }
                    }
                }
                "error" => {
                    let [error, fatal, dropped] = values(payload)?;
                    let error = decode_error(error)?;
                    if let Some(delivery) = process
                        .delivery
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .as_ref()
                    {
                        delivery.update(unsigned(dropped)?);
                    }
                    if boolean(fatal)? {
                        process.fail(error.clone());
                    }
                    replies.try_send(Err(error)).map_err(worker_error)?;
                }
                "ok" | "shutdown" => {
                    let [value, dropped] = values(payload)?;
                    if let Some(delivery) = process
                        .delivery
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .as_ref()
                    {
                        delivery.update(unsigned(dropped)?);
                    }
                    replies.try_send(Ok(value)).map_err(worker_error)?;
                    if tag_name == "shutdown" {
                        return Ok(());
                    }
                }
                _ => return Err(bad()),
            }
        }
    })();
    if let Err(error) = result {
        if let Some(process) = process.upgrade() {
            process.fail(error.clone());
            let _ = replies.try_send(Err(error));
            process.reap();
        } else {
            let _ = replies.try_send(Err(error));
        }
    }
}
static WORKERS: AtomicUsize = AtomicUsize::new(0);
