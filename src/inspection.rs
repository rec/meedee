//! Python inspection reports. Parsing and aggregation stay in the Rust toolkit.
use crate::files::{limit, os_error, Stream};
use meedee::inspection;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use std::collections::BTreeMap;

#[pyclass(frozen, module = "meedee.inspection")]
struct Report {
    value: inspection::Report,
}

#[pymethods]
impl Report {
    #[getter]
    fn complete(&self) -> bool {
        self.value.complete
    }
    #[getter]
    fn problem(&self) -> Option<&str> {
        self.value.problem.as_deref()
    }
    #[getter]
    fn records(&self) -> u64 {
        self.value.records
    }
    #[getter]
    fn message_counts(&self) -> BTreeMap<&str, u64> {
        self.value.message_counts.clone()
    }
    #[getter]
    fn clocks(&self) -> Vec<crate::packets::Clock> {
        self.value
            .clocks
            .iter()
            .map(|c| crate::packets::Clock { value: c.clone() })
            .collect()
    }
    #[getter]
    fn streams(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.value
            .streams
            .iter()
            .map(|r| crate::archive::to_python(py, r.clone()))
            .collect()
    }
    #[getter]
    fn smf_header(&self) -> Option<crate::files::Header> {
        self.value
            .smf_header
            .clone()
            .map(|value| crate::files::Header { value })
    }
    #[getter]
    fn track_records(&self) -> BTreeMap<u16, u64> {
        self.value.track_records.clone()
    }
    #[getter]
    fn gap_records(&self) -> u64 {
        self.value.gap_records
    }
    #[getter]
    fn lost_events(&self) -> u128 {
        self.value.lost_events
    }
    #[getter]
    fn discontinuities(&self) -> u64 {
        self.value.discontinuities
    }
    #[getter]
    fn untimed_events(&self) -> u64 {
        self.value.untimed_events
    }
    #[getter]
    fn opaque_records(&self) -> u64 {
        self.value.opaque_records
    }
    fn __repr__(&self) -> String {
        format!(
            "Report(records={}, complete={}, problem={:?})",
            self.value.records, self.value.complete, self.value.problem
        )
    }
}

#[pyfunction]
#[pyo3(signature=(source, *, format, payload_limit=1_048_576, preserve_unknown=false))]
fn inspect_file(
    py: Python<'_>,
    source: &Bound<'_, PyAny>,
    format: &str,
    #[pyo3(from_py_with=limit)] payload_limit: usize,
    #[pyo3(from_py_with=crate::ump::boolean)] preserve_unknown: bool,
) -> PyResult<Report> {
    let format = match format {
        "archive" => inspection::Format::Archive,
        "smf" => inspection::Format::Smf,
        "syx" => inspection::Format::Syx,
        "clip" => inspection::Format::Clip,
        _ => {
            return Err(PyValueError::new_err(
                "format must be archive, smf, syx or clip",
            ))
        }
    };
    let source = Stream::input(source)?;
    let value = py
        .detach(move || inspection::inspect(source, format, payload_limit, preserve_unknown))
        .map_err(os_error)?;
    Ok(Report { value })
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Report>()?;
    module.add_function(wrap_pyfunction!(inspect_file, module)?)
}
