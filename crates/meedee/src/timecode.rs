//! MTC full-frame wire values only. No quarter-frame assembly or clock locking.
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
