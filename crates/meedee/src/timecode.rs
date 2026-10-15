//! MTC wire values, SMPTE labels and caller-driven quarter-frame sequence lock.
use crate::messages::{Message, PrimitiveMessage, SysEx, ValueError};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FrameRate {
    Fps24 = 0,
    Fps25 = 1,
    DropFrame30 = 2,
    Fps30 = 3,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FullFrame {
    device_id: u8,
    rate: FrameRate,
    hours: u8,
    minutes: u8,
    seconds: u8,
    frames: u8,
}
impl FullFrame {
    pub fn new(
        device_id: i64,
        rate: FrameRate,
        hours: i64,
        minutes: i64,
        seconds: i64,
        frames: i64,
    ) -> Result<Self, ValueError> {
        if device_id != 127 {
            return Err(ValueError(
                "standard full-frame MTC requires broadcast device_id=127".into(),
            ));
        }
        if !(0..=23).contains(&hours)
            || !(0..=59).contains(&minutes)
            || !(0..=59).contains(&seconds)
        {
            return Err(ValueError(
                "invalid device_id or time-code hours/minutes/seconds".into(),
            ));
        }
        let count = match rate {
            FrameRate::Fps24 => 24,
            FrameRate::Fps25 => 25,
            _ => 30,
        };
        if !(0..count).contains(&frames) {
            return Err(ValueError(
                "frames outside the selected rate's label range".into(),
            ));
        }
        Ok(Self {
            device_id: device_id as u8,
            rate,
            hours: hours as u8,
            minutes: minutes as u8,
            seconds: seconds as u8,
            frames: frames as u8,
        })
    }
    pub fn device_id(&self) -> u8 {
        self.device_id
    }
    pub fn rate(&self) -> FrameRate {
        self.rate
    }
    pub fn hours(&self) -> u8 {
        self.hours
    }
    pub fn minutes(&self) -> u8 {
        self.minutes
    }
    pub fn seconds(&self) -> u8 {
        self.seconds
    }
    pub fn frames(&self) -> u8 {
        self.frames
    }
}
impl Message for FullFrame {}
impl PrimitiveMessage for FullFrame {
    fn encode(&self) -> Vec<u8> {
        vec![
            240,
            127,
            self.device_id,
            1,
            1,
            (self.rate as u8) << 5 | self.hours,
            self.minutes,
            self.seconds,
            self.frames,
            247,
        ]
    }
}
pub fn decode(message: &SysEx) -> Result<Option<FullFrame>, ValueError> {
    let data = message.data();
    if data.len() < 3 || data[0] != 127 || data[2] != 1 {
        return Ok(None);
    }
    if data.len() < 4 {
        return Err(ValueError("truncated MTC header".into()));
    }
    if data[3] != 1 {
        return Ok(None);
    }
    if data.len() != 8 {
        return Err(ValueError(
            "full-frame MTC requires eight payload bytes".into(),
        ));
    }
    let rate = match data[4] >> 5 {
        0 => FrameRate::Fps24,
        1 => FrameRate::Fps25,
        2 => FrameRate::DropFrame30,
        _ => FrameRate::Fps30,
    };
    Ok(Some(FullFrame::new(
        data[1].into(),
        rate,
        (data[4] & 31).into(),
        data[5].into(),
        data[6].into(),
        data[7].into(),
    )?))
}

/// A validated SMPTE label, including skipped drop-frame labels.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Position {
    pub rate: FrameRate,
    pub hours: u8,
    pub minutes: u8,
    pub seconds: u8,
    pub frames: u8,
}
impl Position {
    pub fn new(
        rate: FrameRate,
        hours: i64,
        minutes: i64,
        seconds: i64,
        frames: i64,
    ) -> Result<Self, ValueError> {
        FullFrame::new(127, rate, hours, minutes, seconds, frames)?;
        if rate == FrameRate::DropFrame30 && minutes % 10 != 0 && seconds == 0 && frames < 2 {
            return Err(ValueError("skipped drop-frame label".into()));
        }
        Ok(Self {
            rate,
            hours: hours as u8,
            minutes: minutes as u8,
            seconds: seconds as u8,
            frames: frames as u8,
        })
    }
    pub fn advance(&self, frames: i64) -> Self {
        let fps = match self.rate {
            FrameRate::Fps24 => 24,
            FrameRate::Fps25 => 25,
            _ => 30,
        };
        let minutes = i64::from(self.hours) * 60 + i64::from(self.minutes);
        let nominal = (minutes * 60 + i64::from(self.seconds)) * fps + i64::from(self.frames);
        let (count, day) = if self.rate == FrameRate::DropFrame30 {
            (nominal - 2 * (minutes - minutes / 10), 2_589_408)
        } else {
            (nominal, fps * 86400)
        };
        // Reduce first so arbitrary signed offsets cannot overflow.
        let mut label = (count + frames.rem_euclid(day)).rem_euclid(day);
        if self.rate == FrameRate::DropFrame30 {
            let d = label / 17982;
            let m = label % 17982;
            label += 18 * d + if m >= 2 { 2 * ((m - 2) / 1798) } else { 0 };
        }
        Self {
            rate: self.rate,
            hours: (label / (fps * 3600)) as u8,
            minutes: ((label / (fps * 60)) % 60) as u8,
            seconds: ((label / fps) % 60) as u8,
            frames: (label % fps) as u8,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssemblyDiagnostic {
    Discontinuity,
    Malformed,
    Timeout,
    Relocated,
    Reset,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TimecodeObservation {
    pub encoded: Position,
    pub compensated: Position,
    pub direction: i8,
    pub locked: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AssemblyResult {
    pub observation: Option<TimecodeObservation>,
    pub diagnostic: Option<AssemblyDiagnostic>,
}
#[derive(Clone, Debug)]
pub struct Assembler {
    timeout_ns: u64,
    last_now: Option<u64>,
    last_input: Option<u64>,
    nibbles: [u8; 8],
    expected: Option<u8>,
    direction: i8,
    previous: Option<Position>,
    locked: bool,
}
impl Assembler {
    pub fn new(timeout_ns: u64) -> Result<Self, ValueError> {
        if timeout_ns == 0 {
            return Err(ValueError("timeout_ns must be positive".into()));
        }
        Ok(Self {
            timeout_ns,
            last_now: None,
            last_input: None,
            nibbles: [0; 8],
            expected: None,
            direction: 0,
            previous: None,
            locked: false,
        })
    }
    pub fn locked(&self) -> bool {
        self.locked
    }
    pub fn reset(&mut self) {
        self.expected = None;
        self.direction = 0;
        self.previous = None;
        self.locked = false;
        self.last_input = None;
    }
    fn now(&mut self, now_ns: u64) -> Result<(), ValueError> {
        if self.last_now.is_some_and(|t| now_ns < t) {
            return Err(ValueError("time must be monotonic".into()));
        }
        self.last_now = Some(now_ns);
        Ok(())
    }
    pub fn poll(&mut self, now_ns: u64) -> Result<Option<AssemblyDiagnostic>, ValueError> {
        self.now(now_ns)?;
        if self
            .last_input
            .is_some_and(|t| now_ns - t >= self.timeout_ns)
        {
            self.reset();
            Ok(Some(AssemblyDiagnostic::Timeout))
        } else {
            Ok(None)
        }
    }
    pub fn relocate(
        &mut self,
        value: &FullFrame,
        now_ns: u64,
    ) -> Result<AssemblyResult, ValueError> {
        let position = Position::new(
            value.rate,
            value.hours.into(),
            value.minutes.into(),
            value.seconds.into(),
            value.frames.into(),
        )?;
        self.now(now_ns)?;
        self.reset();
        Ok(AssemblyResult {
            observation: Some(TimecodeObservation {
                encoded: position.clone(),
                compensated: position,
                direction: 0,
                locked: false,
            }),
            diagnostic: Some(AssemblyDiagnostic::Relocated),
        })
    }
    pub fn feed(
        &mut self,
        value: &crate::messages::QuarterFrame,
        now_ns: u64,
    ) -> Result<AssemblyResult, ValueError> {
        let mut diagnostic = self.poll(now_ns)?;
        let t = value.message_type();
        let n = value.value();
        self.last_input = Some(now_ns);
        if matches!(t, 1) && n > 1 || matches!(t, 3 | 5) && n > 3 || t == 7 && n > 7 {
            self.reset();
            return Ok(AssemblyResult {
                observation: None,
                diagnostic: Some(AssemblyDiagnostic::Malformed),
            });
        }
        if self.expected.is_some_and(|e| e != t) {
            self.reset();
            diagnostic = Some(AssemblyDiagnostic::Discontinuity);
            self.last_input = Some(now_ns);
        }
        if self.expected.is_none() {
            let direction = if t == 0 {
                1
            } else if t == 7 {
                -1
            } else {
                return Ok(AssemblyResult {
                    observation: None,
                    diagnostic,
                });
            };
            if self.direction != 0 && self.direction != direction {
                self.reset();
                diagnostic = Some(AssemblyDiagnostic::Discontinuity);
                self.last_input = Some(now_ns);
            }
            self.direction = direction;
        }
        self.nibbles[usize::from(t)] = n;
        let end = self.direction == 1 && t == 7 || self.direction == -1 && t == 0;
        self.expected = if end {
            None
        } else {
            Some((i16::from(t) + i16::from(self.direction)) as u8)
        };
        if !end {
            return Ok(AssemblyResult {
                observation: None,
                diagnostic,
            });
        }
        let n = &self.nibbles;
        let rate = match n[7] >> 1 {
            0 => FrameRate::Fps24,
            1 => FrameRate::Fps25,
            2 => FrameRate::DropFrame30,
            _ => FrameRate::Fps30,
        };
        let position = Position::new(
            rate,
            i64::from(n[6] | ((n[7] & 1) << 4)),
            i64::from(n[4] | n[5] << 4),
            i64::from(n[2] | n[3] << 4),
            i64::from(n[0] | n[1] << 4),
        );
        let Ok(encoded) = position else {
            self.reset();
            return Ok(AssemblyResult {
                observation: None,
                diagnostic: Some(AssemblyDiagnostic::Malformed),
            });
        };
        self.locked = self
            .previous
            .as_ref()
            .is_some_and(|p| p.advance(i64::from(self.direction) * 2) == encoded);
        if self.previous.is_some() && !self.locked {
            diagnostic = Some(AssemblyDiagnostic::Discontinuity);
        }
        self.previous = Some(encoded.clone());
        let compensated = encoded.advance(i64::from(self.direction) * 2);
        Ok(AssemblyResult {
            observation: Some(TimecodeObservation {
                encoded,
                compensated,
                direction: self.direction,
                locked: self.locked,
            }),
            diagnostic,
        })
    }
}
