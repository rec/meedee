//! Native pure packet decoder and typed-send preparation.
use crate::event_values::Event;
use crate::files::{limit, os_error, uint};
use crate::packets::{Protocol, ReceivedPacket, UmpPacket};
use meedee::{events::Payload, live as model};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
#[pyclass(frozen, module = "meedee.events")]
pub struct StreamDiagnostic {
    pub value: model::Diagnostic,
}
#[pymethods]
impl StreamDiagnostic {
    #[getter]
    fn stream(&self) -> &str {
        &self.value.stream
    }
    #[getter]
    fn packet(&self) -> u64 {
        self.value.packet
    }
    #[getter]
    fn kind(&self) -> &'static str {
        match self.value.kind {
            model::DiagnosticKind::Loss => "loss",
            model::DiagnosticKind::Malformed => "malformed",
            model::DiagnosticKind::ClockChange => "clock_change",
            model::DiagnosticKind::Truncated => "truncated",
        }
    }
    #[getter]
    fn lost(&self) -> u64 {
        self.value.lost
    }
    #[getter]
    fn reason(&self) -> &str {
        &self.value.reason
    }
    fn __repr__(&self) -> String {
        format!(
            "StreamDiagnostic(stream={:?}, packet={}, kind={:?}, lost={}, reason={:?})",
            self.value.stream,
            self.value.packet,
            self.kind(),
            self.value.lost,
            self.value.reason
        )
    }
}
#[pyclass(module = "meedee.events")]
struct PacketDecoder {
    value: model::Decoder,
}
#[pymethods]
impl PacketDecoder {
    #[new]
    #[pyo3(signature=(*,stream,protocol,sysex_limit=1_048_576))]
    fn new(
        stream: String,
        protocol: Protocol,
        #[pyo3(from_py_with=limit)] sysex_limit: usize,
    ) -> PyResult<Self> {
        Ok(Self {
            value: model::Decoder::new(stream, protocol.native(), sysex_limit)
                .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
        })
    }
    fn feed(
        &mut self,
        py: Python<'_>,
        packet: PyRef<'_, ReceivedPacket>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        items(
            py,
            self.value
                .feed(
                    &packet.packet,
                    &packet.clock.value,
                    packet.timestamp_ns,
                    packet.sequence,
                )
                .map_err(os_error)?,
        )
    }
    #[pyo3(signature=(*,lost,reason))]
    fn loss(
        &mut self,
        py: Python<'_>,
        lost: &Bound<'_, PyAny>,
        reason: String,
    ) -> PyResult<Py<PyAny>> {
        item(py, self.value.loss(uint(lost)?, reason))
    }
    #[getter]
    fn pending(&self) -> bool {
        self.value.pending()
    }
    fn reset(&mut self) {
        self.value.reset();
    }
    fn finish(&mut self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        items(py, self.value.finish())
    }
}
fn items(py: Python<'_>, v: Vec<model::Item>) -> PyResult<Vec<Py<PyAny>>> {
    v.into_iter().map(|v| item(py, v)).collect()
}
pub(crate) fn item(py: Python<'_>, v: model::Item) -> PyResult<Py<PyAny>> {
    match v {
        model::Item::Event(value) => Ok(Py::new(py, Event { value: *value })?.into_any()),
        model::Item::Diagnostic(value) => Ok(Py::new(py, StreamDiagnostic { value })?.into_any()),
    }
}
#[pyfunction]
pub fn prepare_message(
    py: Python<'_>,
    protocol: Protocol,
    message: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    if let Ok(p) = crate::event_values::payload(message) {
        match (protocol, p) {
            (Protocol::Midi1, Payload::Midi1(v)) => {
                use meedee::messages::PrimitiveMessage;
                return Ok(PyBytes::new(py, &v.encode()).into_any().unbind());
            }
            (Protocol::Ump, Payload::Ump(meedee::ump::UmpMessage::Opaque(_))) => {
                return Err(crate::errors::CapabilityError::new_err(
                    "opaque UMP requires deliberate raw sending; typed support is unknown",
                ))
            }
            (
                Protocol::Ump,
                Payload::Ump(
                    meedee::ump::UmpMessage::DeltaClockstamp(_)
                    | meedee::ump::UmpMessage::DeltaClockstampTicksPerQuarter(_),
                ),
            ) => {
                return Err(crate::errors::CapabilityError::new_err(
                    "file timing records are not live messages",
                ))
            }
            (Protocol::Ump, Payload::Ump(meedee::ump::UmpMessage::Stream(v)))
                if matches!(v.status(), 32 | 33) =>
            {
                return Err(crate::errors::CapabilityError::new_err(
                    "clip markers are not live messages",
                ))
            }
            (Protocol::Ump, Payload::Ump(v)) => {
                use meedee::ump::UmpValue;
                return Ok(Py::new(
                    py,
                    UmpPacket {
                        words: v.encode_words(),
                    },
                )?
                .into_any());
            }
            (_, Payload::Command(_)) => {
                return Err(crate::errors::CapabilityError::new_err(
                    "use send_command for compound commands",
                ))
            }
            _ => {
                return Err(crate::errors::CapabilityError::new_err(
                    "message and transport protocols differ; translate explicitly",
                ))
            }
        }
    }
    if protocol == Protocol::Midi1 {
        return Ok(PyBytes::new(py, &crate::messages::encode_message(message)?)
            .into_any()
            .unbind());
    }
    Err(crate::errors::CapabilityError::new_err(
        "native UMP message required; no automatic translation",
    ))
}
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PacketDecoder>()?;
    m.add_class::<StreamDiagnostic>()
}

#[pyfunction]
pub fn live_size(item: &Bound<'_, PyAny>) -> PyResult<usize> {
    if let Ok(v) = item.extract::<PyRef<'_, Event>>() {
        return Ok(match &v.value.message {
            Payload::Midi1(meedee::messages::AnyMessage::SysEx(v)) => v.data().len() + 2,
            Payload::Midi1(v) => {
                use meedee::messages::PrimitiveMessage;
                v.encode().len()
            }
            Payload::Ump(v) => {
                use meedee::ump::UmpValue;
                v.encode_words().len() * 4
            }
            Payload::Command(_) => 64,
        });
    }
    if let Ok(v) = item.extract::<PyRef<'_, StreamDiagnostic>>() {
        return Ok(v.value.reason.len());
    }
    Err(pyo3::exceptions::PyTypeError::new_err(
        "expected Event or StreamDiagnostic",
    ))
}
