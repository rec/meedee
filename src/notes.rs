//! Python values over the shared, caller-driven note observer.
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;

#[pyclass(module = "meedee.notes")]
struct Tracker {
    value: meedee::notes::Tracker,
    stream: String,
    detail_limit: usize,
}
#[pymethods]
impl Tracker {
    #[new]
    #[pyo3(signature=(*,stream,detail_limit=128))]
    fn new(
        stream: String,
        #[pyo3(from_py_with=crate::files::limit)] detail_limit: usize,
    ) -> PyResult<Self> {
        if stream.is_empty() {
            return Err(PyValueError::new_err("stream must not be empty"));
        }
        Ok(Self {
            value: meedee::notes::Tracker::default(),
            stream,
            detail_limit,
        })
    }
    fn feed(&mut self, item: &Bound<'_, PyAny>) -> PyResult<()> {
        if let Ok(e) = item.extract::<PyRef<'_, crate::event_values::Event>>() {
            if e.value.stream != self.stream {
                return Err(PyValueError::new_err("tracker belongs to one source"));
            }
            e.value
                .validate()
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
            self.value.feed(
                &self.stream,
                &e.value.message,
                e.value.timestamp.as_ref().map(|t| t.clock.identity()),
                self.detail_limit,
            );
        } else if let Ok(d) = item.extract::<PyRef<'_, crate::live::StreamDiagnostic>>() {
            if d.value.stream != self.stream {
                return Err(PyValueError::new_err("tracker belongs to one source"));
            }
            self.value.boundary(&self.stream, self.detail_limit);
        } else {
            return Err(PyTypeError::new_err("expected Event or StreamDiagnostic"));
        }
        Ok(())
    }
    fn invalidate(&mut self) {
        self.value.boundary(&self.stream, self.detail_limit);
    }
    fn snapshot(&self) -> Report {
        Report {
            value: self.value.snapshot(),
        }
    }
}

#[pyclass(frozen, module = "meedee.notes")]
pub struct Report {
    pub value: meedee::notes::Report,
}
#[pymethods]
impl Report {
    #[getter]
    fn channels(&self) -> Vec<ChannelState> {
        self.value
            .channels
            .iter()
            .map(|v| ChannelState { value: v.clone() })
            .collect()
    }
    #[getter]
    fn omitted_events(&self) -> u64 {
        self.value.omitted_events
    }
    #[getter]
    fn boundaries(&self) -> u64 {
        self.value.boundaries
    }
}

#[pyclass(frozen, module = "meedee.notes")]
pub struct ChannelState {
    pub value: meedee::notes::ChannelState,
}
#[pymethods]
impl ChannelState {
    #[getter]
    fn source(&self) -> &str {
        &self.value.source
    }
    #[getter]
    fn group(&self) -> Option<u8> {
        self.value.group
    }
    #[getter]
    fn channel(&self) -> u8 {
        self.value.channel
    }
    #[getter]
    fn protocol(&self) -> &str {
        self.value.protocol
    }
    #[getter]
    fn notes(&self) -> Vec<NoteState> {
        self.value
            .notes
            .iter()
            .map(|v| NoteState { value: v.clone() })
            .collect()
    }
    #[getter]
    fn sustain(&self) -> Option<u32> {
        self.value.sustain
    }
    #[getter]
    fn sostenuto(&self) -> Option<u32> {
        self.value.sostenuto
    }
    #[getter]
    fn unmatched_releases(&self) -> u64 {
        self.value.unmatched_releases
    }
    #[getter]
    fn uncertain(&self) -> bool {
        self.value.uncertain
    }
}
#[pyclass(frozen, module = "meedee.notes")]
struct NoteState {
    value: meedee::notes::NoteState,
}
#[pymethods]
impl NoteState {
    #[getter]
    fn note(&self) -> u8 {
        self.value.note
    }
    #[getter]
    fn held(&self) -> u64 {
        self.value.held
    }
    #[getter]
    fn released(&self) -> u64 {
        self.value.released
    }
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Tracker>()?;
    module.add_class::<Report>()?;
    module.add_class::<ChannelState>()?;
    module.add_class::<NoteState>()
}
