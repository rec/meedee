//! Streaming concatenated MIDI 1 SysEx dumps. Realtime/other bytes are rejected.
use crate::file_io::{invalid, Source};
use crate::messages::SysEx;
use std::io::{self, Read, Write};
mod dump_models;
mod dumps;
pub use dumps::{dump, dump_with, DumpOptions, DumpReport};

/// Explicit supported model names. Remote scopes and manual instructions are
/// documented in doc/sysex-dumps.md; a listed name does not imply remote support.
pub fn models() -> Vec<&'static str> {
    dump_models::MODELS.to_vec()
}
pub struct Reader<R> {
    source: Source<R>,
    failed: bool,
}
impl<R: Read> Reader<R> {
    pub fn new(inner: R, limit: usize) -> io::Result<Self> {
        Ok(Self {
            source: Source::new(inner, limit)?,
            failed: false,
        })
    }
    fn read_message(&mut self) -> io::Result<Option<SysEx>> {
        let Some(b) = self.source.optional_byte()? else {
            return Ok(None);
        };
        if b != 240 {
            return Err(invalid(self.source.offset - 1, "SYX expects F0"));
        }
        let mut data = Vec::new();
        loop {
            let b = self.source.byte()?;
            if b == 247 {
                break;
            }
            if b > 127 {
                return Err(invalid(self.source.offset - 1, "non-payload status in SYX"));
            }
            if data.len() == self.source.limit {
                return Err(invalid(self.source.offset - 1, "SYX payload limit"));
            }
            data.push(b);
        }
        Ok(Some(SysEx::new(data).expect("seven-bit payload")))
    }
}
impl<R: Read> Iterator for Reader<R> {
    type Item = io::Result<SysEx>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.read_message() {
            Ok(v) => v.map(Ok),
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}
pub fn write(writer: &mut impl Write, message: &SysEx, limit: usize) -> io::Result<()> {
    if limit == 0 || message.data().len() > limit {
        return Err(invalid(0, "SYX payload limit"));
    }
    writer.write_all(&[240])?;
    writer.write_all(message.data())?;
    writer.write_all(&[247])
}

impl Reader<std::io::BufReader<std::fs::File>> {
    pub fn open(path: impl AsRef<std::path::Path>) -> io::Result<Self> {
        Self::new(
            std::io::BufReader::new(std::fs::File::open(path)?),
            1_048_576,
        )
    }
}

/// Send untimed SysEx messages, waiting at least `gap` after each successful
/// handoff before the next. Does not close the output, retry, or await replies.
pub fn send(
    output: &mut crate::midi::Output,
    messages: impl IntoIterator<Item = io::Result<SysEx>>,
    gap: std::time::Duration,
) -> Result<crate::playback::Report, crate::playback::Failure<crate::midi::Error>> {
    send_with(
        output,
        messages,
        gap,
        &mut crate::playback::MonotonicClock::default(),
    )
}

#[doc(hidden)]
pub fn send_with(
    output: &mut impl crate::drivers::Sender,
    messages: impl IntoIterator<Item = io::Result<SysEx>>,
    gap: std::time::Duration,
    clock: &mut impl crate::playback::PlaybackClock,
) -> Result<crate::playback::Report, crate::playback::Failure<crate::midi::Error>> {
    use crate::playback::{Cause, Failure, Report};
    let mut report = Report::default();
    let result = (|| {
        let gap: u64 = gap.as_nanos().try_into().map_err(|_| {
            Cause::Input(io::Error::new(
                io::ErrorKind::InvalidInput,
                "gap exceeds u64 nanoseconds",
            ))
        })?;
        let cancel = crate::playback::Cancellation::default();
        let mut completed_ns: Option<u64> = None;
        for message in messages {
            let message = message.map_err(Cause::Input)?;
            if let Some(completed) = completed_ns {
                let deadline = completed.checked_add(gap).ok_or_else(|| {
                    Cause::Input(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "SysEx pacing deadline overflow",
                    ))
                })?;
                while clock.now_ns() < deadline {
                    if !clock.wait_until(deadline, &cancel).map_err(Cause::Input)? {
                        report.cancelled = true;
                        return Ok(());
                    }
                }
            }
            match output.send(crate::events::Payload::Midi1(
                crate::messages::AnyMessage::SysEx(message),
            )) {
                Ok(()) => {}
                Err(crate::midi::Error::Cancelled) => {
                    report.cancelled = true;
                    return Ok(());
                }
                Err(error) => return Err(Cause::Send(error)),
            }
            report.sent_events += 1;
            completed_ns = Some(clock.now_ns());
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(report),
        Err(cause) => Err(Failure { report, cause }),
    }
}

#[cfg(test)]
mod send_tests {
    use super::*;
    use crate::drivers::tests::{Clock, Output};
    use std::time::Duration;

    #[test]
    fn waits_after_handoff_and_failure_retains_confirmed_prefix() {
        let clock = Clock::default();
        let mut output = Output {
            clock: clock.clone(),
            sent: vec![],
            fail_at: Some(2),
        };
        let error = send_with(
            &mut output,
            (0..4).map(|_| Ok(SysEx::new(vec![1]).unwrap())),
            Duration::from_nanos(10),
            &mut clock.clone(),
        )
        .unwrap_err();
        assert_eq!(
            output.sent.iter().map(|v| v.0).collect::<Vec<_>>(),
            [0, 110]
        );
        assert_eq!(error.report.sent_events, 2);
        assert!(matches!(error.cause, crate::playback::Cause::Send(_)));
    }

    #[test]
    fn cancelled_output_is_not_counted_as_a_successful_handoff() {
        let cancel = crate::playback::Cancellation::default();
        cancel.cancel();
        let mut output = crate::midi::Output::new().with_playback(crate::midi::PlaybackOptions {
            cancel,
            ..Default::default()
        });
        let report = send(
            &mut output,
            [Ok(SysEx::new(vec![1]).unwrap())],
            Duration::ZERO,
        )
        .unwrap();
        assert!(report.cancelled);
        assert_eq!(report.sent_events, 0);
    }

    #[test]
    fn empty_and_invalid_sources_do_not_send_and_final_message_has_no_wait() {
        let clock = Clock::default();
        let mut output = Output {
            clock: clock.clone(),
            sent: vec![],
            fail_at: None,
        };
        assert_eq!(
            send_with(&mut output, [], Duration::from_secs(1), &mut clock.clone())
                .unwrap()
                .sent_events,
            0
        );
        let message = SysEx::new(vec![1]).unwrap();
        send_with(
            &mut output,
            [Ok(message)],
            Duration::from_secs(1),
            &mut clock.clone(),
        )
        .unwrap();
        assert_eq!(clock.0.get(), 100);
        let error = send_with(
            &mut output,
            [Err(io::Error::other("bad file"))],
            Duration::ZERO,
            &mut clock.clone(),
        )
        .unwrap_err();
        assert_eq!(error.report.sent_events, 0);
        assert_eq!(output.sent.len(), 1);
    }
}
