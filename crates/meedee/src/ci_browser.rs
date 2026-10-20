//! One bounded management scan over the existing caller-driven CI session.
use crate::ci::{Body, Header, Management, Notice};
use crate::ci_session::{Action, Peer, Session, Update};
use crate::messages::{SysEx, ValueError};
use std::collections::{BTreeSet, VecDeque};

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub kind: String,
    pub peer_muid: Option<u32>,
    pub token: Option<u64>,
    pub reason: String,
}
#[derive(Clone, Debug)]
pub struct Report {
    pub status: &'static str,
    pub local_muid: u32,
    pub peers: Vec<Peer>,
    pub confirmed_actions: u64,
    pub uncertain_token: Option<u64>,
    pub pending_tokens: Vec<u64>,
    pub diagnostics: Vec<Diagnostic>,
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
    report: Report,
    outstanding: BTreeSet<u64>,
    targets: VecDeque<u32>,
}
impl Browser {
    pub fn route(&self) -> &crate::ci_session::Route {
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
            return Err(err("fresh session, discovery >= 3s, positive endpoint/detail limits and overall timeout > discovery required"));
        }
        let local_muid = session.local_muid();
        Ok(Self {
            session,
            discovery_ns,
            endpoint_ns,
            timeout_ns,
            deadline: None,
            detail_limit,
            report: Report {
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
            return Err(err("browser scan has already started"));
        }
        let deadline = now
            .checked_add(self.timeout_ns)
            .ok_or_else(|| err("scan deadline overflow"))?;
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
            return Err(err("unknown or retired browser action"));
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
    pub fn snapshot(&self) -> Report {
        let mut report = self.report.clone();
        report.pending_tokens = self.outstanding.iter().copied().collect();
        report
    }
    fn running(&self) -> Result<(), ValueError> {
        if !matches!(self.report.status, "discovering" | "inquiring" | "stopping") {
            return Err(err("browser is not running"));
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
                        self.report.diagnostics.push(Diagnostic {
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
fn err(message: &str) -> ValueError {
    ValueError(message.into())
}
