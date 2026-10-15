//! Bounded asynchronous archive recording. Native callbacks only admit owned records.
use crate::{
    archive::{self, Record},
    file_io::invalid,
};
use std::io::{self, Write};
use std::sync::{
    mpsc::{self, RecvTimeoutError, TrySendError},
    Arc, Condvar, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub complete: bool,
    pub records: u64,
    pub gap_records: u64,
    pub failure: Option<String>,
}
struct Admission {
    used: usize,
    records: usize,
    stopping: bool,
    finalizing: bool,
    failure: Option<String>,
    result: Option<Report>,
}
struct Shared {
    state: Mutex<Admission>,
    changed: Condvar,
}
pub struct Recorder {
    shared: Arc<Shared>,
    sender: mpsc::SyncSender<(Record, usize)>,
    record_limit: usize,
    byte_limit: usize,
    payload_limit: usize,
    best_effort: bool,
}
impl Recorder {
    pub fn new(
        writer: impl Write + Send + 'static,
        descriptors: Vec<Record>,
        payload_limit: usize,
        record_limit: usize,
        byte_limit: usize,
        best_effort: bool,
    ) -> io::Result<Self> {
        if record_limit == 0 || byte_limit == 0 || payload_limit == 0 {
            return Err(invalid(0, "recording limits must be positive"));
        }
        let mut writer = archive::Writer::new(writer, payload_limit)?;
        for d in &descriptors {
            if !matches!(d, Record::Clock(_) | Record::Stream { .. }) {
                return Err(invalid(
                    0,
                    "recording descriptors must be clocks or streams",
                ));
            }
            writer.write(d)?;
        }
        let (sender, receiver) = mpsc::sync_channel::<(Record, usize)>(record_limit);
        let shared = Arc::new(Shared {
            state: Mutex::new(Admission {
                used: 0,
                records: 0,
                stopping: false,
                finalizing: false,
                failure: None,
                result: None,
            }),
            changed: Condvar::new(),
        });
        let worker = Arc::clone(&shared);
        thread::Builder::new()
            .name("meedee recording".into())
            .spawn(move || {
                let mut count = descriptors.len() as u64;
                let mut gaps = 0;
                loop {
                    match receiver.recv_timeout(Duration::from_millis(20)) {
                        Ok((record, size)) => {
                            let gap = matches!(record, Record::Gap { .. });
                            let result = writer.write(&record);
                            let mut s = worker.state.lock().unwrap_or_else(|p| p.into_inner());
                            s.used -= size;
                            s.records -= 1;
                            if let Err(e) = result {
                                s.failure.get_or_insert_with(|| e.to_string());
                                break;
                            }
                            count += 1;
                            gaps += u64::from(gap);
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            let s = worker.state.lock().unwrap_or_else(|p| p.into_inner());
                            if s.stopping || s.failure.is_some() {
                                break;
                            }
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                let failure = {
                    let mut s = worker.state.lock().unwrap_or_else(|p| p.into_inner());
                    s.finalizing = true;
                    s.failure.clone()
                };
                let complete = failure.is_none();
                let result = writer.finish(complete, failure.as_deref().unwrap_or(""));
                let mut s = worker.state.lock().unwrap_or_else(|p| p.into_inner());
                if let Err(e) = result {
                    s.failure.get_or_insert_with(|| e.to_string());
                }
                s.result = Some(Report {
                    complete: s.failure.is_none(),
                    records: count,
                    gap_records: gaps,
                    failure: s.failure.clone(),
                });
                worker.changed.notify_all();
            })?;
        Ok(Self {
            shared,
            sender,
            record_limit,
            byte_limit,
            payload_limit,
            best_effort,
        })
    }
    pub fn record(&self, record: Record) -> bool {
        match &record {
            Record::Gap { reason, .. } if !self.best_effort => {
                self.fail(reason.clone());
                return false;
            }
            Record::Discontinuity { reason, .. } => {
                self.fail(reason.clone());
                return false;
            }
            _ => {}
        }

        let size = match archive::payload_size(&record) {
            Ok(n) => n,
            Err(e) => {
                self.fail(e.to_string());
                return false;
            }
        };
        let mut s = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        if s.stopping || s.failure.is_some() || s.result.is_some() {
            return false;
        }
        if size > self.payload_limit
            || size > self.byte_limit.saturating_sub(s.used)
            || s.records >= self.record_limit
        {
            s.failure = Some("recording queue or payload limit exceeded".into());
            self.shared.changed.notify_all();
            return false;
        }
        s.used += size;
        s.records += 1;
        match self.sender.try_send((record, size)) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                s.used -= size;
                s.records -= 1;
                s.failure = Some("recording queue is full".into());
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                s.used -= size;
                s.records -= 1;
                s.failure = Some("recording writer stopped".into());
                false
            }
        }
    }
    pub fn fail(&self, reason: String) {
        let mut s = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        if !s.finalizing && s.result.is_none() {
            s.failure.get_or_insert(reason);
        }
        self.shared.changed.notify_all();
    }
    pub fn failure(&self) -> Option<String> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .failure
            .clone()
    }
    pub fn stop(&self, timeout: Duration) -> io::Result<Report> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| invalid(0, "timeout overflow"))?;
        let mut s = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        s.stopping = true;
        loop {
            if let Some(r) = &s.result {
                return Ok(r.clone());
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "recording writer did not stop; resources remain owned by its thread",
                ));
            };
            s = self
                .shared
                .changed
                .wait_timeout(s, remaining)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }
}
impl Drop for Recorder {
    fn drop(&mut self) {
        let mut s = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        if !s.finalizing && s.result.is_none() {
            s.failure.get_or_insert("recording was abandoned".into());
            s.stopping = true;
            self.shared.changed.notify_all();
        }
    }
}
