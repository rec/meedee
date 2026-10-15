//! M2-104-UM 1.1.2 values and pure codecs. Word order is explicit; byte order
//! belongs to the selected transport/file format and must be specified by callers.
use crate::codecs::{DecodeItem, Decoder as ByteDecoder};
use crate::messages::{AnyMessage, Message, PrimitiveMessage, SysEx, ValueError};
#[path = "ump_stream.rs"]
mod streams;
#[path = "ump_translation.rs"]
mod translation;
pub use streams::{
    packetize_flex_text, packetize_stream_text, AssembledData, Assembler, AssemblyDiagnostic,
    AssemblyItem, DataKind, DecodeDiagnostic, DecodeOutput, Decoder,
};
pub use translation::{scale_up, to_midi1, to_midi2, Translation, TranslationStatus};
#[path = "ump_helpers.rs"]
mod helpers;
pub use helpers::{
    device_identity, end_of_clip, endpoint_discovery, endpoint_info, function_block_discovery,
    function_block_info, key_signature, metronome, start_of_clip, stream_configuration, tempo,
    time_signature,
};
include!("ump_values.rs");

pub trait UmpValue: Message {
    fn encode_words(&self) -> Vec<u32>;
}
macro_rules! scalar {
    ($($name:ident [$first:expr,$second:expr] {$($field:ident:$ty:ty = $lo:literal ..= $hi:literal => [$word:literal,$shift:literal,$mask:literal,$bias:literal]),*};)*)=>{
        $(#[derive(Clone,Debug,PartialEq,Eq,Hash)]
        pub struct $name {$($field:$ty),*}
        impl $name {
            #[allow(clippy::too_many_arguments)]
            pub fn new($($field:i64),*)->Result<Self,ValueError>{
                $(let $field=bounded($field,$lo,$hi,stringify!($field))? as $ty;)*
                let result=Self{$($field),*};validate_scalar(&result.encode_words())?;Ok(result)
            }
            $(pub fn $field(&self)->$ty{self.$field})*
        }
        impl Message for $name{}
        impl UmpValue for $name {fn encode_words(&self)->Vec<u32>{
            let mut words=vec![$first as u32,$second as u32];
            $(words[$word]|=((self.$field as i64+$bias) as u32)<<$shift;)*
            words.truncate(word_count(words[0]));words
        }})*
        #[derive(Clone,Debug,PartialEq,Eq,Hash)]
        pub enum UmpMessage {$($name($name),)* Midi1(Midi1Packet),SysEx7(SysEx7Packet),SysEx8(SysEx8Packet),MixedDataSet(MixedDataSet),FlexData(FlexData),Stream(StreamMessage),Opaque(OpaquePacket)}
        impl Message for UmpMessage{}
        impl UmpValue for UmpMessage {fn encode_words(&self)->Vec<u32>{match self{$(Self::$name(v)=>v.encode_words(),)*Self::Midi1(v)=>v.encode_words(),Self::SysEx7(v)=>v.encode_words(),Self::SysEx8(v)=>v.encode_words(),Self::MixedDataSet(v)=>v.encode_words(),Self::FlexData(v)=>v.encode_words(),Self::Stream(v)=>v.encode_words(),Self::Opaque(v)=>v.encode_words()}}}
        fn scalar_decode(words:&[u32])->Result<Option<UmpMessage>,ValueError>{
            let key=if words[0]>>28==0{words[0]&0xfff00000}else{words[0]&0xf0f00000};
            $(if key==$first {
                $(let raw=(words[$word]>>$shift)&$mask;let $field=if stringify!($ty)=="i32"&&($bias as i64)==0{(raw as i32) as i64}else{i64::from(raw)-$bias};)*
                let value=$name::new($($field),*)?;
                if value.encode_words()!=words{return Err(ValueError("nonzero reserved scalar fields".into()));}
                return Ok(Some(UmpMessage::$name(value)));
            })*Ok(None)
        }
    };
}
scalar_values!(scalar);
fn validate_scalar(words: &[u32]) -> Result<(), ValueError> {
    if words[0] >> 28 == 4 {
        let status = (words[0] >> 20) & 15;
        let attr = words[0] & 255;
        if matches!(status, 8 | 9) && attr == 0 && words[1] & 65535 != 0 {
            return Err(ValueError(
                "attribute_data must be zero for attribute_type=0".into(),
            ));
        }
        if status == 12 && words[0] & 1 == 0 && words[1] & 65535 != 0 {
            return Err(ValueError(
                "bank bytes must be zero without bank-valid option".into(),
            ));
        }
        if status == 0 && !matches!(attr,1..=3|7|8|10|11|70..=79|91..=95) {
            return Err(ValueError("reserved registered per-note controller".into()));
        }
        // CC indexes replaced by unified messages are retained as raw values;
        // their receiver semantics are deliberately not inferred.
    }
    Ok(())
}
pub fn word_count(first: u32) -> usize {
    match first >> 28 {
        0..=2 | 6 | 7 => 1,
        3 | 4 | 8..=10 => 2,
        11 | 12 => 3,
        _ => 4,
    }
}
pub fn encode(value: &(impl UmpValue + ?Sized)) -> Vec<u32> {
    value.encode_words()
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Midi1Packet {
    group: u8,
    message: AnyMessage,
}
impl Midi1Packet {
    pub fn new(group: i64, message: AnyMessage) -> Result<Self, ValueError> {
        if matches!(message, AnyMessage::SysEx(_)) {
            return Err(ValueError("use SysEx7 packetization for SysEx".into()));
        }
        Ok(Self {
            group: bounded(group, 1, 16, "group")? as u8,
            message,
        })
    }
    pub fn group(&self) -> u8 {
        self.group
    }
    pub fn message(&self) -> &AnyMessage {
        &self.message
    }
}
impl Message for Midi1Packet {}
impl UmpValue for Midi1Packet {
    fn encode_words(&self) -> Vec<u32> {
        let b = self.message.encode();
        let mut bytes = [0u8; 4];
        bytes[0] = if b[0] >= 240 { 0x10 } else { 0x20 } | (self.group - 1);
        bytes[1..1 + b.len()].copy_from_slice(&b);
        vec![u32::from_be_bytes(bytes)]
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SysEx7Packet {
    group: u8,
    format: u8,
    data: Vec<u8>,
}
impl SysEx7Packet {
    pub fn new(group: i64, format: i64, data: Vec<u8>) -> Result<Self, ValueError> {
        if data.len() > 6 || data.iter().any(|&b| b > 127) {
            return Err(ValueError(
                "SysEx7 packet requires at most six seven-bit bytes".into(),
            ));
        }
        Ok(Self {
            group: bounded(group, 1, 16, "group")? as u8,
            format: bounded(format, 0, 3, "format")? as u8,
            data,
        })
    }
    pub fn group(&self) -> u8 {
        self.group
    }
    pub fn format(&self) -> u8 {
        self.format
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}
impl Message for SysEx7Packet {}
impl UmpValue for SysEx7Packet {
    fn encode_words(&self) -> Vec<u32> {
        let mut b = [0u8; 8];
        b[0] = 0x30 | (self.group - 1);
        b[1] = self.format << 4 | self.data.len() as u8;
        b[2..2 + self.data.len()].copy_from_slice(&self.data);
        bytes_words(&b)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SysEx8Packet {
    group: u8,
    format: u8,
    stream_id: u8,
    data: Vec<u8>,
    aborted: bool,
}
impl SysEx8Packet {
    pub fn new(
        group: i64,
        format: i64,
        stream_id: i64,
        data: Vec<u8>,
        aborted: bool,
    ) -> Result<Self, ValueError> {
        let format = bounded(format, 0, 3, "format")? as u8;
        if data.len() > 13 || aborted && (format != 3 || !data.is_empty()) {
            return Err(ValueError(
                "SysEx8 requires at most 13 payload bytes; abort requires empty End".into(),
            ));
        }
        Ok(Self {
            group: bounded(group, 1, 16, "group")? as u8,
            format,
            stream_id: bounded(stream_id, 0, 255, "stream_id")? as u8,
            data,
            aborted,
        })
    }
    pub fn group(&self) -> u8 {
        self.group
    }
    pub fn format(&self) -> u8 {
        self.format
    }
    pub fn stream_id(&self) -> u8 {
        self.stream_id
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    pub fn aborted(&self) -> bool {
        self.aborted
    }
}
impl Message for SysEx8Packet {}
impl UmpValue for SysEx8Packet {
    fn encode_words(&self) -> Vec<u32> {
        let mut b = [0u8; 16];
        b[0] = 0x50 | (self.group - 1);
        b[1] = self.format << 4
            | if self.aborted {
                15
            } else {
                self.data.len() as u8 + 1
            };
        b[2] = self.stream_id;
        b[3..3 + self.data.len()].copy_from_slice(&self.data);
        bytes_words(&b)
    }
}
/// Header/payload data is retained exactly; dataset transfer/acknowledgment is
/// not implied. Each packet has fourteen bytes after its MDS identifier.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MixedDataSet {
    group: u8,
    header: bool,
    mds_id: u8,
    data: [u8; 14],
}
impl MixedDataSet {
    pub fn new(group: i64, header: bool, mds_id: i64, data: Vec<u8>) -> Result<Self, ValueError> {
        let data: [u8; 14] = data
            .try_into()
            .map_err(|_| ValueError("MDS packet requires exactly fourteen bytes".into()))?;
        if header && u16::from_be_bytes([data[0], data[1]]) < 16 {
            return Err(ValueError(
                "MDS valid chunk byte count includes its 16-byte header".into(),
            ));
        }
        Ok(Self {
            group: bounded(group, 1, 16, "group")? as u8,
            header,
            mds_id: bounded(mds_id, 0, 15, "mds_id")? as u8,
            data,
        })
    }
    pub fn group(&self) -> u8 {
        self.group
    }
    pub fn header(&self) -> bool {
        self.header
    }
    pub fn mds_id(&self) -> u8 {
        self.mds_id
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}
impl Message for MixedDataSet {}
impl UmpValue for MixedDataSet {
    fn encode_words(&self) -> Vec<u32> {
        let mut b = [0u8; 16];
        b[0] = 0x50 | (self.group - 1);
        b[1] = if self.header { 0x80 } else { 0x90 } | self.mds_id;
        b[2..].copy_from_slice(&self.data);
        bytes_words(&b)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FlexData {
    group: u8,
    format: u8,
    channel: Option<u8>,
    status_bank: u8,
    status: u8,
    data: [u32; 3],
}
impl FlexData {
    pub fn new(
        group: i64,
        format: i64,
        channel: Option<i64>,
        status_bank: i64,
        status: i64,
        data: Vec<u32>,
    ) -> Result<Self, ValueError> {
        let data: [u32; 3] = data
            .try_into()
            .map_err(|_| ValueError("FlexData requires three data words".into()))?;
        let value = Self {
            group: bounded(group, 1, 16, "group")? as u8,
            format: bounded(format, 0, 3, "format")? as u8,
            channel: channel
                .map(|v| bounded(v, 1, 16, "channel").map(|v| v as u8))
                .transpose()?,
            status_bank: bounded(status_bank, 0, 255, "status_bank")? as u8,
            status: bounded(status, 0, 255, "status")? as u8,
            data,
        };
        validate_flex(&value)?;
        Ok(value)
    }
    pub fn group(&self) -> u8 {
        self.group
    }
    pub fn format(&self) -> u8 {
        self.format
    }
    pub fn channel(&self) -> Option<u8> {
        self.channel
    }
    pub fn status_bank(&self) -> u8 {
        self.status_bank
    }
    pub fn status(&self) -> u8 {
        self.status
    }
    pub fn data(&self) -> &[u32] {
        &self.data
    }
}
impl Message for FlexData {}
impl UmpValue for FlexData {
    fn encode_words(&self) -> Vec<u32> {
        vec![
            0xd0000000
                | u32::from(self.group - 1) << 24
                | u32::from(self.format) << 22
                | self.channel.map_or(1 << 20, |c| u32::from(c - 1) << 16)
                | u32::from(self.status_bank) << 8
                | u32::from(self.status),
            self.data[0],
            self.data[1],
            self.data[2],
        ]
    }
}
fn validate_flex(v: &FlexData) -> Result<(), ValueError> {
    if v.status_bank == 0 {
        if v.format != 0 {
            return Err(ValueError(
                "setup/performance FlexData must be complete".into(),
            ));
        }
        let d = &v.data;
        let invalid = match v.status {
            0 => v.channel.is_some() || d[1] != 0 || d[2] != 0,
            1 => v.channel.is_some() || d[0] & 255 != 0 || d[1] != 0 || d[2] != 0,
            2 => v.channel.is_some() || d[1] & 65535 != 0 || d[2] != 0,
            5 => d[0] & 0x00ffffff != 0 || d[1] != 0 || d[2] != 0 || (d[0] >> 24) & 15 > 7,
            6 => {
                let b = words_to_bytes(d, false);
                let tonic_ok = matches!(b[0] >> 4, 0..=2 | 14 | 15) && b[0] & 15 <= 7;
                let bass_ok = matches!(b[8] >> 4, 0..=2 | 8 | 14 | 15)
                    && b[8] & 15 <= 7
                    && (b[8] >> 4 != 8 || b[8] & 15 == 0);
                !tonic_ok
                    || !bass_ok
                    || b[1] > 27
                    || b[9] > 27
                    || [b[2], b[3], b[4], b[5], b[10], b[11]]
                        .iter()
                        .any(|v| v >> 4 > 4)
                    || d[1] & 65535 != 0
            }
            _ => return Err(ValueError("unknown FlexData setup status".into())),
        };
        if invalid {
            return Err(ValueError(
                "invalid FlexData addressing or reserved fields".into(),
            ));
        }
    } else if !(v.status_bank == 1 && v.status <= 12 || v.status_bank == 2 && v.status <= 4) {
        return Err(ValueError("unknown FlexData text status".into()));
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StreamMessage {
    format: u8,
    status: u16,
    data: Vec<u8>,
}
impl StreamMessage {
    /// Exactly fourteen bytes after the Stream header, including zero padding.
    pub fn new(format: i64, status: i64, data: Vec<u8>) -> Result<Self, ValueError> {
        if data.len() != 14 {
            return Err(ValueError(
                "StreamMessage requires fourteen data bytes".into(),
            ));
        }
        let value = Self {
            format: bounded(format, 0, 3, "format")? as u8,
            status: bounded(status, 0, 1023, "status")? as u16,
            data,
        };
        validate_stream(&value)?;
        Ok(value)
    }
    pub fn format(&self) -> u8 {
        self.format
    }
    pub fn status(&self) -> u16 {
        self.status
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}
impl Message for StreamMessage {}
impl UmpValue for StreamMessage {
    fn encode_words(&self) -> Vec<u32> {
        let mut b = [0u8; 16];
        let h = 0xf0000000 | u32::from(self.format) << 26 | u32::from(self.status) << 16;
        b[..4].copy_from_slice(&h.to_be_bytes());
        b[2..].copy_from_slice(&self.data);
        bytes_words(&b)
    }
}
fn validate_stream(v: &StreamMessage) -> Result<(), ValueError> {
    let d = &v.data;
    if !matches!(v.status, 3 | 4 | 18) && v.format != 0 {
        return Err(ValueError("this stream status cannot be segmented".into()));
    }
    let invalid = match v.status {
        0 => d[2..5].iter().any(|&b| b != 0) || d[5] & !31 != 0 || d[6..].iter().any(|&b| b != 0),
        1 => {
            d[2] & 127 > 32
                || d[3] != 0
                || d[4] & !3 != 0
                || d[5] & !3 != 0
                || d[6..].iter().any(|&b| b != 0)
        }
        2 => d[..3].iter().any(|&b| b != 0) || d.iter().any(|&b| b > 127),
        3 | 4 => false,
        5 | 6 => !matches!(d[0], 1 | 2) || d[1] & !3 != 0 || d[2..].iter().any(|&b| b != 0),
        16 => !(d[0] <= 31 || d[0] == 255) || d[1] & !3 != 0 || d[2..].iter().any(|&b| b != 0),
        17 => {
            d[0] & 127 > 31
                || d[1] & 0xc0 != 0
                || d[1] & 3 == 0
                || (d[1] >> 2) & 3 == 3
                || d[2] > 15
                || !(1..=16).contains(&d[3])
                || usize::from(d[2]) + usize::from(d[3]) > 16
                || ((d[1] >> 2) & 3 != 0 && d[3] != 1)
                || d[6..].iter().any(|&b| b != 0)
        }
        18 => d[0] > 31,
        32 | 33 => d.iter().any(|&b| b != 0),
        _ => return Err(ValueError("unknown stream status".into())),
    };
    if invalid {
        return Err(ValueError("invalid stream fields or reserved bits".into()));
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OpaquePacket {
    words: Vec<u32>,
}
impl OpaquePacket {
    pub fn new(words: Vec<u32>) -> Result<Self, ValueError> {
        if words.is_empty() || words.len() != word_count(words[0]) {
            return Err(ValueError("invalid opaque UMP length".into()));
        }
        Ok(Self { words })
    }
    pub fn words(&self) -> &[u32] {
        &self.words
    }
}
impl Message for OpaquePacket {}
impl UmpValue for OpaquePacket {
    fn encode_words(&self) -> Vec<u32> {
        self.words.clone()
    }
}

pub fn decode(words: &[u32], preserve_unknown: bool) -> Result<UmpMessage, ValueError> {
    if words.is_empty() || words.len() != word_count(words[0]) {
        return Err(ValueError("expected exactly one complete UMP".into()));
    }
    let fallback = |error: ValueError| {
        if preserve_unknown {
            Ok(UmpMessage::Opaque(OpaquePacket::new(words.to_vec())?))
        } else {
            Err(error)
        }
    };
    match decode_known(words) {
        Ok(Some(v)) => Ok(v),
        Ok(None) => fallback(ValueError("unknown UMP type/status".into())),
        Err(e) => fallback(e),
    }
}
fn decode_known(words: &[u32]) -> Result<Option<UmpMessage>, ValueError> {
    let b = words
        .iter()
        .flat_map(|w| w.to_be_bytes())
        .collect::<Vec<_>>();
    let group = i64::from(b[0] & 15) + 1;
    match b[0] >> 4 {
        0 | 4 => scalar_decode(words),
        1 | 2 => {
            let len = match b[1] {
                0x80..=0xbf | 0xe0..=0xef | 0xf2 => 3,
                0xc0..=0xdf | 0xf1 | 0xf3 => 2,
                0xf6 | 0xf8 | 0xfa..=0xfc | 0xfe | 0xff => 1,
                _ => return Err(ValueError("unknown system/channel status".into())),
            };
            if (b[0] >> 4 == 1) != (b[1] >= 240) || b[1 + len..].iter().any(|&v| v != 0) {
                return Err(ValueError("invalid MIDI 1 UMP type or padding".into()));
            }
            let mut decoder = ByteDecoder::new(1)?;
            let mut items = decoder.feed(&b[1..1 + len]);
            items.extend(decoder.finish());
            if items.len() != 1 {
                return Err(ValueError("invalid MIDI 1 UMP data".into()));
            }
            let DecodeItem::Message(message) = items.remove(0) else {
                return Err(ValueError("invalid MIDI 1 UMP data".into()));
            };
            Ok(Some(UmpMessage::Midi1(Midi1Packet::new(group, message)?)))
        }
        3 => {
            let n = usize::from(b[1] & 15);
            if n > 6 || b[2 + n..].iter().any(|&b| b != 0) {
                return Err(ValueError("invalid SysEx7 byte count/padding".into()));
            }
            Ok(Some(UmpMessage::SysEx7(SysEx7Packet::new(
                group,
                i64::from(b[1] >> 4),
                b[2..2 + n].to_vec(),
            )?)))
        }
        5 => {
            let status = b[1] >> 4;
            if status == 8 || status == 9 {
                return Ok(Some(UmpMessage::MixedDataSet(MixedDataSet::new(
                    group,
                    status == 8,
                    i64::from(b[1] & 15),
                    b[2..].to_vec(),
                )?)));
            }
            let n = usize::from(b[1] & 15);
            let abort = status == 3 && n == 15;
            if !(1..=14).contains(&n) && !abort {
                return Err(ValueError("invalid SysEx8 byte count".into()));
            }
            if b[if abort { 3 } else { 2 + n }..].iter().any(|&b| b != 0) {
                return Err(ValueError("nonzero SysEx8 padding".into()));
            }
            Ok(Some(UmpMessage::SysEx8(SysEx8Packet::new(
                group,
                i64::from(status),
                b[2].into(),
                if abort { vec![] } else { b[3..2 + n].to_vec() },
                abort,
            )?)))
        }
        13 => {
            let address = (b[1] >> 4) & 3;
            if address > 1 || address == 1 && b[1] & 15 != 0 {
                return Err(ValueError("invalid FlexData address".into()));
            }
            Ok(Some(UmpMessage::FlexData(FlexData::new(
                group,
                i64::from(b[1] >> 6),
                if address == 0 {
                    Some(i64::from(b[1] & 15) + 1)
                } else {
                    None
                },
                b[2].into(),
                b[3].into(),
                words[1..].to_vec(),
            )?)))
        }
        15 => Ok(Some(UmpMessage::Stream(StreamMessage::new(
            i64::from((b[0] >> 2) & 3),
            i64::from(((u16::from(b[0]) & 3) << 8) | u16::from(b[1])),
            b[2..].to_vec(),
        )?))),
        _ => Ok(None),
    }
}
pub fn packetize_sysex7(group: i64, data: &[u8]) -> Result<Vec<SysEx7Packet>, ValueError> {
    bounded(group, 1, 16, "group")?;
    if data.iter().any(|&b| b > 127) {
        return Err(ValueError("SysEx7 requires seven-bit payload".into()));
    }
    let count = data.len().max(1).div_ceil(6);
    (0..count)
        .map(|i| {
            SysEx7Packet::new(
                group,
                segment_format(i, count),
                data[i * 6..data.len().min((i + 1) * 6)].to_vec(),
            )
        })
        .collect()
}
pub fn packetize_sysex8(
    group: i64,
    stream_id: i64,
    data: &[u8],
) -> Result<Vec<SysEx8Packet>, ValueError> {
    bounded(group, 1, 16, "group")?;
    bounded(stream_id, 0, 255, "stream_id")?;
    let count = data.len().max(1).div_ceil(13);
    (0..count)
        .map(|i| {
            SysEx8Packet::new(
                group,
                segment_format(i, count),
                stream_id,
                data[i * 13..data.len().min((i + 1) * 13)].to_vec(),
                false,
            )
        })
        .collect()
}
pub fn segment_format(index: usize, count: usize) -> i64 {
    if count == 1 {
        0
    } else if index == 0 {
        1
    } else if index + 1 == count {
        3
    } else {
        2
    }
}
pub fn words_to_bytes(words: &[u32], little_endian: bool) -> Vec<u8> {
    words
        .iter()
        .flat_map(|w| {
            if little_endian {
                w.to_le_bytes()
            } else {
                w.to_be_bytes()
            }
        })
        .collect()
}
pub fn bytes_to_words(bytes: &[u8], little_endian: bool) -> Result<Vec<u32>, ValueError> {
    if !bytes.chunks_exact(4).remainder().is_empty() {
        return Err(ValueError("byte length must be divisible by four".into()));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|v| {
            let b = [v[0], v[1], v[2], v[3]];
            if little_endian {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            }
        })
        .collect())
}
fn bytes_words(b: &[u8]) -> Vec<u32> {
    bytes_to_words(b, false).expect("word-sized internal packet")
}
fn bounded(value: i64, lo: i64, hi: i64, name: &str) -> Result<i64, ValueError> {
    if (lo..=hi).contains(&value) {
        Ok(value)
    } else {
        Err(ValueError(format!("{name} must be {lo}..{hi}")))
    }
}

/// Explicitly declared endpoint facts, not inferred OS/device health.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Capabilities {
    pub midi1: bool,
    pub midi2: bool,
    pub sysex8_streams: u8,
}
impl Capabilities {
    pub fn new(midi1: bool, midi2: bool, sysex8_streams: i64) -> Result<Self, ValueError> {
        Ok(Self {
            midi1,
            midi2,
            sysex8_streams: bounded(sysex8_streams, 0, 255, "sysex8_streams")? as u8,
        })
    }
    /// None means these declarations do not establish support for this family.
    pub fn supports(&self, message: &UmpMessage) -> Option<bool> {
        match message {
            UmpMessage::Opaque(_)
            | UmpMessage::MixedDataSet(_)
            | UmpMessage::FlexData(_)
            | UmpMessage::Stream(_) => None,
            UmpMessage::Midi1(v) => Some(if v.message.encode()[0] >= 240 {
                self.midi1 || self.midi2
            } else {
                self.midi1
            }),
            UmpMessage::SysEx8(v) => Some(v.stream_id() < self.sysex8_streams),
            UmpMessage::SysEx7(_) => Some(self.midi1 || self.midi2),
            _ if message.encode_words()[0] >> 28 == 4 => Some(self.midi2),
            _ => None,
        }
    }
}
