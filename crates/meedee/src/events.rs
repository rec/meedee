//! Immutable events and exact clock arithmetic, independent of native transport.
use crate::file_io::invalid;
use crate::messages::{AnyCommand, AnyMessage};
use crate::ump::UmpMessage;
use std::io;
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Clock {
    identity: String,
    numerator: u64,
    denominator: u64,
    origin: String,
    resolution_ticks: Option<u64>,
    backend: Option<i32>,
    mode: Option<bool>,
}
impl Clock {
    pub fn new(
        identity: String,
        numerator: u64,
        denominator: u64,
        origin: String,
        resolution_ticks: Option<u64>,
    ) -> io::Result<Self> {
        if identity.is_empty()
            || origin.is_empty()
            || numerator == 0
            || denominator == 0
            || resolution_ticks == Some(0)
        {
            return Err(invalid(0, "invalid clock descriptor"));
        }
        Ok(Self {
            identity,
            numerator,
            denominator,
            origin,
            resolution_ticks,
            backend: None,
            mode: None,
        })
    }
    pub fn with_transport(mut self, backend: i32, enabled: bool) -> Self {
        self.backend = Some(backend);
        self.mode = Some(enabled);
        self
    }
    pub fn identity(&self) -> &str {
        &self.identity
    }
    pub fn numerator(&self) -> u64 {
        self.numerator
    }
    pub fn denominator(&self) -> u64 {
        self.denominator
    }
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub fn resolution_ticks(&self) -> Option<u64> {
        self.resolution_ticks
    }
    pub fn backend(&self) -> Option<i32> {
        self.backend
    }
    pub fn mode(&self) -> Option<bool> {
        self.mode
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Timestamp {
    pub ticks: i64,
    pub clock: Clock,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub first_packet: u64,
    pub last_packet: u64,
    pub first_timestamp: Option<Timestamp>,
    pub last_timestamp: Option<Timestamp>,
}
impl Span {
    pub(crate) fn validate(&self, timestamp: Option<&Timestamp>) -> io::Result<()> {
        if self.first_packet > self.last_packet {
            return Err(invalid(0, "reversed packet span"));
        }
        let clocks = [
            self.first_timestamp.as_ref(),
            self.last_timestamp.as_ref(),
            timestamp,
        ];
        let mut first = None;
        for t in clocks.into_iter().flatten() {
            if t.clock.mode() == Some(false) {
                return Err(invalid(0, "disabled clock cannot have span timestamps"));
            }
            if first.is_some_and(|c| c != &t.clock) {
                return Err(invalid(0, "a message cannot span clock generations"));
            }
            first = Some(&t.clock);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Payload {
    Midi1(AnyMessage),
    Ump(UmpMessage),
    Command(AnyCommand),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Event {
    pub stream: String,
    pub timestamp: Option<Timestamp>,
    pub span: Option<Span>,
    pub message: Payload,
}
impl Event {
    pub fn validate(&self) -> io::Result<()> {
        if self.stream.is_empty() {
            return Err(invalid(0, "empty stream identity"));
        }
        if self
            .timestamp
            .as_ref()
            .is_some_and(|t| t.clock.mode() == Some(false))
        {
            return Err(invalid(0, "disabled clock cannot have a timestamp"));
        }
        if let Some(s) = &self.span {
            s.validate(self.timestamp.as_ref())?;
        }
        Ok(())
    }
}
/// Checked rational seconds, reduced on construction. Floating point is never used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rational {
    numerator: i128,
    denominator: i128,
}
impl Rational {
    pub fn new(numerator: i128, denominator: i128) -> io::Result<Self> {
        if denominator <= 0 {
            return Err(invalid(0, "denominator must be positive"));
        }
        let g = gcd(numerator.unsigned_abs(), denominator as u128);
        Ok(Self {
            numerator: numerator / (g as i128),
            denominator: denominator / (g as i128),
        })
    }
    pub fn numerator(&self) -> i128 {
        self.numerator
    }
    pub fn denominator(&self) -> i128 {
        self.denominator
    }
    pub fn checked_add(self, rhs: Self) -> io::Result<Self> {
        let g = gcd(self.denominator as u128, rhs.denominator as u128) as i128;
        let a = self.denominator / g;
        let b = rhs.denominator / g;
        let n = self
            .numerator
            .checked_mul(b)
            .and_then(|v| rhs.numerator.checked_mul(a).and_then(|r| v.checked_add(r)))
            .ok_or_else(|| invalid(0, "rational overflow"))?;
        Self::new(
            n,
            a.checked_mul(rhs.denominator)
                .ok_or_else(|| invalid(0, "rational overflow"))?,
        )
    }
    pub fn checked_sub(self, rhs: Self) -> io::Result<Self> {
        let g = gcd(self.denominator as u128, rhs.denominator as u128) as i128;
        let a = self.denominator / g;
        let b = rhs.denominator / g;
        let n = self
            .numerator
            .checked_mul(b)
            .and_then(|v| rhs.numerator.checked_mul(a).and_then(|r| v.checked_sub(r)))
            .ok_or_else(|| invalid(0, "rational overflow"))?;
        Self::new(
            n,
            a.checked_mul(rhs.denominator)
                .ok_or_else(|| invalid(0, "rational overflow"))?,
        )
    }
    pub fn multiply(self, n: i128, d: i128) -> io::Result<Self> {
        let rhs = Self::new(n, d)?;
        let a = gcd(self.numerator.unsigned_abs(), rhs.denominator as u128) as i128;
        let b = gcd(rhs.numerator.unsigned_abs(), self.denominator as u128) as i128;
        Self::new(
            (self.numerator / a)
                .checked_mul(rhs.numerator / b)
                .ok_or_else(|| invalid(0, "rational overflow"))?,
            (self.denominator / b)
                .checked_mul(rhs.denominator / a)
                .ok_or_else(|| invalid(0, "rational overflow"))?,
        )
    }
    pub fn round(self, allow_rounding: bool) -> io::Result<(i128, Self)> {
        let q = self.numerator.div_euclid(self.denominator);
        let r = self.numerator.rem_euclid(self.denominator);
        if r != 0 && !allow_rounding {
            return Err(invalid(0, "conversion requires rounding permission"));
        }
        let up = r > self.denominator - r || r == self.denominator - r && q % 2 != 0;
        let result = q + i128::from(up);
        let error = Self::new(
            result
                .checked_mul(self.denominator)
                .and_then(|v| v.checked_sub(self.numerator))
                .ok_or_else(|| invalid(0, "rounding overflow"))?,
            self.denominator,
        )?;
        Ok((result, error))
    }
}
fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a.max(1)
}
pub fn timestamp_seconds(t: &Timestamp) -> io::Result<Rational> {
    Rational::new(i128::from(t.ticks), 1)?
        .multiply(t.clock.numerator().into(), t.clock.denominator().into())
}

/// Opaque conversion/connection generation identity. It is not a timestamp origin.
pub(crate) fn fresh_identity(kind: &str) -> io::Result<String> {
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    };
    static NAMESPACE: OnceLock<String> = OnceLock::new();
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let namespace = NAMESPACE
        .get_or_init(|| format!("{}:{:?}", std::process::id(), std::time::SystemTime::now()));
    let mut n = NEXT.load(Ordering::Relaxed);
    loop {
        let next = n
            .checked_add(1)
            .ok_or_else(|| invalid(0, "clock generation space exhausted"))?;
        match NEXT.compare_exchange_weak(n, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return Ok(format!("{namespace}:{kind}:{n}")),
            Err(current) => n = current,
        }
    }
}
