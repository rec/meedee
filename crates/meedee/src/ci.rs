//! MIDI-CI 1.2.1 management messages. Profile/property rules are deliberately deferred.
use crate::messages::{Message, PrimitiveMessage, SysEx, ValueError};

pub const BROADCAST_MUID: u32 = 0x0fff_ffff;
pub const MAX_MUID: u32 = 0x0fff_feff;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Address(u8);
impl Address {
    pub const GROUP: Self = Self(126);
    pub const FUNCTION_BLOCK: Self = Self(127);
    pub fn channel(channel: i64) -> Result<Self, ValueError> {
        Ok(Self(range(channel, 1, 16, "channel")? as u8 - 1))
    }
    pub fn channel_number(self) -> Option<u8> {
        (self.0 < 16).then_some(self.0 + 1)
    }
    pub(crate) fn from_wire(value: u8) -> Result<Self, ValueError> {
        if !(16..126).contains(&value) {
            Ok(Self(value))
        } else {
            Err(error("reserved CI address"))
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Header {
    source_muid: u32,
    destination_muid: u32,
    version: u8,
    address: Address,
}
impl Header {
    pub fn new(
        source_muid: i64,
        destination_muid: i64,
        version: i64,
        address: Address,
    ) -> Result<Self, ValueError> {
        let source_muid = muid(source_muid)?;
        let destination_muid = if destination_muid == i64::from(BROADCAST_MUID) {
            BROADCAST_MUID
        } else {
            muid(destination_muid)?
        };
        Ok(Self {
            source_muid,
            destination_muid,
            version: range(version, 1, 31, "version")? as u8,
            address,
        })
    }
    pub fn source_muid(&self) -> u32 {
        self.source_muid
    }
    pub fn destination_muid(&self) -> u32 {
        self.destination_muid
    }
    pub fn version(&self) -> u8 {
        self.version
    }
    pub fn address(&self) -> Address {
        self.address
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Identity {
    manufacturer: [u8; 3],
    family: u16,
    model: u16,
    revision: [u8; 4],
}
impl Identity {
    pub fn new(
        manufacturer: Vec<u8>,
        family: i64,
        model: i64,
        revision: Vec<u8>,
    ) -> Result<Self, ValueError> {
        let manufacturer: [u8; 3] = seven(manufacturer)?
            .try_into()
            .map_err(|_| error("manufacturer requires three bytes"))?;
        if manufacturer[0] != 0 && (manufacturer[1] != 0 || manufacturer[2] != 0) {
            return Err(error(
                "one-byte manufacturer IDs require two padding zeroes",
            ));
        }
        Ok(Self {
            manufacturer,
            family: range(family, 0, 16383, "family")? as u16,
            model: range(model, 0, 16383, "model")? as u16,
            revision: seven(revision)?
                .try_into()
                .map_err(|_| error("revision requires four bytes"))?,
        })
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
    pub fn revision(&self) -> &[u8] {
        &self.revision
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DiscoveryInfo {
    identity: Identity,
    capabilities: u8,
    max_sysex: u32,
    output_path: Option<u8>,
}
impl DiscoveryInfo {
    pub fn new(
        identity: Identity,
        capabilities: i64,
        max_sysex: i64,
        output_path: Option<i64>,
    ) -> Result<Self, ValueError> {
        Ok(Self {
            identity,
            capabilities: range(capabilities, 0, 127, "capabilities")? as u8,
            max_sysex: range(max_sysex, 128, 0x0fff_ffff, "max_sysex")? as u32,
            output_path: output_path
                .map(|n| range(n, 0, 127, "output_path").map(|n| n as u8))
                .transpose()?,
        })
    }
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
    pub fn capabilities(&self) -> u8 {
        self.capabilities
    }
    pub fn max_sysex(&self) -> u32 {
        self.max_sysex
    }
    pub fn output_path(&self) -> Option<u8> {
        self.output_path
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Notice {
    original: u8,
    status: u8,
    status_data: u8,
    details: [u8; 5],
    message_text: Vec<u8>,
}
impl Notice {
    pub fn new(
        original: i64,
        status: i64,
        status_data: i64,
        details: Vec<u8>,
        message_text: Vec<u8>,
    ) -> Result<Self, ValueError> {
        let message_text = seven(message_text)?;
        if message_text.len() > 16383 {
            return Err(error("notice text exceeds 14-bit length"));
        }
        Ok(Self {
            original: range(original, 0, 127, "original")? as u8,
            status: range(status, 0, 127, "status")? as u8,
            status_data: range(status_data, 0, 127, "status_data")? as u8,
            details: seven(details)?
                .try_into()
                .map_err(|_| error("details requires five bytes"))?,
            message_text,
        })
    }
    pub fn original(&self) -> u8 {
        self.original
    }
    pub fn status(&self) -> u8 {
        self.status
    }
    pub fn status_data(&self) -> u8 {
        self.status_data
    }
    pub fn details(&self) -> &[u8] {
        &self.details
    }
    /// Raw seven-bit message text, including any JSON-style escapes. Not interpreted.
    pub fn message_text(&self) -> &[u8] {
        &self.message_text
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Body {
    Discovery(DiscoveryInfo),
    DiscoveryReply {
        info: DiscoveryInfo,
        function_block: Option<u8>,
    },
    EndpointInquiry,
    EndpointReply {
        product_instance_id: String,
    },
    InvalidateMuid {
        target: u32,
    },
    Ack(Notice),
    Nak(Option<Notice>),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Management {
    header: Header,
    body: Body,
    extensions: Vec<u8>,
}
impl Management {
    pub fn new(header: Header, body: Body, extensions: Vec<u8>) -> Result<Self, ValueError> {
        let broadcast = matches!(body, Body::Discovery(_) | Body::InvalidateMuid { .. });
        if (header.destination_muid == BROADCAST_MUID) != broadcast {
            return Err(error(
                "message has an invalid broadcast/unicast destination",
            ));
        }
        if !matches!(body, Body::Ack(_) | Body::Nak(_)) && header.address != Address::FUNCTION_BLOCK
        {
            return Err(error("management message requires Function Block address"));
        }
        match &body {
            Body::Discovery(info) | Body::DiscoveryReply { info, .. } => {
                if info.output_path.is_some() != (header.version >= 2) {
                    return Err(error(
                        "output_path is absent in version 1 and required in version 2 or later",
                    ));
                }
            }
            Body::EndpointInquiry | Body::EndpointReply { .. } | Body::Ack(_)
                if header.version == 1 =>
            {
                return Err(error("message requires CI format version 2 or later"))
            }
            Body::Nak(notice) if notice.is_some() != (header.version >= 2) => {
                return Err(error(
                    "NAK fields are absent in version 1 and required in version 2 or later",
                ))
            }
            Body::InvalidateMuid { target } => {
                muid(i64::from(*target))?;
            }
            _ => {}
        }
        if let Body::DiscoveryReply {
            function_block: Some(n),
            ..
        } = &body
        {
            if *n == 127 || header.version == 1 || (*n > 31 && header.version <= 2) {
                return Err(error(
                    "function_block requires version 2 and a number 0..31",
                ));
            }
        }
        if let Body::EndpointReply {
            product_instance_id,
        } = &body
        {
            if product_instance_id.len() > 42
                || !product_instance_id.bytes().all(|n| (32..=126).contains(&n))
            {
                return Err(error(
                    "product_instance_id requires at most 42 printable ASCII bytes",
                ));
            }
        }
        let extensions = seven(extensions)?;
        if header.version <= 2 && !extensions.is_empty() {
            return Err(error("unexpected fields in a known CI format version"));
        }
        let base_size = if matches!(body, Body::Discovery(_)) {
            32
        } else {
            19
        };
        if broadcast && extensions.len() > 512 - base_size {
            return Err(error(
                "unchunked broadcast CI messages cannot exceed 512 bytes",
            ));
        }
        Ok(Self {
            header,
            body,
            extensions,
        })
    }
    pub fn header(&self) -> &Header {
        &self.header
    }
    pub fn body(&self) -> &Body {
        &self.body
    }
    pub fn extensions(&self) -> &[u8] {
        &self.extensions
    }
    pub fn kind(&self) -> u8 {
        match self.body {
            Body::Discovery(_) => 0x70,
            Body::DiscoveryReply { .. } => 0x71,
            Body::EndpointInquiry => 0x72,
            Body::EndpointReply { .. } => 0x73,
            Body::Ack(_) => 0x7d,
            Body::InvalidateMuid { .. } => 0x7e,
            Body::Nak(_) => 0x7f,
        }
    }
    pub fn sysex(&self) -> SysEx {
        let bytes = self.encode();
        SysEx::new(bytes[1..bytes.len() - 1].to_vec()).expect("validated CI message")
    }
}
impl Message for Management {}
impl PrimitiveMessage for Management {
    fn encode(&self) -> Vec<u8> {
        let mut b = vec![
            240,
            126,
            self.header.address.0,
            13,
            self.kind(),
            self.header.version,
        ];
        put(&mut b, self.header.source_muid, 4);
        put(&mut b, self.header.destination_muid, 4);
        match &self.body {
            Body::Discovery(info) | Body::DiscoveryReply { info, .. } => {
                b.extend(info.identity.manufacturer);
                put(&mut b, info.identity.family.into(), 2);
                put(&mut b, info.identity.model.into(), 2);
                b.extend(info.identity.revision);
                b.push(info.capabilities);
                put(&mut b, info.max_sysex, 4);
                if let Some(path) = info.output_path {
                    b.push(path);
                }
                if let Body::DiscoveryReply { function_block, .. } = &self.body {
                    if self.header.version >= 2 {
                        b.push(function_block.unwrap_or(127));
                    }
                }
            }
            Body::EndpointInquiry => b.push(0),
            Body::EndpointReply {
                product_instance_id,
            } => {
                b.push(0);
                put(&mut b, product_instance_id.len() as u32, 2);
                b.extend(product_instance_id.as_bytes());
            }
            Body::InvalidateMuid { target } => put(&mut b, *target, 4),
            Body::Ack(n) | Body::Nak(Some(n)) => {
                b.extend([n.original, n.status, n.status_data]);
                b.extend(n.details);
                put(&mut b, n.message_text.len() as u32, 2);
                b.extend(&n.message_text);
            }
            Body::Nak(None) => {}
        }
        b.extend(&self.extensions);
        b.push(247);
        b
    }
}
/// None means non-CI or an unsupported CI message family/endpoint selector.
/// Malformed supported messages fail; callers retain the original SysEx.
pub fn decode(sysex: &SysEx) -> Result<Option<Management>, ValueError> {
    let b = sysex.data();
    if b.first() != Some(&126) || b.get(2) != Some(&13) {
        return Ok(None);
    }
    let (header, kind) = envelope(b)?;
    if !matches!(kind,0x70..=0x73|0x7d..=0x7f) {
        return Ok(None);
    }
    let mut f = Fields { b, offset: 13 };
    let body = match kind {
        0x70 | 0x71 => {
            let identity = Identity::new(
                f.bytes(3)?.to_vec(),
                i64::from(f.number(2)?),
                i64::from(f.number(2)?),
                f.bytes(4)?.to_vec(),
            )?;
            let capabilities = f.byte()?;
            let max = f.number(4)?;
            let path = if header.version >= 2 {
                Some(i64::from(f.byte()?))
            } else {
                None
            };
            let info = DiscoveryInfo::new(identity, capabilities.into(), max.into(), path)?;
            if kind == 0x70 {
                Body::Discovery(info)
            } else {
                let function_block = if header.version >= 2 {
                    match f.byte()? {
                        127 => None,
                        n => Some(n),
                    }
                } else {
                    None
                };
                Body::DiscoveryReply {
                    info,
                    function_block,
                }
            }
        }
        0x72 => {
            if f.byte()? != 0 {
                return Ok(None);
            }
            Body::EndpointInquiry
        }
        0x73 => {
            if f.byte()? != 0 {
                return Ok(None);
            }
            let n = f.number(2)? as usize;
            let text = String::from_utf8(f.bytes(n)?.to_vec())
                .map_err(|_| error("invalid endpoint text"))?;
            Body::EndpointReply {
                product_instance_id: text,
            }
        }
        0x7e => Body::InvalidateMuid {
            target: f.number(4)?,
        },
        0x7d | 0x7f => {
            let notice = if header.version == 1 && kind == 0x7f {
                None
            } else {
                let original = f.byte()?;
                let status = f.byte()?;
                let data = f.byte()?;
                let details = f.bytes(5)?.to_vec();
                let n = f.number(2)? as usize;
                Some(Notice::new(
                    original.into(),
                    status.into(),
                    data.into(),
                    details,
                    f.bytes(n)?.to_vec(),
                )?)
            };
            if kind == 0x7d {
                Body::Ack(notice.ok_or_else(|| error("missing ACK fields"))?)
            } else {
                Body::Nak(notice)
            }
        }
        _ => unreachable!(),
    };
    Management::new(header, body, b[f.offset..].to_vec()).map(Some)
}
fn envelope(b: &[u8]) -> Result<(Header, u8), ValueError> {
    if b.len() < 13 {
        return Err(error("truncated CI header"));
    }
    Ok((
        Header::new(
            number(&b[5..9]).into(),
            number(&b[9..13]).into(),
            b[4].into(),
            Address::from_wire(b[1])?,
        )?,
        b[3],
    ))
}
fn muid(n: i64) -> Result<u32, ValueError> {
    Ok(range(n, 0, i64::from(MAX_MUID), "MUID")? as u32)
}
fn range(n: i64, lo: i64, hi: i64, name: &str) -> Result<i64, ValueError> {
    if (lo..=hi).contains(&n) {
        Ok(n)
    } else {
        Err(error(&format!("{name} must be {lo}..{hi}")))
    }
}
fn seven(b: Vec<u8>) -> Result<Vec<u8>, ValueError> {
    if b.iter().any(|n| *n > 127) {
        Err(error("CI fields require seven-bit bytes"))
    } else {
        Ok(b)
    }
}
fn error(s: &str) -> ValueError {
    ValueError(s.into())
}
fn put(b: &mut Vec<u8>, n: u32, count: usize) {
    for i in 0..count {
        b.push(((n >> (i * 7)) & 127) as u8);
    }
}
fn number(b: &[u8]) -> u32 {
    b.iter()
        .enumerate()
        .fold(0, |n, (i, b)| n | u32::from(*b) << (7 * i))
}
struct Fields<'a> {
    b: &'a [u8],
    offset: usize,
}
impl<'a> Fields<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], ValueError> {
        let end = self
            .offset
            .checked_add(n)
            .ok_or_else(|| error("CI length overflow"))?;
        let out = self
            .b
            .get(self.offset..end)
            .ok_or_else(|| error("truncated CI message"))?;
        self.offset = end;
        Ok(out)
    }
    fn byte(&mut self) -> Result<u8, ValueError> {
        Ok(self.bytes(1)?[0])
    }
    fn number(&mut self, n: usize) -> Result<u32, ValueError> {
        Ok(number(self.bytes(n)?))
    }
}
