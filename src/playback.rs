//! Clock planning and caller-thread replay, sharing the Rust scheduler.
use crate::event_values::{payload_python, Event, Rational, Timestamp};
use crate::files::{frozen, os_error};
use crate::packets::Clock;
use meedee::playback as model;
use pyo3::exceptions::{PyException, PyStopIteration, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};
pyo3::create_exception!(_core, PlaybackError, PyException);
#[pyclass(frozen, module = "meedee.events")]
pub(crate) struct ClockMapping {
    pub(crate) value: model::Mapping,
}
frozen!(ClockMapping;source,target,rate,offset,uncertainty;{
#[new]#[pyo3(signature=(*,source,target,rate=None,offset=None,uncertainty=None))]fn new(source:PyRef<'_,Clock>,target:PyRef<'_,Clock>,rate:Option<PyRef<'_,Rational>>,offset:Option<PyRef<'_,Rational>>,uncertainty:Option<PyRef<'_,Rational>>)->PyResult<Self>{Ok(Self{value:model::Mapping::new(source.value.clone(),target.value.clone(),rate.map_or(meedee::events::Rational::new(1,1).expect("constant"),|v|v.value),offset.map_or(meedee::events::Rational::new(0,1).expect("constant"),|v|v.value),uncertainty.map(|v|v.value)).map_err(|e|pyo3::exceptions::PyValueError::new_err(e.to_string()))?})}
#[getter]fn source(&self)->Clock{Clock{value:self.value.source.clone()}}#[getter]fn target(&self)->Clock{Clock{value:self.value.target.clone()}}#[getter]fn rate(&self)->Rational{Rational{value:self.value.rate}}#[getter]fn offset(&self)->Rational{Rational{value:self.value.offset}}#[getter]fn uncertainty(&self)->Option<Rational>{self.value.uncertainty.map(|value|Rational{value})}
fn map_time(&self,timestamp:PyRef<'_,Timestamp>)->PyResult<Rational>{Ok(Rational{value:self.value.map(&timestamp.value).map_err(|e|pyo3::exceptions::PyValueError::new_err(e.to_string()))?})}
});
#[pyclass(frozen, module = "meedee.events")]
struct PlannedEvent {
    value: model::Planned,
}
#[pymethods]
impl PlannedEvent {
    #[getter]
    fn delay_ns(&self) -> u64 {
        self.value.delay_ns
    }
    #[getter]
    fn rounding_error_ns(&self) -> Rational {
        Rational {
            value: self.value.rounding_error_ns,
        }
    }
}
#[pyclass(module = "meedee.events")]
struct PlaybackPlan {
    value: model::Planner,
}
#[pymethods]
impl PlaybackPlan {
    #[new]
    #[pyo3(signature=(*,origin,mappings=None,allow_rounding=false))]
    fn new(
        origin: PyRef<'_, Timestamp>,
        mappings: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with=crate::ump::boolean)] allow_rounding: bool,
    ) -> PyResult<Self> {
        Ok(Self {
            value: model::Planner::new(
                origin.value.clone(),
                self::mappings(mappings)?,
                allow_rounding,
            )
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
        })
    }
    fn deadline(&mut self, event: PyRef<'_, Event>) -> PyResult<PlannedEvent> {
        Ok(PlannedEvent {
            value: self
                .value
                .deadline(&event.value)
                .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
        })
    }
}
#[pyclass(frozen, module = "meedee.events")]
struct Cancellation {
    value: model::Cancellation,
}
#[pymethods]
impl Cancellation {
    #[new]
    fn new() -> Self {
        Self {
            value: model::Cancellation::default(),
        }
    }
    fn cancel(&self) {
        self.value.cancel();
    }
    #[getter]
    fn cancelled(&self) -> bool {
        self.value.cancelled()
    }
}
#[pyclass(frozen, module = "meedee.events")]
struct PlaybackReport {
    value: model::Report,
}
#[pymethods]
impl PlaybackReport {
    #[getter]
    fn lateness(&self) -> Option<crate::metrics::Distribution> {
        self.value
            .lateness
            .as_deref()
            .cloned()
            .map(|value| crate::metrics::Distribution { value })
    }
    #[getter]
    fn sent_events(&self) -> u64 {
        self.value.sent_events
    }
    #[getter]
    fn cancelled(&self) -> bool {
        self.value.cancelled
    }
    #[getter]
    fn max_lateness_ns(&self) -> u64 {
        self.value.max_lateness_ns
    }
    #[getter]
    fn rounded_events(&self) -> u64 {
        self.value.rounded_events
    }
}
fn mappings(values: Option<&Bound<'_, PyAny>>) -> PyResult<Vec<model::Mapping>> {
    let mut out = vec![];
    if let Some(values) = values {
        for v in values.try_iter()? {
            if out.len() == 128 {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "mapping count limit",
                ));
            }
            out.push(v?.extract::<PyRef<'_, ClockMapping>>()?.value.clone());
        }
    }
    Ok(out)
}
struct Source {
    iterator: Py<PyAny>,
}
impl Iterator for Source {
    type Item = std::io::Result<meedee::events::Event>;
    fn next(&mut self) -> Option<Self::Item> {
        Python::attach(|py| loop {
            let item = match self.iterator.bind(py).call_method0("__next__") {
                Ok(v) => v,
                Err(e) if e.is_instance_of::<PyStopIteration>(py) => return None,
                Err(e) => return Some(Err(std::io::Error::other(e.to_string()))),
            };
            if let Ok(event) = item.extract::<PyRef<'_, Event>>() {
                return Some(Ok(event.value.clone()));
            }
            match crate::archive::extract(&item){Ok(meedee::archive::Record::Clock(_)|meedee::archive::Record::Stream{..}|meedee::archive::Record::Metadata{..})=>continue,Ok(_)=>return Some(Err(std::io::Error::other("gap, discontinuity or opaque data requires an explicit replay policy; no automatic skipping"))),Err(e)=>return Some(Err(std::io::Error::other(e.to_string())))}
        })
    }
}
#[pyfunction]
#[pyo3(signature=(output,source,*,origin,mappings=None,cancel=None,allow_rounding=false,metrics=false))]
#[allow(clippy::too_many_arguments)]
fn play(
    py: Python<'_>,
    output: &Bound<'_, PyAny>,
    source: &Bound<'_, PyAny>,
    origin: PyRef<'_, Timestamp>,
    mappings: Option<&Bound<'_, PyAny>>,
    cancel: Option<PyRef<'_, Cancellation>>,
    #[pyo3(from_py_with=crate::ump::boolean)] allow_rounding: bool,
    #[pyo3(from_py_with=crate::ump::boolean)] metrics: bool,
) -> PyResult<PlaybackReport> {
    crate::callback::check_thread()?;
    let planner = model::Planner::new(
        origin.value.clone(),
        self::mappings(mappings)?,
        allow_rounding,
    )
    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    let cancel = cancel.map_or_else(model::Cancellation::default, |v| v.value.clone());
    let source = Source {
        iterator: source.try_iter()?.into_any().unbind(),
    };
    let send = output.getattr("send")?.unbind();
    if !send.bind(py).is_callable() {
        return Err(PyTypeError::new_err("MIDI output with callable send required"));
    }
    let result = py.detach(move || {
        model::play(
            source,
            planner,
            &mut model::MonotonicClock::default(),
            &cancel,
            |message| {
                Python::attach(|py| {
                    send
                        .bind(py)
                        .call1((payload_python(py, message.clone())?,))?;
                    Ok::<(), PyErr>(())
                })
            },
            metrics,
        )
        .map_err(Box::new)
    });
    match result {
        Ok(value) => Ok(PlaybackReport { value }),
        Err(failure) => {
            let err = PlaybackError::new_err(failure.to_string());
            err.value(py)
                .setattr("completed_events", failure.report.sent_events)?;
            err.value(py).setattr(
                "report",
                Py::new(
                    py,
                    PlaybackReport {
                        value: failure.report,
                    },
                )?,
            )?;
            let cause = match failure.cause {
                model::Cause::Input(e) => os_error(e),
                model::Cause::Send(e) => e,
            };
            err.set_cause(py, Some(cause));
            Err(err)
        }
    }
}
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ClockMapping>()?;
    m.add_class::<PlannedEvent>()?;
    m.add_class::<PlaybackPlan>()?;
    m.add_class::<Cancellation>()?;
    m.add_class::<PlaybackReport>()?;
    let error = m.py().get_type::<PlaybackError>();
    error.setattr("__module__", "meedee.events")?;
    m.add("PlaybackError", error)
}
pub fn register_native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(play, m)?)?;
    Ok(())
}
