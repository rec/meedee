use crate::callback::check_blocking_thread;
use crate::errors::{native_call, transport_error, StateError};
use crate::lifecycle::{self, Managed, Resource};
use crate::packets::{extract_packet, Backend, Encoding};
use crate::ports::Port;
use crate::state::Client;
use meedee::transport;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::{PyTraverseError, PyVisit};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Binding to the same confirmed-output tracker used by Rust's Output.
#[pyclass(module = "meedee._native")]
pub(crate) struct OutputState {
    value: meedee::midi::OutputState,
}
#[pymethods]
impl OutputState {
    #[new]
    fn new() -> Self {
        Self {
            value: Default::default(),
        }
    }
    fn feed(&mut self, packet: &[u8]) -> PyResult<()> {
        self.value.feed(packet).map_err(crate::files::os_error)
    }
    fn cleanup(&mut self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        std::mem::take(&mut self.value)
            .cleanup()
            .map(|m| crate::messages::to_python(py, m))
            .collect()
    }
}

#[pyclass(frozen, module = "meedee._native")]
pub struct Output {
    state: Arc<Client<transport::Output>>,
    identity: String,
    diagnostics: transport::WorkerDiagnostics,
    snapshot: AtomicU64,
    encoding: Encoding,
}
#[pymethods]
impl Output {
    #[new]
    #[pyo3(signature = (*, identity, encoding=Encoding::Bytes, backend=None, worker_program=None, worker_args=None, worker_timeout=5.0))]
    fn new(
        py: Python<'_>,
        identity: String,
        encoding: Encoding,
        backend: Option<Backend>,
        worker_program: Option<std::path::PathBuf>,
        worker_args: Option<Vec<std::ffi::OsString>>,
        worker_timeout: f64,
    ) -> PyResult<Self> {
        check_blocking_thread()?;
        lifecycle::initialize(py)?;
        let native = py
            .detach(|| {
                native_call("create output", || {
                    let options = transport::OutputOptions {
                        encoding: encoding.native(),
                        backend: backend.map(Backend::native),
                    };
                    if let Some(program) = worker_program {
                        transport::Output::new_subprocess(
                            options,
                            transport::subprocess::SubprocessOptions {
                                program,
                                args: worker_args.unwrap_or_default(),
                                timeout: std::time::Duration::try_from_secs_f64(worker_timeout)
                                    .map_err(|_| {
                                        transport::Error::Invalid(
                                            "worker timeout must be finite and nonnegative".into(),
                                        )
                                    })?,
                            },
                        )
                    } else {
                        transport::Output::new(options)
                    }
                })
            })?
            .map_err(|e| transport_error(e, "init"))?;
        let diagnostics = native.worker_diagnostics();
        Ok(Self {
            diagnostics,
            state: Arc::new(Client::new(native)),
            identity,
            snapshot: AtomicU64::new(0),
            encoding,
        })
    }
    #[getter]
    fn worker_failure(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.diagnostics.failure().map(|e| {
            transport_error(e, &self.diagnostics.operation())
                .into_value(py)
                .into_any()
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
                    .map(|(index, native)| {
                        Port::new(native, &self.identity, snapshot, index, false)
                    })
                    .collect())
            })
        })
    }
    fn connect(slf: Py<Self>, py: Python<'_>, port: &Port) -> PyResult<Py<OutputConnection>> {
        let port = port.native.clone();
        OutputConnection::open(py, slf, move |native| match port {
            Some(port) => native.connect(&port),
            None => Err(transport::Error::Invalid(
                "port is not a native snapshot from this client".into(),
            )),
        })
    }
    fn create_virtual(
        slf: Py<Self>,
        py: Python<'_>,
        name: String,
    ) -> PyResult<Py<OutputConnection>> {
        OutputConnection::open(py, slf, move |native| native.create_virtual(&name))
    }
}
#[pyclass(frozen, weakref, module = "meedee._native")]
pub struct OutputConnection {
    resource: Arc<Managed<transport::Output, transport::OutputConnection>>,
    client: Py<Output>,
    encoding: Encoding,
}
#[pymethods]
impl OutputConnection {
    #[getter]
    fn worker_failure(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.client.get().diagnostics.failure().map(|e| {
            transport_error(e, &self.client.get().diagnostics.operation())
                .into_value(py)
                .into_any()
        })
    }

    fn send_command(&self, py: Python<'_>, command: &Bound<'_, PyAny>) -> PyResult<()> {
        check_blocking_thread()?;
        let command = crate::messages::extract_command(command)?;
        // Validate before entering Managed::send: preparation errors must not
        // retire a healthy connection as though a backend send had failed.
        if self.encoding == Encoding::Ump {
            let words = meedee::ump::encode(&meedee::events::Payload::Command(command.clone()))
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
            transport::validate_packet(self.encoding.native(), &transport::Packet::Ump(words))
                .map_err(|e| transport_error(e, "send"))?;
        } else {
            use meedee::messages::PrimitiveMessage;
            for message in meedee::codecs::expand(&command)
                .map_err(|e| PyValueError::new_err(e.to_string()))?
            {
                message
                    .encode()
                    .map_err(|e| PyValueError::new_err(e.to_string()))?;
            }
        }
        let mut failure = None;
        let result =
            self.resource
                .native
                .send(&self.resource.client, py, &command, |native, command| {
                    native.send_command(command).map_err(|error| {
                        failure = Some(error);
                        crate::errors::SendError::new_err("compound MIDI send failed")
                    })
                });
        // Construct the Python progress exception after releasing the connection
        // lock. Importing/calling a Python exception class can execute user code.
        match failure {
            Some(error) => Err(transport_error(error, "send")),
            None => result,
        }
    }

    fn send(&self, py: Python<'_>, packet: &Bound<'_, PyAny>) -> PyResult<()> {
        check_blocking_thread()?;
        let packet = extract_packet(packet)?;
        transport::validate_packet(self.encoding.native(), &packet)
            .map_err(|e| transport_error(e, "send"))?;
        self.resource
            .native
            .send(&self.resource.client, py, &packet, |native, packet| {
                native.send(packet).map_err(|e| transport_error(e, "send"))
            })
    }
    fn close(&self, py: Python<'_>) -> PyResult<()> {
        check_blocking_thread()?;
        py.detach(|| self.resource.close())
    }
    #[getter]
    fn closed(&self) -> bool {
        self.resource.native.closed() || self.client.get().diagnostics.failure().is_some()
    }
    fn __enter__(slf: Py<Self>) -> PyResult<Py<Self>> {
        slf.get().resource.native.check_failed()?;
        if slf.get().closed() {
            return Err(StateError::new_err("MIDI connection is closed"));
        }
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
        visit.call(&self.client)
    }
    fn __clear__(&self) {
        lifecycle::defer(self.resource.clone());
    }
}
impl OutputConnection {
    fn open(
        py: Python<'_>,
        client: Py<Output>,
        operation: impl FnOnce(transport::Output) -> transport::Result<transport::OutputConnection>
            + Send,
    ) -> PyResult<Py<Self>> {
        check_blocking_thread()?;
        let encoding = client.get().encoding;
        let resource = Arc::new(Managed::new(
            Arc::clone(&client.get().state),
            None,
            |native: transport::OutputConnection| {
                native.close().map_err(|e| transport_error(e, "close"))
            },
        ));
        let connection = Py::new(
            py,
            Self {
                resource: Arc::clone(&resource),
                client,
                encoding,
            },
        )?;
        lifecycle::register(resource.clone())?;
        py.detach(|| {
            resource.connect(|native| operation(native).map_err(|e| transport_error(e, "open")))
        })?;
        Ok(connection)
    }
}
impl Drop for OutputConnection {
    fn drop(&mut self) {
        lifecycle::defer(self.resource.clone());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::state::Connection;
    use std::sync::Mutex;
    #[pyclass(frozen)]
    pub(crate) struct OutputDriver {
        connection: Connection<()>,
        client: Client<()>,
        pub(crate) messages: Mutex<Vec<Vec<u8>>>,
    }
    #[pymethods]
    impl OutputDriver {
        fn send(&self, py: Python<'_>, packet: &Bound<'_, PyAny>) -> PyResult<()> {
            check_blocking_thread()?;
            let packet = extract_packet(packet)?;
            transport::validate_packet(transport::Encoding::Bytes, &packet)
                .map_err(|e| transport_error(e, "send"))?;
            self.connection
                .send(&self.client, py, &packet, |(), packet| {
                    if let transport::Packet::Bytes(bytes) = packet {
                        self.messages.lock().unwrap().push(bytes.clone());
                    }
                    Ok(())
                })
        }
        fn close(&self, py: Python<'_>) -> PyResult<()> {
            check_blocking_thread()?;
            py.detach(|| self.connection.close(&self.client, |()| Ok(())))
        }
    }
    impl OutputDriver {
        pub(crate) fn new(py: Python<'_>) -> Py<Self> {
            let driver = Py::new(
                py,
                Self {
                    connection: Connection::new(),
                    client: Client::new(()),
                    messages: Mutex::new(Vec::new()),
                },
            )
            .unwrap();
            driver
                .get()
                .client
                .connect(&driver.get().connection, Ok)
                .unwrap();
            driver
        }
    }
}
