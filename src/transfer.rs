//! Python values over the shared SysEx transfer planner and state machine.
use crate::files::{limit, os_error, uint, Stream};
use crate::messages::{to_python, SysEx};
use meedee::transfer;
use pyo3::exceptions::{PyIndexError, PyValueError};
use pyo3::prelude::*;

#[pyclass(frozen, module = "meedee.transfer")]
struct Plan {
    value: transfer::Plan,
}
#[pymethods]
impl Plan {
    #[new]
    #[pyo3(signature=(messages, *, gap_ns, message_limit=256, byte_limit=8_388_608))]
    fn new(
        messages: Vec<PyRef<'_, SysEx>>,
        #[pyo3(from_py_with=uint)] gap_ns: u64,
        #[pyo3(from_py_with=limit)] message_limit: usize,
        #[pyo3(from_py_with=limit)] byte_limit: usize,
    ) -> PyResult<Self> {
        Ok(Self {
            value: transfer::Plan::new(
                messages.iter().map(|v| Ok(v.value.clone())),
                gap_ns,
                message_limit,
                byte_limit,
            )
            .map_err(error)?,
        })
    }
    #[getter]
    fn message_count(&self) -> usize {
        self.value.message_count()
    }
    #[getter]
    fn wire_bytes(&self) -> usize {
        self.value.wire_bytes()
    }
    #[getter]
    fn gap_ns(&self) -> u64 {
        self.value.gap_ns()
    }
    fn message(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with=position)] index: usize,
    ) -> PyResult<Py<PyAny>> {
        let value = self
            .value
            .message(index)
            .ok_or_else(|| PyIndexError::new_err("transfer message index out of range"))?;
        to_python(py, meedee::messages::AnyMessage::SysEx(value.clone()))
    }
}
#[pyfunction]
#[pyo3(signature=(source, *, gap_ns, message_limit=256, byte_limit=8_388_608, payload_limit=1_048_576))]
fn read_file(
    py: Python<'_>,
    source: &Bound<'_, PyAny>,
    #[pyo3(from_py_with=uint)] gap_ns: u64,
    #[pyo3(from_py_with=limit)] message_limit: usize,
    #[pyo3(from_py_with=limit)] byte_limit: usize,
    #[pyo3(from_py_with=limit)] payload_limit: usize,
) -> PyResult<Plan> {
    let source = Stream::input(source)?;
    Ok(Plan {
        value: py
            .detach(|| {
                transfer::Plan::read(source, gap_ns, message_limit, byte_limit, payload_limit)
            })
            .map_err(os_error)?,
    })
}
#[pyclass(frozen, module = "meedee.transfer")]
struct Step {
    value: transfer::Step,
}
#[pymethods]
impl Step {
    #[getter]
    fn index(&self) -> usize {
        self.value.index
    }
    #[getter]
    fn message(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        to_python(
            py,
            meedee::messages::AnyMessage::SysEx(self.value.message.clone()),
        )
    }
}
#[pyclass(frozen, module = "meedee.transfer")]
struct Report {
    value: transfer::Report,
}
#[pymethods]
impl Report {
    #[getter]
    fn status(&self) -> &str {
        self.value.status
    }
    #[getter]
    fn total_messages(&self) -> usize {
        self.value.total_messages
    }
    #[getter]
    fn total_bytes(&self) -> usize {
        self.value.total_bytes
    }
    #[getter]
    fn confirmed_messages(&self) -> usize {
        self.value.confirmed_messages
    }
    #[getter]
    fn confirmed_bytes(&self) -> usize {
        self.value.confirmed_bytes
    }
    #[getter]
    fn remaining_messages(&self) -> usize {
        self.value.remaining_messages
    }
    #[getter]
    fn in_flight(&self) -> Option<usize> {
        self.value.in_flight
    }
    #[getter]
    fn uncertain_message(&self) -> Option<usize> {
        self.value.uncertain_message
    }
    #[getter]
    fn next_ready_ns(&self) -> Option<u64> {
        self.value.next_ready_ns
    }
    #[getter]
    fn reason(&self) -> Option<&str> {
        self.value.reason.as_deref()
    }
}
#[pyclass(module = "meedee.transfer")]
struct Session {
    value: transfer::Session,
}
#[pymethods]
impl Session {
    #[new]
    fn new(plan: PyRef<'_, Plan>) -> Self {
        Self {
            value: transfer::Session::new(plan.value.clone()),
        }
    }
    fn prepare(&mut self, #[pyo3(from_py_with=uint)] now_ns: u64) -> PyResult<Option<Step>> {
        Ok(self
            .value
            .prepare(now_ns)
            .map_err(error)?
            .map(|value| Step { value }))
    }
    fn confirm(
        &mut self,
        #[pyo3(from_py_with=position)] index: usize,
        #[pyo3(from_py_with=uint)] now_ns: u64,
    ) -> PyResult<()> {
        self.value.confirm(index, now_ns).map_err(error)
    }
    fn fail(&mut self, #[pyo3(from_py_with=position)] index: usize, reason: &str) -> PyResult<()> {
        self.value.fail(index, reason).map_err(error)
    }
    fn cancel(&mut self, reason: &str) {
        self.value.cancel(reason);
    }
    fn snapshot(&self) -> Report {
        Report {
            value: self.value.snapshot(),
        }
    }
}
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Plan>()?;
    module.add_class::<Step>()?;
    module.add_class::<Report>()?;
    module.add_class::<Session>()?;
    module.add_function(wrap_pyfunction!(read_file, module)?)
}
fn error(e: std::io::Error) -> PyErr {
    PyValueError::new_err(e.to_string())
}

fn position(value: &Bound<'_, PyAny>) -> PyResult<usize> {
    uint(value)?
        .try_into()
        .map_err(|_| PyValueError::new_err("index exceeds platform size"))
}
