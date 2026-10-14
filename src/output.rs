use crate::callback::check_blocking_thread;
use crate::errors::{native_call, transport_error, StateError};
use crate::lifecycle::{self, Managed, Resource};
use crate::packets::{extract_packet, Backend, Protocol};
use crate::ports::Port;
use crate::state::Client;
use meedee::transport;
use pyo3::prelude::*;
use pyo3::{PyTraverseError, PyVisit};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[pyclass(frozen, module = "meedee._native")]
pub struct Output {
    state: Arc<Client<transport::Output>>,
    identity: String,
    snapshot: AtomicU64,
    protocol: Protocol,
}
#[pymethods]
impl Output {
    #[new]
    #[pyo3(signature = (*, identity, protocol=Protocol::Midi1, backend=None))]
    fn new(
        py: Python<'_>,
        identity: String,
        protocol: Protocol,
        backend: Option<Backend>,
    ) -> PyResult<Self> {
        check_blocking_thread()?;
        let native = py
            .detach(|| {
                native_call("create output", || {
                    transport::Output::new(transport::OutputOptions {
                        protocol: protocol.native(),
                        backend: backend.map(Backend::native),
                    })
                })
            })?
            .map_err(|e| transport_error(e, "init"))?;
        Ok(Self {
            state: Arc::new(Client::new(native)),
            identity,
            snapshot: AtomicU64::new(0),
            protocol,
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
                "port is not a native snapshot from this client",
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
    protocol: Protocol,
}
#[pymethods]
impl OutputConnection {
    fn send(&self, py: Python<'_>, packet: &Bound<'_, PyAny>) -> PyResult<()> {
        check_blocking_thread()?;
        let packet = extract_packet(packet)?;
        transport::validate_packet(self.protocol.native(), &packet)
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
        self.resource.native.closed()
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
        let protocol = client.get().protocol;
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
                protocol,
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
            transport::validate_packet(transport::Protocol::Midi1, &packet)
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
