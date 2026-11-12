//! Caller-driven discovery polling. A failed scan preserves the last complete snapshot.
use crate::{midi, topology, transport};

pub struct Poller {
    encoding: transport::Encoding,
    backend: Option<transport::Backend>,
    state: PollState,
    input: Option<transport::Input>,
    output: Option<transport::Output>,
}
impl Poller {
    pub fn new(encoding: transport::Encoding) -> midi::Result<Self> {
        Ok(Self {
            encoding,
            backend: None,
            state: PollState::new(256, 128, 128)?,
            input: None,
            output: None,
        })
    }
    pub fn with_backend(mut self, backend: transport::Backend) -> midi::Result<Self> {
        self.configure()?;
        self.backend = Some(backend);
        Ok(self)
    }
    pub fn with_limits(
        mut self,
        port_limit: usize,
        detail_limit: usize,
        history_limit: usize,
    ) -> midi::Result<Self> {
        self.configure()?;
        self.state = PollState::new(port_limit, detail_limit, history_limit)?;
        Ok(self)
    }
    pub fn poll(&mut self) -> midi::Result<topology::Report> {
        if self.closed() {
            return Err(crate::drivers::invalid("discovery poller is closed"));
        }
        let encoding = self.encoding;
        let backend = self.backend;
        let input = &mut self.input;
        let output = &mut self.output;
        let result = self.state.scan(|direction| {
            let ports = if direction == "input" {
                if input.is_none() {
                    *input = Some(
                        transport::Input::new(transport::InputOptions {
                            encoding,
                            backend,
                            ..Default::default()
                        })
                        .map_err(midi::Error::Init)?,
                    );
                }
                input.as_ref().expect("initialized input").ports()
            } else {
                if output.is_none() {
                    *output = Some(
                        transport::Output::new(transport::OutputOptions { encoding, backend })
                            .map_err(midi::Error::Init)?,
                    );
                }
                output.as_ref().expect("initialized output").ports()
            }
            .map_err(midi::Error::Transport)?;
            ports
                .into_iter()
                .map(|p| {
                    Ok(topology::Endpoint::new(
                        format!("{:?}", p.backend()),
                        match encoding {
                            transport::Encoding::Bytes => "bytes",
                            transport::Encoding::Ump => "ump",
                        }
                        .into(),
                        direction.into(),
                        p.backend_id(),
                        p.name().into(),
                    )?)
                })
                .collect()
        });
        match result {
            Ok(report) => Ok(report),
            Err(error) => {
                let report = self.state.fail(&error.to_string());
                self.close();
                Ok(report)
            }
        }
    }
    pub fn snapshot(&self) -> topology::Report {
        self.state.snapshot()
    }
    pub fn closed(&self) -> bool {
        self.state.closed()
    }
    pub fn close(&mut self) {
        self.state.close();
        self.input = None;
        self.output = None;
    }
    fn configure(&self) -> midi::Result<()> {
        if self.closed() || self.input.is_some() || self.output.is_some() {
            return Err(crate::drivers::invalid(
                "configure discovery before polling",
            ));
        }
        Ok(())
    }
}

/// Shared complete-scan admission and lifecycle, independent of client ownership.
#[doc(hidden)]
pub struct PollState {
    tracker: topology::Tracker,
    port_limit: usize,
    closed: bool,
}
impl PollState {
    pub fn new(port_limit: usize, detail_limit: usize, history_limit: usize) -> midi::Result<Self> {
        Ok(Self {
            tracker: topology::Tracker::new(port_limit, detail_limit, history_limit)?,
            port_limit,
            closed: false,
        })
    }
    pub fn scan(
        &mut self,
        mut ports: impl FnMut(&str) -> midi::Result<Vec<topology::Endpoint>>,
    ) -> midi::Result<topology::Report> {
        if self.closed {
            return Err(crate::drivers::invalid("discovery poller is closed"));
        }
        let mut endpoints = Vec::new();
        for direction in ["input", "output"] {
            let ports = ports(direction)?;
            if ports.len() > self.port_limit - endpoints.len() {
                return Err(crate::drivers::invalid(
                    "topology port limit exceeded; complete scan rejected",
                ));
            }
            endpoints.extend(ports);
        }
        Ok(self.tracker.observe(endpoints)?)
    }
    pub fn fail(&mut self, reason: &str) -> topology::Report {
        self.tracker.stop(reason);
        self.close();
        self.snapshot()
    }
    pub fn snapshot(&self) -> topology::Report {
        self.tracker.snapshot()
    }
    pub fn closed(&self) -> bool {
        self.closed
    }
    pub fn close(&mut self) {
        self.closed = true;
    }
}
