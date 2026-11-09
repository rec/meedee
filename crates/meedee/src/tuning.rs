//! MIDI Tuning Standard values, codecs, caller-driven tracking and live queries.
pub use crate::event_views::Tuning as Decoder;
use crate::{
    drivers::{self, Receiver, Sender},
    events::Payload,
    live::Item,
    messages::{AnyMessage, Message, PrimitiveMessage, SysEx, ValueError},
    midi,
    playback::{Cancellation, MonotonicClock, PlaybackClock},
    tuning,
};
use std::io::{self, Write};

const STEPS: f64 = 16384.0;
const MAX_PITCH: u32 = 2_097_150; // 7F 7F 7F is not a frequency.

/// Build an ordinary or banked 128-key dump. Partial coverage and NoChange
/// entries require explicit preservation. Does not send anything.
pub fn bulk_dump(
    result: &[NoteTuning],
    device_id: i64,
    program: i64,
    name: String,
    bank: Option<i64>,
    preserve_unmapped: bool,
) -> Result<TuningMessage, ValueError> {
    check_selection(result, preserve_unmapped)?;
    if result.len() != 128 && !preserve_unmapped {
        return Err(error(
            "bulk dumps require all 128 keys or explicit preservation",
        ));
    }
    let mut values = [TuningValue::NoChange; 128];
    for change in result {
        values[change.note as usize] = change.pitch;
    }
    let table = TuningTable(values);
    match bank {
        None => Ok(BulkDump::new(device_id, program, name, table)?.into()),
        Some(bank) => Ok(KeyBasedDump::new(device_id, bank, program, name, table)?.into()),
    }
}

/// Build ordered messages of at most 127 keys each. Unbanked changes must
/// be real-time. NoChange entries require explicit preservation.
pub fn note_changes(
    result: &[NoteTuning],
    device_id: i64,
    program: i64,
    realtime: bool,
    bank: Option<i64>,
    preserve_unmapped: bool,
) -> Result<Vec<TuningMessage>, ValueError> {
    if bank.is_none() && !realtime {
        return Err(error(
            "unbanked note changes are real-time; use a bank for non-real-time",
        ));
    }
    check_selection(result, preserve_unmapped)?;
    result
        .chunks(127)
        .map(|part| match bank {
            None => Ok(SingleNoteChange::new(device_id, program, part.to_vec())?.into()),
            Some(bank) => {
                Ok(
                    BankedNoteChange::new(device_id, bank, program, realtime, part.to_vec())?
                        .into(),
                )
            }
        })
        .collect()
}

/// Exact absolute pitch in 1/16384-semitone units above MIDI note zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pitch(u32);
impl Pitch {
    pub fn new(semitone: i64, fraction: i64) -> Result<Self, ValueError> {
        let semitone = range(semitone, 127, "semitone")?;
        let fraction = range(fraction, 16383, "fraction")?;
        let units = semitone * 16384 + fraction;
        if units > MAX_PITCH {
            return Err(error("reserved no-change pitch"));
        }
        Ok(Self(units))
    }
    pub fn semitone(self) -> u8 {
        (self.0 / 16384) as u8
    }
    pub fn fraction(self) -> u16 {
        (self.0 % 16384) as u16
    }
    pub fn frequency_hz(self) -> f64 {
        440.0 * 2.0_f64.powf((self.0 as f64 / STEPS - 69.0) / 12.0)
    }
    pub fn from_frequency(frequency_hz: f64) -> Result<PitchConversion, ValueError> {
        if !frequency_hz.is_finite() || frequency_hz <= 0.0 {
            return Err(error("frequency_hz must be finite and positive"));
        }
        // Check Hz before log2/subtraction can round an out-of-range value
        // onto an endpoint of the finite pitch range.
        if frequency_hz < Self(0).frequency_hz() || frequency_hz > Self(MAX_PITCH).frequency_hz() {
            return Err(error("frequency_hz outside the MTS frequency range"));
        }
        Self::quantize(69.0 + 12.0 * (frequency_hz / 440.0).log2())
    }
    /// Offset in cents from the given equal-tempered MIDI note (A4 = 440 Hz).
    pub fn from_cents(note: i64, cents: f64) -> Result<PitchConversion, ValueError> {
        let note = range(note, 127, "note")?;
        Self::quantize(note as f64 + cents / 100.0)
    }
    fn quantize(semitones: f64) -> Result<PitchConversion, ValueError> {
        let requested = semitones * STEPS;
        if !requested.is_finite() || requested < 0.0 || requested > MAX_PITCH as f64 {
            return Err(error("pitch outside the MTS frequency range"));
        }
        let pitch = Self(requested.round_ties_even() as u32);
        Ok(PitchConversion {
            pitch,
            error_cents: (pitch.0 as f64 - requested) * 100.0 / STEPS,
        })
    }
    fn bytes(self) -> [u8; 3] {
        [
            self.semitone(),
            (self.fraction() >> 7) as u8,
            (self.fraction() & 127) as u8,
        ]
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PitchConversion {
    pub pitch: Pitch,
    pub error_cents: f64,
}

/// The reserved triplet is preserved rather than converted to a frequency.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TuningValue {
    Pitch(Pitch),
    NoChange,
}
impl TuningValue {
    fn bytes(self) -> [u8; 3] {
        match self {
            Self::Pitch(pitch) => pitch.bytes(),
            Self::NoChange => [127; 3],
        }
    }
    fn decode(bytes: &[u8]) -> Self {
        if bytes == [127; 3] {
            Self::NoChange
        } else {
            Self::Pitch(Pitch(
                (bytes[0] as u32 * 16384) + bytes[1] as u32 * 128 + bytes[2] as u32,
            ))
        }
    }
}
/// Immutable table indexed by all 128 MIDI note numbers.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TuningTable([TuningValue; 128]);
impl TuningTable {
    pub fn new(values: Vec<TuningValue>) -> Result<Self, ValueError> {
        Ok(Self(values.try_into().map_err(|_| {
            error("table must contain exactly 128 pitches or NoChange values")
        })?))
    }
    pub fn values(&self) -> &[TuningValue; 128] {
        &self.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NoteTuning {
    note: u8,
    pitch: TuningValue,
}
impl NoteTuning {
    pub fn new(note: i64, pitch: TuningValue) -> Result<Self, ValueError> {
        Ok(Self {
            note: range(note, 127, "note")? as u8,
            pitch,
        })
    }
    pub fn note(&self) -> u8 {
        self.note
    }
    pub fn pitch(&self) -> TuningValue {
        self.pitch
    }
}
/// Exact signed offsets C through B. Seven-bit units are cents; fourteen-bit
/// units are 100/8192 cents. The positive fourteen-bit limit is below 100 cents.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ScaleOffsets {
    resolution: u8,
    values: [i16; 12],
}
impl ScaleOffsets {
    pub fn new(resolution: i64, values: Vec<i64>) -> Result<Self, ValueError> {
        let bound = match resolution {
            7 => 64,
            14 => 8192,
            _ => return Err(error("resolution must be 7 or 14")),
        };
        let values: [i64; 12] = values
            .try_into()
            .map_err(|_| error("scale must contain exactly 12 offsets, C through B"))?;
        if values
            .iter()
            .any(|value| *value < -bound || *value >= bound)
        {
            return Err(error("scale offset outside its encoding range"));
        }
        Ok(Self {
            resolution: resolution as u8,
            values: values.map(|value| value as i16),
        })
    }
    pub fn resolution(&self) -> u8 {
        self.resolution
    }
    pub fn values(&self) -> &[i16; 12] {
        &self.values
    }
    pub fn from_cents(resolution: i64, cents: Vec<f64>) -> Result<ScaleConversion, ValueError> {
        let (step, bound) = match resolution {
            7 => (1.0, 64.0),
            14 => (100.0 / 8192.0, 8192.0),
            _ => return Err(error("resolution must be 7 or 14")),
        };
        if cents.len() != 12 {
            return Err(error("scale must contain exactly 12 offsets"));
        }
        let mut values = Vec::with_capacity(12);
        let mut errors = [0.0; 12];
        for (i, cents) in cents.into_iter().enumerate() {
            let requested = cents / step;
            if !requested.is_finite() || requested < -bound || requested > bound - 1.0 {
                return Err(error("scale offset outside its encoding range"));
            }
            let value = requested.round_ties_even();
            values.push(value as i64);
            errors[i] = value * step - cents;
        }
        Ok(ScaleConversion {
            offsets: Self::new(resolution, values)?,
            error_cents: errors,
        })
    }
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(24);
        for value in self.values {
            let raw = (value as i32 + if self.resolution == 7 { 64 } else { 8192 }) as u16;
            if self.resolution == 14 {
                out.push((raw >> 7) as u8);
            }
            out.push((raw & 127) as u8);
        }
        out
    }
    fn decode(resolution: u8, data: &[u8]) -> Self {
        let mut values = [0; 12];
        for (i, value) in values.iter_mut().enumerate() {
            *value = if resolution == 7 {
                data[i] as i16 - 64
            } else {
                (data[i * 2] as i16 * 128 + data[i * 2 + 1] as i16) - 8192
            };
        }
        Self { resolution, values }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct ScaleConversion {
    pub offsets: ScaleOffsets,
    pub error_cents: [f64; 12],
}

macro_rules! requests {
    ($($name:ident { $($field:ident),* };)*) => { $(
        #[derive(Clone, Debug)]
        pub struct $name { metadata: Option<Box<crate::messages::Metadata>>, device_id:u8, $($field:u8),* }
        crate::messages::value_metadata!($name { device_id, $($field),* });
        impl $name {
            pub fn new(device_id:i64, $($field:i64),*) -> Result<Self,ValueError> {
                Ok(Self {metadata: None, device_id:range(device_id,127,"device_id")? as u8, $($field:range($field,127,stringify!($field))? as u8),*})
            }
            pub fn device_id(&self)->u8 {self.device_id}
            $(pub fn $field(&self)->u8 {self.$field})*
        }
    )* };
}
requests! { BulkDumpRequest {program}; BankedDumpRequest {bank,program}; }

macro_rules! dumps {
    ($($name:ident {$($extra:ident),*} $value:ident : $ty:ty;)*) => { $(
        #[derive(Clone, Debug)]
        pub struct $name {metadata: Option<Box<crate::messages::Metadata>>, device_id:u8, $($extra:u8,)* program:u8, name:String, $value:$ty}
        crate::messages::value_metadata!($name { device_id, $($extra,)* program, name, $value });
        impl $name {
            pub fn new(device_id:i64, $($extra:i64,)* program:i64, name:String, $value:$ty)->Result<Self,ValueError> {
                if !name.is_ascii() || name.len()>16 {return Err(error("tuning name must be at most 16 ASCII characters"));}
                Ok(Self {metadata: None, device_id:range(device_id,127,"device_id")? as u8,$($extra:range($extra,127,stringify!($extra))? as u8,)* program:range(program,127,"program")? as u8,name:format!("{name:<16}"),$value})
            }
            pub fn device_id(&self)->u8 {self.device_id}
            $(pub fn $extra(&self)->u8 {self.$extra})*
            pub fn program(&self)->u8 {self.program}
            pub fn name(&self)->&str {&self.name}
            pub fn $value(&self)->&$ty {&self.$value}
        }
    )* };
}
dumps! {
    BulkDump {} table:TuningTable;
    KeyBasedDump {bank} table:TuningTable;
    ScaleOctaveDump {bank} offsets:ScaleOffsets;
}
#[derive(Clone, Debug)]
pub struct SingleNoteChange {
    metadata: Option<Box<crate::messages::Metadata>>,
    device_id: u8,
    program: u8,
    changes: Box<[NoteTuning]>,
}
impl SingleNoteChange {
    pub fn new(device_id: i64, program: i64, changes: Vec<NoteTuning>) -> Result<Self, ValueError> {
        check_changes(&changes)?;
        Ok(Self {
            metadata: None,
            device_id: range(device_id, 127, "device_id")? as u8,
            program: range(program, 127, "program")? as u8,
            changes: changes.into_boxed_slice(),
        })
    }
    pub fn device_id(&self) -> u8 {
        self.device_id
    }
    pub fn program(&self) -> u8 {
        self.program
    }
    pub fn changes(&self) -> &[NoteTuning] {
        &self.changes
    }
}
#[derive(Clone, Debug)]
pub struct BankedNoteChange {
    metadata: Option<Box<crate::messages::Metadata>>,
    device_id: u8,
    bank: u8,
    program: u8,
    realtime: bool,
    changes: Box<[NoteTuning]>,
}
impl BankedNoteChange {
    pub fn new(
        device_id: i64,
        bank: i64,
        program: i64,
        realtime: bool,
        changes: Vec<NoteTuning>,
    ) -> Result<Self, ValueError> {
        check_changes(&changes)?;
        Ok(Self {
            metadata: None,
            device_id: range(device_id, 127, "device_id")? as u8,
            bank: range(bank, 127, "bank")? as u8,
            program: range(program, 127, "program")? as u8,
            realtime,
            changes: changes.into_boxed_slice(),
        })
    }
    pub fn device_id(&self) -> u8 {
        self.device_id
    }
    pub fn bank(&self) -> u8 {
        self.bank
    }
    pub fn program(&self) -> u8 {
        self.program
    }
    pub fn realtime(&self) -> bool {
        self.realtime
    }
    pub fn changes(&self) -> &[NoteTuning] {
        &self.changes
    }
}
#[derive(Clone, Debug)]
pub struct ScaleOctaveChange {
    metadata: Option<Box<crate::messages::Metadata>>,
    device_id: u8,
    realtime: bool,
    channels: Box<[u8]>,
    offsets: ScaleOffsets,
}
impl ScaleOctaveChange {
    pub fn new(
        device_id: i64,
        realtime: bool,
        channels: Vec<i64>,
        offsets: ScaleOffsets,
    ) -> Result<Self, ValueError> {
        if channels.len() > 16 {
            return Err(error(
                "channels must contain at most 16 distinct channel numbers",
            ));
        }
        let mut selected = Vec::with_capacity(channels.len());
        for channel in channels {
            if !(1..=16).contains(&channel) {
                return Err(error("channels must be in 1..=16"));
            }
            selected.push(channel as u8);
        }
        selected.sort_unstable();
        if selected.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(error("channels must be distinct"));
        }
        Ok(Self {
            metadata: None,
            device_id: range(device_id, 127, "device_id")? as u8,
            realtime,
            channels: selected.into_boxed_slice(),
            offsets,
        })
    }
    pub fn device_id(&self) -> u8 {
        self.device_id
    }
    pub fn realtime(&self) -> bool {
        self.realtime
    }
    pub fn channels(&self) -> &[u8] {
        &self.channels
    }
    pub fn offsets(&self) -> &ScaleOffsets {
        &self.offsets
    }
}

macro_rules! message_types {
    ($($name:ident),*) => {
        #[derive(Clone,Debug,PartialEq,Eq,Hash)]
        pub enum TuningMessage {$($name($name)),*}
        crate::messages::enum_metadata!(TuningMessage { $($name),* });
        $(
        impl From<$name> for TuningMessage {fn from(value:$name)->Self {Self::$name(value)}}
        impl PrimitiveMessage for $name {fn encode(&self) -> Result<Vec<u8>, crate::messages::ValueError> {TuningMessage::from(self.clone()).encode()}})*
    };
}
message_types!(
    BulkDumpRequest,
    BankedDumpRequest,
    BulkDump,
    KeyBasedDump,
    ScaleOctaveDump,
    SingleNoteChange,
    BankedNoteChange,
    ScaleOctaveChange
);
impl PrimitiveMessage for TuningMessage {
    fn encode(&self) -> Result<Vec<u8>, crate::messages::ValueError> {
        let mut payload = Vec::new();
        let mut checksum = false;
        match self {
            Self::BulkDumpRequest(value) => {
                payload.extend([126, value.device_id, 8, 0, value.program])
            }
            Self::BankedDumpRequest(value) => {
                payload.extend([126, value.device_id, 8, 3, value.bank, value.program])
            }
            Self::BulkDump(value) => {
                payload.extend([126, value.device_id, 8, 1, value.program]);
                payload.extend(value.name.bytes());
                push_table(&mut payload, &value.table);
                checksum = true;
            }
            Self::KeyBasedDump(value) => {
                payload.extend([126, value.device_id, 8, 4, value.bank, value.program]);
                payload.extend(value.name.bytes());
                push_table(&mut payload, &value.table);
                checksum = true;
            }
            Self::ScaleOctaveDump(value) => {
                payload.extend([
                    126,
                    value.device_id,
                    8,
                    if value.offsets.resolution == 7 { 5 } else { 6 },
                    value.bank,
                    value.program,
                ]);
                payload.extend(value.name.bytes());
                payload.extend(value.offsets.bytes());
                checksum = true;
            }
            Self::SingleNoteChange(value) => {
                payload.extend([
                    127,
                    value.device_id,
                    8,
                    2,
                    value.program,
                    value.changes.len() as u8,
                ]);
                push_changes(&mut payload, &value.changes);
            }
            Self::BankedNoteChange(value) => {
                payload.extend([
                    if value.realtime { 127 } else { 126 },
                    value.device_id,
                    8,
                    7,
                    value.bank,
                    value.program,
                    value.changes.len() as u8,
                ]);
                push_changes(&mut payload, &value.changes);
            }
            Self::ScaleOctaveChange(value) => {
                payload.extend([
                    if value.realtime { 127 } else { 126 },
                    value.device_id,
                    8,
                    if value.offsets.resolution == 7 { 8 } else { 9 },
                ]);
                let mask = value
                    .channels
                    .iter()
                    .fold(0u16, |mask, channel| mask | (1u16 << (*channel - 1)));
                payload.extend([
                    (mask >> 14) as u8,
                    ((mask >> 7) & 127) as u8,
                    (mask & 127) as u8,
                ]);
                payload.extend(value.offsets.bytes());
            }
        }
        if checksum {
            payload.push(xor(&payload));
        }
        let mut wire = Vec::with_capacity(payload.len() + 2);
        wire.push(240);
        wire.extend(payload);
        wire.push(247);
        Ok(wire)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChecksumStatus {
    NotPresent,
    Matches,
    LegacyMismatch,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Decoded {
    pub message: TuningMessage,
    pub checksum: ChecksumStatus,
}
/// Interpret a complete SysEx. Unrecognized (including future MTS) forms return
/// None; recognized malformed forms return an error. Legacy checksum mismatches
/// remain visible but do not discard a structurally valid original bulk dump.
pub fn decode(message: &SysEx) -> Result<Option<Decoded>, ValueError> {
    let p = message.data();
    if p.len() < 3 || !matches!(p[0], 126 | 127) || p[2] != 8 {
        return Ok(None);
    }
    if p.len() < 4 {
        return Err(error("truncated MTS header"));
    }
    let rt = p[0] == 127;
    let id = p[1] as i64;
    let kind = p[3];
    let known = matches!(
        (rt, kind),
        (false, 0 | 1 | 3 | 4 | 5 | 6) | (true, 2) | (_, 7..=9)
    );
    if !known {
        return Ok(None);
    }
    let mut checksum = ChecksumStatus::NotPresent;
    let message = match kind {
        0 => {
            length(p, 5)?;
            BulkDumpRequest::new(id, p[4] as i64)?.into()
        }
        3 => {
            length(p, 6)?;
            BankedDumpRequest::new(id, p[4] as i64, p[5] as i64)?.into()
        }
        1 | 4 | 5 | 6 => {
            let header = if kind == 1 { 5 } else { 6 };
            let size = match kind {
                1 | 4 => 384,
                5 => 12,
                _ => 24,
            };
            length(p, header + 16 + size + 1)?;
            checksum = if xor(p) == 0 {
                ChecksumStatus::Matches
            } else if kind == 1 {
                ChecksumStatus::LegacyMismatch
            } else {
                return Err(error("invalid MTS dump checksum"));
            };
            let name = String::from_utf8(p[header..header + 16].to_vec())
                .map_err(|_| error("invalid ASCII name"))?;
            let data = &p[header + 16..p.len() - 1];
            match kind {
                1 => BulkDump::new(id, p[4] as i64, name, decode_table(data))?.into(),
                4 => KeyBasedDump::new(id, p[4] as i64, p[5] as i64, name, decode_table(data))?
                    .into(),
                _ => ScaleOctaveDump::new(
                    id,
                    p[4] as i64,
                    p[5] as i64,
                    name,
                    ScaleOffsets::decode(if kind == 5 { 7 } else { 14 }, data),
                )?
                .into(),
            }
        }
        2 | 7 => {
            let header = if kind == 2 { 6 } else { 7 };
            if p.len() < header {
                return Err(error("truncated single-note tuning header"));
            }
            let count = p[header - 1] as usize;
            length(p, header + count * 4)?;
            let changes = p[header..]
                .chunks_exact(4)
                .map(|entry| NoteTuning {
                    note: entry[0],
                    pitch: TuningValue::decode(&entry[1..]),
                })
                .collect();
            if kind == 2 {
                SingleNoteChange::new(id, p[4] as i64, changes)?.into()
            } else {
                BankedNoteChange::new(id, p[4] as i64, p[5] as i64, rt, changes)?.into()
            }
        }
        _ => {
            let resolution = if kind == 8 { 7 } else { 14 };
            length(p, 7 + if resolution == 7 { 12 } else { 24 })?;
            if p[4] & 124 != 0 {
                return Err(error("reserved scale channel bits must be zero"));
            }
            let mask = (p[4] as u16) << 14 | (p[5] as u16) << 7 | p[6] as u16;
            let channels = (1..=16)
                .filter(|channel| mask & (1 << (*channel - 1)) != 0)
                .collect();
            ScaleOctaveChange::new(id, rt, channels, ScaleOffsets::decode(resolution, &p[7..]))?
                .into()
        }
    };
    Ok(Some(Decoded { message, checksum }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RequestStatus {
    Pending,
    Completed,
    TimedOut,
    Cancelled,
}
/// One outstanding unicast dump request. Caller supplies monotonic integer ns
/// from one clock and retains unrelated messages. MTS has no transaction ID:
/// identical unsolicited/late replies cannot be distinguished from a response.
#[derive(Clone, Debug)]
pub struct ReplyTracker {
    device_id: u8,
    bank: Option<u8>,
    program: u8,
    deadline_ns: u64,
    last_ns: u64,
    status: RequestStatus,
}
impl ReplyTracker {
    pub fn new(request: &TuningMessage, now_ns: u64, timeout_ns: u64) -> Result<Self, ValueError> {
        let (device_id, bank, program) = match request {
            TuningMessage::BulkDumpRequest(value) => (value.device_id, None, value.program),
            TuningMessage::BankedDumpRequest(value) => {
                (value.device_id, Some(value.bank), value.program)
            }
            _ => return Err(error("tracker requires a dump request")),
        };
        if device_id == 127 {
            return Err(error(
                "reply tracking requires a specific device_id, not broadcast",
            ));
        }
        if timeout_ns == 0 {
            return Err(error("timeout_ns must be positive"));
        }
        let deadline_ns = now_ns
            .checked_add(timeout_ns)
            .ok_or_else(|| error("deadline exceeds u64 nanoseconds"))?;
        Ok(Self {
            device_id,
            bank,
            program,
            deadline_ns,
            last_ns: now_ns,
            status: RequestStatus::Pending,
        })
    }
    pub fn status(&self) -> RequestStatus {
        self.status
    }
    pub fn deadline_ns(&self) -> u64 {
        self.deadline_ns
    }
    pub fn poll(&mut self, now_ns: u64) -> Result<RequestStatus, ValueError> {
        if now_ns < self.last_ns {
            return Err(error("now_ns must not move backwards"));
        }
        self.last_ns = now_ns;
        if self.status == RequestStatus::Pending && now_ns >= self.deadline_ns {
            self.status = RequestStatus::TimedOut;
        }
        Ok(self.status)
    }
    pub fn cancel(&mut self) -> RequestStatus {
        if self.status == RequestStatus::Pending {
            self.status = RequestStatus::Cancelled;
        }
        self.status
    }
    pub fn observe(&mut self, message: &TuningMessage, now_ns: u64) -> Result<bool, ValueError> {
        if self.poll(now_ns)? != RequestStatus::Pending {
            return Ok(false);
        }
        let matches = match message {
            TuningMessage::BulkDump(value) => {
                self.bank.is_none()
                    && value.device_id == self.device_id
                    && value.program == self.program
            }
            TuningMessage::KeyBasedDump(value) => {
                self.bank == Some(value.bank)
                    && value.device_id == self.device_id
                    && value.program == self.program
            }
            TuningMessage::ScaleOctaveDump(value) => {
                self.bank == Some(value.bank)
                    && value.device_id == self.device_id
                    && value.program == self.program
            }
            _ => false,
        };
        if matches {
            self.status = RequestStatus::Completed;
        }
        Ok(matches)
    }
}
fn error(message: &str) -> ValueError {
    ValueError(message.into())
}
fn range(value: i64, max: i64, name: &str) -> Result<u32, ValueError> {
    if !(0..=max).contains(&value) {
        Err(ValueError(format!("{name} must be in 0..={max}")))
    } else {
        Ok(value as u32)
    }
}
fn length(data: &[u8], expected: usize) -> Result<(), ValueError> {
    if data.len() != expected {
        Err(error("incorrect MTS message length"))
    } else {
        Ok(())
    }
}
fn xor(data: &[u8]) -> u8 {
    data.iter().fold(0, |sum, byte| sum ^ byte) & 127
}
fn check_changes(changes: &[NoteTuning]) -> Result<(), ValueError> {
    if changes.len() > 127 {
        Err(error("changes must contain at most 127 entries"))
    } else {
        Ok(())
    }
}
fn check_selection(changes: &[NoteTuning], preserve_unmapped: bool) -> Result<(), ValueError> {
    if !(1..=128).contains(&changes.len()) {
        return Err(error("select between one and 128 MIDI keys"));
    }
    let mut seen = [false; 128];
    for change in changes {
        if std::mem::replace(&mut seen[change.note as usize], true) {
            return Err(error("selected MIDI keys must be distinct"));
        }
    }
    if !preserve_unmapped && changes.iter().any(|c| c.pitch == TuningValue::NoChange) {
        return Err(error("NoChange requires explicit preservation"));
    }
    Ok(())
}
fn push_table(out: &mut Vec<u8>, table: &TuningTable) {
    for pitch in table.0 {
        out.extend(pitch.bytes());
    }
}
fn push_changes(out: &mut Vec<u8>, changes: &[NoteTuning]) {
    for change in changes {
        out.push(change.note);
        out.extend(change.pitch.bytes());
    }
}
fn decode_table(data: &[u8]) -> TuningTable {
    let mut values = [TuningValue::NoChange; 128];
    for (value, data) in values.iter_mut().zip(data.chunks_exact(3)) {
        *value = TuningValue::decode(data);
    }
    TuningTable(values)
}

crate::messages::value_metadata!(SingleNoteChange {
    device_id,
    program,
    changes
});

crate::messages::value_metadata!(BankedNoteChange {
    device_id,
    bank,
    program,
    realtime,
    changes
});

crate::messages::value_metadata!(ScaleOctaveChange {
    device_id,
    realtime,
    channels,
    offsets
});

#[derive(Clone, Debug)]
pub struct QueryReport {
    pub status: &'static str,
    pub request_sent: bool,
    pub uncertain_send: bool,
    pub ignored_events: u64,
    pub deadline_ns: Option<u64>,
    pub reply: Option<SysEx>,
    pub decoded: Option<Decoded>,
    pub reason: Option<String>,
}
pub struct Query {
    request: TuningMessage,
    stream: String,
    timeout_ns: u64,
    tracker: Option<ReplyTracker>,
    report: QueryReport,
}
impl Query {
    pub fn new(
        request: TuningMessage,
        stream: String,
        timeout_ns: u64,
    ) -> Result<Self, ValueError> {
        if stream.is_empty() {
            return Err(invalid("query requires an input stream"));
        }
        ReplyTracker::new(&request, 0, timeout_ns)?;
        Ok(Self {
            request,
            stream,
            timeout_ns,
            tracker: None,
            report: QueryReport {
                status: "ready",
                request_sent: false,
                uncertain_send: false,
                ignored_events: 0,
                deadline_ns: None,
                reply: None,
                decoded: None,
                reason: None,
            },
        })
    }
    pub fn prepare(&mut self, now_ns: u64) -> Result<SysEx, ValueError> {
        if self.report.status != "ready" {
            return Err(invalid("query must be fresh"));
        }
        let tracker = ReplyTracker::new(&self.request, now_ns, self.timeout_ns)?;
        self.report.deadline_ns = Some(tracker.deadline_ns());
        self.tracker = Some(tracker);
        self.report.status = "sending";
        let wire = self.request.encode().expect("validated system message");
        SysEx::new(wire[1..wire.len() - 1].to_vec())
    }
    pub fn confirm(&mut self, now_ns: u64) -> Result<(), ValueError> {
        if self.report.status != "sending" {
            return Err(invalid("no query send is pending"));
        }
        // Validate time before recording the known whole-message handoff.
        let status = self.tracker.as_mut().expect("started query").poll(now_ns)?;
        self.report.request_sent = true;
        self.report.status = "waiting";
        if status == RequestStatus::TimedOut {
            self.finish("timed_out", "query deadline expired during send");
        }
        Ok(())
    }
    pub fn poll(&mut self, now_ns: u64) -> Result<(), ValueError> {
        if !matches!(self.report.status, "sending" | "waiting") {
            return Err(invalid("query is not active"));
        }
        if self.tracker.as_mut().expect("started query").poll(now_ns)? == RequestStatus::TimedOut {
            self.finish("timed_out", "query deadline expired");
        }
        Ok(())
    }
    pub fn feed(&mut self, item: &Item, now_ns: u64) -> Result<(), ValueError> {
        if self.report.status != "waiting" {
            return Err(invalid("query must have a confirmed send before receiving"));
        }
        self.poll(now_ns)?;
        if self.report.status != "waiting" {
            return Ok(());
        }
        let event = match item {
            Item::Diagnostic(d) => {
                self.fail(&format!("input diagnostic {:?}: {}", d.kind, d.reason));
                return Ok(());
            }
            Item::Message(e) => e,
        };
        if event.stream().unwrap_or_default() != self.stream {
            self.fail("query received an unrelated source");
            return Ok(());
        }
        match event.as_ref() {
            Payload::Midi1(AnyMessage::SysEx(raw)) => {
                let decoded = match tuning::decode(raw) {
                    Ok(v) => v,
                    Err(e) => {
                        self.fail(&e.to_string());
                        return Ok(());
                    }
                };
                if let Some(decoded) = decoded {
                    if self
                        .tracker
                        .as_mut()
                        .expect("started query")
                        .observe(&decoded.message, now_ns)?
                    {
                        self.report.reply = Some(raw.clone());
                        self.report.decoded = Some(decoded);
                        self.report.status = "completed";
                        return Ok(());
                    }
                }
            }
            Payload::Midi1(_) => {}
            _ => {
                self.fail("query requires MIDI 1 byte events");
                return Ok(());
            }
        }
        self.report.ignored_events = self
            .report
            .ignored_events
            .checked_add(1)
            .ok_or_else(|| invalid("query ignored-event count exhausted"))?;
        Ok(())
    }
    pub fn fail(&mut self, reason: &str) {
        if matches!(self.report.status, "ready" | "sending" | "waiting") {
            self.finish("failed", reason);
        }
    }
    pub fn cancel(&mut self, reason: &str) {
        if matches!(self.report.status, "ready" | "sending" | "waiting") {
            self.finish("cancelled", reason);
        }
    }
    pub fn snapshot(&self) -> QueryReport {
        self.report.clone()
    }
    pub fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        let reply = self
            .report
            .reply
            .as_ref()
            .filter(|_| self.report.status == "completed")
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "only a completed query can be exported",
                )
            })?;
        crate::sysex::write(writer, reply, reply.data().len())?;
        writer.flush()
    }
    fn finish(&mut self, status: &'static str, reason: &str) {
        self.report.uncertain_send = self.report.status == "sending";
        self.report.status = status;
        self.report.reason = Some(reason.chars().take(1024).collect());
    }
}
fn invalid(reason: &str) -> ValueError {
    ValueError(reason.into())
}

pub fn query(
    query: &mut Query,
    receiver: &mut midi::Input,
    output: &mut midi::Output,
    cancel: Option<&Cancellation>,
) -> midi::Result<QueryReport> {
    if query.snapshot().status != "ready" {
        return Err(drivers::invalid("query must be fresh"));
    }
    receiver.stream()?;
    query_with(
        query,
        receiver,
        output,
        &mut MonotonicClock::default(),
        cancel.unwrap_or(&Cancellation::default()),
    )
}
#[doc(hidden)]
pub fn query_with(
    query: &mut Query,
    receiver: &mut impl Receiver,
    output: &mut impl Sender,
    clock: &mut impl PlaybackClock,
    cancel: &Cancellation,
) -> midi::Result<QueryReport> {
    if query.snapshot().status != "ready" {
        return Err(drivers::invalid("query must be fresh"));
    }
    while matches!(query.snapshot().status, "ready" | "waiting") {
        if cancel.cancelled() {
            query.cancel("cancelled by caller");
            break;
        }
        if let Err(error) = receiver.check() {
            if drivers::interrupted(&error) {
                query.cancel("interrupted");
            } else {
                query.fail(&error.to_string());
            }
            break;
        }
        if query.snapshot().status == "ready" {
            let request = query.prepare(clock.now_ns())?;
            match output.send(crate::events::Payload::Midi1(
                crate::messages::AnyMessage::SysEx(request),
            )) {
                Ok(()) => query.confirm(clock.now_ns())?,
                Err(error) if drivers::interrupted(&error) => query.cancel("interrupted"),
                Err(error) => query.fail(&error.to_string()),
            }
            continue;
        }
        query.poll(clock.now_ns())?;
        let report = query.snapshot();
        if report.status != "waiting" {
            break;
        }
        let wait = drivers::remaining(clock, report.deadline_ns.expect("waiting query deadline"));
        match receiver.receive(wait) {
            Ok(item) => {
                if let Err(error) = receiver.check() {
                    if drivers::interrupted(&error) {
                        query.cancel("interrupted");
                    } else {
                        query.fail(&error.to_string());
                    }
                    break;
                }
                if cancel.cancelled() {
                    query.cancel("cancelled by caller");
                    break;
                }
                if let Some(item) = item {
                    query.feed(&item, clock.now_ns())?;
                }
            }
            Err(error) if drivers::interrupted(&error) => query.cancel("interrupted"),
            Err(error) => query.fail(&error.to_string()),
        }
    }
    Ok(query.snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::tests::{Clock, Input, Output};
    use crate::tuning::BulkDumpRequest;
    fn query() -> Query {
        Query::new(BulkDumpRequest::new(1, 2).unwrap().into(), "s".into(), 1000).unwrap()
    }
    #[test]
    fn query_sends_once_times_out_and_retains_uncertain_failure() {
        let mut clock = Clock::default();
        let mut input = Input {
            clock: clock.clone(),
            items: [].into(),
            lost: false,
        };
        let mut output = Output {
            clock: clock.clone(),
            sent: vec![],
            fail_at: None,
        };
        let report = query_with(
            &mut query(),
            &mut input,
            &mut output,
            &mut clock,
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(report.status, "timed_out");
        assert!(report.request_sent);
        assert_eq!(output.sent.len(), 1);
        output.fail_at = Some(1);
        let report = query_with(
            &mut query(),
            &mut input,
            &mut output,
            &mut clock,
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(report.status, "failed");
        assert!(report.uncertain_send);
        assert!(!report.request_sent);
        input.lost = true;
        let report = query_with(
            &mut query(),
            &mut input,
            &mut output,
            &mut clock,
            &Cancellation::default(),
        )
        .unwrap();
        assert!(!report.uncertain_send);
        assert_eq!(output.sent.len(), 1);
    }
}
