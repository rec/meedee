//! Identity inquiry reports; codecs remain in the profile bindings.
use pyo3::prelude::*;

#[pyclass(frozen, module = "middy.universal")]
pub(crate) struct IdentityReport {
    pub(crate) value: middy::universal::IdentityReport,
}
#[pymethods]
impl IdentityReport {
    #[getter]
    fn messages(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.value
            .messages
            .iter()
            .map(|m| crate::messages::to_python(py, middy::messages::AnyMessage::SysEx(m.clone())))
            .collect()
    }
    #[getter]
    fn status(&self) -> &str {
        self.value.status
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
    fn reason(&self) -> Option<&str> {
        self.value.reason.as_deref()
    }
}
pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<IdentityReport>()
}
