//! Native event-view adapters. All interpretation and contributor bookkeeping are Rust-owned.
use crate::event_values::{Event, Timestamp};
use crate::files::{limit, uint};
use crate::live::StreamDiagnostic;
use meedee::event_views::{self as model, Value, View};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
#[pyclass(frozen, module = "meedee.events")]
pub struct Observation {
    value: model::Observation,
}
#[pymethods]
impl Observation {
    #[getter]
    fn stream(&self) -> &str {
        &self.value.stream
    }
    #[getter]
    fn group(&self) -> Option<u8> {
        self.value.group
    }
    #[getter]
    fn first_index(&self) -> Option<usize> {
        self.value.first_index
    }
    #[getter]
    fn last_index(&self) -> Option<usize> {
        self.value.last_index
    }
    #[getter]
    fn first_timestamp(&self) -> Option<Timestamp> {
        self.value
            .first_timestamp
            .clone()
            .map(|value| Timestamp { value })
    }
    #[getter]
    fn last_timestamp(&self) -> Option<Timestamp> {
        self.value
            .last_timestamp
            .clone()
            .map(|value| Timestamp { value })
    }
    #[getter]
    fn value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        value_object(py, self.value.value.clone())
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|v| self.value == v.value)
    }
    fn __hash__(&self) -> u64 {
        crate::tuning::hash(&self.value)
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Observation(stream={:?}, group={:?}, first_index={:?}, last_index={:?}, value={})",
            self.value.stream,
            self.value.group,
            self.value.first_index,
            self.value.last_index,
            self.value(py)?.bind(py).repr()?
        ))
    }
}
fn value_object(py: Python<'_>, v: Value) -> PyResult<Py<PyAny>> {
    match v {
        Value::Control(v) => Ok(crate::controls::to_python(py, vec![v])?.remove(0)),
        Value::Tuning(v) => crate::tuning::decoded_object(py, *v),
        Value::Ump(v) => Ok(crate::ump::assembly_outputs(py, vec![v])?.remove(0)),
        Value::Boundary(value) => Ok(Py::new(py, StreamDiagnostic { value })?.into_any()),
        v => crate::profiles::engines::view_value(py, v),
    }
}
fn error(e: std::io::Error) -> PyErr {
    PyValueError::new_err(e.to_string())
}
fn selected(v: Option<&Bound<'_, PyAny>>) -> PyResult<Option<u8>> {
    v.map(uint)
        .transpose()?
        .map(|v| {
            v.try_into()
                .map_err(|_| PyValueError::new_err("group must be 1..16"))
        })
        .transpose()
}
fn output(values: Vec<model::Observation>) -> Vec<Observation> {
    values
        .into_iter()
        .map(|value| Observation { value })
        .collect()
}
fn feed(
    view: &mut impl View,
    value: &Bound<'_, PyAny>,
    now: Option<u64>,
) -> PyResult<Vec<Observation>> {
    let values = if let Ok(e) = value.extract::<PyRef<'_, Event>>() {
        view.feed(&e.value, now)
    } else if let Ok(d) = value.extract::<PyRef<'_, StreamDiagnostic>>() {
        view.boundary(&d.value)
    } else {
        return Err(PyTypeError::new_err("Event or StreamDiagnostic required"));
    };
    Ok(output(values.map_err(error)?))
}
macro_rules! view_methods{($name:ident;{$($ctor:item)*})=>{
#[pymethods]impl $name{$($ctor)* fn feed(&mut self,value:&Bound<'_,PyAny>)->PyResult<Vec<Observation>>{feed(&mut self.value,value,None)}fn reset(&mut self){self.value.reset();}}
};}
#[pyclass(name = "EventInterpreter", module = "meedee.controls")]
struct Controls {
    value: model::Controls,
}
view_methods!(Controls;{#[new]#[pyo3(signature=(*,group=None))]fn new(group:Option<&Bound<'_,PyAny>>)->PyResult<Self>{Ok(Self{value:model::Controls::new(selected(group)?).map_err(error)?})}});
#[pyclass(name = "EventVelocityInterpreter", module = "meedee.controls")]
struct Velocity {
    value: model::Velocity,
}
view_methods!(Velocity;{#[new]#[pyo3(signature=(*,group=None))]fn new(group:Option<&Bound<'_,PyAny>>)->PyResult<Self>{Ok(Self{value:model::Velocity::new(selected(group)?).map_err(error)?})}});
#[pyclass(name = "EventTracker", module = "meedee.mpe")]
struct Mpe {
    value: model::Mpe,
}
view_methods!(Mpe;{
#[new]#[pyo3(signature=(*,zone,group=None))]fn new(zone:&Bound<'_,PyAny>,group:Option<&Bound<'_,PyAny>>)->PyResult<Self>{let meedee::messages::AnyCommand::ZoneConfiguration(zone)=crate::messages::extract_command(zone)?else{return Err(PyTypeError::new_err("ZoneConfiguration required"));};Ok(Self{value:model::Mpe::new(zone,selected(group)?).map_err(error)?})}
#[pyo3(signature=(*,channel))]fn snapshot(&self,py:Python<'_>,channel:&Bound<'_,PyAny>)->PyResult<Py<PyAny>>{value_object(py,Value::Mpe(self.value.snapshot(crate::messages::integer(channel,"channel")?).map_err(|e|PyValueError::new_err(e.to_string()))?))}
});
#[pyclass(name = "EventAssembler", module = "meedee.ump")]
struct Ump {
    value: model::Ump,
}
view_methods!(Ump;{#[new]#[pyo3(signature=(*,message_limit=1_048_576,total_limit=8_388_608,max_streams=64))]fn new(#[pyo3(from_py_with=limit)]message_limit:usize,#[pyo3(from_py_with=limit)]total_limit:usize,#[pyo3(from_py_with=limit)]max_streams:usize)->PyResult<Self>{Ok(Self{value:model::Ump::new(message_limit,total_limit,max_streams).map_err(error)?})}});
#[pyclass(name = "EventDecoder", module = "meedee.tuning")]
struct Tuning {
    value: model::Tuning,
}
view_methods!(Tuning;{
#[new]#[pyo3(signature=(*,group=None))]fn new(group:Option<&Bound<'_,PyAny>>)->PyResult<Self>{Ok(Self{value:model::Tuning::new(selected(group)?).map_err(error)?})}
fn feed_assembled(&mut self,observation:PyRef<'_,Observation>)->PyResult<Vec<Observation>>{Ok(output(self.value.assembled(&observation.value).map_err(error)?))}
});
#[pyclass(name = "EventAssembler", module = "meedee.timecode")]
struct Timecode {
    value: model::Timecode,
}
#[pymethods]
impl Timecode {
    #[new]
    #[pyo3(signature=(*,timeout_ns,group=None))]
    fn new(timeout_ns: &Bound<'_, PyAny>, group: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        Ok(Self {
            value: model::Timecode::new(uint(timeout_ns)?, selected(group)?).map_err(error)?,
        })
    }
    #[pyo3(signature=(value,*,now_ns))]
    fn feed(
        &mut self,
        value: &Bound<'_, PyAny>,
        now_ns: &Bound<'_, PyAny>,
    ) -> PyResult<Vec<Observation>> {
        feed(&mut self.value, value, Some(uint(now_ns)?))
    }
    #[pyo3(signature=(*,now_ns))]
    fn poll(&mut self, now_ns: &Bound<'_, PyAny>) -> PyResult<Vec<Observation>> {
        Ok(output(self.value.poll(uint(now_ns)?).map_err(error)?))
    }
    fn reset(&mut self) {
        self.value.reset();
    }
}
pub fn register_events(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Observation>()
}
pub fn register_controls(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Controls>()?;
    m.add_class::<Velocity>()
}
pub fn register_mpe(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Mpe>()
}
pub fn register_ump(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Ump>()
}
pub fn register_tuning(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Tuning>()
}
pub fn register_timecode(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Timecode>()
}
