//! Explicit protocol conversion, independent of packet framing.
use crate::messages;
use crate::ump::{boolean, error};
use middy::translation as model;
use pyo3::prelude::*;

#[pyclass(eq, frozen, from_py_object, module = "middy.translation")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum TranslationStatus {
    Exact,
    Quantized,
    Unsupported,
    ProfileDependent,
    ContextRequired,
}
impl From<model::TranslationStatus> for TranslationStatus {
    fn from(v: model::TranslationStatus) -> Self {
        match v {
            model::TranslationStatus::Exact => Self::Exact,
            model::TranslationStatus::Quantized => Self::Quantized,
            model::TranslationStatus::Unsupported => Self::Unsupported,
            model::TranslationStatus::ProfileDependent => Self::ProfileDependent,
            model::TranslationStatus::ContextRequired => Self::ContextRequired,
        }
    }
}
#[pyclass(frozen, module = "middy.translation")]
struct Translation {
    value: model::Translation<middy::messages::AnyMessage>,
}
#[pymethods]
impl Translation {
    #[getter]
    fn status(&self) -> TranslationStatus {
        self.value.status.into()
    }
    #[getter]
    fn reason(&self) -> &str {
        self.value.reason
    }
    #[getter]
    fn group(&self) -> Option<u8> {
        self.value.group
    }
    #[getter]
    fn converted(&self) -> bool {
        self.value.converted()
    }
    #[getter]
    fn messages(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.value
            .messages
            .iter()
            .cloned()
            .map(|m| crate::event_values::to_python(py, m.into()))
            .collect()
    }
}
#[pyfunction]
#[pyo3(signature=(message,*,allow_loss=false))]
fn to_midi1(
    message: &Bound<'_, PyAny>,
    #[pyo3(from_py_with=boolean)] allow_loss: bool,
) -> PyResult<Translation> {
    Ok(Translation {
        value: model::to_midi1(&crate::event_values::extract(message)?, allow_loss)
            .map_err(error)?,
    })
}
#[pyfunction]
#[pyo3(signature=(message,*,group))]
fn to_midi2(message: &Bound<'_, PyAny>, group: &Bound<'_, PyAny>) -> PyResult<Translation> {
    let value = match crate::event_values::extract(message)? {
        middy::events::Payload::Message(v) => v,
        middy::events::Payload::Command(middy::messages::AnyCommand::ControlChange(v)) => {
            middy::messages::AnyMessage::ControlChange(v)
        }
        _ => {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "to_midi2 requires a MIDI 1 semantic message",
            ))
        }
    };
    Ok(Translation {
        value: model::to_midi2(&value, messages::integer(group, "group")?).map_err(error)?,
    })
}
#[pyfunction]
#[pyo3(signature=(*,value,source_bits,destination_bits))]
fn scale_up(
    value: &Bound<'_, PyAny>,
    source_bits: &Bound<'_, PyAny>,
    destination_bits: &Bound<'_, PyAny>,
) -> PyResult<u32> {
    use pyo3::exceptions::PyValueError;
    let value = u32::try_from(messages::integer(value, "value")?)
        .map_err(|_| PyValueError::new_err("value must be unsigned 32-bit"))?;
    let source = u8::try_from(messages::integer(source_bits, "source_bits")?)
        .map_err(|_| PyValueError::new_err("invalid source_bits"))?;
    let destination = u8::try_from(messages::integer(destination_bits, "destination_bits")?)
        .map_err(|_| PyValueError::new_err("invalid destination_bits"))?;
    model::scale_up(value, source, destination).map_err(error)
}
pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<TranslationStatus>()?;
    module.add_class::<Translation>()?;
    module.add_function(wrap_pyfunction!(to_midi1, module)?)?;
    module.add_function(wrap_pyfunction!(to_midi2, module)?)?;
    module.add_function(wrap_pyfunction!(scale_up, module)?)
}
