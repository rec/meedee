//! Pure Python metric snapshots and caller-driven accumulators.
use crate::files::uint;
use meedee::metrics;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

#[pyclass(frozen, module = "meedee.metrics")]
pub struct Distribution {
    pub value: metrics::Histogram,
}
#[pymethods]
impl Distribution {
    #[getter]
    fn count(&self) -> u64 {
        self.value.count()
    }
    #[getter]
    fn sum_ns(&self) -> u128 {
        self.value.sum_ns()
    }
    #[getter]
    fn minimum_ns(&self) -> Option<u64> {
        self.value.minimum_ns()
    }
    #[getter]
    fn maximum_ns(&self) -> Option<u64> {
        self.value.maximum_ns()
    }
    #[getter]
    fn upper_bounds_ns(&self) -> Vec<u64> {
        metrics::UPPER_BOUNDS_NS.to_vec()
    }
    #[getter]
    fn buckets(&self) -> Vec<u64> {
        self.value.buckets().to_vec()
    }
}
#[pyclass(module = "meedee.metrics")]
struct Histogram {
    value: metrics::Histogram,
}
#[pymethods]
impl Histogram {
    #[new]
    fn new() -> Self {
        Self {
            value: metrics::Histogram::default(),
        }
    }
    fn observe(&mut self, #[pyo3(from_py_with=uint)] nanoseconds: u64) -> PyResult<()> {
        self.value.observe(nanoseconds).map_err(error)
    }
    fn snapshot(&self) -> Distribution {
        Distribution {
            value: self.value.clone(),
        }
    }
}
#[pyclass(frozen, module = "meedee.metrics")]
pub struct Pressure {
    pub value: metrics::Pressure,
}
#[pymethods]
impl Pressure {
    #[getter]
    fn accepted_records(&self) -> u64 {
        self.value.accepted_records
    }
    #[getter]
    fn capacity_rejections(&self) -> u64 {
        self.value.capacity_rejections
    }
    #[getter]
    fn peak_reserved_records(&self) -> usize {
        self.value.peak_reserved_records
    }
    #[getter]
    fn peak_reserved_bytes(&self) -> usize {
        self.value.peak_reserved_bytes
    }
}
#[pyclass(frozen, module = "meedee.metrics")]
struct DeliveryReport {
    value: metrics::DeliveryReport,
}
#[pymethods]
impl DeliveryReport {
    #[getter]
    fn events(&self) -> u64 {
        self.value.events
    }
    #[getter]
    fn elapsed_ns(&self) -> u64 {
        self.value.elapsed_ns
    }
    #[getter]
    fn intervals(&self) -> Distribution {
        Distribution {
            value: self.value.intervals.clone(),
        }
    }
}
#[pyclass(module = "meedee.metrics")]
struct Delivery {
    value: metrics::Delivery,
}
#[pymethods]
impl Delivery {
    #[new]
    #[pyo3(signature=(*,start_ns))]
    fn new(#[pyo3(from_py_with=uint)] start_ns: u64) -> Self {
        Self {
            value: metrics::Delivery::new(start_ns),
        }
    }
    fn observe(&mut self, #[pyo3(from_py_with=uint)] now_ns: u64) -> PyResult<()> {
        self.value.observe(now_ns).map_err(error)
    }
    fn snapshot(&self, #[pyo3(from_py_with=uint)] now_ns: u64) -> PyResult<DeliveryReport> {
        Ok(DeliveryReport {
            value: self.value.snapshot(now_ns).map_err(error)?,
        })
    }
}
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Distribution>()?;
    module.add_class::<Histogram>()?;
    module.add_class::<Pressure>()?;
    module.add_class::<DeliveryReport>()?;
    module.add_class::<Delivery>()
}
fn error(e: std::io::Error) -> PyErr {
    PyValueError::new_err(e.to_string())
}
