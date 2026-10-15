//! Native immutable event values, shared by files and typed transport.
use crate::files::{frozen, os_error, uint};
use crate::packets::Clock;
use meedee::events as model;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyInt, PyTuple};
#[pyclass(frozen, skip_from_py_object, module = "meedee.events")]
#[derive(Clone)]
pub struct Timestamp {
    pub value: model::Timestamp,
}
frozen!(Timestamp;ticks,clock;{
#[new]#[pyo3(signature=(*,ticks,clock))]fn new(ticks:&Bound<'_,PyAny>,clock:PyRef<'_,Clock>)->PyResult<Self>{let value=model::Timestamp{ticks:crate::messages::integer(ticks,"ticks")?,clock:clock.value.clone()};if value.clock.mode()==Some(false){return Err(PyValueError::new_err("disabled clock has no timestamps"));}Ok(Self{value})}
#[getter]fn ticks(&self)->i64{self.value.ticks}#[getter]fn clock(&self)->Clock{Clock{value:self.value.clock.clone()}}#[getter]fn seconds(&self)->PyResult<Rational>{Ok(Rational{value:model::timestamp_seconds(&self.value).map_err(os_error)?})}
});
#[pyclass(frozen, skip_from_py_object, module = "meedee.events")]
#[derive(Clone)]
pub struct SourceSpan {
    pub value: model::Span,
}
frozen!(SourceSpan;first_packet,last_packet,first_timestamp,last_timestamp;{
#[new]#[pyo3(signature=(*,first_packet,last_packet,first_timestamp=None,last_timestamp=None))]fn new(first_packet:&Bound<'_,PyAny>,last_packet:&Bound<'_,PyAny>,first_timestamp:Option<PyRef<'_,Timestamp>>,last_timestamp:Option<PyRef<'_,Timestamp>>)->PyResult<Self>{let value=model::Span{first_packet:uint(first_packet)?,last_packet:uint(last_packet)?,first_timestamp:first_timestamp.map(|v|v.value.clone()),last_timestamp:last_timestamp.map(|v|v.value.clone())};if value.first_packet>value.last_packet{return Err(PyValueError::new_err("reversed packet span"));}if let(Some(a),Some(b))=(&value.first_timestamp,&value.last_timestamp){if a.clock!=b.clock{return Err(PyValueError::new_err("a span cannot combine clock generations"));}}Ok(Self{value})}
#[getter]fn first_packet(&self)->u64{self.value.first_packet}#[getter]fn last_packet(&self)->u64{self.value.last_packet}#[getter]fn first_timestamp(&self)->Option<Timestamp>{self.value.first_timestamp.clone().map(|value|Timestamp{value})}#[getter]fn last_timestamp(&self)->Option<Timestamp>{self.value.last_timestamp.clone().map(|value|Timestamp{value})}
});
#[pyclass(frozen, skip_from_py_object, module = "meedee.events")]
#[derive(Clone)]
pub struct Event {
    pub value: model::Event,
}
frozen!(Event;stream,message,timestamp,span;{
#[new]#[pyo3(signature=(*,stream,message,timestamp=None,span=None))]fn new(stream:String,message:&Bound<'_,PyAny>,timestamp:Option<PyRef<'_,Timestamp>>,span:Option<PyRef<'_,SourceSpan>>)->PyResult<Self>{let value=model::Event{stream,message:payload(message)?,timestamp:timestamp.map(|v|v.value.clone()),span:span.map(|v|v.value.clone())};value.validate().map_err(os_error)?;Ok(Self{value})}
#[getter]fn stream(&self)->&str{&self.value.stream}#[getter]fn message(&self,py:Python<'_>)->PyResult<Py<PyAny>>{payload_python(py,self.value.message.clone())}#[getter]fn timestamp(&self)->Option<Timestamp>{self.value.timestamp.clone().map(|value|Timestamp{value})}#[getter]fn span(&self)->Option<SourceSpan>{self.value.span.clone().map(|value|SourceSpan{value})}
});
pub fn payload(v: &Bound<'_, PyAny>) -> PyResult<model::Payload> {
    if let Ok(v) = crate::messages::extract_primitive(v) {
        return Ok(model::Payload::Midi1(v));
    }
    if let Ok(v) = crate::messages::extract_command(v) {
        return Ok(model::Payload::Command(v));
    }
    if let Ok(v) = crate::ump::extract(v) {
        return Ok(model::Payload::Ump(v));
    }
    Err(PyTypeError::new_err("Event requires a primitive MIDI message, UMP value or supported command; explicitly encode profile messages as SysEx"))
}
pub fn payload_python(py: Python<'_>, v: model::Payload) -> PyResult<Py<PyAny>> {
    match v {
        model::Payload::Midi1(v) => crate::messages::to_python(py, v),
        model::Payload::Ump(v) => crate::ump::to_python(py, v),
        model::Payload::Command(v) => {
            let (name, fields) = crate::messages::command_wire(&v);
            let module = match name.as_str() {
                "BankProgramSelect" => "meedee.gm",
                "ZoneConfiguration" => "meedee.mpe",
                _ => "meedee.messages",
            };
            let kwargs = PyDict::new(py);
            for (k, v) in fields {
                kwargs.set_item(k, v)?;
            }
            Ok(PyModule::import(py, module)?
                .getattr(name)?
                .call((), Some(&kwargs))?
                .unbind())
        }
    }
}
#[pyclass(frozen, module = "meedee.events")]
pub struct Rational {
    pub value: model::Rational,
}
frozen!(Rational;numerator,denominator;{
#[new]#[pyo3(signature=(*,numerator,denominator))]fn new(numerator:&Bound<'_,PyAny>,denominator:&Bound<'_,PyAny>)->PyResult<Self>{Ok(Self{value:model::Rational::new(int128(numerator)?,int128(denominator)?).map_err(os_error)?})}
#[getter]fn numerator(&self)->i128{self.value.numerator()}#[getter]fn denominator(&self)->i128{self.value.denominator()}
#[pyo3(signature=(*,allow_rounding=false))]fn round(&self,#[pyo3(from_py_with=crate::ump::boolean)]allow_rounding:bool)->PyResult<(i128,Rational)>{let(n,error)=self.value.round(allow_rounding).map_err(os_error)?;Ok((n,Rational{value:error}))}
});
fn int128(v: &Bound<'_, PyAny>) -> PyResult<i128> {
    if !v.is_exact_instance_of::<PyInt>() {
        return Err(PyTypeError::new_err("expected built-in integer"));
    }
    v.extract()
        .map_err(|_| PyValueError::new_err("integer must fit signed 128 bits"))
}
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Timestamp>()?;
    m.add_class::<SourceSpan>()?;
    m.add_class::<Event>()?;
    m.add_class::<Rational>()
}
