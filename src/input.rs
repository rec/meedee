use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use meedee::transport;
use pyo3::prelude::*;
use pyo3::{PyTraverseError, PyVisit};

use crate::callback::{check_blocking_thread, Callback};
use crate::errors::{native_call, transport_error, StateError};
use crate::lifecycle::{self, Managed, Resource};
use crate::packets::{Backend, Clock, Protocol, ReceivedPacket, TimestampMode};
use crate::ports::Port;
use crate::state::Client;

#[pyclass(frozen, module = "meedee._native")]
pub struct Input {
    state: Arc<Client<transport::Input>>,
    identity: String,
    snapshot: AtomicU64,
    mode: TimestampMode,
}

#[pymethods]
impl Input {
    #[new]
    #[pyo3(signature = (*, identity, protocol=Protocol::Midi1, backend=None, timestamps=TimestampMode::Absolute, receive_byte_limit=8_388_608))]
    fn new(
        py: Python<'_>,
        identity: String,
        protocol: Protocol,
        backend: Option<Backend>,
        timestamps: TimestampMode,
        receive_byte_limit: usize,
    ) -> PyResult<Self> {
        check_blocking_thread()?;
        let native = py
            .detach(|| {
                native_call("create input", || {
                    transport::Input::new(transport::InputOptions {
                        protocol: protocol.native(),
                        backend: backend.map(Backend::native),
                        timestamps: timestamps.native(),
                        receive_byte_limit,
                    })
                })
            })?
            .map_err(|e| transport_error(e, "init"))?;
        Ok(Self {
            state: Arc::new(Client::new(native)),
            identity,
            snapshot: AtomicU64::new(0),
            mode: timestamps,
        })
    }
    fn ports(&self, py: Python<'_>) -> PyResult<Vec<Port>> {
        py.detach(|| {
            self.state.with_available(|native| {
                let ports = native.ports().map_err(|e| transport_error(e, "ports"))?;
                let snapshot = self.snapshot.fetch_add(1, Ordering::Relaxed) + 1;
                Ok(ports
                    .into_iter()
                    .enumerate()
                    .map(|(index, native)| Port::new(native, &self.identity, snapshot, index, true))
                    .collect())
            })
        })
    }
    fn connect(
        slf: Py<Self>,
        py: Python<'_>,
        port: &Port,
        callback: Py<PyAny>,
        clock_identity: String,
    ) -> PyResult<Py<InputConnection>> {
        let native_port = port.native.clone();
        InputConnection::open(
            py,
            slf,
            callback,
            clock_identity,
            move |native, callback| match native_port {
                Some(port) => native.connect(&port, callback),
                None => Err(transport::Error::Invalid(
                    "port is not a native snapshot from this client",
                )),
            },
        )
    }
    fn create_virtual(
        slf: Py<Self>,
        py: Python<'_>,
        name: String,
        callback: Py<PyAny>,
        clock_identity: String,
    ) -> PyResult<Py<InputConnection>> {
        InputConnection::open(
            py,
            slf,
            callback,
            clock_identity,
            move |native, callback| native.create_virtual(&name, callback),
        )
    }
}

#[pyclass(frozen, weakref, module = "meedee._native")]
pub struct InputConnection {
    resource: Arc<Managed<transport::Input, transport::InputConnection>>,
    client: Py<Input>,
    clock: OnceLock<Clock>,
    diagnostics: OnceLock<transport::InputDiagnostics>,
}

#[pymethods]
impl InputConnection {
    fn close(&self, py: Python<'_>) -> PyResult<()> {
        check_blocking_thread()?;
        self.resource.retire();
        py.detach(|| self.resource.close())
    }
    #[getter]
    fn closed(&self) -> bool {
        self.resource.native.closed()
    }
    #[getter]
    fn clock(&self) -> PyResult<Clock> {
        self.clock
            .get()
            .cloned()
            .ok_or_else(|| StateError::new_err("input is not open"))
    }
    #[getter]
    fn dropped_packets(&self) -> u64 {
        self.diagnostics
            .get()
            .map_or(0, transport::InputDiagnostics::dropped_packets)
    }
    #[getter]
    fn last_message_time(&self) -> Option<f64> {
        self.resource
            .callback
            .as_ref()
            .and_then(|c| c.last_message_time())
    }
    fn __enter__(slf: Py<Self>, py: Python<'_>) -> PyResult<Py<Self>> {
        slf.get().resource.native.check_failed()?;
        if slf.get().closed() {
            return Err(StateError::new_err("MIDI connection is closed"));
        }
        let _ = py;
        Ok(slf)
    }
    fn __exit__(
        &self,
        py: Python<'_>,
        _exc_type: &Bound<'_, PyAny>,
        _exc_value: &Bound<'_, PyAny>,
        _traceback: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        self.close(py)
    }
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.client)?;
        self.resource.callback.as_ref().unwrap().traverse(visit)
    }
    fn __clear__(&self) {
        lifecycle::defer(self.resource.clone());
    }
}

impl InputConnection {
    fn open(
        py: Python<'_>,
        client: Py<Input>,
        callable: Py<PyAny>,
        identity: String,
        operation: impl FnOnce(
                transport::Input,
                Box<dyn FnMut(transport::ReceivedPacket) + Send>,
            ) -> transport::Result<transport::InputConnection>
            + Send,
    ) -> PyResult<Py<Self>> {
        check_blocking_thread()?;
        let callback = Arc::new(Callback::new(py, callable)?);
        let resource = Arc::new(Managed::new(
            Arc::clone(&client.get().state),
            Some(Arc::clone(&callback)),
            |native: transport::InputConnection| {
                native.close().map_err(|e| transport_error(e, "close"))
            },
        ));
        let mode = client.get().mode;
        let connection = Py::new(
            py,
            Self {
                resource: Arc::clone(&resource),
                client,
                clock: OnceLock::new(),
                diagnostics: OnceLock::new(),
            },
        )?;
        lifecycle::register(resource.clone())?;
        let callback_identity = identity.clone();
        let result = py.detach(|| {
            resource.connect(|native| {
                let input = operation(
                    native,
                    Box::new(move |packet| {
                        let clock = Clock {
                            identity: callback_identity.clone(),
                            backend: Backend::from_native(packet.clock.backend),
                            mode,
                        };
                        callback.deliver(move |py| {
                            Ok(Py::new(
                                py,
                                ReceivedPacket {
                                    timestamp_ns: packet.timestamp_ns,
                                    clock,
                                    packet: packet.packet,
                                },
                            )?
                            .into_any())
                        });
                    }),
                )
                .map_err(|e| transport_error(e, "open"))?;
                connection
                    .get()
                    .clock
                    .set(Clock {
                        identity,
                        backend: Backend::from_native(input.clock().backend),
                        mode,
                    })
                    .ok();
                connection.get().diagnostics.set(input.diagnostics()).ok();
                Ok(input)
            })
        });
        if result.is_err() {
            resource.retire();
        }
        result?;
        Ok(connection)
    }
}
impl Drop for InputConnection {
    fn drop(&mut self) {
        lifecycle::defer(self.resource.clone());
    }
}
