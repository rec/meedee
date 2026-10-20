//! Fixed-memory observational metrics. All times are explicit nanoseconds.
use std::io;

pub const UPPER_BOUNDS_NS: [u64; 8] = [
    0,
    1_000,
    10_000,
    100_000,
    1_000_000,
    10_000_000,
    100_000_000,
    1_000_000_000,
];

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Histogram {
    count: u64,
    sum_ns: u128,
    minimum_ns: Option<u64>,
    maximum_ns: Option<u64>,
    buckets: [u64; 9],
}
impl Histogram {
    pub fn observe(&mut self, nanoseconds: u64) -> io::Result<()> {
        let count = self
            .count
            .checked_add(1)
            .ok_or_else(|| invalid("histogram count exhausted"))?;
        let sum = self
            .sum_ns
            .checked_add(nanoseconds.into())
            .ok_or_else(|| invalid("histogram sum exhausted"))?;
        let index = UPPER_BOUNDS_NS
            .iter()
            .position(|v| nanoseconds <= *v)
            .unwrap_or(UPPER_BOUNDS_NS.len());
        self.count = count;
        self.sum_ns = sum;
        self.minimum_ns = Some(self.minimum_ns.map_or(nanoseconds, |v| v.min(nanoseconds)));
        self.maximum_ns = Some(self.maximum_ns.map_or(nanoseconds, |v| v.max(nanoseconds)));
        self.buckets[index] += 1;
        Ok(())
    }
    pub fn count(&self) -> u64 {
        self.count
    }
    pub fn sum_ns(&self) -> u128 {
        self.sum_ns
    }
    pub fn minimum_ns(&self) -> Option<u64> {
        self.minimum_ns
    }
    pub fn maximum_ns(&self) -> Option<u64> {
        self.maximum_ns
    }
    pub fn buckets(&self) -> &[u64; 9] {
        &self.buckets
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pressure {
    pub accepted_records: u64,
    pub capacity_rejections: u64,
    pub peak_reserved_records: usize,
    pub peak_reserved_bytes: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryReport {
    pub events: u64,
    pub elapsed_ns: u64,
    pub intervals: Histogram,
}
pub struct Delivery {
    start: u64,
    last: Option<u64>,
    events: u64,
    intervals: Histogram,
}
impl Delivery {
    pub fn new(start_ns: u64) -> Self {
        Self {
            start: start_ns,
            last: None,
            events: 0,
            intervals: Histogram::default(),
        }
    }
    pub fn observe(&mut self, now_ns: u64) -> io::Result<()> {
        let previous = self.last.unwrap_or(self.start);
        let interval = now_ns
            .checked_sub(previous)
            .ok_or_else(|| invalid("delivery clock regressed"))?;
        let count = self
            .events
            .checked_add(1)
            .ok_or_else(|| invalid("delivery count exhausted"))?;
        if self.last.is_some() {
            self.intervals.observe(interval)?;
        }
        self.events = count;
        self.last = Some(now_ns);
        Ok(())
    }
    pub fn snapshot(&self, now_ns: u64) -> io::Result<DeliveryReport> {
        if now_ns < self.last.unwrap_or(self.start) {
            return Err(invalid("delivery snapshot precedes the last observation"));
        }
        Ok(DeliveryReport {
            events: self.events,
            elapsed_ns: now_ns - self.start,
            intervals: self.intervals.clone(),
        })
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
