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
pub struct RecordingReport {
    pub complete: bool,
    pub records: u64,
    pub gap_records: u64,
    pub failure: Option<String>,
    pub pressure: Option<crate::metrics::Pressure>,
}
struct Admission {
    captured: Option<Vec<Record>>,
    diagnostic_failure: Option<String>,
    used: usize,
    records: usize,
    stopping: bool,
    finalizing: bool,
    failure: Option<String>,
    result: Option<RecordingReport>,
    pressure: Option<crate::metrics::Pressure>,
}
struct Shared {
    state: Mutex<Admission>,
    changed: Condvar,
}
pub struct Recorder {
    shared: Arc<Shared>,
    sender: Option<mpsc::SyncSender<(Record, usize)>>,
    record_limit: usize,
    byte_limit: usize,
    payload_limit: usize,
    best_effort: bool,
}
impl Recorder {
    /// Capture owned records in memory. Retention grows with the recording.
    pub fn memory(descriptors: Vec<Record>, collect_metrics: bool) -> io::Result<Self> {
        if descriptors
            .iter()
            .any(|r| !matches!(r, Record::Clock(_) | Record::Stream { .. }))
        {
            return Err(invalid(
                0,
                "recording descriptors must be clocks or streams",
            ));
        }
        Ok(Self {
            shared: Arc::new(Shared {
                state: Mutex::new(Admission {
                    captured: Some(descriptors),
                    diagnostic_failure: None,
                    used: 0,
                    records: 0,
                    stopping: false,
                    finalizing: false,
                    failure: None,
                    result: None,
                    pressure: collect_metrics.then(crate::metrics::Pressure::default),
                }),
                changed: Condvar::new(),
            }),
            sender: None,
            record_limit: 0,
            byte_limit: 0,
            payload_limit: 0,
            best_effort: true,
        })
    }
    /// Owned memory records after stopping; None for file or active recorders.
    pub fn records(&self) -> Option<Vec<Record>> {
        let s = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        s.result.as_ref()?;
        s.captured.clone()
    }
    pub(crate) fn is_memory(&self) -> bool {
        self.sender.is_none()
    }
    pub fn new(
        writer: impl Write + Send + 'static,
        descriptors: Vec<Record>,
        payload_limit: usize,
        record_limit: usize,
        byte_limit: usize,
        best_effort: bool,
        collect_metrics: bool,
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
                captured: None,
                diagnostic_failure: None,
                used: 0,
                records: 0,
                stopping: false,
                finalizing: false,
                failure: None,
                result: None,
                pressure: collect_metrics.then(crate::metrics::Pressure::default),
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
                s.result = Some(RecordingReport {
                    complete: s.failure.is_none(),
                    records: count,
                    gap_records: gaps,
                    failure: s.failure.clone(),
                    pressure: s.pressure.clone(),
                });
                worker.changed.notify_all();
            })?;
        Ok(Self {
            shared,
            sender: Some(sender),
            record_limit,
            byte_limit,
            payload_limit,
            best_effort,
        })
    }
    pub fn record(&self, record: Record) -> bool {
        if self.sender.is_none() {
            let mut s = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
            if s.stopping || s.result.is_some() {
                return false;
            }
            if let Record::Gap { reason, .. } | Record::Discontinuity { reason, .. } = &record {
                s.diagnostic_failure.get_or_insert_with(|| reason.clone());
            }
            s.captured.as_mut().expect("memory recorder").push(record);
            if let Some(p) = &mut s.pressure {
                p.accepted_records += 1;
            }
            return true;
        }
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
            if let Some(p) = &mut s.pressure {
                p.capacity_rejections += 1;
            }
            self.shared.changed.notify_all();
            return false;
        }
        s.used += size;
        s.records += 1;
        match self
            .sender
            .as_ref()
            .expect("file recorder")
            .try_send((record, size))
        {
            Ok(()) => {
                let (records, bytes) = (s.records, s.used);
                if let Some(p) = &mut s.pressure {
                    p.accepted_records += 1;
                    p.peak_reserved_records = p.peak_reserved_records.max(records);
                    p.peak_reserved_bytes = p.peak_reserved_bytes.max(bytes);
                }
                true
            }
            Err(TrySendError::Full(_)) => {
                s.used -= size;
                s.records -= 1;
                s.failure = Some("recording queue is full".into());
                if let Some(p) = &mut s.pressure {
                    p.capacity_rejections += 1;
                }
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
            if s.failure.is_none() {
                if let Some(records) = &mut s.captured {
                    if let Some(stream) = records.iter().find_map(|r| match r {
                        Record::Stream { id, .. } => Some(id.clone()),
                        _ => None,
                    }) {
                        records.push(Record::Discontinuity {
                            stream,
                            reason: reason.clone(),
                        });
                    }
                }
            }
            s.failure.get_or_insert(reason);
        }
        self.shared.changed.notify_all();
    }
    pub fn failure(&self) -> Option<String> {
        let s = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        s.failure.clone().or_else(|| s.diagnostic_failure.clone())
    }
    pub fn pressure(&self) -> Option<crate::metrics::Pressure> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pressure
            .clone()
    }
    pub fn stop(&self, timeout: Duration) -> io::Result<RecordingReport> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| invalid(0, "timeout overflow"))?;
        let mut s = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        s.stopping = true;
        if s.result.is_none() && self.sender.is_none() {
            let captured = s.captured.as_ref().expect("memory recorder");
            let records = captured.len() as u64;
            let gap_records = captured
                .iter()
                .filter(|r| matches!(r, Record::Gap { .. }))
                .count() as u64;
            let failure = s.failure.clone().or_else(|| s.diagnostic_failure.clone());
            s.result = Some(RecordingReport {
                complete: failure.is_none(),
                records,
                gap_records,
                failure,
                pressure: s.pressure.clone(),
            });
        }
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
