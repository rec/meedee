//! Native pure packet decoder and typed-send preparation.
use crate::files::{os_error, uint};
use crate::packets::{Encoding, ReceivedPacket, UmpPacket};
use middi::{events as model, events::Payload};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyTuple};
#[pyclass(frozen, module = "middi.events")]
pub struct StreamDiagnostic {
    pub value: model::StreamDiagnostic,
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
            model::StreamDiagnosticKind::Loss => "loss",
            model::StreamDiagnosticKind::Malformed => "malformed",
            model::StreamDiagnosticKind::ClockChange => "clock_change",
            model::StreamDiagnosticKind::Truncated => "truncated",
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
#[pyclass(module = "middi.events")]
struct PacketDecoder {
    value: model::PacketDecoder,
}
#[pymethods]
impl PacketDecoder {
    #[new]
    #[pyo3(signature=(*,stream,encoding,sysex_limit=1_048_576))]
    fn new(
        stream: String,
        encoding: Encoding,
        #[pyo3(from_py_with=crate::units::bytes)] sysex_limit: usize,
    ) -> PyResult<Self> {
        Ok(Self {
            value: model::PacketDecoder::new(stream, encoding.native(), sysex_limit)
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
fn items(py: Python<'_>, v: Vec<model::InputItem>) -> PyResult<Vec<Py<PyAny>>> {
    v.into_iter().map(|v| item(py, v)).collect()
}
pub(crate) fn item(py: Python<'_>, v: model::InputItem) -> PyResult<Py<PyAny>> {
    match v {
        model::InputItem::Message(value) => crate::event_values::to_python(py, *value),
        model::InputItem::Diagnostic(value) => {
            Ok(Py::new(py, StreamDiagnostic { value })?.into_any())
        }
    }
}
#[pyfunction]
pub fn resolve_output_channel(
    py: Python<'_>,
    message: &Bound<'_, PyAny>,
    channel: Option<u8>,
) -> PyResult<Py<PyAny>> {
    if !message.is_instance_of::<crate::messages::Message>() {
        return Ok(message.clone().unbind());
    }
    match crate::event_values::extract(message) {
        Ok(value) => crate::event_values::to_python(
            py,
            value
                .resolve_channel(channel)
                .map_err(|e| PyValueError::new_err(e.to_string()))?,
        ),
        Err(e) if e.is_instance_of::<pyo3::exceptions::PyTypeError>(py) => {
            Ok(message.clone().unbind())
        }
        Err(e) => Err(e),
    }
}
#[pyfunction]
pub fn prepare_output(
    py: Python<'_>,
    encoding: Encoding,
    message: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    if !message.is_instance_of::<crate::messages::Message>() {
        return crate::packets::prepare_send(py, encoding, message);
    }
    let prepared = prepare_message(py, encoding, message)?;
    if prepared.bind(py).is_instance_of::<PyTuple>() {
        Ok(prepared)
    } else {
        crate::packets::prepare_send(py, encoding, prepared.bind(py))
    }
}

fn prepare_message(
    py: Python<'_>,
    encoding: Encoding,
    message: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    if let Ok(p) = crate::event_values::payload(message) {
        match (encoding, p) {
            (Encoding::Bytes, Payload::Message(v)) if v.version() == 2 || v.group().is_some() => {
                return Err(crate::errors::CapabilityError::new_err(
                    "message and transport encodings differ; translate or remove group explicitly",
                ));
            }
            (Encoding::Bytes, Payload::Message(v)) => {
                use middi::messages::PrimitiveMessage;
                return Ok(PyBytes::new(
                    py,
                    &v.encode()
                        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
                )
                .into_any()
                .unbind());
            }
            (Encoding::Ump, Payload::Ump(middi::ump::PacketValue::Opaque(_))) => {
                return Err(crate::errors::CapabilityError::new_err(
                    "opaque UMP requires deliberate raw sending; typed support is unknown",
                ))
            }
            (
                Encoding::Ump,
                Payload::Ump(
                    middi::ump::PacketValue::DeltaClockstamp(_)
                    | middi::ump::PacketValue::DeltaClockstampTicksPerQuarter(_),
                ),
            ) => {
                return Err(crate::errors::CapabilityError::new_err(
                    "file timing records are not live messages",
                ))
            }
            (Encoding::Ump, Payload::Ump(middi::ump::PacketValue::Stream(v)))
                if matches!(v.status(), 32 | 33) =>
            {
                return Err(crate::errors::CapabilityError::new_err(
                    "clip markers are not live messages",
                ))
            }
            (Encoding::Ump, v) => {
                use middi::ump::UmpValue;
                return Ok(Py::new(
                    py,
                    UmpPacket {
                        words: v
                            .encode_words()
                            .map_err(|e| PyValueError::new_err(e.to_string()))?,
                    },
                )?
                .into_any());
            }
            (Encoding::Bytes, Payload::Command(_)) => {
                return Ok(crate::codecs::prepare_command(encoding, message)?
                    .into_pyobject(py)?
                    .into_any()
                    .unbind());
            }
            _ => {
                return Err(crate::errors::CapabilityError::new_err(
                    "message and transport protocols differ; translate explicitly",
                ))
            }
        }
    }
    if encoding == Encoding::Bytes {
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
    if let Ok(v) = crate::event_values::extract(&item) {
        return Ok(match &v {
            Payload::Message(v) => v.encoded_len(),
            Payload::Ump(v) => {
                use middi::ump::UmpValue;
                v.encode_words()
                    .map_err(|e| PyValueError::new_err(e.to_string()))?
                    .len()
                    * 4
            }
            Payload::Command(v) => {
                use middi::messages::Command;
                v.expand()
                    .map_err(|e| PyValueError::new_err(e.to_string()))?
                    .iter()
                    .map(|m| m.encoded_len())
                    .sum()
            }
        });
    }
    if let Ok(v) = item.extract::<PyRef<'_, StreamDiagnostic>>() {
        return Ok(v.value.reason.len());
    }
    Err(pyo3::exceptions::PyTypeError::new_err(
        "expected Message or StreamDiagnostic",
    ))
}
