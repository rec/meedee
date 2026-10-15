//! Bounded incremental packet framing and payload assembly, one instance per source.
use super::*;
use std::collections::BTreeMap;
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DecodeDiagnostic {
    pub word_offset: usize,
    pub reason: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DecodeOutput {
    Message(UmpMessage),
    Diagnostic(DecodeDiagnostic),
}
#[derive(Clone, Debug)]
pub struct Decoder {
    preserve_unknown: bool,
    pending: Vec<u32>,
    offset: usize,
    start: usize,
}
impl Decoder {
    pub fn new(preserve_unknown: bool) -> Self {
        Self {
            preserve_unknown,
            pending: Vec::with_capacity(4),
            offset: 0,
            start: 0,
        }
    }
    pub fn feed(&mut self, words: &[u32]) -> Vec<DecodeOutput> {
        self.feed_spanned(words)
            .into_iter()
            .map(|(v, _, _)| v)
            .collect()
    }
    pub fn offset(&self) -> usize {
        self.offset
    }
    pub fn pending_start(&self) -> Option<usize> {
        (!self.pending.is_empty()).then_some(self.start)
    }
    pub fn feed_spanned(&mut self, words: &[u32]) -> Vec<(DecodeOutput, usize, usize)> {
        let mut out = Vec::new();
        for &w in words {
            if self.pending.is_empty() {
                self.start = self.offset;
            }
            self.pending.push(w);
            self.offset = self.offset.saturating_add(1);
            if self.pending.len() == word_count(self.pending[0]) {
                let item = match decode(&self.pending, self.preserve_unknown) {
                    Ok(v) => DecodeOutput::Message(v),
                    Err(e) => DecodeOutput::Diagnostic(DecodeDiagnostic {
                        word_offset: self.start,
                        reason: e.to_string(),
                    }),
                };
                out.push((item, self.start, self.offset - 1));
                self.pending.clear();
            }
        }
        out
    }
    pub fn finish(&mut self) -> Vec<DecodeOutput> {
        if self.pending.is_empty() {
            vec![]
        } else {
            self.pending.clear();
            vec![DecodeOutput::Diagnostic(DecodeDiagnostic {
                word_offset: self.start,
                reason: "truncated UMP".into(),
            })]
        }
    }
    pub fn reset(&mut self) {
        self.pending.clear();
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DataKind {
    SysEx7,
    SysEx8,
    FlexData,
    StreamText,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AssembledData {
    pub kind: DataKind,
    pub group: Option<u8>,
    pub channel: Option<u8>,
    pub stream_id: Option<u8>,
    pub function_block: Option<u8>,
    pub status_bank: Option<u8>,
    pub status: Option<u16>,
    pub data: Vec<u8>,
}
impl AssembledData {
    pub fn text(&self) -> Option<&str> {
        if self.kind == DataKind::StreamText
            || self.kind == DataKind::FlexData && matches!(self.status_bank, Some(1 | 2))
        {
            std::str::from_utf8(&self.data).ok()
        } else {
            None
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssemblyDiagnostic {
    Interrupted,
    UnexpectedContinuation,
    Aborted,
    PayloadLimit,
    StreamLimit,
    MalformedText,
    TooManyPackets,
    Truncated,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AssemblyItem {
    Data(AssembledData),
    Diagnostic(AssemblyDiagnostic),
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    kind: DataKind,
    group: Option<u8>,
    channel: Option<u8>,
    stream: Option<u8>,
    bank: Option<u8>,
    status: Option<u16>,
}
#[derive(Clone, Debug)]
struct Pending {
    first_index: usize,
    data: Vec<u8>,
    packets: usize,
    discard: bool,
}
#[derive(Clone, Debug)]
pub struct Assembler {
    message_limit: usize,
    total_limit: usize,
    max_streams: usize,
    pending: BTreeMap<Key, Pending>,
    bytes: usize,
    next_index: usize,
    current_index: usize,
    completed_start: Option<usize>,
}
impl Assembler {
    pub fn new(
        message_limit: usize,
        total_limit: usize,
        max_streams: usize,
    ) -> Result<Self, ValueError> {
        if message_limit == 0 || total_limit == 0 || max_streams == 0 {
            return Err(ValueError("assembly limits must be positive".into()));
        }
        Ok(Self {
            message_limit,
            total_limit,
            max_streams,
            pending: BTreeMap::new(),
            bytes: 0,
            next_index: 0,
            current_index: 0,
            completed_start: None,
        })
    }
    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }
    pub fn active_streams(&self) -> usize {
        self.pending.len()
    }
    pub fn reset(&mut self) {
        self.pending.clear();
        self.bytes = 0;
    }
    pub fn finish(&mut self) -> Vec<AssemblyItem> {
        let count = self.pending.values().filter(|v| !v.discard).count();
        self.reset();
        vec![AssemblyItem::Diagnostic(AssemblyDiagnostic::Truncated); count]
    }
    pub fn feed(&mut self, value: &UmpMessage) -> Vec<AssemblyItem> {
        self.feed_spanned(value)
            .into_iter()
            .map(|(v, _, _)| v)
            .collect()
    }
    pub fn feed_spanned(&mut self, value: &UmpMessage) -> Vec<(AssemblyItem, usize, usize)> {
        let index = self.next_index;
        self.current_index = index;
        self.next_index = self.next_index.saturating_add(1);
        self.completed_start = None;
        let out = self.accept(value);
        out.into_iter()
            .map(|v| {
                let start = if matches!(v, AssemblyItem::Data(_)) {
                    self.completed_start.unwrap_or(index)
                } else {
                    index
                };
                (v, start, index)
            })
            .collect()
    }
    pub(crate) fn retained_indexes(&self) -> Vec<usize> {
        self.pending.values().map(|p| p.first_index).collect()
    }
    fn accept(&mut self, value: &UmpMessage) -> Vec<AssemblyItem> {
        let words = value.encode_words();
        let mt = words[0] >> 28;
        let group = if matches!(mt, 1..=5 | 13) {
            Some(((words[0] >> 24) & 15) as u8 + 1)
        } else {
            None
        };
        let realtime = matches!(value,UmpMessage::Midi1(v) if v.message().encode()[0]>=248);
        let mut out = Vec::new();
        // A different grouped non-realtime message interrupts SysEx7. FlexData
        // permits only same-message fragments, realtime and groupless JR Clock.
        let interrupted=self.pending.keys().filter(|k|{
            if k.kind==DataKind::SysEx7&&k.group==group&&!realtime {return !matches!(value,UmpMessage::SysEx7(v) if Some(v.group())==k.group);}
            if k.kind==DataKind::FlexData&&(k.group==group||group.is_none()&&!matches!(value,UmpMessage::JrClock(_)))&&!realtime {return !matches!(value,UmpMessage::FlexData(v) if Some(v.group())==k.group&&v.channel()==k.channel&&Some(v.status_bank())==k.bank&&Some(u16::from(v.status()))==k.status);}
            false
        }).cloned().collect::<Vec<_>>();
        for key in interrupted {
            self.remove(&key);
            out.push(AssemblyItem::Diagnostic(AssemblyDiagnostic::Interrupted));
        }
        let (key, format, data, abort, text, max_packets, max_len) = match value {
            UmpMessage::SysEx7(v) => (
                Key {
                    kind: DataKind::SysEx7,
                    group: Some(v.group()),
                    channel: None,
                    stream: None,
                    bank: None,
                    status: None,
                },
                v.format(),
                v.data().to_vec(),
                false,
                false,
                usize::MAX,
                self.message_limit,
            ),
            UmpMessage::SysEx8(v) => (
                Key {
                    kind: DataKind::SysEx8,
                    group: Some(v.group()),
                    channel: None,
                    stream: Some(v.stream_id()),
                    bank: None,
                    status: None,
                },
                v.format(),
                v.data().to_vec(),
                v.aborted(),
                false,
                usize::MAX,
                self.message_limit,
            ),
            UmpMessage::FlexData(v) => (
                Key {
                    kind: DataKind::FlexData,
                    group: Some(v.group()),
                    channel: v.channel(),
                    stream: None,
                    bank: Some(v.status_bank()),
                    status: Some(v.status().into()),
                },
                v.format(),
                words_to_bytes(v.data(), false),
                false,
                v.status_bank() != 0,
                32,
                384,
            ),
            UmpMessage::Stream(v) if matches!(v.status(), 3 | 4 | 18) => {
                let fb = v.status() == 18;
                let limit = match v.status() {
                    3 => 98,
                    4 => 42,
                    _ => 91,
                };
                (
                    Key {
                        kind: DataKind::StreamText,
                        group: None,
                        channel: None,
                        stream: if fb { Some(v.data()[0]) } else { None },
                        bank: None,
                        status: Some(v.status()),
                    },
                    v.format(),
                    v.data()[usize::from(fb)..].to_vec(),
                    false,
                    true,
                    if v.status() == 4 { 3 } else { 7 },
                    limit,
                )
            }
            _ => return out,
        };
        if abort {
            self.remove(&key);
            out.push(AssemblyItem::Diagnostic(AssemblyDiagnostic::Aborted));
            return out;
        }
        if matches!(format, 0 | 1) {
            if self.pending.contains_key(&key) {
                self.remove(&key);
                out.push(AssemblyItem::Diagnostic(AssemblyDiagnostic::Interrupted));
            }
            if self.pending.len() >= self.max_streams {
                out.push(AssemblyItem::Diagnostic(AssemblyDiagnostic::StreamLimit));
                return out;
            }
            self.pending.insert(
                key.clone(),
                Pending {
                    first_index: self.current_index,
                    data: Vec::new(),
                    packets: 0,
                    discard: false,
                },
            );
        }
        let Some(mut pending) = self.pending.remove(&key) else {
            out.push(AssemblyItem::Diagnostic(
                AssemblyDiagnostic::UnexpectedContinuation,
            ));
            return out;
        };
        let mut data = data;
        if !pending.discard {
            pending.packets = pending.packets.saturating_add(1);
            let mut failure = None;
            if text {
                if let Some(zero) = data.iter().position(|&b| b == 0) {
                    if matches!(format, 1 | 2) || data[zero..].iter().any(|&b| b != 0) {
                        failure = Some(AssemblyDiagnostic::MalformedText);
                    } else {
                        data.truncate(zero);
                    }
                }
            }
            if pending.packets > max_packets {
                failure = Some(AssemblyDiagnostic::TooManyPackets);
            }
            if data.len() > self.message_limit.saturating_sub(pending.data.len())
                || data.len() > max_len.saturating_sub(pending.data.len())
                || data.len() > self.total_limit.saturating_sub(self.bytes)
            {
                failure = Some(AssemblyDiagnostic::PayloadLimit);
            }
            if let Some(reason) = failure {
                self.bytes -= pending.data.len();
                pending.data = Vec::new();
                pending.discard = true;
                out.push(AssemblyItem::Diagnostic(reason));
            } else {
                self.bytes += data.len();
                pending.data.extend(data);
            }
        }
        if matches!(format, 0 | 3) {
            self.bytes -= pending.data.len();
            if !pending.discard {
                let invalid = text
                    && (std::str::from_utf8(&pending.data).is_err()
                        || pending.data.starts_with(&[239, 187, 191])
                        || key.kind == DataKind::StreamText
                            && key.status == Some(4)
                            && pending.data.iter().any(|&b| !(32..=126).contains(&b)));
                if invalid {
                    out.push(AssemblyItem::Diagnostic(AssemblyDiagnostic::MalformedText));
                } else {
                    self.completed_start = Some(pending.first_index);
                    out.push(AssemblyItem::Data(AssembledData {
                        kind: key.kind,
                        group: key.group,
                        channel: key.channel,
                        stream_id: if key.kind == DataKind::SysEx8 {
                            key.stream
                        } else {
                            None
                        },
                        function_block: if key.kind == DataKind::StreamText
                            && key.status == Some(18)
                        {
                            key.stream
                        } else {
                            None
                        },
                        status_bank: key.bank,
                        status: key.status,
                        data: pending.data,
                    }));
                }
            }
        } else {
            self.pending.insert(key, pending);
        }
        out
    }
    fn remove(&mut self, key: &Key) {
        if let Some(v) = self.pending.remove(key) {
            self.bytes -= v.data.len();
        }
    }
}

pub fn packetize_flex_text(
    group: i64,
    channel: Option<i64>,
    status_bank: i64,
    status: i64,
    text: &str,
) -> Result<Vec<FlexData>, ValueError> {
    let b = text.as_bytes();
    if b.len() > 384 || b.contains(&0) || b.starts_with(&[239, 187, 191]) {
        return Err(ValueError(
            "Flex text must be UTF-8 without NUL/BOM and at most 384 bytes".into(),
        ));
    }
    if status_bank != 1 && status_bank != 2 {
        return Err(ValueError("text requires Flex status bank 1 or 2".into()));
    }
    let count = b.len().max(1).div_ceil(12);
    (0..count)
        .map(|i| {
            let mut bytes = [0u8; 12];
            let part = &b[i * 12..b.len().min((i + 1) * 12)];
            bytes[..part.len()].copy_from_slice(part);
            FlexData::new(
                group,
                segment_format(i, count),
                channel,
                status_bank,
                status,
                bytes_words(&bytes),
            )
        })
        .collect()
}
pub fn packetize_stream_text(
    status: i64,
    function_block: Option<i64>,
    text: &str,
) -> Result<Vec<StreamMessage>, ValueError> {
    let fb = status == 18;
    let cap = if fb { 13 } else { 14 };
    let max = match status {
        3 => 98,
        4 => 42,
        18 => 91,
        _ => {
            return Err(ValueError(
                "status must identify an endpoint name, product ID or function-block name".into(),
            ))
        }
    };
    if fb != function_block.is_some() {
        return Err(ValueError(
            "function_block is required only for function-block names".into(),
        ));
    }
    let block = function_block
        .map(|v| bounded(v, 0, 31, "function_block"))
        .transpose()?
        .unwrap_or(0) as u8;
    let b = text.as_bytes();
    if b.len() > max
        || b.contains(&0)
        || b.starts_with(&[239, 187, 191])
        || status == 4 && b.iter().any(|b| !(32..=126).contains(b))
    {
        return Err(ValueError("invalid stream text length/characters".into()));
    }
    let count = b.len().max(1).div_ceil(cap);
    (0..count)
        .map(|i| {
            let mut data = vec![0; 14];
            if fb {
                data[0] = block;
            }
            let part = &b[i * cap..b.len().min((i + 1) * cap)];
            data[usize::from(fb)..usize::from(fb) + part.len()].copy_from_slice(part);
            StreamMessage::new(segment_format(i, count), status, data)
        })
        .collect()
}
