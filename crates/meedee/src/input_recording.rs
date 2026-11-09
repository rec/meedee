//! In-memory capture with the same records used by file archives and replay.
use super::{Core, Input};
use crate::archive::{Record, Writer};
use crate::live::{DiagnosticKind, Item};
use crate::messages::Message;
use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::{self, BufWriter};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A capture, including clock/source declarations and any receive diagnostics.
/// Retained records grow with capture size; receive-queue limits do not bound this
/// application-owned memory. Pass the capture directly to `Output::send`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recording {
    pub records: Vec<Record>,
}
impl IntoIterator for Recording {
    type Item = io::Result<Record>;
    type IntoIter = std::iter::Map<std::vec::IntoIter<Record>, fn(Record) -> io::Result<Record>>;
    fn into_iter(self) -> Self::IntoIter {
        self.records.into_iter().map(Ok)
    }
}
impl Input {
    /// Record for elapsed time, closing input and preserving diagnostics in the
    /// result. Setup errors return Err; receive failures become discontinuities.
    /// The duration starts after opening. Closing drains already accepted packets.
    ///
    /// ```no_run
    /// use meedee::midi::{Input, Output};
    /// use std::time::Duration;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// Output::new().send(Input::new().record(Duration::from_secs(10))?)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn record(mut self, duration: Duration) -> crate::midi::Result<Recording> {
        if self.closed() {
            return Err(crate::midi::Error::Closed);
        }
        if self.callback_mode {
            return Err(super::invalid("recording is unavailable in callback mode").into());
        }
        Instant::now()
            .checked_add(duration)
            .ok_or_else(|| super::invalid("recording duration exceeds clock range"))?;
        let recorder = self.start(None::<&Path>, &Default::default())?;
        let deadline = Instant::now()
            .checked_add(duration)
            .ok_or_else(|| super::invalid("recording duration exceeds clock range"))?;
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            match self.receive(Some(remaining), None) {
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(error) => {
                    recorder.fail(error.to_string());
                    break;
                }
            }
        }
        if let Err(error) = self.close() {
            recorder.fail(error.to_string());
        }
        Ok(Recording {
            records: recorder.records().expect("stopped memory recording"),
        })
    }
    /// Capture in memory, then save the same records and return them. Existing
    /// files are preserved unless overwrite is true. A write failure can leave
    /// a partial file, which the strict reader rejects.
    ///
    /// ```no_run
    /// use meedee::midi::{Input, Output};
    /// use meedee::archive::{Reader};
    /// use std::time::Duration;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// Input::new().record_to(Duration::from_secs(10), "rec.meedee", true)?;
    /// Output::new().send(Reader::open("rec.meedee")?)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn record_to(
        self,
        duration: Duration,
        path: impl AsRef<Path>,
        overwrite: bool,
    ) -> crate::midi::Result<Recording> {
        let recording = self.record(duration)?;
        let file = OpenOptions::new()
            .write(true)
            .create(overwrite)
            .truncate(overwrite)
            .create_new(!overwrite)
            .open(path)?;
        let mut writer = Writer::new(BufWriter::new(file), 1_048_576)?;
        for record in &recording.records {
            writer.write(record)?;
        }
        // Completeness is structural. Gaps and discontinuities remain explicit
        // records and are not silently converted into a clean capture.
        writer.finish(true, "")?;
        Ok(recording)
    }
}
pub(super) struct Attachment {
    pub recorder: Arc<crate::archive::Recorder>,
    stopping: bool,
    clocks: BTreeSet<String>,
}
impl Core {
    pub(super) fn attach_recording(
        &mut self,
        recorder: Arc<crate::archive::Recorder>,
    ) -> crate::midi::Result<()> {
        if self.closed {
            return Err(crate::midi::Error::Closed);
        }
        if self.recording.is_some() {
            return Err(super::invalid("recording is already active").into());
        }
        let clock = self.clock.as_ref().expect("opened clock");
        self.recording = Some(Attachment {
            recorder: recorder.clone(),
            stopping: false,
            clocks: BTreeSet::from([clock.identity().into()]),
        });
        if recorder.is_memory() {
            let pending: Vec<_> = self
                .pending
                .iter()
                .filter_map(|r| r.as_ref().ok())
                .cloned()
                .collect();
            for item in pending {
                self.record_item(&item);
            }
        }
        Ok(())
    }
    pub(super) fn record_item(&mut self, item: &Item) {
        let Some(attachment) = &mut self.recording else {
            return;
        };
        if attachment.stopping {
            return;
        }
        let record = match item {
            Item::Message(message) => {
                if let Some(timestamp) = message.timestamp() {
                    if attachment.clocks.insert(timestamp.clock.identity().into()) {
                        attachment
                            .recorder
                            .record(Record::Clock(timestamp.clock.clone()));
                    }
                }
                Record::Event(message.clone())
            }
            Item::Diagnostic(diagnostic) => {
                if diagnostic.kind == DiagnosticKind::ClockChange {
                    Record::Discontinuity {
                        stream: diagnostic.stream.clone(),
                        reason: diagnostic.reason.clone(),
                    }
                } else {
                    Record::Gap {
                        stream: diagnostic.stream.clone(),
                        lost: diagnostic.lost,
                        reason: diagnostic.reason.clone(),
                    }
                }
            }
        };
        attachment.recorder.record(record);
    }
    pub(super) fn stop_recording_admission(&mut self) -> Option<Arc<crate::archive::Recorder>> {
        let attachment = self.recording.as_ref()?;
        if attachment.stopping {
            return Some(attachment.recorder.clone());
        }
        // Establish the boundary under the decode lock, including admitted ingress.
        let packets = {
            let mut state = self.queue.state.lock().unwrap_or_else(|p| p.into_inner());
            state.bytes = 0;
            std::mem::take(&mut state.packets)
        };
        for packet in packets {
            self.decode(packet);
        }
        let lost = self.dropped_packets().saturating_sub(self.reported_loss);
        if lost != 0 {
            let decoder = self.decoder.as_mut().expect("decoded recording");
            let item = decoder.loss(lost, "known input loss at recording/close boundary".into());
            self.expected = self.expected.saturating_add(lost);
            decoder.begin_at(self.expected);
            self.accept_item(item);
            self.reported_loss = self.reported_loss.saturating_add(lost);
        }
        let attachment = self.recording.as_mut().expect("attached recorder");
        if self
            .decoder
            .as_ref()
            .is_some_and(crate::live::Decoder::pending)
        {
            attachment
                .recorder
                .fail("recording ended with an unfinished message".into());
        }
        attachment.stopping = true;
        Some(attachment.recorder.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        events,
        live::Decoder,
        messages::{AnyMessage, NoteOn},
        midi,
        transport::Protocol,
    };
    use std::io::Write;
    use std::sync::{mpsc, Condvar, Mutex};

    #[test]
    fn stopping_recorder_survives_timeout_and_close_until_final_report_is_available() {
        struct Gate {
            active: bool,
            released: bool,
        }
        struct Writer {
            gate: Arc<(Mutex<Gate>, Condvar)>,
            entered: mpsc::SyncSender<()>,
        }
        impl Write for Writer {
            fn write(&mut self, data: &[u8]) -> io::Result<usize> {
                let (lock, changed) = &*self.gate;
                let mut gate = lock.lock().unwrap();
                if gate.active && !gate.released {
                    let _ = self.entered.try_send(());
                    while !gate.released {
                        gate = changed.wait(gate).unwrap();
                    }
                }
                Ok(data.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let clock =
            events::Clock::new("retry".into(), 1, 1_000_000_000, "test".into(), None).unwrap();
        let gate = Arc::new((
            Mutex::new(Gate {
                active: false,
                released: false,
            }),
            Condvar::new(),
        ));
        let (entered, waiting) = mpsc::sync_channel(1);
        let recorder = Arc::new(
            crate::archive::Recorder::new(
                Writer {
                    gate: gate.clone(),
                    entered,
                },
                vec![
                    Record::Clock(clock.clone()),
                    Record::Stream {
                        id: "retry".into(),
                        label: String::new(),
                        clock_identity: Some(clock.identity().into()),
                    },
                ],
                1024,
                128,
                8192,
                false,
                false,
            )
            .unwrap(),
        );
        gate.0.lock().unwrap().active = true;
        let message = events::Payload::Midi1(AnyMessage::NoteOn(
            NoteOn::new(60).unwrap().with_channel(1).unwrap(),
        ))
        .with_stream("retry")
        .unwrap();
        assert!(recorder.record(Record::Event(Box::new(message))));
        waiting.recv_timeout(Duration::from_secs(1)).unwrap();
        let mut core = Core::new();
        core.stream = Some("retry".into());
        core.clock = Some(clock);
        core.decoder = Some(Decoder::new("retry".into(), Protocol::Midi1, 1024).unwrap());
        core.recording = Some(Attachment {
            recorder,
            stopping: false,
            clocks: BTreeSet::from(["retry".into()]),
        });
        let mut input = Input::decoded(core);
        assert!(
            matches!(input.stop(Duration::from_millis(1)), Err(midi::Error::Io(e)) if e.kind() == io::ErrorKind::TimedOut)
        );
        assert!(input.recording_report().is_none());
        assert!(input.close().is_err());
        assert!(input.closed());
        assert!(input.recording_report().is_none());
        gate.0.lock().unwrap().released = true;
        gate.1.notify_all();
        let report = input.stop(Duration::from_secs(1)).unwrap();
        assert!(report.complete);
        assert_eq!(input.recording_report(), Some(report));
        input.close().unwrap();
    }
}
