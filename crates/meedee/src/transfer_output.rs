//! Whole-message SysEx transfer, with pacing after confirmed sends and no retries.
use crate::{
    drivers::{self, Sender},
    midi,
    playback::{Cancellation, MonotonicClock, PlaybackClock},
    transfer::{Report, Session},
};

pub fn run(
    session: &mut Session,
    output: &mut midi::Output,
    cancel: Option<&Cancellation>,
) -> midi::Result<Report> {
    run_with(
        session,
        output,
        &mut MonotonicClock::default(),
        cancel.unwrap_or(&Cancellation::default()),
    )
}
#[doc(hidden)]
pub fn run_with(
    session: &mut Session,
    output: &mut impl Sender,
    clock: &mut impl PlaybackClock,
    cancel: &Cancellation,
) -> midi::Result<Report> {
    let initial = session.snapshot();
    if initial.status != "active" || initial.confirmed_messages != 0 || initial.in_flight.is_some()
    {
        return Err(drivers::invalid("transfer session must be fresh"));
    }
    while session.snapshot().status == "active" {
        if cancel.cancelled() {
            session.cancel("cancelled by caller");
            break;
        }
        let Some(step) = session.prepare(clock.now_ns())? else {
            let deadline = session
                .snapshot()
                .next_ready_ns
                .expect("paced session deadline");
            if let Err(error) = clock.wait_until(deadline, cancel) {
                if error.kind() == std::io::ErrorKind::Interrupted {
                    session.cancel("interrupted");
                    break;
                }
                return Err(error.into());
            }
            continue;
        };
        match output.send(crate::events::Payload::Midi1(
            crate::messages::AnyMessage::SysEx(step.message),
        )) {
            Ok(()) => {
                if let Err(error) = session.confirm(step.index, clock.now_ns()) {
                    session.fail(step.index, "send confirmation failed")?;
                    return Err(error.into());
                }
            }
            Err(error) if drivers::interrupted(&error) => {
                session.cancel("interrupted");
                break;
            }
            Err(error) => session.fail(step.index, &error.to_string())?,
        }
    }
    Ok(session.snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::tests::{Clock, Output};
    use crate::{messages::SysEx, transfer::Plan};
    fn session() -> Session {
        Session::new(
            Plan::new(
                [
                    Ok(SysEx::new(vec![1]).unwrap()),
                    Ok(SysEx::new(vec![2]).unwrap()),
                ],
                10,
                2,
                6,
            )
            .unwrap(),
        )
    }
    #[test]
    fn pacing_uses_send_completion_and_failure_preserves_progress() {
        let mut clock = Clock::default();
        let mut out = Output {
            clock: clock.clone(),
            sent: vec![],
            fail_at: None,
        };
        let report = run_with(
            &mut session(),
            &mut out,
            &mut clock,
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(out.sent.iter().map(|v| v.0).collect::<Vec<_>>(), [0, 110]);
        assert_eq!(report.status, "completed");
        let mut out = Output {
            clock: clock.clone(),
            sent: vec![],
            fail_at: Some(1),
        };
        let report = run_with(
            &mut session(),
            &mut out,
            &mut clock,
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(report.confirmed_messages, 1);
        assert_eq!(report.uncertain_message, Some(1));
        assert_eq!(report.status, "failed");
    }
    #[test]
    fn fresh_session_and_cancellation_are_checked_before_send() {
        let mut clock = Clock::default();
        let mut out = Output {
            clock: clock.clone(),
            sent: vec![],
            fail_at: None,
        };
        let mut started = session();
        started.prepare(0).unwrap();
        assert!(run_with(&mut started, &mut out, &mut clock, &Cancellation::default()).is_err());
        let cancel = Cancellation::default();
        cancel.cancel();
        assert_eq!(
            run_with(&mut session(), &mut out, &mut clock, &cancel)
                .unwrap()
                .status,
            "cancelled"
        );
        assert!(out.sent.is_empty());
    }
}
