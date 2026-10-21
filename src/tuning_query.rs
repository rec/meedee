//! Python one-shot queries over the shared MTS tracker and codecs.
use crate::files::{os_error, uint, Stream};
use meedee::tuning_query;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;

#[pyclass(frozen, module = "meedee.tuning")]
struct QueryReport {
    value: tuning_query::Report,
}
#[pymethods]
impl QueryReport {
    #[getter]
    fn status(&self) -> &str {
        self.value.status
    }
    #[getter]
    fn request_sent(&self) -> bool {
        self.value.request_sent
    }
    #[getter]
    fn uncertain_send(&self) -> bool {
        self.value.uncertain_send
    }
    #[getter]
    fn ignored_events(&self) -> u64 {
        self.value.ignored_events
    }
    #[getter]
    fn deadline_ns(&self) -> Option<u64> {
        self.value.deadline_ns
    }
    #[getter]
    fn reason(&self) -> Option<&str> {
        self.value.reason.as_deref()
    }
    #[getter]
    fn reply(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.value
            .reply
            .as_ref()
            .map(|v| crate::messages::to_python(py, meedee::messages::AnyMessage::SysEx(v.clone())))
            .transpose()
    }
    #[getter]
    fn decoded(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.value
            .decoded
            .as_ref()
            .map(|v| crate::tuning::decoded_object(py, v.clone()))
            .transpose()
    }
}
#[pyclass(module = "meedee.tuning")]
struct Query {
    value: tuning_query::Query,
}
#[pymethods]
impl Query {
    #[new]
    #[pyo3(signature=(*,request,stream,timeout_ns))]
    fn new(
        request: &Bound<'_, PyAny>,
        stream: String,
        #[pyo3(from_py_with=uint)] timeout_ns: u64,
    ) -> PyResult<Self> {
        Ok(Self {
            value: tuning_query::Query::new(
                crate::tuning::extract_message(request)?,
                stream,
                timeout_ns,
            )
            .map_err(error)?,
        })
    }
    fn prepare(
        &mut self,
        py: Python<'_>,
        #[pyo3(from_py_with=uint)] now_ns: u64,
    ) -> PyResult<Py<PyAny>> {
        crate::messages::to_python(
            py,
            meedee::messages::AnyMessage::SysEx(self.value.prepare(now_ns).map_err(error)?),
        )
    }
    fn confirm(&mut self, #[pyo3(from_py_with=uint)] now_ns: u64) -> PyResult<()> {
        self.value.confirm(now_ns).map_err(error)
    }
    fn poll(&mut self, #[pyo3(from_py_with=uint)] now_ns: u64) -> PyResult<()> {
        self.value.poll(now_ns).map_err(error)
    }
    fn feed(
        &mut self,
        item: &Bound<'_, PyAny>,
        #[pyo3(from_py_with=uint)] now_ns: u64,
    ) -> PyResult<()> {
        let item = if let Ok(e) = item.extract::<PyRef<'_, crate::event_values::Event>>() {
            meedee::live::Item::Event(Box::new(e.value.clone()))
        } else if let Ok(d) = item.extract::<PyRef<'_, crate::live::StreamDiagnostic>>() {
            meedee::live::Item::Diagnostic(d.value.clone())
        } else {
            return Err(PyTypeError::new_err("expected Event or StreamDiagnostic"));
        };
        self.value.feed(&item, now_ns).map_err(error)
    }
    fn fail(&mut self, reason: &str) {
        self.value.fail(reason);
    }
    fn cancel(&mut self, reason: &str) {
        self.value.cancel(reason);
    }
    fn snapshot(&self) -> QueryReport {
        QueryReport {
            value: self.value.snapshot(),
        }
    }
    fn write(&self, py: Python<'_>, destination: &Bound<'_, PyAny>) -> PyResult<()> {
        if self.value.snapshot().status != "completed" {
            return Err(PyValueError::new_err(
                "only a completed query can be exported",
            ));
        }
        let mut stream = Stream::exclusive_output(destination)?;
        py.detach(|| self.value.write(&mut stream))
            .map_err(os_error)
    }
}
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Query>()?;
    module.add_class::<QueryReport>()
}
fn error(e: meedee::messages::ValueError) -> PyErr {
    PyValueError::new_err(e.to_string())
}
