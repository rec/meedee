//! Streaming Python archive bridge; schema and validation live in Rust core.
use crate::event_values::Event;
use crate::files::{bytes, closed, frozen, limit, os_error, uint, Stream as IoStream};
use crate::packets::Clock;
use meedee::archive as model;
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyTuple};
#[pyclass(frozen, module = "meedee.archive")]
pub struct Stream {
    value: (String, String),
}
frozen!(Stream;id,label;{
#[new]#[pyo3(signature=(*,id,label))]fn new(id:String,label:String)->PyResult<Self>{if id.is_empty(){return Err(pyo3::exceptions::PyValueError::new_err("empty stream identity"));}Ok(Self{value:(id,label)})}
#[getter]fn id(&self)->&str{&self.value.0}#[getter]fn label(&self)->&str{&self.value.1}
});
#[pyclass(frozen, module = "meedee.archive")]
pub struct Gap {
    pub value: (String, u64, String),
}
frozen!(Gap;stream,lost,reason;{
#[new]#[pyo3(signature=(*,stream,lost,reason))]fn new(stream:String,lost:&Bound<'_,PyAny>,reason:String)->PyResult<Self>{Ok(Self{value:(stream,uint(lost)?,reason)})}
#[getter]fn stream(&self)->&str{&self.value.0}#[getter]fn lost(&self)->u64{self.value.1}#[getter]fn reason(&self)->&str{&self.value.2}
});
#[pyclass(frozen, module = "meedee.archive")]
pub struct Discontinuity {
    value: (String, String),
}
frozen!(Discontinuity;stream,reason;{
#[new]#[pyo3(signature=(*,stream,reason))]fn new(stream:String,reason:String)->Self{Self{value:(stream,reason)}}
#[getter]fn stream(&self)->&str{&self.value.0}#[getter]fn reason(&self)->&str{&self.value.1}
});
#[pyclass(frozen, module = "meedee.archive")]
pub struct Metadata {
    value: (String, Vec<u8>),
}
frozen!(Metadata;key,data;{
#[new]#[pyo3(signature=(*,key,data))]fn new(key:String,data:&Bound<'_,PyAny>)->PyResult<Self>{Ok(Self{value:(key,bytes(data,usize::MAX)?)})}
#[getter]fn key(&self)->&str{&self.value.0}#[getter]fn data<'py>(&self,py:Python<'py>)->Bound<'py,PyBytes>{PyBytes::new(py,&self.value.1)}
});
#[pyclass(frozen, module = "meedee.archive")]
pub struct Opaque {
    value: (u16, Vec<u8>),
}
frozen!(Opaque;kind,data;{
#[new]#[pyo3(signature=(*,kind,data))]fn new(kind:&Bound<'_,PyAny>,data:&Bound<'_,PyAny>)->PyResult<Self>{let kind=uint(kind)?.try_into().map_err(|_|pyo3::exceptions::PyValueError::new_err("kind exceeds u16"))?;if matches!(kind,0..=6|65535){return Err(pyo3::exceptions::PyValueError::new_err("reserved record ID"));}Ok(Self{value:(kind,bytes(data,usize::MAX)?)})}
#[getter]fn kind(&self)->u16{self.value.0}#[getter]fn data<'py>(&self,py:Python<'py>)->Bound<'py,PyBytes>{PyBytes::new(py,&self.value.1)}
});
pub fn extract(v: &Bound<'_, PyAny>) -> PyResult<model::Record> {
    if let Ok(v) = v.extract::<PyRef<'_, Clock>>() {
        return Ok(model::Record::Clock(v.value.clone()));
    }
    if let Ok(v) = v.extract::<PyRef<'_, Stream>>() {
        return Ok(model::Record::Stream {
            id: v.value.0.clone(),
            label: v.value.1.clone(),
        });
    }
    if let Ok(v) = v.extract::<PyRef<'_, Event>>() {
        return Ok(model::Record::Event(Box::new(v.value.clone())));
    }
    if let Ok(v) = v.extract::<PyRef<'_, Gap>>() {
        return Ok(model::Record::Gap {
            stream: v.value.0.clone(),
            lost: v.value.1,
            reason: v.value.2.clone(),
        });
    }
    if let Ok(v) = v.extract::<PyRef<'_, Discontinuity>>() {
        return Ok(model::Record::Discontinuity {
            stream: v.value.0.clone(),
            reason: v.value.1.clone(),
        });
    }
    if let Ok(v) = v.extract::<PyRef<'_, Metadata>>() {
        return Ok(model::Record::Metadata {
            key: v.value.0.clone(),
            data: v.value.1.clone(),
        });
    }
    if let Ok(v) = v.extract::<PyRef<'_, Opaque>>() {
        return Ok(model::Record::Opaque {
            kind: v.value.0,
            data: v.value.1.clone(),
        });
    }
    Err(PyTypeError::new_err(
        "expected an archive record, Clock or Event",
    ))
}
pub fn to_python(py: Python<'_>, r: model::Record) -> PyResult<Py<PyAny>> {
    match r {
        model::Record::Clock(value) => Ok(Py::new(py, Clock { value })?.into_any()),
        model::Record::Stream { id, label } => {
            Ok(Py::new(py, Stream { value: (id, label) })?.into_any())
        }
        model::Record::Event(value) => Ok(Py::new(py, Event { value: *value })?.into_any()),
        model::Record::Gap {
            stream,
            lost,
            reason,
        } => Ok(Py::new(
            py,
            Gap {
                value: (stream, lost, reason),
            },
        )?
        .into_any()),
        model::Record::Discontinuity { stream, reason } => Ok(Py::new(
            py,
            Discontinuity {
                value: (stream, reason),
            },
        )?
        .into_any()),
        model::Record::Metadata { key, data } => {
            Ok(Py::new(py, Metadata { value: (key, data) })?.into_any())
        }
        model::Record::Opaque { kind, data } => Ok(Py::new(
            py,
            Opaque {
                value: (kind, data),
            },
        )?
        .into_any()),
    }
}
#[pyclass(module = "meedee.archive")]
struct Reader {
    value: model::Reader<IoStream>,
}
#[pymethods]
impl Reader {
    #[new]
    #[pyo3(signature=(source,*,payload_limit=1_048_576,recover=false,preserve_unknown=false))]
    fn new(
        source: &Bound<'_, PyAny>,
        #[pyo3(from_py_with=limit)] payload_limit: usize,
        #[pyo3(from_py_with=crate::ump::boolean)] recover: bool,
        #[pyo3(from_py_with=crate::ump::boolean)] preserve_unknown: bool,
    ) -> PyResult<Self> {
        Ok(Self {
            value: model::Reader::new(
                IoStream::input(source)?,
                payload_limit,
                recover,
                preserve_unknown,
            )
            .map_err(os_error)?,
        })
    }
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.value
            .next()
            .transpose()
            .map_err(os_error)?
            .map(|v| to_python(py, v))
            .transpose()
    }
    #[getter]
    fn complete(&self) -> bool {
        self.value.complete
    }
    #[getter]
    fn problem(&self) -> Option<&str> {
        self.value.problem.as_deref()
    }
}
#[pyclass(module = "meedee.archive")]
struct Writer {
    value: Option<model::Writer<IoStream>>,
}
#[pymethods]
impl Writer {
    #[new]
    #[pyo3(signature=(destination,*,payload_limit=1_048_576))]
    fn new(
        destination: &Bound<'_, PyAny>,
        #[pyo3(from_py_with=limit)] payload_limit: usize,
    ) -> PyResult<Self> {
        Ok(Self {
            value: Some(
                model::Writer::new(IoStream::output(destination)?, payload_limit)
                    .map_err(os_error)?,
            ),
        })
    }
    fn write(&mut self, record: &Bound<'_, PyAny>) -> PyResult<()> {
        self.value
            .as_mut()
            .ok_or_else(closed)?
            .write(&extract(record)?)
            .map_err(os_error)
    }
    #[pyo3(signature=(*,complete=true,reason=""))]
    fn finish(
        &mut self,
        #[pyo3(from_py_with=crate::ump::boolean)] complete: bool,
        reason: &str,
    ) -> PyResult<()> {
        self.value
            .as_mut()
            .ok_or_else(closed)?
            .finish(complete, reason)
            .map_err(os_error)?;
        self.value = None;
        Ok(())
    }
}
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Stream>()?;
    m.add_class::<Gap>()?;
    m.add_class::<Discontinuity>()?;
    m.add_class::<Metadata>()?;
    m.add_class::<Opaque>()?;
    m.add_class::<Reader>()?;
    m.add_class::<Writer>()
}
