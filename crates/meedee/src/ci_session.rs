//! Caller-driven MIDI-CI management. No ports, random generator, retries or threads.
use crate::ci::{self, Body, DiscoveryInfo, Header, Identity, Management, Notice, BROADCAST_MUID};
use crate::messages::{SysEx, ValueError};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Route {
    input: String,
    output: String,
    group: Option<u8>,
}
impl Route {
    pub fn new(input: String, output: String, group: Option<i64>) -> Result<Self, ValueError> {
        if input.is_empty() || output.is_empty() {
            return Err(err("route identities must be nonempty"));
        }
        if input.len() > 1024 || output.len() > 1024 {
            return Err(err("route identities exceed 1024 bytes"));
        }
        if group.is_some_and(|n| !(1..=16).contains(&n)) {
            return Err(err("group must be 1..16"));
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
            return Err(err("session limits must be positive"));
        }
        if function_block.is_some_and(|n| !(0..=31).contains(&n)) {
            return Err(err("function_block must be 0..31 or absent"));
        }
        if route.group.is_none() && function_block.is_some() {
            return Err(err("a byte-stream route has no UMP Function Block"));
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
            return Err(err("session time regressed"));
        }
        self.now = now;
        Ok(())
    }
    fn running(&self) -> Result<(), ValueError> {
        if self.active {
            Ok(())
        } else {
            Err(err("session requires explicit restart with a fresh MUID"))
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
            return Err(err("timeout must be positive"));
        }
        if self.pending.len() >= self.exchange_limit {
            return Err(err("active exchange limit reached"));
        }
        let expires = self
            .now
            .checked_add(timeout)
            .ok_or_else(|| err("deadline overflow"))?;
        let token = self.next_token;
        self.next_token = self
            .next_token
            .checked_add(1)
            .ok_or_else(|| err("action token exhausted"))?;
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
            return Err(err("Discovery requires at least three seconds"));
        }
        if self
            .pending
            .iter()
            .any(|p| matches!(p.action.message.body(), Body::Discovery(_)))
        {
            return Err(err("Discovery is already pending"));
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
            return Err(err("endpoint inquiry requires a discovered format-2 peer"));
        }
        if self.pending.iter().any(|p| {
            p.action.message.header().destination_muid() == peer
                && matches!(p.action.message.body(), Body::EndpointInquiry)
        }) {
            return Err(err("endpoint inquiry already pending for peer"));
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
            .ok_or_else(|| err("unknown or expired action token"))?;
        if self.pending[i].sent {
            return Err(err("send result already reported"));
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
                .ok_or_else(|| err("deadline overflow"))?;
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
            return Err(err("restart requires a newly randomized MUID"));
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
            return Err(err("message belongs to another input route/group"));
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
            let size = message.encode().len();
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
            return Err(err(
                "respond requires a local format-2 endpoint reply, ACK or NAK",
            ));
        }
        let peer = self
            .peers
            .iter()
            .find(|p| p.muid == message.header().destination_muid())
            .ok_or_else(|| err("response requires a discovered peer"))?;
        use crate::messages::PrimitiveMessage;
        if message.encode().len() > peer.info.max_sysex() as usize {
            return Err(err("response exceeds peer receive limit"));
        }
        self.enqueue(message, timeout)
    }
}
fn read_muid(b: &[u8]) -> u32 {
    b.iter()
        .enumerate()
        .fold(0, |n, (i, b)| n | u32::from(*b) << (7 * i))
}
fn err(s: &str) -> ValueError {
    ValueError(s.into())
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
