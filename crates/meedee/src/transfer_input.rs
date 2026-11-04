//! Passive capture using the input's existing decoder; no request or retry traffic.
use crate::{
    drivers::{self, Receiver},
    midi,
    playback::{Cancellation, MonotonicClock, PlaybackClock},
    transfer::{Capture, CaptureReport},
};

pub fn run(
    capture: &mut Capture,
    receiver: &mut midi::Input,
    cancel: Option<&Cancellation>,
) -> midi::Result<CaptureReport> {
    if capture.snapshot().status != "ready" {
        return Err(drivers::invalid("capture must be fresh"));
    }
    receiver.stream()?;
    run_with(
        capture,
        receiver,
        &mut MonotonicClock::default(),
        cancel.unwrap_or(&Cancellation::default()),
    )
}
#[doc(hidden)]
pub fn run_with(
    capture: &mut Capture,
    receiver: &mut impl Receiver,
    clock: &mut impl PlaybackClock,
    cancel: &Cancellation,
) -> midi::Result<CaptureReport> {
    capture.start(clock.now_ns())?;
    while capture.snapshot().status == "active" {
        if cancel.cancelled() {
            capture.cancel("cancelled by caller");
            break;
        }
        if let Err(error) = receiver.check() {
            if drivers::interrupted(&error) {
                capture.cancel("interrupted");
            } else {
                capture.fail(&error.to_string());
            }
            break;
        }
        capture.poll(clock.now_ns())?;
        let report = capture.snapshot();
        if report.status != "active" {
            break;
        }
        let wait = drivers::remaining(clock, report.deadline_ns.expect("active capture deadline"));
        match receiver.receive(wait) {
            Ok(item) => {
                if let Err(error) = receiver.check() {
                    if drivers::interrupted(&error) {
                        capture.cancel("interrupted");
                    } else {
                        capture.fail(&error.to_string());
                    }
                    break;
                }
                if cancel.cancelled() {
                    capture.cancel("cancelled by caller");
                    break;
                }
                if let Some(item) = item {
                    capture.feed(&item, clock.now_ns())?;
                }
            }
            Err(error) if drivers::interrupted(&error) => capture.cancel("interrupted"),
            Err(error) => capture.fail(&error.to_string()),
        }
    }
    Ok(capture.snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::tests::{sysex, Clock, Input};
    #[test]
    fn capture_completes_on_count_and_reports_loss_or_silence() {
        let mut clock = Clock::default();
        let mut input = Input {
            clock: clock.clone(),
            items: [sysex(&[1]), sysex(&[2])].into(),
            lost: false,
        };
        let mut capture = Capture::new("s".into(), 2, 1000, 6, 1).unwrap();
        assert_eq!(
            run_with(
                &mut capture,
                &mut input,
                &mut clock,
                &Cancellation::default()
            )
            .unwrap()
            .status,
            "completed"
        );
        let mut wire = vec![];
        capture.write(&mut wire).unwrap();
        assert_eq!(wire, [240, 1, 247, 240, 2, 247]);
        let mut capture = Capture::new("s".into(), 1, 1000, 3, 1).unwrap();
        assert_eq!(
            run_with(
                &mut capture,
                &mut input,
                &mut clock,
                &Cancellation::default()
            )
            .unwrap()
            .status,
            "timed_out"
        );
        input.lost = true;
        let mut capture = Capture::new("s".into(), 1, 1000, 3, 1).unwrap();
        assert_eq!(
            run_with(
                &mut capture,
                &mut input,
                &mut clock,
                &Cancellation::default()
            )
            .unwrap()
            .status,
            "failed"
        );
    }
}
