//! File and memory recording backed by the shared recorder lifecycle.
use crate::files::{limit, os_error};
use crate::packets::Clock;
use middi::messages::Message;
use middi::{archive as model, archive::Record};
use pyo3::exceptions::{PyTimeoutError, PyValueError};
use pyo3::prelude::*;
use std::fs::File;
use std::path::PathBuf;
use std::time::Duration;
#[pyclass(frozen, module = "middi.archive")]
struct RecordingReport {
    value: model::RecordingReport,
}
#[pymethods]
impl RecordingReport {
    #[getter]
    fn pressure(&self) -> Option<crate::metrics::Pressure> {
        self.value
            .pressure
            .clone()
            .map(|value| crate::metrics::Pressure { value })
    }
    #[getter]
    fn complete(&self) -> bool {
        self.value.complete
    }
    #[getter]
    fn gap_records(&self) -> u64 {
        self.value.gap_records
    }
    #[getter]
    fn records(&self) -> u64 {
        self.value.records
    }
    #[getter]
    fn failure(&self) -> Option<&str> {
        self.value.failure.as_deref()
    }
}
#[pyclass(frozen, module = "middi.archive")]
pub struct Recorder {
    pub value: model::Recorder,
    stream: String,
}
#[pymethods]
impl Recorder {
    #[new]
    #[pyo3(signature=(destination=None,*,clock,stream,label="",best_effort=false,payload_limit=1_048_576,record_limit=128,byte_limit=8_388_608,metrics=false))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        destination: Option<PathBuf>,
        clock: PyRef<'_, Clock>,
        stream: String,
        label: &str,
        #[pyo3(from_py_with=crate::ump::boolean)] best_effort: bool,
        #[pyo3(from_py_with=crate::units::bytes)] payload_limit: usize,
        #[pyo3(from_py_with=limit)] record_limit: usize,
        #[pyo3(from_py_with=crate::units::bytes)] byte_limit: usize,
        #[pyo3(from_py_with=crate::ump::boolean)] metrics: bool,
    ) -> PyResult<Self> {
        crate::callback::check_thread()?;
        let descriptors = vec![
            Record::Clock(clock.value.clone()),
            Record::Stream {
                id: stream.clone(),
                label: label.into(),
                clock_identity: destination.as_ref().map(|_| clock.value.identity().into()),
            },
        ];
        let value = py
            .detach(move || {
                let Some(destination) = destination else {
                    return model::Recorder::memory(descriptors, metrics);
                };
                model::Recorder::new(
                    File::create(destination)?,
                    descriptors,
                    payload_limit,
                    record_limit,
                    byte_limit,
                    best_effort,
                    metrics,
                )
            })
            .map_err(os_error)?;
        Ok(Self { value, stream })
    }
    #[getter]
    fn records(&self, py: Python<'_>) -> PyResult<Option<Vec<Py<PyAny>>>> {
        self.value
            .records()
            .map(|r| {
                r.into_iter()
                    .map(|r| crate::archive::to_python(py, r))
                    .collect()
            })
            .transpose()
    }
    #[getter]
    fn pressure(&self) -> Option<crate::metrics::Pressure> {
        self.value
            .pressure()
            .map(|value| crate::metrics::Pressure { value })
    }
    fn record(&self, record: &Bound<'_, PyAny>) -> PyResult<bool> {
        if let Ok(d) = record.extract::<PyRef<'_, crate::live::StreamDiagnostic>>() {
            if d.value.kind == middi::events::StreamDiagnosticKind::ClockChange {
                return Ok(self.value.record(Record::Discontinuity {
                    stream: d.value.stream.clone(),
                    reason: d.value.reason.clone(),
                }));
            }
            return Ok(self.value.record(Record::Gap {
                stream: d.value.stream.clone(),
                lost: d.value.lost,
                reason: d.value.reason.clone(),
            }));
        }
        let r = crate::archive::extract(record)?;
        if let Record::Event(e) = &r {
            if e.stream() != Some(self.stream.as_str()) {
                return Err(PyValueError::new_err(
                    "recording source differs from event stream",
                ));
            }
        }
        Ok(self.value.record(r))
    }
    fn fail(&self, reason: String) {
        self.value.fail(reason);
    }
    #[getter]
    fn failure(&self) -> Option<String> {
        self.value.failure()
    }
    #[pyo3(signature=(*,timeout=5.0))]
    fn stop(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with=crate::units::seconds)] timeout: f64,
    ) -> PyResult<RecordingReport> {
        crate::callback::check_thread()?;
        if !timeout.is_finite() || timeout <= 0.0 {
            return Err(PyValueError::new_err("timeout must be positive and finite"));
        }
        let timeout = Duration::try_from_secs_f64(timeout)
            .map_err(|_| PyValueError::new_err("timeout too large"))?;
        let result = py.detach(|| self.value.stop(timeout)).map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut {
                PyTimeoutError::new_err(e.to_string())
            } else {
                os_error(e)
            }
        })?;
        Ok(RecordingReport { value: result })
    }
}
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Recorder>()?;
    m.add_class::<RecordingReport>()
}
