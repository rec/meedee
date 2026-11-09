//! Public Python values for opaque SysEx dump capture.
use pyo3::prelude::*;

#[pyclass(frozen, module = "meedee.sysex")]
pub(crate) struct DumpReport {
    pub(crate) value: meedee::sysex::DumpReport,
}
#[pymethods]
impl DumpReport {
    #[getter]
    fn model(&self) -> &str {
        &self.value.model
    }
    #[getter]
    fn scope(&self) -> &str {
        &self.value.scope
    }
    #[getter]
    fn status(&self) -> &str {
        self.value.status
    }
    #[getter]
    fn complete(&self) -> bool {
        self.value.complete
    }
    #[getter]
    fn requests_sent(&self) -> usize {
        self.value.requests_sent
    }
    #[getter]
    fn uncertain_send(&self) -> bool {
        self.value.uncertain_send
    }
    #[getter]
    fn ignored_events(&self) -> usize {
        self.value.ignored_events
    }
    #[getter]
    fn wire_bytes(&self) -> usize {
        self.value.wire_bytes
    }
    #[getter]
    fn reason(&self) -> Option<&str> {
        self.value.reason.as_deref()
    }
    #[getter]
    fn messages(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.value
            .messages
            .iter()
            .map(|m| crate::messages::to_python(py, meedee::messages::AnyMessage::SysEx(m.clone())))
            .collect()
    }
}
#[pyfunction]
fn models() -> Vec<&'static str> {
    meedee::sysex::models()
}

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<DumpReport>()?;
    module.add_function(wrap_pyfunction!(models, module)?)
}
