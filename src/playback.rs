//! Clock planning and caller-thread replay, sharing the Rust scheduler.
use crate::event_values::{Rational, Timestamp};
use crate::files::{frozen, os_error};
use crate::packets::Clock;
use meedee::events as model;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};
pyo3::create_exception!(_core, PlaybackError, PyException);
#[pyclass(frozen, module = "meedee.events")]
pub(crate) struct ClockMapping {
    pub(crate) value: model::ClockMapping,
}
frozen!(ClockMapping;source,target,rate,offset,uncertainty;{
#[new]#[pyo3(signature=(*,source,target,rate=None,offset=None,uncertainty=None))]fn new(source:PyRef<'_,Clock>,target:PyRef<'_,Clock>,rate:Option<PyRef<'_,Rational>>,offset:Option<PyRef<'_,Rational>>,uncertainty:Option<PyRef<'_,Rational>>)->PyResult<Self>{Ok(Self{value:model::ClockMapping::new(source.value.clone(),target.value.clone(),rate.map_or(meedee::events::Rational::new(1,1).expect("constant"),|v|v.value),offset.map_or(meedee::events::Rational::new(0,1).expect("constant"),|v|v.value),uncertainty.map(|v|v.value)).map_err(|e|pyo3::exceptions::PyValueError::new_err(e.to_string()))?})}
#[getter]fn source(&self)->Clock{Clock{value:self.value.source.clone()}}#[getter]fn target(&self)->Clock{Clock{value:self.value.target.clone()}}#[getter]fn rate(&self)->Rational{Rational{value:self.value.rate}}#[getter]fn offset(&self)->Rational{Rational{value:self.value.offset}}#[getter]fn uncertainty(&self)->Option<Rational>{self.value.uncertainty.map(|value|Rational{value})}
fn map_time(&self,timestamp:PyRef<'_,Timestamp>)->PyResult<Rational>{Ok(Rational{value:self.value.map(&timestamp.value).map_err(|e|pyo3::exceptions::PyValueError::new_err(e.to_string()))?})}
});
#[pyclass(frozen, module = "meedee.events")]
struct PlannedEvent {
    value: model::PlannedEvent,
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
    value: model::PlaybackPlan,
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
            value: model::PlaybackPlan::new(
                origin.value.clone(),
                self::mappings(mappings)?,
                allow_rounding,
            )
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
        })
    }
    fn deadline(&mut self, event: &Bound<'_, PyAny>) -> PyResult<PlannedEvent> {
        let event = crate::event_values::extract(event)?;
        Ok(PlannedEvent {
            value: self
                .value
                .deadline(&event)
                .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
        })
    }
}
#[pyclass(frozen, module = "meedee.events")]
pub(crate) struct Cancellation {
    pub(crate) value: model::Cancellation,
}
#[pymethods]
impl Cancellation {
    #[new]
    fn new() -> Self {
        Self {
            value: model::Cancellation::default(),
        }
    }
    fn wait_for(&self, py: Python<'_>, duration: &Bound<'_, PyAny>) -> PyResult<bool> {
        let duration = std::time::Duration::from_nanos(crate::units::duration(duration)?);
        Ok(py.detach(|| self.value.wait_for(duration)))
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
    value: model::PlaybackReport,
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
fn mappings(values: Option<&Bound<'_, PyAny>>) -> PyResult<Vec<model::ClockMapping>> {
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
/// Execute scalar and recorded segments on one caller-thread clock.
#[pyfunction]
#[pyo3(signature=(send,timeline,*,cancel=None,metrics=false))]
fn output_sequence(
    py: Python<'_>,
    send: &Bound<'_, PyAny>,
    timeline: &Bound<'_, PyAny>,
    cancel: Option<PyRef<'_, Cancellation>>,
    #[pyo3(from_py_with=crate::ump::boolean)] metrics: bool,
) -> PyResult<PlaybackReport> {
    use model::PlaybackClock;
    crate::callback::check_thread()?;
    let cancel = cancel.map_or_else(model::Cancellation::default, |v| v.value.clone());
    let mut clock = model::MonotonicClock::default();
    let mut histogram = metrics.then(meedee::metrics::Histogram::default);
    let mut report = model::PlaybackReport {
        lateness: histogram.as_ref().map(|h| Box::new(h.snapshot())),
        ..model::PlaybackReport::default()
    };
    let result: PyResult<()> = (|| {
        let mut iterator = timeline.try_iter()?;
        loop {
            if cancel.cancelled() {
                report.cancelled = true;
                return Ok(());
            }
            let Some(item) = iterator.next() else {
                return Ok(());
            };
            let (deadline, value, rounded): (u64, Py<PyAny>, bool) = item?.extract()?;
            while clock.now_ns() < deadline {
                let next = deadline.min(clock.now_ns().saturating_add(50_000_000));
                if !py
                    .detach(|| clock.wait_until(next, &cancel))
                    .map_err(os_error)?
                {
                    report.cancelled = true;
                    return Ok(());
                }
                py.check_signals()?;
            }
            if cancel.cancelled() {
                report.cancelled = true;
                return Ok(());
            }
            if !value.bind(py).is_none() {
                let lateness = clock.now_ns().saturating_sub(deadline);
                send.call1((value,))?;
                report.sent_events += 1;
                report.rounded_events += u64::from(rounded);
                report.max_lateness_ns = report.max_lateness_ns.max(lateness);
                if let Some(histogram) = &mut histogram {
                    histogram.observe(lateness).map_err(os_error)?;
                    report.lateness = Some(Box::new(histogram.snapshot()));
                }
            }
        }
    })();
    match result {
        Ok(()) => Ok(PlaybackReport { value: report }),
        Err(error) => {
            error
                .value(py)
                .setattr("completed_events", report.sent_events)?;
            error
                .value(py)
                .setattr("report", Py::new(py, PlaybackReport { value: report })?)?;
            Err(error)
        }
    }
}

pub fn register_native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(output_sequence, m)?)?;
    Ok(())
}
