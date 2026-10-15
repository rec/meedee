//! Explicit clock mapping and cancellable caller-thread replay. No automatic cleanup sends.
use crate::{
    events::{timestamp_seconds, Clock, Event, Payload, Rational, Timestamp},
    file_io::invalid,
};
use std::fmt;
use std::io;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Mapping {
    pub source: Clock,
    pub target: Clock,
    pub rate: Rational,
    pub offset: Rational,
    pub uncertainty: Option<Rational>,
}
impl Mapping {
    pub fn new(
        source: Clock,
        target: Clock,
        rate: Rational,
        offset: Rational,
        uncertainty: Option<Rational>,
    ) -> io::Result<Self> {
        if rate.numerator() <= 0 || uncertainty.is_some_and(|v| v.numerator() < 0) {
            return Err(invalid(
                0,
                "mapping rate must be positive and uncertainty nonnegative",
            ));
        }
        Ok(Self {
            source,
            target,
            rate,
            offset,
            uncertainty,
        })
    }
    pub fn map(&self, t: &Timestamp) -> io::Result<Rational> {
        if t.clock != self.source {
            return Err(invalid(0, "mapping source clock differs"));
        }
        timestamp_seconds(t)?
            .multiply(self.rate.numerator(), self.rate.denominator())?
            .checked_add(self.offset)
    }
}
pub struct Planner {
    clock: Clock,
    mappings: Vec<Mapping>,
    origin: Rational,
    last: Option<Rational>,
    allow_rounding: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Planned {
    pub delay_ns: u64,
    pub rounding_error_ns: Rational,
}
impl Planner {
    pub fn new(
        origin: Timestamp,
        mappings: Vec<Mapping>,
        allow_rounding: bool,
    ) -> io::Result<Self> {
        let clock = origin.clock.clone();
        if clock.mode() == Some(false) {
            return Err(invalid(0, "disabled playback origin"));
        }
        let origin = timestamp_seconds(&origin)?;
        if mappings.len() > 128 {
            return Err(invalid(0, "too many clock mappings"));
        }
        for (i, m) in mappings.iter().enumerate() {
            if m.source.identity() == clock.identity()
                || m.target != clock
                || mappings[..i]
                    .iter()
                    .any(|p| p.source.identity() == m.source.identity())
            {
                return Err(invalid(
                    0,
                    "mapping must have a distinct source, consistent target and unique source identity",
                ));
            }
        }
        Ok(Self {
            clock,
            mappings,
            origin,
            last: None,
            allow_rounding,
        })
    }
    pub fn deadline(&mut self, event: &Event) -> io::Result<Planned> {
        event.validate()?;
        let timestamp = event
            .timestamp
            .as_ref()
            .ok_or_else(|| invalid(0, "playback requires an explicit timestamp"))?;
        let seconds = if timestamp.clock == self.clock {
            timestamp_seconds(timestamp)?
        } else {
            self.mappings
                .iter()
                .find(|m| m.source == timestamp.clock)
                .ok_or_else(|| invalid(0, "unmapped clock domain"))?
                .map(timestamp)?
        };
        if let Some(last) = self.last {
            if seconds.checked_sub(last)?.numerator() < 0 {
                return Err(invalid(
                    0,
                    "playback times regress; resolve discontinuities explicitly",
                ));
            }
        }
        let first = self.origin;
        let (n, error) = seconds
            .checked_sub(first)?
            .multiply(1_000_000_000, 1)?
            .round(self.allow_rounding)?;
        let delay_ns = n
            .try_into()
            .map_err(|_| invalid(0, "playback duration exceeds u64 nanoseconds"))?;
        self.last = Some(seconds);
        Ok(Planned {
            delay_ns,
            rounding_error_ns: error,
        })
    }
}
#[derive(Clone, Default)]
pub struct Cancellation {
    state: Arc<(Mutex<bool>, Condvar)>,
}
impl Cancellation {
    pub fn cancel(&self) {
        let mut s = self.state.0.lock().unwrap_or_else(|p| p.into_inner());
        *s = true;
        self.state.1.notify_all();
    }
    pub fn cancelled(&self) -> bool {
        *self.state.0.lock().unwrap_or_else(|p| p.into_inner())
    }
    pub fn wait_for(&self, timeout: Duration) -> bool {
        let s = self.state.0.lock().unwrap_or_else(|p| p.into_inner());
        let (s, _) = self
            .state
            .1
            .wait_timeout_while(s, timeout, |v| !*v)
            .unwrap_or_else(|p| p.into_inner());
        !*s
    }
}
pub trait PlaybackClock {
    fn now_ns(&mut self) -> u64;
    fn wait_until(&mut self, deadline_ns: u64, cancel: &Cancellation) -> io::Result<bool>;
}
pub struct MonotonicClock {
    origin: Instant,
}
impl Default for MonotonicClock {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}
impl PlaybackClock for MonotonicClock {
    fn now_ns(&mut self) -> u64 {
        self.origin.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
    }
    fn wait_until(&mut self, deadline: u64, cancel: &Cancellation) -> io::Result<bool> {
        let instant = self
            .origin
            .checked_add(Duration::from_nanos(deadline))
            .ok_or_else(|| invalid(0, "monotonic deadline overflow"))?;
        while let Some(remaining) = instant.checked_duration_since(Instant::now()) {
            if !cancel.wait_for(remaining) {
                return Ok(false);
            }
        }
        Ok(!cancel.cancelled())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Report {
    pub sent_events: u64,
    pub cancelled: bool,
    pub max_lateness_ns: u64,
    pub rounded_events: u64,
}
#[derive(Debug)]
pub enum Cause<E> {
    Input(io::Error),
    Send(E),
}
#[derive(Debug)]
pub struct Failure<E> {
    pub report: Report,
    pub cause: Cause<E>,
}
impl<E: fmt::Display> fmt::Display for Failure<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "playback stopped after {} accepted events: ",
            self.report.sent_events
        )?;
        match &self.cause {
            Cause::Input(e) => e.fmt(f),
            Cause::Send(e) => e.fmt(f),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for Failure<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match &self.cause {
            Cause::Input(e) => e,
            Cause::Send(e) => e,
        })
    }
}
pub fn play<E>(
    mut source: impl Iterator<Item = io::Result<Event>>,
    mut planner: Planner,
    clock: &mut impl PlaybackClock,
    cancel: &Cancellation,
    mut send: impl FnMut(&Payload) -> Result<(), E>,
) -> Result<Report, Failure<E>> {
    let mut report = Report::default();
    loop {
        if cancel.cancelled() {
            report.cancelled = true;
            return Ok(report);
        }
        let Some(event) = source.next() else {
            return Ok(report);
        };
        let event = event.map_err(|e| Failure {
            report: report.clone(),
            cause: Cause::Input(e),
        })?;
        let planned = planner.deadline(&event).map_err(|e| Failure {
            report: report.clone(),
            cause: Cause::Input(e),
        })?;
        if !clock
            .wait_until(planned.delay_ns, cancel)
            .map_err(|e| Failure {
                report: report.clone(),
                cause: Cause::Input(e),
            })?
            || cancel.cancelled()
        {
            report.cancelled = true;
            return Ok(report);
        }
        let lateness = clock.now_ns().saturating_sub(planned.delay_ns);
        send(&event.message).map_err(|e| Failure {
            report: report.clone(),
            cause: Cause::Send(e),
        })?;
        report.sent_events = report.sent_events.checked_add(1).ok_or_else(|| Failure {
            report: report.clone(),
            cause: Cause::Input(invalid(0, "playback event count exhausted")),
        })?;
        report.max_lateness_ns = report.max_lateness_ns.max(lateness);
        report.rounded_events += u64::from(planned.rounding_error_ns.numerator() != 0);
    }
}
