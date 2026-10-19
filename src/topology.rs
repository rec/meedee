//! Frozen Python discovery observations, independent of native client startup.
use meedee::topology;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

#[pyclass(frozen, module = "meedee.topology")]
struct Endpoint {
    value: topology::Endpoint,
}
#[pymethods]
impl Endpoint {
    #[new]
    #[pyo3(signature=(*,backend,protocol,direction,backend_id,name))]
    fn new(
        backend: String,
        protocol: String,
        direction: String,
        backend_id: u64,
        name: String,
    ) -> PyResult<Self> {
        Ok(Self {
            value: topology::Endpoint::new(backend, protocol, direction, backend_id, name)
                .map_err(error)?,
        })
    }
    #[getter]
    fn backend(&self) -> &str {
        self.value.backend()
    }
    #[getter]
    fn protocol(&self) -> &str {
        self.value.protocol()
    }
    #[getter]
    fn direction(&self) -> &str {
        self.value.direction()
    }
    #[getter]
    fn backend_id(&self) -> u64 {
        self.value.backend_id()
    }
    #[getter]
    fn name(&self) -> &str {
        self.value.name()
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|v| self.value == v.value)
    }
    fn __hash__(&self) -> u64 {
        crate::tuning::hash(&self.value)
    }
}
#[pyclass(frozen, module = "meedee.topology")]
struct PortCount {
    value: topology::PortCount,
}
#[pymethods]
impl PortCount {
    #[getter]
    fn endpoint(&self) -> Endpoint {
        Endpoint {
            value: self.value.endpoint.clone(),
        }
    }
    #[getter]
    fn count(&self) -> usize {
        self.value.count
    }
}
#[pyclass(frozen, module = "meedee.topology")]
struct Change {
    value: topology::Change,
}
#[pymethods]
impl Change {
    #[getter]
    fn kind(&self) -> &str {
        self.value.kind
    }
    #[getter]
    fn endpoint(&self) -> Endpoint {
        Endpoint {
            value: self.value.endpoint.clone(),
        }
    }
    #[getter]
    fn count(&self) -> usize {
        self.value.count
    }
}
#[pyclass(frozen, module = "meedee.topology")]
struct Delta {
    value: topology::Delta,
}
#[pymethods]
impl Delta {
    #[getter]
    fn generation(&self) -> u64 {
        self.value.generation
    }
    #[getter]
    fn added(&self) -> usize {
        self.value.added
    }
    #[getter]
    fn removed(&self) -> usize {
        self.value.removed
    }
    #[getter]
    fn changes(&self) -> Vec<Change> {
        self.value
            .changes
            .iter()
            .map(|v| Change { value: v.clone() })
            .collect()
    }
    #[getter]
    fn omitted_changes(&self) -> usize {
        self.value.omitted_changes
    }
}
#[pyclass(frozen, module = "meedee.topology")]
struct Report {
    value: topology::Report,
}
#[pymethods]
impl Report {
    #[getter]
    fn generation(&self) -> u64 {
        self.value.generation
    }
    #[getter]
    fn port_count(&self) -> usize {
        self.value.port_count
    }
    #[getter]
    fn ports(&self) -> Vec<PortCount> {
        self.value
            .ports
            .iter()
            .map(|v| PortCount { value: v.clone() })
            .collect()
    }
    #[getter]
    fn last(&self) -> Option<Delta> {
        self.value.last.clone().map(|value| Delta { value })
    }
    #[getter]
    fn history(&self) -> Vec<Delta> {
        self.value
            .history
            .iter()
            .map(|v| Delta { value: v.clone() })
            .collect()
    }
    #[getter]
    fn omitted_history(&self) -> u64 {
        self.value.omitted_history
    }
    #[getter]
    fn stopped(&self) -> bool {
        self.value.stopped
    }
    #[getter]
    fn reason(&self) -> Option<&str> {
        self.value.reason.as_deref()
    }
}
#[pyclass(module = "meedee.topology")]
struct Tracker {
    value: topology::Tracker,
}
#[pymethods]
impl Tracker {
    #[new]
    #[pyo3(signature=(*,port_limit=256,detail_limit=128,history_limit=128))]
    fn new(
        #[pyo3(from_py_with=crate::files::limit)] port_limit: usize,
        #[pyo3(from_py_with=crate::files::limit)] detail_limit: usize,
        #[pyo3(from_py_with=crate::files::limit)] history_limit: usize,
    ) -> PyResult<Self> {
        Ok(Self {
            value: topology::Tracker::new(port_limit, detail_limit, history_limit)
                .map_err(error)?,
        })
    }
    fn observe(&mut self, ports: Vec<PyRef<'_, Endpoint>>) -> PyResult<Report> {
        Ok(Report {
            value: self
                .value
                .observe(ports.iter().map(|p| p.value.clone()).collect())
                .map_err(error)?,
        })
    }
    fn stop(&mut self, reason: &str) {
        self.value.stop(reason);
    }
    fn snapshot(&self) -> Report {
        Report {
            value: self.value.snapshot(),
        }
    }
}
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Endpoint>()?;
    module.add_class::<PortCount>()?;
    module.add_class::<Change>()?;
    module.add_class::<Delta>()?;
    module.add_class::<Report>()?;
    module.add_class::<Tracker>()
}
fn error(e: std::io::Error) -> PyErr {
    PyValueError::new_err(e.to_string())
}
