//! Pure Python codec entry points. Never initializes native MIDI transport.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use meedee::codecs;
use pyo3::buffer::PyBuffer;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::messages;

#[pyclass(eq, frozen, from_py_object, module = "meedee.codecs")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum DiagnosticKind {
    UnexpectedData,
    UndefinedStatus,
    UnexpectedEnd,
    InterruptedMessage,
    InterruptedSysEx,
    SysExTooLarge,
    TruncatedMessage,
    TruncatedSysEx,
}

#[pyclass(frozen, module = "meedee.codecs")]
struct Diagnostic {
    value: codecs::Diagnostic,
}

#[pymethods]
impl Diagnostic {
    #[getter]
    fn kind(&self) -> DiagnosticKind {
        match self.value.kind {
            codecs::DiagnosticKind::UnexpectedData => DiagnosticKind::UnexpectedData,
            codecs::DiagnosticKind::UndefinedStatus => DiagnosticKind::UndefinedStatus,
            codecs::DiagnosticKind::UnexpectedEnd => DiagnosticKind::UnexpectedEnd,
            codecs::DiagnosticKind::InterruptedMessage => DiagnosticKind::InterruptedMessage,
            codecs::DiagnosticKind::InterruptedSysEx => DiagnosticKind::InterruptedSysEx,
            codecs::DiagnosticKind::SysExTooLarge => DiagnosticKind::SysExTooLarge,
            codecs::DiagnosticKind::TruncatedMessage => DiagnosticKind::TruncatedMessage,
            codecs::DiagnosticKind::TruncatedSysEx => DiagnosticKind::TruncatedSysEx,
        }
    }
    #[getter]
    fn offset(&self) -> usize {
        self.value.offset
    }
    #[getter]
    fn byte(&self) -> Option<u8> {
        self.value.byte
    }
    fn __repr__(&self) -> String {
        format!(
            "Diagnostic(kind=DiagnosticKind.{:?}, offset={}, byte={})",
            self.kind(),
            self.offset(),
            self.byte()
                .map_or_else(|| "None".into(), |byte| byte.to_string()),
        )
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|other| self.value == other.value)
    }
    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.value.hash(&mut hasher);
        hasher.finish()
    }
}

#[pyclass(module = "meedee.codecs")]
struct Decoder {
    value: codecs::Decoder,
}

#[pymethods]
impl Decoder {
    #[new]
    #[pyo3(signature = (*, sysex_limit=1_048_576))]
    fn new(#[pyo3(from_py_with = strict_limit)] sysex_limit: usize) -> PyResult<Self> {
        Ok(Self {
            value: codecs::Decoder::new(sysex_limit)
                .map_err(|error| PyValueError::new_err(error.to_string()))?,
        })
    }
    #[getter]
    fn sysex_limit(&self) -> usize {
        self.value.sysex_limit()
    }
    fn feed(&mut self, py: Python<'_>, data: &Bound<'_, PyAny>) -> PyResult<Vec<Py<PyAny>>> {
        let buffer = PyBuffer::<u8>::get(data)?;
        if buffer.dimensions() != 1 {
            return Err(PyTypeError::new_err(
                "data must be a one-dimensional unsigned-byte buffer",
            ));
        }
        let data = buffer.to_vec(py)?;
        items_to_python(py, self.value.feed(&data))
    }
    fn finish(&mut self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        items_to_python(py, self.value.finish())
    }
    fn reset(&mut self) {
        self.value.reset();
    }
}

fn strict_limit(value: &Bound<'_, PyAny>) -> PyResult<usize> {
    usize::try_from(messages::integer(value, "sysex_limit")?)
        .map_err(|_| PyValueError::new_err("sysex_limit must be positive"))
}

#[pyfunction]
fn encode<'py>(py: Python<'py>, message: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyBytes>> {
    Ok(PyBytes::new(py, &messages::encode_message(message)?))
}

#[pyfunction]
fn expand(py: Python<'_>, command: &Bound<'_, PyAny>) -> PyResult<Vec<Py<PyAny>>> {
    codecs::expand(&messages::extract_command(command)?)
        .into_iter()
        .map(|message| messages::to_python(py, message))
        .collect()
}

#[pyfunction]
pub fn prepare_command(
    protocol: crate::packets::Protocol,
    command: &Bound<'_, PyAny>,
) -> PyResult<(String, std::collections::HashMap<String, i64>, usize)> {
    let command = messages::extract_command(command)?;
    if protocol != crate::packets::Protocol::Midi1 {
        return Err(crate::errors::CapabilityError::new_err(
            "compound commands require MIDI 1.0 byte transport",
        ));
    }
    let total = codecs::expand(&command).len();
    let (name, fields) = messages::command_wire(&command);
    Ok((name, fields, total))
}

fn items_to_python(py: Python<'_>, items: Vec<codecs::DecodeItem>) -> PyResult<Vec<Py<PyAny>>> {
    items
        .into_iter()
        .map(|item| match item {
            codecs::DecodeItem::Message(message) => messages::to_python(py, message),
            codecs::DecodeItem::Diagnostic(value) => {
                Ok(Py::new(py, Diagnostic { value })?.into_any())
            }
        })
        .collect()
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<DiagnosticKind>()?;
    module.add_class::<Diagnostic>()?;
    module.add_class::<Decoder>()?;
    module.add_function(wrap_pyfunction!(encode, module)?)?;
    module.add_function(wrap_pyfunction!(expand, module)?)?;
    Ok(())
}
