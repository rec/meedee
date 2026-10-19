//! Source-local timing observations. File order never implies cross-clock order.
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct TimingSegment {
    pub source: String,
    pub clock_identity: Option<String>,
    pub number: u64,
    pub events: u64,
    pub untimed: u64,
    pub first: Option<i128>,
    pub last: Option<i128>,
    pub minimum: Option<i128>,
    pub maximum: Option<i128>,
    pub regressions: u64,
}

#[derive(Clone, Debug)]
pub struct TimingDiagnostic {
    pub source: String,
    pub record: u64,
    pub kind: &'static str,
}

#[derive(Debug, Default)]
pub struct Timing {
    pub source_counts: BTreeMap<String, u64>,
    pub segments: Vec<TimingSegment>,
    pub diagnostics: Vec<TimingDiagnostic>,
    pub regressions: u64,
    pub omitted_segments: u64,
    pub omitted_diagnostics: u64,
    states: BTreeMap<String, State>,
}

#[derive(Debug, Default)]
struct State {
    number: u64,
    clock: Option<String>,
    previous: Option<i128>,
    active: Option<usize>,
    started: bool,
}

impl Timing {
    pub fn observe(
        &mut self,
        source: &str,
        clock: Option<&str>,
        tick: Option<i128>,
        record: u64,
        limit: usize,
    ) {
        *self.source_counts.entry(source.into()).or_default() += 1;
        let changed = self
            .states
            .get(source)
            .is_some_and(|s| s.started && s.clock.as_deref() != clock);
        if changed {
            self.boundary(source, record, "clock_change", limit);
        }
        let state = self.states.entry(source.into()).or_default();
        if !state.started {
            state.started = true;
            state.clock = clock.map(str::to_owned);
            state.active = if self.segments.len() < limit {
                self.segments.push(TimingSegment {
                    source: source.into(),
                    clock_identity: state.clock.clone(),
                    number: state.number,
                    events: 0,
                    untimed: 0,
                    first: None,
                    last: None,
                    minimum: None,
                    maximum: None,
                    regressions: 0,
                });
                Some(self.segments.len() - 1)
            } else {
                self.omitted_segments += 1;
                None
            };
        }
        let regressed = tick.zip(state.previous).is_some_and(|(t, p)| t < p);
        if let Some(t) = tick {
            state.previous = Some(t);
        }
        if let Some(i) = state.active {
            let segment = &mut self.segments[i];
            segment.events += 1;
            segment.untimed += u64::from(tick.is_none());
            segment.regressions += u64::from(regressed);
            if let Some(t) = tick {
                segment.first.get_or_insert(t);
                segment.last = Some(t);
                segment.minimum = Some(segment.minimum.map_or(t, |p| p.min(t)));
                segment.maximum = Some(segment.maximum.map_or(t, |p| p.max(t)));
            }
        }
        if regressed {
            self.regressions += 1;
            self.diagnostic(source, record, "timestamp_regression", limit);
        }
        if tick.is_none() {
            self.diagnostic(source, record, "missing_timestamp", limit);
        }
    }

    pub fn boundary(&mut self, source: &str, record: u64, kind: &'static str, limit: usize) {
        let state = self.states.entry(source.into()).or_default();
        state.number += 1;
        state.started = false;
        state.previous = None;
        state.active = None;
        self.diagnostic(source, record, kind, limit);
    }

    fn diagnostic(&mut self, source: &str, record: u64, kind: &'static str, limit: usize) {
        if self.diagnostics.len() < limit {
            self.diagnostics.push(TimingDiagnostic {
                source: source.into(),
                record,
                kind,
            });
        } else {
            self.omitted_diagnostics += 1;
        }
    }
}
