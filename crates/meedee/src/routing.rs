//! Static, one-hop routing. The caller owns input ordering and output delivery.
use crate::messages::PrimitiveMessage;
use crate::{archive, codecs, events, messages, transport::Protocol, ump};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{self, Read};

#[derive(Clone, Debug)]
pub struct Route {
    name: String,
    source: String,
    destination: String,
    protocol: Protocol,
    channel: Option<u8>,
    group: Option<u8>,
    remap_channel: Option<u8>,
    remap_group: Option<u8>,
    include_nonchannel: bool,
}
impl Route {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn destination(&self) -> &str {
        &self.destination
    }
    pub fn protocol(&self) -> Protocol {
        self.protocol
    }
    pub fn channel(&self) -> Option<u8> {
        self.channel
    }
    pub fn group(&self) -> Option<u8> {
        self.group
    }
    pub fn remap_channel(&self) -> Option<u8> {
        self.remap_channel
    }
    pub fn remap_group(&self) -> Option<u8> {
        self.remap_group
    }
    pub fn include_nonchannel(&self) -> bool {
        self.include_nonchannel
    }
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: String,
        source: String,
        destination: String,
        protocol: Protocol,
        channel: Option<u8>,
        group: Option<u8>,
        remap_channel: Option<u8>,
        remap_group: Option<u8>,
        include_nonchannel: bool,
    ) -> io::Result<Self> {
        if name.is_empty()
            || source.is_empty()
            || destination.is_empty()
            || [channel, group, remap_channel, remap_group]
                .into_iter()
                .flatten()
                .any(|v| !(1..=16).contains(&v))
            || protocol == Protocol::Midi1 && (group.is_some() || remap_group.is_some())
        {
            return Err(invalid(
                "nonempty route identities and valid protocol/channel/group required",
            ));
        }
        Ok(Self {
            name,
            source,
            destination,
            protocol,
            channel,
            group,
            remap_channel,
            remap_group,
            include_nonchannel,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Delivery {
    pub id: u64,
    pub route: String,
    pub destination: String,
    /// Transformed message, with the original source, timestamp and span.
    pub event: events::Event,
    pub wire_bytes: usize,
}
#[derive(Clone, Debug, Default)]
pub struct RouteReport {
    pub admitted: u64,
    pub confirmed: u64,
    pub capacity_rejections: u64,
}
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub received: u64,
    pub filtered: u64,
    pub admitted: u64,
    pub confirmed: u64,
    pub capacity_rejections: u64,
    pub pending: usize,
    pub queued_bytes: usize,
    pub stopped: bool,
    pub reason: Option<String>,
    pub uncertain_delivery: Option<u64>,
    pub uncertain_route: Option<String>,
    pub uncertain_destination: Option<String>,
    pub routes: BTreeMap<String, RouteReport>,
}

pub struct Router {
    routes: Vec<Route>,
    queue: VecDeque<Delivery>,
    event_limit: usize,
    byte_limit: usize,
    report: Report,
}
impl Router {
    pub fn boundary(&mut self, source: &str, reason: &str) {
        if self.routes.iter().any(|r| r.source == source) {
            self.stop(reason);
        }
    }
    pub fn new(routes: Vec<Route>, event_limit: usize, byte_limit: usize) -> io::Result<Self> {
        let mut names = BTreeSet::new();
        let mut destinations = BTreeMap::new();
        if routes.is_empty() || routes.len() > 128 || event_limit == 0 || byte_limit == 0 {
            return Err(invalid(
                "between 1 and 128 routes and positive queue limits required",
            ));
        }
        for r in &routes {
            if !names.insert(&r.name) {
                return Err(invalid("route names must be unique"));
            }
            if destinations
                .insert(&r.destination, r.protocol)
                .is_some_and(|p| p != r.protocol)
            {
                return Err(invalid("one destination cannot use multiple protocols"));
            }
        }
        let report = Report {
            routes: routes
                .iter()
                .map(|r| (r.name.clone(), RouteReport::default()))
                .collect(),
            ..Report::default()
        };
        Ok(Self {
            routes,
            queue: VecDeque::new(),
            event_limit,
            byte_limit,
            report,
        })
    }
    pub fn snapshot(&self) -> Report {
        self.report.clone()
    }
    /// Peek does not consume. Confirm only after a complete successful send.
    pub fn peek(&self) -> Option<&Delivery> {
        if self.report.stopped {
            None
        } else {
            self.queue.front()
        }
    }
    pub fn confirm(&mut self, id: u64) -> io::Result<()> {
        self.check_head(id)?;
        let d = self.queue.pop_front().expect("checked queue head");
        self.report.pending -= 1;
        self.report.queued_bytes -= d.wire_bytes;
        self.report.confirmed += 1;
        self.report
            .routes
            .get_mut(&d.route)
            .expect("configured route")
            .confirmed += 1;
        Ok(())
    }
    pub fn fail(&mut self, id: u64, reason: &str) -> io::Result<()> {
        self.check_head(id)?;
        let d = self.queue.front().expect("checked queue head");
        self.report.uncertain_route = Some(d.route.clone());
        self.report.uncertain_destination = Some(d.destination.clone());
        self.report.uncertain_delivery = Some(id);
        self.stop(reason);
        Ok(())
    }
    /// Stops without clearing pending progress, retrying or generating cleanup.
    pub fn stop(&mut self, reason: &str) {
        if !self.report.stopped {
            self.report.stopped = true;
            self.report.reason = Some(reason.chars().take(1024).collect());
        }
    }
    /// All copies of this event are admitted together, or none are enqueued.
    pub fn feed(&mut self, event: &events::Event) -> io::Result<()> {
        if self.report.stopped {
            return Err(invalid("router is stopped"));
        }
        if let Err(e) = self.admit(event) {
            self.stop(&e.to_string());
            return Err(e);
        }
        Ok(())
    }
    fn check_head(&self, id: u64) -> io::Result<()> {
        if self.peek().is_none_or(|d| d.id != id) {
            Err(invalid("delivery is not the active queue head"))
        } else {
            Ok(())
        }
    }
    fn admit(&mut self, event: &events::Event) -> io::Result<()> {
        event.validate()?;
        self.report.received = self
            .report
            .received
            .checked_add(1)
            .ok_or_else(|| invalid("event count exhausted"))?;
        let mut selected = Vec::new();
        for (i, r) in self
            .routes
            .iter()
            .enumerate()
            .filter(|(_, r)| r.source == event.stream)
        {
            let info = classify(&event.message)?;
            if info.protocol != r.protocol
                || r.group.is_some_and(|g| Some(g) != info.group)
                || info
                    .channel
                    .is_some_and(|c| r.channel.is_some_and(|s| s != c))
                || info.channel.is_none() && !r.include_nonchannel
            {
                continue;
            }
            if !info.forwardable {
                return Err(invalid("file-only or opaque UMP is not routable"));
            }
            selected.push(i);
        }
        if selected.is_empty() {
            self.report.filtered += 1;
            return Ok(());
        }
        let bytes = wire_bytes(&event.message);
        let total = bytes
            .checked_mul(selected.len())
            .and_then(|v| self.report.queued_bytes.checked_add(v));
        if self
            .queue
            .len()
            .checked_add(selected.len())
            .is_none_or(|v| v > self.event_limit)
            || total.is_none_or(|v| v > self.byte_limit)
        {
            self.report.capacity_rejections += selected.len() as u64;
            for i in &selected {
                self.report
                    .routes
                    .get_mut(&self.routes[*i].name)
                    .expect("configured route")
                    .capacity_rejections += 1;
            }
            return Err(invalid(
                "routing queue capacity exceeded; fan-out was not admitted",
            ));
        }
        self.report
            .admitted
            .checked_add(selected.len() as u64)
            .ok_or_else(|| invalid("delivery count exhausted"))?;
        // Transform before committing any queue state. Encoded lengths do not change.
        let mut deliveries = Vec::with_capacity(selected.len());
        for (n, i) in selected.into_iter().enumerate() {
            let r = &self.routes[i];
            let outgoing = events::Event {
                stream: event.stream.clone(),
                timestamp: event.timestamp.clone(),
                span: event.span.clone(),
                message: remap(&event.message, r)?,
            };
            deliveries.push(Delivery {
                id: self.report.admitted + n as u64,
                route: r.name.clone(),
                destination: r.destination.clone(),
                event: outgoing,
                wire_bytes: bytes,
            });
        }
        for d in deliveries {
            self.report
                .routes
                .get_mut(&d.route)
                .expect("configured route")
                .admitted += 1;
            self.queue.push_back(d);
            self.report.admitted += 1;
        }
        self.report.pending = self.queue.len();
        self.report.queued_bytes = total.expect("checked byte count");
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Preview {
    pub complete: bool,
    pub problem: Option<String>,
    pub records: u64,
    pub routing: Report,
    pub examples: Vec<Delivery>,
    pub omitted_examples: u64,
}
/// Verify the entire archive, even after routing stops. Confirmation here is
/// simulated preview acceptance, never an output send or device acknowledgment.
pub fn preview<R: Read>(
    source: R,
    routes: Vec<Route>,
    event_limit: usize,
    byte_limit: usize,
    payload_limit: usize,
    detail_limit: usize,
) -> io::Result<Preview> {
    if detail_limit == 0 {
        return Err(invalid("positive detail limit required"));
    }
    let mut router = Router::new(routes, event_limit, byte_limit)?;
    let mut result = Preview {
        complete: false,
        problem: None,
        records: 0,
        routing: router.snapshot(),
        examples: Vec::new(),
        omitted_examples: 0,
    };
    let mut seen = BTreeSet::new();
    let mut example_bytes = 0usize;
    let pass = (|| -> io::Result<()> {
        for record in archive::Reader::new(source, payload_limit, false, false)? {
            let record = record?;
            result.records += 1;
            if let archive::Record::Stream { id, .. } = &record {
                if router.routes.iter().any(|r| r.source == *id) {
                    seen.insert(id.clone());
                }
            }
            if router.report.stopped {
                continue;
            }
            match record {
                archive::Record::Event(e) => {
                    let _ = router.feed(&e);
                }
                archive::Record::Gap { stream, .. }
                | archive::Record::Discontinuity { stream, .. } => {
                    router.boundary(&stream, "routed source has a gap or discontinuity");
                }
                _ => {}
            }
            while let Some(d) = router.peek() {
                let id = d.id;
                if result.examples.len() < detail_limit
                    && d.wire_bytes <= byte_limit - example_bytes
                {
                    example_bytes += d.wire_bytes;
                    result.examples.push(d.clone());
                } else {
                    result.omitted_examples += 1;
                }
                router.confirm(id)?;
            }
        }
        Ok(())
    })();
    match pass {
        Ok(()) => result.complete = true,
        Err(e)
            if e.kind() == io::ErrorKind::InvalidData
                || e.kind() == io::ErrorKind::UnexpectedEof =>
        {
            result.problem = Some(e.to_string())
        }
        Err(e) => return Err(e),
    }
    if result.complete && router.routes.iter().any(|r| !seen.contains(&r.source)) {
        return Err(invalid("configured source is absent from the archive"));
    }
    result.routing = router.snapshot();
    Ok(result)
}

struct Info {
    protocol: Protocol,
    group: Option<u8>,
    channel: Option<u8>,
    forwardable: bool,
}
fn classify(payload: &events::Payload) -> io::Result<Info> {
    match payload {
        events::Payload::Midi1(v) => Ok(Info {
            protocol: Protocol::Midi1,
            group: None,
            channel: midi1_channel(v),
            forwardable: true,
        }),
        events::Payload::Command(_) => Err(invalid(
            "compound commands must be explicitly expanded before routing",
        )),
        events::Payload::Ump(v) => {
            let words = ump::encode(v);
            let kind = words[0] >> 28;
            Ok(Info {
                protocol: Protocol::Ump,
                group: matches!(kind, 1..=5 | 13).then_some(((words[0] >> 24) & 15) as u8 + 1),
                channel: matches!(kind, 2 | 4).then_some(((words[0] >> 16) & 15) as u8 + 1),
                forwardable: !matches!(
                    v,
                    ump::UmpMessage::Opaque(_)
                        | ump::UmpMessage::DeltaClockstamp(_)
                        | ump::UmpMessage::DeltaClockstampTicksPerQuarter(_)
                ) && !matches!(v, ump::UmpMessage::Stream(v) if matches!(v.status(), 32 | 33)),
            })
        }
    }
}
fn remap(payload: &events::Payload, route: &Route) -> io::Result<events::Payload> {
    let info = classify(payload)?;
    match payload {
        events::Payload::Midi1(v) if info.channel.is_some() && route.remap_channel.is_some() => {
            let mut bytes = v.encode();
            bytes[0] = (bytes[0] & 0xf0) | (route.remap_channel.expect("selected channel") - 1);
            let mut decoder = codecs::Decoder::new(1).map_err(|e| invalid(&e.to_string()))?;
            match decoder.feed(&bytes).as_slice() {
                [codecs::DecodeItem::Message(v)] => Ok(events::Payload::Midi1(v.clone())),
                _ => Err(invalid("channel remap did not produce one message")),
            }
        }
        events::Payload::Ump(v) if route.remap_group.is_some() || route.remap_channel.is_some() => {
            let mut words = ump::encode(v);
            if let Some(g) = route.remap_group.filter(|_| info.group.is_some()) {
                words[0] = (words[0] & !0x0f000000) | (u32::from(g - 1) << 24);
            }
            if let Some(c) = route.remap_channel.filter(|_| info.channel.is_some()) {
                words[0] = (words[0] & !0x000f0000) | (u32::from(c - 1) << 16);
            }
            Ok(events::Payload::Ump(
                ump::decode(&words, false).map_err(|e| invalid(&e.to_string()))?,
            ))
        }
        _ => Ok(payload.clone()),
    }
}
fn midi1_channel(value: &messages::AnyMessage) -> Option<u8> {
    use messages::AnyMessage::*;
    match value {
        NoteOn(v) => Some(v.channel()),
        NoteOff(v) => Some(v.channel()),
        PolyPressure(v) => Some(v.channel()),
        ControlChange(v) => Some(v.channel()),
        ProgramChange(v) => Some(v.channel()),
        ChannelPressure(v) => Some(v.channel()),
        PitchBend(v) => Some(v.channel()),
        _ => None,
    }
}
fn wire_bytes(payload: &events::Payload) -> usize {
    match payload {
        events::Payload::Midi1(messages::AnyMessage::SysEx(v)) => v.data().len() + 2,
        events::Payload::Midi1(v) => v.encode().len(),
        events::Payload::Ump(v) => ump::encode(v).len() * 4,
        events::Payload::Command(_) => unreachable!("commands rejected during selection"),
    }
}
fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason)
}
