//! In-memory capture with the same records used by file archives and replay.
use super::Input;
use crate::archive::{Record, Writer};
use crate::live::{DiagnosticKind, Item};
use crate::messages::Message;
use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::{self, BufWriter};
use std::path::Path;
use std::time::{Duration, Instant};

/// A capture, including clock/source declarations and any receive diagnostics.
/// Retained records grow with capture size; receive-queue limits do not bound this
/// application-owned memory. Pass the capture directly to `Output::play`.
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
    /// use meedee::{Input, Output};
    /// use std::time::Duration;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// Output::new().play(Input::new().record(Duration::from_secs(10))?)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn record(mut self, duration: Duration) -> io::Result<Recording> {
        if self.closed {
            return Err(super::invalid("input is closed"));
        }
        Instant::now()
            .checked_add(duration)
            .ok_or_else(|| super::invalid("recording duration exceeds clock range"))?;
        self.open()?;
        let deadline = Instant::now()
            .checked_add(duration)
            .ok_or_else(|| super::invalid("recording duration exceeds clock range"))?;
        let clock = self.clock.as_ref().expect("opened input clock").clone();
        let stream = self.stream.as_ref().expect("opened input source").clone();
        let mut clocks = BTreeSet::from([clock.identity().to_owned()]);
        let mut recording = Recording {
            records: vec![
                Record::Clock(clock),
                Record::Stream {
                    id: stream.clone(),
                    label: String::new(),
                    clock_identity: None,
                },
            ],
        };
        while let Some(item) = self.next_item(Some(deadline)) {
            append(&mut recording.records, item, &stream, &mut clocks);
        }
        if let Err(error) = self.close() {
            append(&mut recording.records, Err(error), &stream, &mut clocks);
        }
        while let Some(item) = self.next_item(None) {
            append(&mut recording.records, item, &stream, &mut clocks);
        }
        Ok(recording)
    }
    /// Capture in memory, then save the same records and return them. Existing
    /// files are preserved unless overwrite is true. A write failure can leave
    /// a partial file, which the strict reader rejects.
    ///
    /// ```no_run
    /// use meedee::{Input, Output, Reader};
    /// use std::time::Duration;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// Input::new().record_to(Duration::from_secs(10), "rec.meedee", true)?;
    /// Output::new().play(Reader::open("rec.meedee")?)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn record_to(
        self,
        duration: Duration,
        path: impl AsRef<Path>,
        overwrite: bool,
    ) -> io::Result<Recording> {
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
fn append(
    records: &mut Vec<Record>,
    item: io::Result<Item>,
    stream: &str,
    clocks: &mut BTreeSet<String>,
) {
    let record = match item {
        Ok(Item::Message(message)) => {
            if let Some(timestamp) = message.timestamp() {
                if clocks.insert(timestamp.clock.identity().into()) {
                    records.push(Record::Clock(timestamp.clock.clone()));
                }
            }
            Record::Event(message)
        }
        Ok(Item::Diagnostic(diagnostic)) => {
            if diagnostic.kind == DiagnosticKind::ClockChange {
                Record::Discontinuity {
                    stream: diagnostic.stream,
                    reason: diagnostic.reason,
                }
            } else {
                Record::Gap {
                    stream: diagnostic.stream,
                    lost: diagnostic.lost,
                    reason: diagnostic.reason,
                }
            }
        }
        Err(error) => Record::Discontinuity {
            stream: stream.into(),
            reason: error.to_string(),
        },
    };
    records.push(record);
}
