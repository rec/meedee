//! Complete discovery observations, not persistent physical device identities.
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Endpoint {
    backend: String,
    protocol: String,
    direction: String,
    backend_id: u64,
    name: String,
}
impl Endpoint {
    pub fn new(
        backend: String,
        protocol: String,
        direction: String,
        backend_id: u64,
        name: String,
    ) -> io::Result<Self> {
        if backend.is_empty()
            || !matches!(protocol.as_str(), "midi1" | "ump")
            || !matches!(direction.as_str(), "input" | "output")
        {
            return Err(invalid(
                "nonempty backend, midi1/ump protocol and input/output direction required",
            ));
        }
        Ok(Self {
            backend,
            protocol,
            direction,
            backend_id,
            name,
        })
    }
    pub fn backend(&self) -> &str {
        &self.backend
    }
    pub fn protocol(&self) -> &str {
        &self.protocol
    }
    pub fn direction(&self) -> &str {
        &self.direction
    }
    pub fn backend_id(&self) -> u64 {
        self.backend_id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Debug)]
pub struct PortCount {
    pub endpoint: Endpoint,
    pub count: usize,
}
#[derive(Clone, Debug)]
pub struct Change {
    pub kind: &'static str,
    pub endpoint: Endpoint,
    pub count: usize,
}
#[derive(Clone, Debug)]
pub struct Delta {
    pub generation: u64,
    pub added: usize,
    pub removed: usize,
    pub changes: Vec<Change>,
    pub omitted_changes: usize,
}
#[derive(Clone, Debug)]
pub struct Report {
    pub generation: u64,
    pub port_count: usize,
    pub ports: Vec<PortCount>,
    pub last: Option<Delta>,
    pub history: Vec<Delta>,
    pub omitted_history: u64,
    pub stopped: bool,
    pub reason: Option<String>,
}

pub struct Tracker {
    ports: BTreeMap<Endpoint, usize>,
    port_limit: usize,
    detail_limit: usize,
    history_limit: usize,
    generation: u64,
    last: Option<Delta>,
    history: VecDeque<Delta>,
    omitted_history: u64,
    reason: Option<String>,
}
impl Tracker {
    pub fn new(port_limit: usize, detail_limit: usize, history_limit: usize) -> io::Result<Self> {
        if port_limit == 0 || detail_limit == 0 || history_limit == 0 {
            return Err(invalid("positive topology limits required"));
        }
        Ok(Self {
            ports: BTreeMap::new(),
            port_limit,
            detail_limit,
            history_limit,
            generation: 0,
            last: None,
            history: VecDeque::new(),
            omitted_history: 0,
            reason: None,
        })
    }
    /// A rejected scan never replaces the last complete observation.
    pub fn observe(&mut self, ports: Vec<Endpoint>) -> io::Result<Report> {
        if self.reason.is_some() {
            return Err(invalid("topology tracker is stopped"));
        }
        if ports.len() > self.port_limit {
            self.stop("topology port limit exceeded; complete scan rejected");
            return Err(invalid(self.reason.as_deref().expect("stopped tracker")));
        }
        let Some(generation) = self.generation.checked_add(1) else {
            self.stop("topology generation exhausted");
            return Err(invalid("topology generation exhausted"));
        };
        let mut next = BTreeMap::new();
        for p in ports {
            *next.entry(p).or_insert(0usize) += 1;
        }
        let mut delta = Delta {
            generation,
            added: 0,
            removed: 0,
            changes: Vec::new(),
            omitted_changes: 0,
        };
        // The first complete scan establishes a baseline, not device arrivals.
        if self.generation != 0 {
            let keys: BTreeSet<_> = self.ports.keys().chain(next.keys()).collect();
            for key in keys {
                let old = self.ports.get(key).copied().unwrap_or(0);
                let new = next.get(key).copied().unwrap_or(0);
                let (kind, count) = if new > old {
                    delta.added += new - old;
                    ("added", new - old)
                } else if old > new {
                    delta.removed += old - new;
                    ("removed", old - new)
                } else {
                    continue;
                };
                if delta.changes.len() < self.detail_limit {
                    delta.changes.push(Change {
                        kind,
                        endpoint: key.clone(),
                        count,
                    });
                } else {
                    delta.omitted_changes += 1;
                }
            }
        }
        if delta.added != 0 || delta.removed != 0 {
            if self.history.len() == self.history_limit {
                self.history.pop_front();
                self.omitted_history += 1;
            }
            self.history.push_back(delta.clone());
        }
        self.generation = generation;
        self.ports = next;
        self.last = Some(delta);
        Ok(self.snapshot())
    }
    pub fn stop(&mut self, reason: &str) {
        if self.reason.is_none() {
            self.reason = Some(reason.chars().take(1024).collect());
        }
    }
    pub fn snapshot(&self) -> Report {
        Report {
            generation: self.generation,
            port_count: self.ports.values().sum(),
            ports: self
                .ports
                .iter()
                .map(|(p, c)| PortCount {
                    endpoint: p.clone(),
                    count: *c,
                })
                .collect(),
            last: self.last.clone(),
            history: self.history.iter().cloned().collect(),
            omitted_history: self.omitted_history,
            stopped: self.reason.is_some(),
            reason: self.reason.clone(),
        }
    }
}
fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason)
}
