//! Selected universal MIDI 1.0 messages. Explicit device addressing, no I/O.
use crate::messages::{Message, PrimitiveMessage, SysEx, ValueError};

macro_rules! device_messages {
    ($($name:ident { $($field:ident : $ty:ty = $low:literal ..= $high:literal),* };)*) => {$(
        #[derive(Clone,Debug,PartialEq,Eq,Hash)]
        pub struct $name {device_id:u8,$($field:$ty),*}
        impl $name {
            pub fn new(device_id:i64,$($field:i64),*)->Result<Self,ValueError> {
                Ok(Self {device_id:bounded(device_id,"device_id",0,127)? as u8,
                    $($field:bounded($field,stringify!($field),$low,$high)? as $ty),*})
            }
            pub fn device_id(&self)->u8 {self.device_id}
            $(pub fn $field(&self)->$ty {self.$field})*
        }
        impl Message for $name {}
    )*};
}
device_messages! {
    IdentityRequest {};
    MasterVolume {value:u16 = 0..=16383};
    MasterFineTuning {value:i16 = -8192..=8191};
    MasterCoarseTuning {semitones:i8 = -64..=63};
}
impl PrimitiveMessage for IdentityRequest {
    fn encode(&self) -> Vec<u8> {
        vec![240, 126, self.device_id, 6, 1, 247]
    }
}
impl PrimitiveMessage for MasterVolume {
    fn encode(&self) -> Vec<u8> {
        device_control(self.device_id, 1, self.value)
    }
}
impl PrimitiveMessage for MasterFineTuning {
    fn encode(&self) -> Vec<u8> {
        device_control(self.device_id, 3, (self.value as i32 + 8192) as u16)
    }
}
impl PrimitiveMessage for MasterCoarseTuning {
    fn encode(&self) -> Vec<u8> {
        device_control(
            self.device_id,
            4,
            ((self.semitones as i16 + 64) as u16) << 7,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IdentityReply {
    device_id: u8,
    manufacturer: Box<[u8]>,
    family: u16,
    model: u16,
    revision: [u8; 4],
}
impl IdentityReply {
    pub fn new(
        device_id: i64,
        manufacturer: Vec<i64>,
        family: i64,
        model: i64,
        revision: Vec<i64>,
    ) -> Result<Self, ValueError> {
        if !matches!(manufacturer.len(), 1 | 3) || revision.len() != 4 {
            return Err(ValueError(
                "manufacturer requires one or three bytes and revision requires four".into(),
            ));
        }
        let manufacturer = seven_bit(manufacturer)?;
        if !((manufacturer.len() == 1 && manufacturer[0] != 0)
            || (manufacturer.len() == 3 && manufacturer[0] == 0))
        {
            return Err(ValueError(
                "manufacturer must be one nonzero byte or three bytes starting with zero".into(),
            ));
        }
        let revision: [u8; 4] = seven_bit(revision)?
            .try_into()
            .map_err(|_| ValueError("revision must contain four seven-bit bytes".into()))?;
        Ok(Self {
            device_id: bounded(device_id, "device_id", 0, 127)? as u8,
            manufacturer: manufacturer.into_boxed_slice(),
            family: bounded(family, "family", 0, 16383)? as u16,
            model: bounded(model, "model", 0, 16383)? as u16,
            revision,
        })
    }
    pub fn device_id(&self) -> u8 {
        self.device_id
    }
    pub fn manufacturer(&self) -> &[u8] {
        &self.manufacturer
    }
    pub fn family(&self) -> u16 {
        self.family
    }
    pub fn model(&self) -> u16 {
        self.model
    }
    pub fn revision(&self) -> &[u8; 4] {
        &self.revision
    }
}
impl Message for IdentityReply {}
impl PrimitiveMessage for IdentityReply {
    fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![240, 126, self.device_id, 6, 2];
        bytes.extend_from_slice(&self.manufacturer);
        bytes.extend([
            (self.family & 127) as u8,
            (self.family >> 7) as u8,
            (self.model & 127) as u8,
            (self.model >> 7) as u8,
        ]);
        bytes.extend(self.revision);
        bytes.push(247);
        bytes
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum UniversalMessage {
    IdentityRequest(IdentityRequest),
    IdentityReply(IdentityReply),
    MasterVolume(MasterVolume),
    MasterFineTuning(MasterFineTuning),
    MasterCoarseTuning(MasterCoarseTuning),
}
pub fn decode(message: &SysEx) -> Result<Option<UniversalMessage>, ValueError> {
    let data = message.data();
    if data.len() < 3 || !matches!((data[0], data[2]), (126, 6) | (127, 4)) {
        return Ok(None);
    }
    if data.len() < 4 {
        return Err(ValueError("truncated universal header".into()));
    }
    let id = data[1].into();
    Ok(Some(match (data[0], data[3]) {
        (126, 1) => {
            length(data, 4)?;
            UniversalMessage::IdentityRequest(IdentityRequest::new(id)?)
        }
        (126, 2) => {
            if data.len() < 5 {
                return Err(ValueError("truncated manufacturer ID".into()));
            }
            let end = if data[4] == 0 { 7 } else { 5 };
            length(data, end + 8)?;
            UniversalMessage::IdentityReply(IdentityReply::new(
                id,
                data[4..end].iter().map(|&v| v as i64).collect(),
                data[end] as i64 + 128 * data[end + 1] as i64,
                data[end + 2] as i64 + 128 * data[end + 3] as i64,
                data[end + 4..].iter().map(|&v| v as i64).collect(),
            )?)
        }
        (127, 1 | 3 | 4) => {
            length(data, 6)?;
            let raw = data[4] as i64 + 128 * data[5] as i64;
            match data[3] {
                1 => UniversalMessage::MasterVolume(MasterVolume::new(id, raw)?),
                3 => UniversalMessage::MasterFineTuning(MasterFineTuning::new(id, raw - 8192)?),
                _ => {
                    if data[4] != 0 {
                        return Err(ValueError("master coarse tuning LSB must be zero".into()));
                    }
                    UniversalMessage::MasterCoarseTuning(MasterCoarseTuning::new(
                        id,
                        data[5] as i64 - 64,
                    )?)
                }
            }
        }
        _ => return Ok(None),
    }))
}
fn device_control(device_id: u8, kind: u8, value: u16) -> Vec<u8> {
    vec![
        240,
        127,
        device_id,
        4,
        kind,
        (value & 127) as u8,
        (value >> 7) as u8,
        247,
    ]
}
fn bounded(value: i64, name: &str, low: i64, high: i64) -> Result<i64, ValueError> {
    if !(low..=high).contains(&value) {
        Err(ValueError(format!("{name} must be in {low}..={high}")))
    } else {
        Ok(value)
    }
}
fn seven_bit(values: Vec<i64>) -> Result<Vec<u8>, ValueError> {
    values
        .into_iter()
        .map(|value| Ok(bounded(value, "byte", 0, 127)? as u8))
        .collect()
}
fn length(data: &[u8], expected: usize) -> Result<(), ValueError> {
    if data.len() != expected {
        Err(ValueError("incorrect universal message length".into()))
    } else {
        Ok(())
    }
}
