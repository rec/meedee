//! MIDI-CI 1.2.1 management, caller-driven sessions and live browsing.
//! Profiles and Property Exchange remain deferred.
use crate::{
    ci,
    drivers::{self, Receiver, Sender},
    events::{ObservationValue, Payload},
    live::Item,
    messages::{AnyMessage, Message, PrimitiveMessage, SysEx, ValueError},
    midi,
    playback::{Cancellation, MonotonicClock, PlaybackClock},
    ump::{self, AssemblyItem, PacketValue},
};
use std::{
    collections::{BTreeSet, VecDeque},
    time::Duration,
};

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
#[derive(Clone, Debug)]
pub struct Management {
    metadata: Option<Box<crate::messages::Metadata>>,
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
            metadata: None,
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
        let bytes = self.encode().expect("validated system message");
        SysEx::new(bytes[1..bytes.len() - 1].to_vec()).expect("validated CI message")
    }
}
crate::messages::value_metadata!(Management {
    header,
    body,
    extensions
});
impl PrimitiveMessage for Management {
    fn encode(&self) -> Result<Vec<u8>, crate::messages::ValueError> {
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
        Ok(b)
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Route {
    input: String,
    output: String,
    group: Option<u8>,
}
impl Route {
    pub fn new(input: String, output: String, group: Option<i64>) -> Result<Self, ValueError> {
        if input.is_empty() || output.is_empty() {
            return Err(error("route identities must be nonempty"));
        }
        if input.len() > 1024 || output.len() > 1024 {
            return Err(error("route identities exceed 1024 bytes"));
        }
        if group.is_some_and(|n| !(1..=16).contains(&n)) {
            return Err(error("group must be 1..16"));
        }
        Ok(Self {
            input,
            output,
            group: group.map(|n| n as u8),
        })
    }
    pub fn input(&self) -> &str {
        &self.input
    }
    pub fn output(&self) -> &str {
        &self.output
    }
    pub fn group(&self) -> Option<u8> {
        self.group
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Peer {
    pub muid: u32,
    pub version: u8,
    pub info: DiscoveryInfo,
    pub function_block: Option<u8>,
    pub product_instance_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Action {
    pub token: u64,
    pub route: Route,
    pub message: Management,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Observation {
    pub kind: String,
    pub peer_muid: Option<u32>,
    pub token: Option<u64>,
    pub message: Option<Management>,
    pub reason: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Update {
    Send(Action),
    Observe(Observation),
}
struct Pending {
    action: Action,
    sent: bool,
    expires: u64,
    timeout: u64,
    provisional: Vec<(Management, usize)>,
}
pub struct Session {
    local: u32,
    identity: Identity,
    route: Route,
    output_path: u8,
    function_block: Option<u8>,
    max_sysex: u32,
    peer_limit: usize,
    exchange_limit: usize,
    reply_byte_limit: usize,
    peers: Vec<Peer>,
    pending: Vec<Pending>,
    next_token: u64,
    now: u64,
    active: bool,
}
impl Session {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        local_muid: i64,
        identity: Identity,
        route: Route,
        output_path: i64,
        function_block: Option<i64>,
        max_sysex: i64,
        peer_limit: usize,
        exchange_limit: usize,
        reply_byte_limit: usize,
    ) -> Result<Self, ValueError> {
        let local = Header::new(
            local_muid,
            BROADCAST_MUID.into(),
            2,
            ci::Address::FUNCTION_BLOCK,
        )?
        .source_muid();
        let info = DiscoveryInfo::new(identity.clone(), 0, max_sysex, Some(output_path))?;
        if peer_limit == 0 || exchange_limit == 0 || reply_byte_limit == 0 {
            return Err(error("session limits must be positive"));
        }
        if function_block.is_some_and(|n| !(0..=31).contains(&n)) {
            return Err(error("function_block must be 0..31 or absent"));
        }
        if route.group.is_none() && function_block.is_some() {
            return Err(error("a byte-stream route has no UMP Function Block"));
        }
        Ok(Self {
            local,
            identity,
            route,
            output_path: info.output_path().expect("version 2 path"),
            function_block: function_block.map(|n| n as u8),
            max_sysex: info.max_sysex(),
            peer_limit,
            exchange_limit,
            reply_byte_limit,
            peers: vec![],
            pending: vec![],
            next_token: 0,
            now: 0,
            active: true,
        })
    }
    pub fn local_muid(&self) -> u32 {
        self.local
    }
    pub fn route(&self) -> &Route {
        &self.route
    }
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn peers(&self) -> &[Peer] {
        &self.peers
    }
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
    fn time(&mut self, now: u64) -> Result<(), ValueError> {
        if now < self.now {
            return Err(error("session time regressed"));
        }
        self.now = now;
        Ok(())
    }
    fn running(&self) -> Result<(), ValueError> {
        if self.active {
            Ok(())
        } else {
            Err(error("session requires explicit restart with a fresh MUID"))
        }
    }
    fn header(&self, destination: u32) -> Result<Header, ValueError> {
        Header::new(
            self.local.into(),
            destination.into(),
            2,
            ci::Address::FUNCTION_BLOCK,
        )
    }
    fn info(&self) -> DiscoveryInfo {
        DiscoveryInfo::new(
            self.identity.clone(),
            0,
            self.max_sysex.into(),
            Some(self.output_path.into()),
        )
        .expect("validated settings")
    }
    fn enqueue(&mut self, message: Management, timeout: u64) -> Result<Action, ValueError> {
        if timeout == 0 {
            return Err(error("timeout must be positive"));
        }
        if self.pending.len() >= self.exchange_limit {
            return Err(error("active exchange limit reached"));
        }
        let expires = self
            .now
            .checked_add(timeout)
            .ok_or_else(|| error("deadline overflow"))?;
        let token = self.next_token;
        self.next_token = self
            .next_token
            .checked_add(1)
            .ok_or_else(|| error("action token exhausted"))?;
        let action = Action {
            token,
            route: self.route.clone(),
            message,
        };
        self.pending.push(Pending {
            action: action.clone(),
            sent: false,
            expires,
            timeout,
            provisional: vec![],
        });
        Ok(action)
    }
    pub fn discover(&mut self, now: u64, timeout: u64) -> Result<Action, ValueError> {
        self.running()?;
        self.time(now)?;
        if timeout < 3_000_000_000 {
            return Err(error("Discovery requires at least three seconds"));
        }
        if self
            .pending
            .iter()
            .any(|p| matches!(p.action.message.body(), Body::Discovery(_)))
        {
            return Err(error("Discovery is already pending"));
        }
        let message = Management::new(
            self.header(BROADCAST_MUID)?,
            Body::Discovery(self.info()),
            vec![],
        )?;
        self.enqueue(message, timeout)
    }
    pub fn inquire_endpoint(
        &mut self,
        peer: u32,
        now: u64,
        timeout: u64,
    ) -> Result<Action, ValueError> {
        self.running()?;
        self.time(now)?;
        if !self.peers.iter().any(|p| p.muid == peer && p.version >= 2) {
            return Err(error(
                "endpoint inquiry requires a discovered format-2 peer",
            ));
        }
        if self.pending.iter().any(|p| {
            p.action.message.header().destination_muid() == peer
                && matches!(p.action.message.body(), Body::EndpointInquiry)
        }) {
            return Err(error("endpoint inquiry already pending for peer"));
        }
        let message = Management::new(self.header(peer)?, Body::EndpointInquiry, vec![])?;
        self.enqueue(message, timeout)
    }
    pub fn report_send(
        &mut self,
        token: u64,
        sent: bool,
        now: u64,
    ) -> Result<Vec<Update>, ValueError> {
        self.time(now)?;
        let i = self
            .pending
            .iter()
            .position(|p| p.action.token == token)
            .ok_or_else(|| error("unknown or expired action token"))?;
        if self.pending[i].sent {
            return Err(error("send result already reported"));
        }
        let peer = self.pending[i].action.message.header().destination_muid();
        if self.pending[i].expires <= now {
            self.pending.remove(i);
            return Ok(vec![observe(
                "send_confirmation_timeout",
                Some(peer),
                Some(token),
                None,
                "send result arrived after its admission deadline",
            )]);
        }
        if !sent {
            self.pending.remove(i);
            return Ok(vec![observe(
                "send_failed",
                Some(peer),
                Some(token),
                None,
                "delivery is uncertain; no automatic retry",
            )]);
        }
        let request = matches!(
            self.pending[i].action.message.body(),
            Body::Discovery(_) | Body::EndpointInquiry
        );
        let provisional = if request {
            let p = &mut self.pending[i];
            let expires = now
                .checked_add(p.timeout)
                .ok_or_else(|| error("deadline overflow"))?;
            p.sent = true;
            p.expires = expires;
            std::mem::take(&mut p.provisional)
        } else {
            self.pending.remove(i);
            vec![]
        };
        let mut out = vec![observe(
            "send_accepted",
            Some(peer),
            Some(token),
            None,
            "backend acceptance is not device acknowledgment",
        )];
        let route = self.route.clone();
        for (message, _) in provisional {
            if !self.active {
                break;
            }
            out.extend(self.feed(&message.sysex(), route.input(), route.group(), now)?);
        }
        Ok(out)
    }

    pub fn poll(&mut self, now: u64) -> Result<Vec<Update>, ValueError> {
        self.time(now)?;
        let mut out = vec![];
        let mut i = 0;
        while i < self.pending.len() {
            if self.pending[i].expires <= now {
                let p = self.pending.remove(i);
                let kind = if !p.sent {
                    "send_confirmation_timeout"
                } else if matches!(p.action.message.body(), Body::Discovery(_)) {
                    "discovery_finished"
                } else {
                    "timeout"
                };
                out.push(observe(
                    kind,
                    Some(p.action.message.header().destination_muid()),
                    Some(p.action.token),
                    None,
                    "deadline reached; silence does not establish device failure",
                ));
            } else {
                i += 1;
            }
        }
        Ok(out)
    }
    pub fn reset(&mut self, reason: String) -> Vec<Update> {
        self.active = false;
        self.peers.clear();
        self.pending.clear();
        vec![observe("restart_required", None, None, None, &reason)]
    }
    pub fn restart(&mut self, local_muid: i64, now: u64) -> Result<(), ValueError> {
        self.time(now)?;
        let local = Header::new(
            local_muid,
            BROADCAST_MUID.into(),
            2,
            ci::Address::FUNCTION_BLOCK,
        )?
        .source_muid();
        if local == self.local {
            return Err(error("restart requires a newly randomized MUID"));
        }
        self.local = local;
        self.pending.clear();
        self.peers.clear();
        self.active = true;
        Ok(())
    }
    fn reply(&mut self, message: Management, out: &mut Vec<Update>) {
        match self.enqueue(message, 3_000_000_000) {
            Ok(action) => out.push(Update::Send(action)),
            Err(e) => out.push(observe("resource_limit", None, None, None, &e.0)),
        }
    }
    fn nak(
        &mut self,
        header: &Header,
        original: u8,
        status: u8,
        out: &mut Vec<Update>,
    ) -> Result<(), ValueError> {
        let notice = Notice::new(original.into(), status.into(), 0, vec![0; 5], vec![])?;
        let message = Management::new(
            Header::new(
                self.local.into(),
                header.source_muid().into(),
                2,
                header.address(),
            )?,
            Body::Nak(Some(notice)),
            vec![],
        )?;
        self.reply(message, out);
        Ok(())
    }
    fn forget(&mut self, peer: u32) {
        self.peers.retain(|p| p.muid != peer);
        self.pending.retain(|p| {
            p.action.message.header().destination_muid() != peer
                && !matches!(p.action.message.body(), Body::Discovery(_))
        });
    }
    fn remember(
        &mut self,
        header: &Header,
        info: &DiscoveryInfo,
        function_block: Option<u8>,
        out: &mut Vec<Update>,
    ) -> Result<bool, ValueError> {
        let source = header.source_muid();
        if let Some(p) = self.peers.iter().find(|p| p.muid == source) {
            if p.version != header.version()
                || p.info.identity() != info.identity()
                || p.function_block
                    .zip(function_block)
                    .is_some_and(|(a, b)| a != b)
            {
                self.forget(source);
                self.pending.clear();
                let invalidate = Management::new(
                    self.header(BROADCAST_MUID)?,
                    Body::InvalidateMuid { target: source },
                    vec![],
                )?;
                self.reply(invalidate, out);
                out.push(observe(
                    "peer_collision",
                    Some(source),
                    None,
                    None,
                    "conflicting identity or Function Block; explicit rediscovery required",
                ));
                return Ok(false);
            }
            let p = self
                .peers
                .iter_mut()
                .find(|p| p.muid == source)
                .expect("located peer");
            p.version = header.version();
            p.info = info.clone();
            if function_block.is_some() {
                p.function_block = function_block;
            }
            return Ok(true);
        }
        if self.peers.len() >= self.peer_limit {
            out.push(observe(
                "resource_limit",
                Some(source),
                None,
                None,
                "discovered peer limit reached",
            ));
            return Ok(false);
        }
        self.peers.push(Peer {
            muid: source,
            version: header.version(),
            info: info.clone(),
            function_block,
            product_instance_id: None,
        });
        out.push(observe(
            "peer_discovered",
            Some(source),
            None,
            None,
            "peer declaration is not proof of responsiveness",
        ));
        Ok(true)
    }
    /// Feed complete SysEx from exactly this input route/group. Assemble UMP SysEx7 first.
    pub fn feed(
        &mut self,
        sysex: &SysEx,
        input: &str,
        group: Option<u8>,
        now: u64,
    ) -> Result<Vec<Update>, ValueError> {
        self.running()?;
        if input != self.route.input || group != self.route.group {
            return Err(error("message belongs to another input route/group"));
        }
        self.time(now)?;
        let mut out = self.poll(now)?;
        let b = sysex.data();
        if b.len() + 2 > self.max_sysex as usize {
            return Ok(vec![observe(
                "resource_limit",
                None,
                None,
                None,
                "incoming SysEx exceeds advertised receive limit",
            )]);
        }
        if b.first() != Some(&126) || b.get(2) != Some(&13) {
            return Ok(out);
        }
        if b.len() < 13 {
            out.push(observe(
                "malformed",
                None,
                None,
                None,
                "truncated CI header",
            ));
            return Ok(out);
        }
        let source = read_muid(&b[5..9]);
        let destination = read_muid(&b[9..13]);
        let kind = b[3];
        if destination != self.local && destination != BROADCAST_MUID {
            return Ok(out);
        }
        let valid_header = Header::new(
            source.into(),
            destination.into(),
            2,
            match ci::Address::from_wire(b[1]) {
                Ok(a) => a,
                Err(e) => {
                    out.push(observe("malformed", None, None, None, &e.0));
                    return Ok(out);
                }
            },
        );
        let Ok(reply_header) = valid_header else {
            out.push(observe(
                "malformed",
                None,
                None,
                None,
                "reserved or invalid MUID",
            ));
            return Ok(out);
        };
        let message = match ci::decode(sysex) {
            Ok(Some(m)) => m,
            Ok(None) => {
                if destination == self.local {
                    self.nak(&reply_header, kind, 1, &mut out)?;
                }
                out.push(observe(
                    "unsupported",
                    Some(source),
                    None,
                    None,
                    "CI category or endpoint selector is not implemented",
                ));
                return Ok(out);
            }
            Err(e) => {
                if destination == self.local || kind == 0x70 {
                    self.nak(
                        &reply_header,
                        kind,
                        if b[4] == 0 || b[4] & 0x60 != 0 {
                            2
                        } else {
                            0x41
                        },
                        &mut out,
                    )?;
                }
                out.push(observe("malformed", Some(source), None, None, &e.0));
                return Ok(out);
            }
        };
        if source == self.local {
            if kind == 0x70 || kind == 0x71 {
                out.extend(self.reset("local MUID collision; send invalidation then restart with a fresh random MUID".into()));
                let invalidate = Management::new(
                    self.header(BROADCAST_MUID)?,
                    Body::InvalidateMuid { target: self.local },
                    vec![],
                )?;
                self.reply(invalidate, &mut out);
            } else {
                out.push(observe(
                    "self_addressed",
                    Some(source),
                    None,
                    None,
                    "looped-back or conflicting source; not accepted as a peer",
                ));
            }
            return Ok(out);
        }
        let provisional = self.pending.iter().position(|p| {
            !p.sent
                && match message.body() {
                    Body::DiscoveryReply { .. } => {
                        matches!(p.action.message.body(), Body::Discovery(_))
                    }
                    Body::EndpointReply { .. } => {
                        p.action.message.header().destination_muid() == source
                            && matches!(p.action.message.body(), Body::EndpointInquiry)
                    }
                    Body::Nak(Some(n)) => {
                        n.original() == 0x72
                            && p.action.message.header().destination_muid() == source
                            && matches!(p.action.message.body(), Body::EndpointInquiry)
                    }
                    _ => false,
                }
        });
        if let Some(i) = provisional {
            use crate::messages::PrimitiveMessage;
            let token = self.pending[i].action.token;
            let duplicate = self.pending[i]
                .provisional
                .iter()
                .any(|(m, _)| m == &message);
            if duplicate {
                out.push(observe(
                    "duplicate_provisional_reply",
                    Some(source),
                    Some(token),
                    None,
                    "identical reply already staged",
                ));
                return Ok(out);
            }
            let used: usize = self
                .pending
                .iter()
                .flat_map(|p| &p.provisional)
                .map(|(_, n)| n)
                .sum();
            let size = message.encode().expect("validated system message").len();
            let count_limit = if matches!(message.body(), Body::DiscoveryReply { .. }) {
                self.peer_limit
            } else {
                1
            };
            if self.pending[i].provisional.len() >= count_limit
                || size > self.reply_byte_limit.saturating_sub(used)
            {
                out.push(observe(
                    "resource_limit",
                    Some(source),
                    Some(token),
                    None,
                    "provisional reply admission exceeded; reply not retained",
                ));
            } else {
                self.pending[i].provisional.push((message, size));
                out.push(observe(
                    "reply_pending_send_result",
                    Some(source),
                    Some(token),
                    None,
                    "reply retained until the send result is reported",
                ));
            }
            return Ok(out);
        }
        match message.body() {
            Body::Discovery(info) => {
                if self.remember(message.header(), info, None, &mut out)? {
                    let reply = Management::new(
                        self.header(source)?,
                        Body::DiscoveryReply {
                            info: DiscoveryInfo::new(
                                self.identity.clone(),
                                0,
                                self.max_sysex.into(),
                                Some(info.output_path().unwrap_or(0).into()),
                            )?,
                            function_block: self.function_block,
                        },
                        vec![],
                    )?;
                    self.reply(reply, &mut out);
                }
            }
            Body::DiscoveryReply {
                info,
                function_block,
            } => {
                if let Some(p) = self
                    .pending
                    .iter()
                    .find(|p| p.sent && matches!(p.action.message.body(), Body::Discovery(_)))
                {
                    let token = p.action.token;
                    if info.output_path().is_some_and(|n| n != self.output_path) {
                        out.push(observe(
                            "unrelated_reply",
                            Some(source),
                            Some(token),
                            Some(message.clone()),
                            "reply output path differs",
                        ));
                    } else if self.remember(message.header(), info, *function_block, &mut out)? {
                        out.push(observe(
                            "discovery_reply",
                            Some(source),
                            Some(token),
                            Some(message.clone()),
                            "reply received during discovery window",
                        ));
                    }
                } else {
                    out.push(observe(
                        "unrelated_reply",
                        Some(source),
                        None,
                        Some(message.clone()),
                        "no confirmed Discovery is pending",
                    ));
                }
            }
            Body::EndpointInquiry => out.push(observe(
                "endpoint_request",
                Some(source),
                None,
                Some(message.clone()),
                "caller must explicitly supply an endpoint reply or NAK",
            )),
            Body::EndpointReply {
                product_instance_id,
            } => {
                let i = self.pending.iter().position(|p| {
                    p.sent
                        && p.action.message.header().destination_muid() == source
                        && matches!(p.action.message.body(), Body::EndpointInquiry)
                });
                if let Some(i) = i {
                    let p = self.pending.remove(i);
                    if let Some(peer) = self.peers.iter_mut().find(|p| p.muid == source) {
                        peer.product_instance_id = Some(product_instance_id.clone());
                    }
                    out.push(observe(
                        "endpoint_reply",
                        Some(source),
                        Some(p.action.token),
                        Some(message.clone()),
                        "reply cannot distinguish an identical late response",
                    ));
                } else {
                    out.push(observe(
                        "unrelated_reply",
                        Some(source),
                        None,
                        Some(message.clone()),
                        "no endpoint inquiry pending",
                    ));
                }
            }
            Body::InvalidateMuid { target } => {
                if *target == self.local {
                    out.extend(
                        self.reset("local MUID invalidated; fresh random MUID required".into()),
                    );
                } else {
                    self.forget(*target);
                    out.push(observe(
                        "peer_invalidated",
                        Some(*target),
                        None,
                        Some(message.clone()),
                        "cached identity and transactions discarded",
                    ));
                }
            }
            Body::Nak(notice) => {
                if notice.as_ref().is_some_and(|n| n.original() == 0x72) {
                    self.pending.retain(|p| {
                        !(p.sent
                            && p.action.message.header().destination_muid() == source
                            && matches!(p.action.message.body(), Body::EndpointInquiry))
                    });
                }
                out.push(observe(
                    "nak",
                    Some(source),
                    None,
                    Some(message.clone()),
                    "no automatic retry or suggested state change",
                ));
            }
            Body::Ack(_) => out.push(observe(
                "ack",
                Some(source),
                None,
                Some(message.clone()),
                "acknowledgment retained; deferred category semantics not applied",
            )),
        }
        Ok(out)
    }
    pub fn respond(
        &mut self,
        message: Management,
        now: u64,
        timeout: u64,
    ) -> Result<Action, ValueError> {
        self.running()?;
        self.time(now)?;
        if message.header().source_muid() != self.local
            || message.header().version() != 2
            || !matches!(
                message.body(),
                Body::EndpointReply { .. } | Body::Nak(_) | Body::Ack(_)
            )
        {
            return Err(error(
                "respond requires a local format-2 endpoint reply, ACK or NAK",
            ));
        }
        let peer = self
            .peers
            .iter()
            .find(|p| p.muid == message.header().destination_muid())
            .ok_or_else(|| error("response requires a discovered peer"))?;
        use crate::messages::PrimitiveMessage;
        if message.encode().expect("validated system message").len()
            > peer.info.max_sysex() as usize
        {
            return Err(error("response exceeds peer receive limit"));
        }
        self.enqueue(message, timeout)
    }
}
fn read_muid(b: &[u8]) -> u32 {
    b.iter()
        .enumerate()
        .fold(0, |n, (i, b)| n | u32::from(*b) << (7 * i))
}
fn observe(
    kind: &str,
    peer_muid: Option<u32>,
    token: Option<u64>,
    message: Option<Management>,
    reason: &str,
) -> Update {
    Update::Observe(Observation {
        kind: kind.into(),
        peer_muid,
        token,
        message,
        reason: reason.into(),
    })
}

#[derive(Clone, Debug)]
pub struct BrowseDiagnostic {
    pub kind: String,
    pub peer_muid: Option<u32>,
    pub token: Option<u64>,
    pub reason: String,
}
#[derive(Clone, Debug)]
pub struct BrowseReport {
    pub status: &'static str,
    pub local_muid: u32,
    pub peers: Vec<Peer>,
    pub confirmed_actions: u64,
    pub uncertain_token: Option<u64>,
    pub pending_tokens: Vec<u64>,
    pub diagnostics: Vec<BrowseDiagnostic>,
    pub omitted_diagnostics: u64,
    pub problems: u64,
    pub reason: Option<String>,
}
pub struct Browser {
    session: Session,
    discovery_ns: u64,
    endpoint_ns: u64,
    timeout_ns: u64,
    deadline: Option<u64>,
    detail_limit: usize,
    report: BrowseReport,
    outstanding: BTreeSet<u64>,
    targets: VecDeque<u32>,
}
impl Browser {
    pub fn route(&self) -> &crate::ci::Route {
        self.session.route()
    }
    pub fn new(
        session: Session,
        discovery_ns: u64,
        endpoint_ns: u64,
        timeout_ns: u64,
        detail_limit: usize,
    ) -> Result<Self, ValueError> {
        if discovery_ns < 3_000_000_000
            || endpoint_ns == 0
            || timeout_ns <= discovery_ns
            || detail_limit == 0
            || !session.peers().is_empty()
            || session.pending_count() != 0
            || !session.active()
        {
            return Err(error("fresh session, discovery >= 3s, positive endpoint/detail limits and overall timeout > discovery required"));
        }
        let local_muid = session.local_muid();
        Ok(Self {
            session,
            discovery_ns,
            endpoint_ns,
            timeout_ns,
            deadline: None,
            detail_limit,
            report: BrowseReport {
                status: "ready",
                local_muid,
                peers: vec![],
                confirmed_actions: 0,
                uncertain_token: None,
                pending_tokens: vec![],
                diagnostics: vec![],
                omitted_diagnostics: 0,
                problems: 0,
                reason: None,
            },
            outstanding: BTreeSet::new(),
            targets: VecDeque::new(),
        })
    }
    pub fn start(&mut self, now: u64) -> Result<Vec<Action>, ValueError> {
        if self.report.status != "ready" {
            return Err(error("browser scan has already started"));
        }
        let deadline = now
            .checked_add(self.timeout_ns)
            .ok_or_else(|| error("scan deadline overflow"))?;
        let action = self.session.discover(now, self.discovery_ns)?;
        self.deadline = Some(deadline);
        self.report.status = "discovering";
        self.outstanding.insert(action.token);
        Ok(vec![action])
    }
    pub fn feed(
        &mut self,
        message: &SysEx,
        input: &str,
        group: Option<u8>,
        now: u64,
    ) -> Result<Vec<Action>, ValueError> {
        self.running()?;
        if self.expired(now) || self.report.status == "stopping" {
            return Ok(vec![]);
        }
        let updates = self.session.feed(message, input, group, now)?;
        self.process(updates, now)
    }
    pub fn poll(&mut self, now: u64) -> Result<Vec<Action>, ValueError> {
        self.running()?;
        if self.expired(now) {
            return Ok(vec![]);
        }
        let updates = self.session.poll(now)?;
        self.process(updates, now)
    }
    pub fn report_send(
        &mut self,
        token: u64,
        sent: bool,
        now: u64,
    ) -> Result<Vec<Action>, ValueError> {
        self.running()?;
        if !self.outstanding.contains(&token) {
            return Err(error("unknown or retired browser action"));
        }
        let updates = self.session.report_send(token, sent, now)?;
        self.outstanding.remove(&token);
        if sent {
            self.report.confirmed_actions += 1;
        } else {
            self.report.uncertain_token = Some(token);
            self.stop("management send failed; delivery may be partial");
        }
        self.process(updates, now)
    }
    pub fn stop(&mut self, reason: &str) {
        if matches!(
            self.report.status,
            "ready" | "discovering" | "inquiring" | "stopping"
        ) {
            self.report.status = "stopped";
            if self.report.reason.is_none() {
                self.report.reason = Some(reason.chars().take(1024).collect());
            }
            self.outstanding.clear();
            self.targets.clear();
        }
    }
    pub fn snapshot(&self) -> BrowseReport {
        let mut report = self.report.clone();
        report.pending_tokens = self.outstanding.iter().copied().collect();
        report
    }
    fn running(&self) -> Result<(), ValueError> {
        if !matches!(self.report.status, "discovering" | "inquiring" | "stopping") {
            return Err(error("browser is not running"));
        }
        Ok(())
    }
    fn expired(&mut self, now: u64) -> bool {
        if self.deadline.is_some_and(|d| now >= d) {
            self.stop("overall scan deadline reached");
            true
        } else {
            false
        }
    }
    fn process(&mut self, updates: Vec<Update>, now: u64) -> Result<Vec<Action>, ValueError> {
        let mut actions = vec![];
        let mut endpoint_requests = vec![];
        let mut collision = None;
        let mut fatal = None;
        for update in updates {
            match update {
                Update::Send(action) => actions.push(action),
                Update::Observe(o) => {
                    match o.kind.as_str() {
                        "discovery_finished" if self.report.status == "discovering" => {
                            self.report.status = "inquiring";
                            self.targets = self
                                .session
                                .peers()
                                .iter()
                                .filter(|p| p.version >= 2)
                                .map(|p| p.muid)
                                .collect();
                        }
                        "endpoint_request" => {
                            if let Some(m) = &o.message {
                                endpoint_requests.push(m.clone());
                            }
                        }
                        "restart_required" | "peer_collision" => {
                            collision = Some(o.reason.clone());
                        }
                        "peer_invalidated" | "resource_limit" | "send_confirmation_timeout" => {
                            fatal = Some(o.reason.clone());
                        }
                        _ => {}
                    }
                    if matches!(
                        o.kind.as_str(),
                        "timeout"
                            | "nak"
                            | "malformed"
                            | "unsupported"
                            | "resource_limit"
                            | "send_confirmation_timeout"
                            | "restart_required"
                            | "peer_collision"
                            | "peer_invalidated"
                    ) {
                        self.report.problems += 1;
                    }
                    if self.report.diagnostics.len() < self.detail_limit {
                        self.report.diagnostics.push(BrowseDiagnostic {
                            kind: o.kind,
                            peer_muid: o.peer_muid,
                            token: o.token,
                            reason: o.reason.chars().take(1024).collect(),
                        });
                    } else {
                        self.report.omitted_diagnostics += 1;
                    }
                }
            }
        }
        if let Some(reason) = collision {
            self.outstanding.clear();
            self.targets.clear();
            actions.retain(|a| matches!(a.message.body(), Body::InvalidateMuid { .. }));
            self.report.status = "stopping";
            self.report.reason = Some(reason.chars().take(1024).collect());
        } else if let Some(reason) = fatal {
            self.stop(&reason);
        } else if matches!(self.report.status, "discovering" | "inquiring") {
            self.report.peers = self.session.peers().to_vec();
            for request in endpoint_requests {
                if !self
                    .session
                    .peers()
                    .iter()
                    .any(|p| p.muid == request.header().source_muid())
                {
                    self.report.problems += 1;
                    continue;
                }
                let header = Header::new(
                    self.session.local_muid().into(),
                    request.header().source_muid().into(),
                    2,
                    request.header().address(),
                )?;
                let notice = Notice::new(0x72, 1, 0, vec![0; 5], vec![])?;
                actions.push(self.session.respond(
                    Management::new(header, Body::Nak(Some(notice)), vec![])?,
                    now,
                    self.endpoint_ns,
                )?);
            }
        }
        if !matches!(self.report.status, "discovering" | "inquiring" | "stopping") {
            return Ok(vec![]);
        }
        for action in &actions {
            self.outstanding.insert(action.token);
        }
        if self.outstanding.is_empty() {
            if self.report.status == "stopping" {
                self.stop("identity recovery required");
            } else if self.report.status == "inquiring" && self.session.pending_count() == 0 {
                if let Some(peer) = self.targets.pop_front() {
                    let action = self.session.inquire_endpoint(peer, now, self.endpoint_ns)?;
                    self.outstanding.insert(action.token);
                    actions.push(action);
                } else {
                    self.report.status = "completed";
                }
            }
        }
        Ok(actions)
    }
}

/// Controls shared with Python's keyword options; drivers own their clock.
#[derive(Clone)]
pub struct Options {
    pub cancel: Cancellation,
    pub message_limit: usize,
    pub byte_limit: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            cancel: Cancellation::default(),
            message_limit: 1_048_576,
            byte_limit: 8_388_608,
        }
    }
}

pub fn browse(
    browser: &mut Browser,
    receiver: &mut midi::Input,
    output: &mut midi::Output,
    options: &Options,
) -> midi::Result<BrowseReport> {
    if browser.snapshot().status != "ready" {
        return Err(drivers::invalid("browser must be fresh"));
    }
    if browser.route().input() != receiver.stream()? {
        return Err(drivers::invalid(
            "browser route must match the receiver stream",
        ));
    }
    browse_with(
        browser,
        receiver,
        output,
        &mut MonotonicClock::default(),
        options,
    )
}
#[doc(hidden)]
pub fn browse_with(
    browser: &mut Browser,
    receiver: &mut impl Receiver,
    output: &mut impl Sender,
    clock: &mut impl PlaybackClock,
    options: &Options,
) -> midi::Result<BrowseReport> {
    let route = browser.route().clone();
    let cancel = &options.cancel;
    let mut assembler = ump::Assembler::new(options.message_limit, options.byte_limit, 1)?;
    let mut queue = VecDeque::from(browser.start(clock.now_ns())?);
    let mut sending = None;
    let result = (|| -> midi::Result<()> {
        'scan: while matches!(
            browser.snapshot().status,
            "discovering" | "inquiring" | "stopping"
        ) {
            if cancel.cancelled() {
                browser.stop("cancelled by caller");
                break;
            }
            if let Err(error) = receiver.check() {
                if drivers::input_loss(&error) {
                    browser.stop(&error.to_string());
                    break;
                }
                return Err(error);
            }
            queue.extend(browser.poll(clock.now_ns())?);
            let pending = browser.snapshot().pending_tokens;
            queue.retain(|a| pending.contains(&a.token));
            if let Some(action) = queue.pop_front() {
                let wire = action.message.encode()?;
                let raw = SysEx::new(wire[1..wire.len() - 1].to_vec())?;
                let packets = if let Some(group) = route.group() {
                    ump::packetize_sysex7(&raw.clone().with_group(group.into())?)?
                        .into_iter()
                        .map(|p| Payload::Ump(PacketValue::SysEx7(p)))
                        .collect::<Vec<_>>()
                } else {
                    vec![Payload::Message(AnyMessage::SysEx(raw))]
                };
                for packet in packets {
                    receiver.check()?;
                    if cancel.cancelled() {
                        browser.stop("cancelled by caller");
                        break 'scan;
                    }
                    sending = Some(action.token);
                    if route.group().is_none() {
                        output.send_management(action.message.clone())?;
                    } else {
                        output.send(packet)?;
                    }
                }
                queue.extend(browser.report_send(action.token, true, clock.now_ns())?);
                sending = None;
                continue;
            }
            if !matches!(browser.snapshot().status, "discovering" | "inquiring") {
                continue;
            }
            let Some(item) = receiver.receive(Duration::from_millis(50))? else {
                continue;
            };
            receiver.check()?;
            if cancel.cancelled() {
                browser.stop("cancelled by caller");
                break;
            }
            let message = match item {
                Item::Message(m) => m,
                Item::Diagnostic(d) => {
                    browser.stop(&d.reason);
                    break;
                }
            };
            if message.stream() != Some(route.input()) {
                browser.stop("received event from an unrelated stream");
                break;
            }
            match (&*message, route.group()) {
                (Payload::Message(AnyMessage::SysEx(raw)), None) => {
                    queue.extend(browser.feed(raw, route.input(), None, clock.now_ns())?)
                }
                (Payload::Ump(PacketValue::SysEx7(packet)), Some(group))
                    if packet.group() == group =>
                {
                    for observation in assembler.feed(&message)? {
                        match observation.value {
                            ObservationValue::Ump(AssemblyItem::Data(data)) => {
                                queue.extend(browser.feed(
                                    &SysEx::new(data.data)?,
                                    route.input(),
                                    Some(group),
                                    clock.now_ns(),
                                )?)
                            }
                            _ => {
                                browser.stop("UMP SysEx7 assembly diagnostic");
                                break;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    })();
    if let Some(token) = sending {
        if browser.snapshot().pending_tokens.contains(&token) {
            browser.report_send(token, false, clock.now_ns())?;
        }
    }
    assembler.reset();
    if let Err(error) = result {
        browser.stop(&error.to_string());
        if drivers::interrupted(&error) {
            return Ok(browser.snapshot());
        }
        return Err(error);
    }
    Ok(browser.snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ci::{Identity, Route, Session},
        drivers::tests::{Clock, Input, Output},
    };
    fn browser(group: Option<u8>) -> Browser {
        let identity = Identity::new(vec![125, 0, 0], 1, 2, vec![0; 4]).unwrap();
        let session = Session::new(
            1,
            identity,
            Route::new("s".into(), "out".into(), group.map(i64::from)).unwrap(),
            0,
            None,
            1024,
            2,
            4,
            4096,
        )
        .unwrap();
        Browser::new(session, 3_000_000_000, 100, 4_000_000_000, 16).unwrap()
    }
    #[test]
    fn options_reject_invalid_limits_and_cancel_without_sending() {
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
        let options = Options {
            message_limit: 0,
            ..Default::default()
        };
        assert!(browse_with(
            &mut browser(Some(1)),
            &mut input,
            &mut output,
            &mut clock,
            &options
        )
        .is_err());
        let options = Options::default();
        options.cancel.cancel();
        let report = browse_with(
            &mut browser(None),
            &mut input,
            &mut output,
            &mut clock,
            &options,
        )
        .unwrap();
        assert_eq!(report.status, "stopped");
        assert!(output.sent.is_empty());
    }

    #[test]
    fn scan_packetizes_ump_and_marks_partial_action_uncertain() {
        for group in [None, Some(1)] {
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
            let report = browse_with(
                &mut browser(group),
                &mut input,
                &mut output,
                &mut clock,
                &Options::default(),
            )
            .unwrap();
            assert_eq!(report.status, "completed");
            assert_eq!(report.confirmed_actions, 1);
            assert!(output.sent.iter().all(|(_, m)| matches!(
                (group, m),
                (None, Payload::Message(_)) | (Some(_), Payload::Ump(_))
            )));
        }
        let mut clock = Clock::default();
        let mut input = Input {
            clock: clock.clone(),
            items: [].into(),
            lost: false,
        };
        let mut output = Output {
            clock: clock.clone(),
            sent: vec![],
            fail_at: Some(1),
        };
        let mut browser = browser(Some(1));
        assert!(browse_with(
            &mut browser,
            &mut input,
            &mut output,
            &mut clock,
            &Options::default()
        )
        .is_err());
        assert_eq!(browser.snapshot().status, "stopped");
        assert!(browser.snapshot().uncertain_token.is_some());
    }
}
