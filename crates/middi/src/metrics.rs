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
pub struct Distribution {
    count: u64,
    sum_ns: u128,
    minimum_ns: Option<u64>,
    maximum_ns: Option<u64>,
    buckets: [u64; 9],
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Histogram {
    distribution: Distribution,
}
impl Histogram {
    pub fn snapshot(&self) -> Distribution {
        self.distribution.clone()
    }
    pub fn observe(&mut self, nanoseconds: u64) -> io::Result<()> {
        let count = self
            .distribution
            .count
            .checked_add(1)
            .ok_or_else(|| invalid("histogram count exhausted"))?;
        let sum = self
            .distribution
            .sum_ns
            .checked_add(nanoseconds.into())
            .ok_or_else(|| invalid("histogram sum exhausted"))?;
        let index = UPPER_BOUNDS_NS
            .iter()
            .position(|v| nanoseconds <= *v)
            .unwrap_or(UPPER_BOUNDS_NS.len());
        self.distribution.count = count;
        self.distribution.sum_ns = sum;
        self.distribution.minimum_ns = Some(
            self.distribution
                .minimum_ns
                .map_or(nanoseconds, |v| v.min(nanoseconds)),
        );
        self.distribution.maximum_ns = Some(
            self.distribution
                .maximum_ns
                .map_or(nanoseconds, |v| v.max(nanoseconds)),
        );
        self.distribution.buckets[index] += 1;
        Ok(())
    }
}
impl Distribution {
    pub fn upper_bounds_ns(&self) -> &[u64; 8] {
        &UPPER_BOUNDS_NS
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
    pub(crate) accepted_records: u64,
    pub(crate) capacity_rejections: u64,
    pub(crate) peak_reserved_records: usize,
    pub(crate) peak_reserved_bytes: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryReport {
    pub(crate) events: u64,
    pub(crate) elapsed_ns: u64,
    pub(crate) intervals: Distribution,
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
            intervals: self.intervals.snapshot(),
        })
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

impl Pressure {
    pub fn accepted_records(&self) -> u64 {
        self.accepted_records
    }
    pub fn capacity_rejections(&self) -> u64 {
        self.capacity_rejections
    }
    pub fn peak_reserved_records(&self) -> usize {
        self.peak_reserved_records
    }
    pub fn peak_reserved_bytes(&self) -> usize {
        self.peak_reserved_bytes
    }
}

impl DeliveryReport {
    pub fn events(&self) -> u64 {
        self.events
    }
    pub fn elapsed_ns(&self) -> u64 {
        self.elapsed_ns
    }
    pub fn intervals(&self) -> &Distribution {
        &self.intervals
    }
}
