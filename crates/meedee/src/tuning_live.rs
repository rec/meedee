//! One fresh MIDI 1 MTS query through caller-owned MIDI connections.
use crate::{
    drivers::{self, Receiver, Sender},
    midi,
    playback::{Cancellation, MonotonicClock, PlaybackClock},
    tuning::{Query, QueryReport},
};

pub fn run(
    query: &mut Query,
    receiver: &mut midi::Input,
    output: &mut midi::Output,
    cancel: Option<&Cancellation>,
) -> midi::Result<QueryReport> {
    if query.snapshot().status != "ready" {
        return Err(drivers::invalid("query must be fresh"));
    }
    receiver.stream()?;
    run_with(
        query,
        receiver,
        output,
        &mut MonotonicClock::default(),
        cancel.unwrap_or(&Cancellation::default()),
    )
}
#[doc(hidden)]
pub fn run_with(
    query: &mut Query,
    receiver: &mut impl Receiver,
    output: &mut impl Sender,
    clock: &mut impl PlaybackClock,
    cancel: &Cancellation,
) -> midi::Result<QueryReport> {
    if query.snapshot().status != "ready" {
        return Err(drivers::invalid("query must be fresh"));
    }
    while matches!(query.snapshot().status, "ready" | "waiting") {
        if cancel.cancelled() {
            query.cancel("cancelled by caller");
            break;
        }
        if let Err(error) = receiver.check() {
            if drivers::interrupted(&error) {
                query.cancel("interrupted");
            } else {
                query.fail(&error.to_string());
            }
            break;
        }
        if query.snapshot().status == "ready" {
            let request = query.prepare(clock.now_ns())?;
            match output.send(crate::events::Payload::Midi1(
                crate::messages::AnyMessage::SysEx(request),
            )) {
                Ok(()) => query.confirm(clock.now_ns())?,
                Err(error) if drivers::interrupted(&error) => query.cancel("interrupted"),
                Err(error) => query.fail(&error.to_string()),
            }
            continue;
        }
        query.poll(clock.now_ns())?;
        let report = query.snapshot();
        if report.status != "waiting" {
            break;
        }
        let wait = drivers::remaining(clock, report.deadline_ns.expect("waiting query deadline"));
        match receiver.receive(wait) {
            Ok(item) => {
                if let Err(error) = receiver.check() {
                    if drivers::interrupted(&error) {
                        query.cancel("interrupted");
                    } else {
                        query.fail(&error.to_string());
                    }
                    break;
                }
                if cancel.cancelled() {
                    query.cancel("cancelled by caller");
                    break;
                }
                if let Some(item) = item {
                    query.feed(&item, clock.now_ns())?;
                }
            }
            Err(error) if drivers::interrupted(&error) => query.cancel("interrupted"),
            Err(error) => query.fail(&error.to_string()),
        }
    }
    Ok(query.snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::tests::{Clock, Input, Output};
    use crate::tuning::BulkDumpRequest;
    fn query() -> Query {
        Query::new(BulkDumpRequest::new(1, 2).unwrap().into(), "s".into(), 1000).unwrap()
    }
    #[test]
    fn query_sends_once_times_out_and_retains_uncertain_failure() {
        let mut clock = Clock::default();
        let mut input = Input {
            clock: clock.clone(),
            items: [].into(),
            lost: false,
        };
        let mut output = Output {
            clock: clock.clone(),
            sent: vec![],
            fail_at: None,
        };
        let report = run_with(
            &mut query(),
            &mut input,
            &mut output,
            &mut clock,
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(report.status, "timed_out");
        assert!(report.request_sent);
        assert_eq!(output.sent.len(), 1);
        output.fail_at = Some(1);
        let report = run_with(
            &mut query(),
            &mut input,
            &mut output,
            &mut clock,
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(report.status, "failed");
        assert!(report.uncertain_send);
        assert!(!report.request_sent);
        input.lost = true;
        let report = run_with(
            &mut query(),
            &mut input,
            &mut output,
            &mut clock,
            &Cancellation::default(),
        )
        .unwrap();
        assert!(!report.uncertain_send);
        assert_eq!(output.sent.len(), 1);
    }
}
