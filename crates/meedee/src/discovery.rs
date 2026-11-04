//! Caller-driven discovery polling. A failed scan preserves the last complete snapshot.
use crate::{midi, topology, transport};

pub struct Poller {
    protocol: transport::Protocol,
    backend: Option<transport::Backend>,
    tracker: topology::Tracker,
    input: Option<transport::Input>,
    output: Option<transport::Output>,
    port_limit: usize,
    closed: bool,
}
impl Poller {
    pub fn new(protocol: transport::Protocol) -> midi::Result<Self> {
        Ok(Self {
            protocol,
            backend: None,
            tracker: topology::Tracker::new(256, 128, 128)?,
            input: None,
            output: None,
            port_limit: 256,
            closed: false,
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
        self.tracker = topology::Tracker::new(port_limit, detail_limit, history_limit)?;
        self.port_limit = port_limit;
        Ok(self)
    }
    pub fn poll(&mut self) -> midi::Result<topology::Report> {
        if self.closed {
            return Err(crate::drivers::invalid("discovery poller is closed"));
        }
        match self.scan() {
            Ok(ports) => match self.tracker.observe(ports) {
                Ok(report) => Ok(report),
                Err(error) => {
                    self.tracker.stop(&error.to_string());
                    self.close();
                    Ok(self.snapshot())
                }
            },
            Err(error) => {
                self.tracker.stop(&error.to_string());
                self.close();
                Ok(self.snapshot())
            }
        }
    }
    pub fn snapshot(&self) -> topology::Report {
        self.tracker.snapshot()
    }
    pub fn closed(&self) -> bool {
        self.closed
    }
    pub fn close(&mut self) {
        self.closed = true;
        self.input = None;
        self.output = None;
    }
    fn configure(&self) -> midi::Result<()> {
        if self.closed || self.input.is_some() || self.output.is_some() {
            return Err(crate::drivers::invalid(
                "configure discovery before polling",
            ));
        }
        Ok(())
    }
    fn scan(&mut self) -> midi::Result<Vec<topology::Endpoint>> {
        if self.input.is_none() {
            self.input = Some(
                transport::Input::new(transport::InputOptions {
                    protocol: self.protocol,
                    backend: self.backend,
                    ..Default::default()
                })
                .map_err(midi::Error::Init)?,
            );
        }
        if self.output.is_none() {
            self.output = Some(
                transport::Output::new(transport::OutputOptions {
                    protocol: self.protocol,
                    backend: self.backend,
                })
                .map_err(midi::Error::Init)?,
            );
        }
        let mut endpoints = Vec::new();
        for (direction, ports) in [
            (
                "input",
                self.input
                    .as_ref()
                    .expect("initialized discovery input")
                    .ports(),
            ),
            (
                "output",
                self.output
                    .as_ref()
                    .expect("initialized discovery output")
                    .ports(),
            ),
        ] {
            let ports = ports.map_err(midi::Error::Transport)?;
            if ports.len() > self.port_limit - endpoints.len() {
                return Err(crate::drivers::invalid(
                    "topology port limit exceeded; complete scan rejected",
                ));
            }
            for port in ports {
                endpoints.push(topology::Endpoint::new(
                    format!("{:?}", port.backend()),
                    match self.protocol {
                        transport::Protocol::Midi1 => "midi1",
                        transport::Protocol::Ump => "ump",
                    }
                    .into(),
                    direction.into(),
                    port.backend_id(),
                    port.name().into(),
                )?);
            }
        }
        Ok(endpoints)
    }
}
