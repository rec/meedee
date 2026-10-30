//! Public Python testing transports backed by the shared deterministic core.
use crate::files::{limit, uint};
use crate::packets::{extract_packet, Clock, Protocol, UmpPacket};
use meedee::{testing, transport};
use pyo3::exceptions::{PyInterruptedError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyInt};

#[pyclass(module = "meedee.testing")]
struct ManualClock {
    value: testing::ManualClock,
}
#[pymethods]
impl ManualClock {
    #[new]
    #[pyo3(signature=(*,now_ns=0))]
    fn new(#[pyo3(from_py_with=uint)] now_ns: u64) -> Self {
        Self {
            value: testing::ManualClock::new(now_ns),
        }
    }
    #[getter]
    fn now_ns(&self) -> u64 {
        self.value.now_ns()
    }
    fn advance(&mut self, #[pyo3(from_py_with=uint)] nanoseconds: u64) -> PyResult<()> {
        self.value.advance(nanoseconds).map_err(error)
    }
}
#[pyclass(module = "meedee.testing")]
struct Input {
    value: testing::Input,
}
#[pymethods]
impl Input {
    #[new]
    #[pyo3(signature=(*,stream,protocol,packet_limit=128,event_limit=128,byte_limit=8_388_608,sysex_limit=1_048_576))]
    fn new(
        stream: String,
        protocol: &str,
        #[pyo3(from_py_with=limit)] packet_limit: usize,
        #[pyo3(from_py_with=limit)] event_limit: usize,
        #[pyo3(from_py_with=limit)] byte_limit: usize,
        #[pyo3(from_py_with=limit)] sysex_limit: usize,
    ) -> PyResult<Self> {
        Ok(Self {
            value: testing::Input::new(
                stream,
                parse_protocol(protocol)?.native(),
                packet_limit,
                event_limit,
                byte_limit,
                sysex_limit,
            )
            .map_err(error)?,
        })
    }
    #[pyo3(signature=(packet, *, clock, timestamp_ns=None))]
    fn inject(
        &mut self,
        packet: &Bound<'_, PyAny>,
        clock: PyRef<'_, Clock>,
        timestamp_ns: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<bool> {
        let timestamp = timestamp_ns
            .map(|v| {
                if !v.is_exact_instance_of::<PyInt>() {
                    return Err(PyTypeError::new_err("timestamp must be a built-in integer"));
                }
                v.extract::<i64>()
                    .map_err(|_| PyValueError::new_err("timestamp must fit signed 64 bits"))
            })
            .transpose()?;
        self.value
            .inject(extract_packet(packet)?, clock.value.clone(), timestamp)
            .map_err(error)
    }
    #[pyo3(signature=(*,timeout=None,cancel=None))]
    fn receive(
        &mut self,
        py: Python<'_>,
        timeout: Option<&Bound<'_, PyAny>>,
        cancel: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Option<Py<PyAny>>> {
        if let Some(timeout) = timeout {
            let v = timeout.extract::<f64>()?;
            if timeout.is_instance_of::<PyBool>() || !v.is_finite() || v < 0.0 {
                return Err(PyValueError::new_err(
                    "timeout must be finite and nonnegative",
                ));
            }
        }
        if let Some(cancel) = cancel {
            if cancel.getattr("cancelled")?.extract::<bool>()? {
                return Err(PyInterruptedError::new_err("simulated receive cancelled"));
            }
        }
        self.value
            .receive()
            .map_err(error)?
            .map(|v| crate::live::item(py, v))
            .transpose()
    }
    fn lose(&mut self, #[pyo3(from_py_with=uint)] count: u64) -> PyResult<()> {
        self.value.lose(count).map_err(error)
    }
    fn fail(&mut self, reason: &str) {
        self.value.fail(reason);
    }
    fn close(&mut self) {
        self.value.close();
    }
    #[getter]
    fn stream(&self) -> &str {
        self.value.stream()
    }
    #[getter]
    fn dropped_packets(&self) -> u64 {
        self.value.snapshot().dropped_packets
    }
    #[getter]
    fn closed(&self) -> bool {
        self.value.snapshot().closed
    }
    #[getter]
    fn failure(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.value.snapshot().failure.map(|r| {
            crate::errors::WorkerError::new_err(r)
                .value(py)
                .clone()
                .into_any()
                .unbind()
        })
    }
    fn snapshot(&self) -> InputReport {
        InputReport {
            value: self.value.snapshot(),
        }
    }
    fn __enter__(slf: PyRef<'_, Self>) -> PyResult<PyRef<'_, Self>> {
        if slf.value.snapshot().closed {
            return Err(crate::errors::StateError::new_err(
                "simulated input is closed",
            ));
        }
        Ok(slf)
    }
    fn __exit__(
        &mut self,
        _kind: &Bound<'_, PyAny>,
        _value: &Bound<'_, PyAny>,
        _traceback: &Bound<'_, PyAny>,
    ) {
        self.value.close();
    }
}
#[pyclass(frozen, module = "meedee.testing")]
struct InputReport {
    value: testing::InputReport,
}
#[pymethods]
impl InputReport {
    #[getter]
    fn next_sequence(&self) -> u64 {
        self.value.next_sequence
    }
    #[getter]
    fn dropped_packets(&self) -> u64 {
        self.value.dropped_packets
    }
    #[getter]
    fn dropped_events(&self) -> u64 {
        self.value.dropped_events
    }
    #[getter]
    fn queued_packets(&self) -> usize {
        self.value.queued_packets
    }
    #[getter]
    fn queued_events(&self) -> usize {
        self.value.queued_events
    }
    #[getter]
    fn queued_bytes(&self) -> usize {
        self.value.queued_bytes
    }
    #[getter]
    fn closed(&self) -> bool {
        self.value.closed
    }
    #[getter]
    fn failure(&self) -> Option<&str> {
        self.value.failure.as_deref()
    }
}
#[pyclass(module = "meedee.testing")]
struct Output {
    value: testing::Output,
    protocol: Protocol,
}
#[pymethods]
impl Output {
    #[new]
    #[pyo3(signature=(*,protocol,packet_limit=128,byte_limit=8_388_608))]
    fn new(
        protocol: &str,
        #[pyo3(from_py_with=limit)] packet_limit: usize,
        #[pyo3(from_py_with=limit)] byte_limit: usize,
    ) -> PyResult<Self> {
        Ok(Self {
            value: testing::Output::new(
                parse_protocol(protocol)?.native(),
                packet_limit,
                byte_limit,
            )
            .map_err(error)?,
            protocol: parse_protocol(protocol)?,
        })
    }
    fn send_packet(&mut self, packet: &Bound<'_, PyAny>) -> PyResult<()> {
        self.value.send(extract_packet(packet)?).map_err(error)
    }
    fn send_message(&mut self, py: Python<'_>, message: &Bound<'_, PyAny>) -> PyResult<()> {
        let packet = crate::live::prepare_message(py, self.protocol, message)?;
        self.value
            .send(extract_packet(packet.bind(py))?)
            .map_err(error)
    }
    #[pyo3(signature=(*,accepted=false,reason="simulated send failure"))]
    fn fail_next_send(
        &mut self,
        #[pyo3(from_py_with=crate::ump::boolean)] accepted: bool,
        reason: &str,
    ) -> PyResult<()> {
        self.value.fail_next_send(accepted, reason).map_err(error)
    }
    fn take_packet(&mut self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.value
            .take_packet()
            .map(|v| match v {
                transport::Packet::Bytes(v) => Ok(PyBytes::new(py, &v).into_any().unbind()),
                transport::Packet::Ump(words) => Ok(Py::new(py, UmpPacket { words })?.into_any()),
            })
            .transpose()
    }
    fn close(&mut self) {
        self.value.close();
    }
    #[getter]
    fn closed(&self) -> bool {
        self.value.snapshot().closed
    }
    fn snapshot(&self) -> OutputReport {
        OutputReport {
            value: self.value.snapshot(),
        }
    }
    fn __enter__(slf: PyRef<'_, Self>) -> PyResult<PyRef<'_, Self>> {
        if slf.value.snapshot().closed {
            return Err(crate::errors::StateError::new_err(
                "simulated output is closed",
            ));
        }
        Ok(slf)
    }
    fn __exit__(
        &mut self,
        _kind: &Bound<'_, PyAny>,
        _value: &Bound<'_, PyAny>,
        _traceback: &Bound<'_, PyAny>,
    ) {
        self.value.close();
    }
}
#[pyclass(frozen, module = "meedee.testing")]
struct OutputReport {
    value: testing::OutputReport,
}
#[pymethods]
impl OutputReport {
    #[getter]
    fn attempts(&self) -> u64 {
        self.value.attempts
    }
    #[getter]
    fn accepted(&self) -> u64 {
        self.value.accepted
    }
    #[getter]
    fn queued_packets(&self) -> usize {
        self.value.queued_packets
    }
    #[getter]
    fn queued_bytes(&self) -> usize {
        self.value.queued_bytes
    }
    #[getter]
    fn closed(&self) -> bool {
        self.value.closed
    }
    #[getter]
    fn failure(&self) -> Option<&str> {
        self.value.failure.as_deref()
    }
}
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<ManualClock>()?;
    module.add_class::<Input>()?;
    module.add_class::<InputReport>()?;
    module.add_class::<Output>()?;
    module.add_class::<OutputReport>()
}
fn error(e: transport::Error) -> PyErr {
    match e {
        transport::Error::Closed => {
            crate::errors::StateError::new_err("simulated connection is closed")
        }
        transport::Error::Worker(r) => crate::errors::WorkerError::new_err(r),
        e => crate::errors::transport_error(e, "send"),
    }
}

fn parse_protocol(value: &str) -> PyResult<Protocol> {
    match value {
        "midi1" => Ok(Protocol::Midi1),
        "ump" => Ok(Protocol::Ump),
        _ => Err(PyValueError::new_err("protocol must be midi1 or ump")),
    }
}
