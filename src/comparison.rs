use crate::files::{limit, os_error, uint, Stream};
use meedee::comparison;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use std::collections::BTreeMap;

#[pyclass(frozen, module = "meedee.inspection")]
struct ComparisonReport {
    value: comparison::Report,
}
#[pymethods]
impl ComparisonReport {
    #[getter]
    fn equal(&self) -> bool {
        self.value.equal
    }
    #[getter]
    fn left_complete(&self) -> bool {
        self.value.left_complete
    }
    #[getter]
    fn right_complete(&self) -> bool {
        self.value.right_complete
    }
    #[getter]
    fn left_problem(&self) -> Option<&str> {
        self.value.left_problem.as_deref()
    }
    #[getter]
    fn right_problem(&self) -> Option<&str> {
        self.value.right_problem.as_deref()
    }
    #[getter]
    fn left_records(&self) -> u64 {
        self.value.left_records
    }
    #[getter]
    fn right_records(&self) -> u64 {
        self.value.right_records
    }
    #[getter]
    fn differences(&self) -> u64 {
        self.value.differences
    }
    #[getter]
    fn examples(&self) -> Vec<Difference> {
        self.value
            .examples
            .iter()
            .map(|v| Difference { value: v.clone() })
            .collect()
    }
}
#[pyclass(frozen, module = "meedee.inspection")]
struct Difference {
    value: comparison::Difference,
}
#[pymethods]
impl Difference {
    #[getter]
    fn record(&self) -> Option<u64> {
        self.value.record
    }
    #[getter]
    fn reason(&self) -> &str {
        self.value.reason
    }
}

#[pyfunction]
#[pyo3(signature=(left, right, *, format, source_map=None, clock_map=None, ignore_timestamps=false, tick_tolerance=0, payload_limit=1_048_576, detail_limit=128, preserve_unknown=false))]
#[allow(clippy::too_many_arguments)]
fn compare_files(
    py: Python<'_>,
    left: &Bound<'_, PyAny>,
    right: &Bound<'_, PyAny>,
    format: &str,
    source_map: Option<BTreeMap<String, String>>,
    clock_map: Option<BTreeMap<String, String>>,
    #[pyo3(from_py_with=crate::ump::boolean)] ignore_timestamps: bool,
    #[pyo3(from_py_with=uint)] tick_tolerance: u64,
    #[pyo3(from_py_with=limit)] payload_limit: usize,
    #[pyo3(from_py_with=limit)] detail_limit: usize,
    #[pyo3(from_py_with=crate::ump::boolean)] preserve_unknown: bool,
) -> PyResult<ComparisonReport> {
    let format = crate::inspection::parse_format(format)?;
    let options = comparison::Options {
        sources: source_map.unwrap_or_default(),
        clocks: clock_map.unwrap_or_default(),
        ignore_timestamps,
        tick_tolerance,
        payload_limit,
        detail_limit,
        preserve_unknown,
    };
    let left = Stream::input(left)?;
    let right = Stream::input(right)?;
    let value = py
        .detach(move || comparison::compare(left, right, format, &options))
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::InvalidInput {
                PyValueError::new_err(e.to_string())
            } else {
                os_error(e)
            }
        })?;
    Ok(ComparisonReport { value })
}
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<ComparisonReport>()?;
    module.add_class::<Difference>()?;
    module.add_function(wrap_pyfunction!(compare_files, module)?)
}
