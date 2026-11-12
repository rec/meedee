//! Python views of the native MIDI 2-only musical values.
use crate::messages::{integer, Message};
use middy::messages as model;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyInt, PyTuple};
include!("../crates/middy/src/midi2_values.rs");

macro_rules! values {
    ($($name:ident [$first:expr,$second:expr] {$($field:ident:$ty:ty = $lo:literal ..= $hi:literal => [$word:literal,$shift:literal,$mask:literal,$bias:literal]),*};)*) => {
        $(#[pyclass(frozen, extends=Message, module="middy.messages")]
        struct $name { value: model::$name }
        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature=(*,$($field),*))]
            #[allow(clippy::too_many_arguments)]
            fn new($($field: &Bound<'_, PyAny>),*) -> PyResult<PyClassInitializer<Self>> {
                Ok(PyClassInitializer::from(Message::default()).add_subclass(Self {
                    value: model::$name::new($(integer($field, stringify!($field))?),*).map_err(crate::ump::error)?,
                }))
            }
            $(#[getter] fn $field(&self) -> $ty { self.value.$field() })*
            #[getter] fn version(&self) -> u8 { self.value.version() }
            fn transpose(slf: PyRef<'_, Self>, #[pyo3(from_py_with=crate::messages::strict_integer)] semitones: i64) -> PyResult<Py<PyAny>> {
                let py = slf.py();
                let fields: &[&str] = &[$(stringify!($field)),*];
                if fields.contains(&"note") { crate::messages::transpose_note(slf.into_pyobject(py)?.as_any(), semitones) }
                else { Ok(slf.into_pyobject(py)?.into_any().unbind()) }
            }
            fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
                other.extract::<PyRef<'_, Self>>().is_ok_and(|v| self.value == v.value)
            }
            fn __hash__(&self) -> u64 { crate::messages::value_hash(&self.value) }
            fn __repr__(slf: PyRef<'_, Self>) -> PyResult<String> {
                let py = slf.py();
                crate::tuning::render(slf.into_pyobject(py)?.as_any(), stringify!($name), &[$(stringify!($field)),*])
            }
            #[pyo3(signature=(**changes))]
            fn replace<'py>(slf: PyRef<'py, Self>, changes: Option<&Bound<'py, PyDict>>) -> PyResult<Bound<'py, PyAny>> {
                let py = slf.py();
                crate::tuning::replacement(slf.into_pyobject(py)?.as_any(), &[$(stringify!($field)),*], changes)
            }
            #[classattr] fn __match_args__(py: Python<'_>) -> PyResult<Bound<'_, PyTuple>> {
                PyTuple::new(py, &[$(stringify!($field)),*])
            }
            #[classattr] fn __annotations__(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
                let d = PyDict::new(py);
                $(d.set_item(stringify!($field), py.get_type::<PyInt>())?;)*
                d.set_item("version", py.get_type::<PyInt>())?;
                Ok(d)
            }
        })*
        fn extract_plain(v: &Bound<'_, PyAny>) -> Option<model::AnyMessage> {
            $(if let Ok(v) = v.extract::<PyRef<'_, $name>>() { return Some(model::AnyMessage::$name(v.value.clone())); })*
            None
        }
        fn plain_to_python(py: Python<'_>, value: model::AnyMessage) -> PyResult<Py<PyAny>> {
            match value {
                $(model::AnyMessage::$name(value) => Ok(Py::new(py, PyClassInitializer::from(Message::default()).add_subclass($name { value }))?.into_any()),)*
                _ => unreachable!("MIDI 2-only semantic type"),
            }
        }
        fn register_plain(module: &Bound<'_, PyModule>) -> PyResult<()> {
            $(module.add_class::<$name>()?;)*
            Ok(())
        }
    };
}
midi2_values!(values);

fn family(value: &str) -> PyResult<model::ControllerFamily> {
    match value {
        "registered" => Ok(model::ControllerFamily::Registered),
        "assignable" => Ok(model::ControllerFamily::Assignable),
        _ => Err(pyo3::exceptions::PyValueError::new_err(
            "family must be registered or assignable",
        )),
    }
}
fn family_name(value: model::ControllerFamily) -> &'static str {
    match value {
        model::ControllerFamily::Registered => "registered",
        model::ControllerFamily::Assignable => "assignable",
    }
}
macro_rules! family_values {
    ($($name:ident [$first:expr,$second:expr] {$($field:ident:$ty:ty = $lo:literal ..= $hi:literal => [$word:literal,$shift:literal,$mask:literal,$bias:literal]),*};)*) => {
        $(#[pyclass(frozen, extends=Message, module="middy.messages")]
        struct $name { value:model::$name }
        #[pymethods]
        impl $name {
            #[new] #[pyo3(signature=(*,$($field,)*family="registered"))]
            fn new($($field:&Bound<'_,PyAny>,)* family:&str)->PyResult<PyClassInitializer<Self>> {
                let value=model::$name::new_with_family($(integer($field,stringify!($field))?,)*self::family(family)?).map_err(crate::ump::error)?;
                Ok(PyClassInitializer::from(Message::default()).add_subclass(Self{value}))
            }
            $(#[getter] fn $field(&self)->$ty { self.value.$field() })*
            #[getter] fn version(&self)->u8 { 2 }
            #[getter] fn family(&self)->&'static str {family_name(self.value.family())}
            fn transpose(slf:PyRef<'_,Self>,#[pyo3(from_py_with=crate::messages::strict_integer)] semitones:i64)->PyResult<Py<PyAny>> {
                let py=slf.py(); let fields:&[&str]=&[$(stringify!($field)),*];
                if fields.contains(&"note") {crate::messages::transpose_note(slf.into_pyobject(py)?.as_any(),semitones)} else {Ok(slf.into_pyobject(py)?.into_any().unbind())}
            }
            fn __eq__(&self,other:&Bound<'_,PyAny>)->bool {other.extract::<PyRef<'_,Self>>().is_ok_and(|v|self.value==v.value)}
            fn __hash__(&self)->u64 {crate::messages::value_hash(&self.value)}
            fn __repr__(slf:PyRef<'_,Self>)->PyResult<String> {let py=slf.py(); crate::tuning::render(slf.into_pyobject(py)?.as_any(),stringify!($name),&[$(stringify!($field),)*"family"])}
            #[pyo3(signature=(**changes))]
            fn replace<'py>(slf:PyRef<'py,Self>,changes:Option<&Bound<'py,PyDict>>)->PyResult<Bound<'py,PyAny>> {let py=slf.py();crate::tuning::replacement(slf.into_pyobject(py)?.as_any(),&[$(stringify!($field),)*"family"],changes)}
            #[classattr] fn __match_args__(py:Python<'_>)->PyResult<Bound<'_,PyTuple>> {PyTuple::new(py,[$(stringify!($field)),*])}
            #[classattr] fn __annotations__(py:Python<'_>)->PyResult<Bound<'_,PyDict>> {
                let fields=PyDict::new(py); $(fields.set_item(stringify!($field),py.get_type::<PyInt>())?;)*
                fields.set_item("family",py.get_type::<pyo3::types::PyString>())?; fields.set_item("version",py.get_type::<PyInt>())?; Ok(fields)
            }
        })*
        pub(crate) fn extract(value:&Bound<'_,PyAny>)->Option<model::AnyMessage> {
            $(if let Ok(value)=value.extract::<PyRef<'_,$name>>() {return Some(model::AnyMessage::$name(value.value.clone()));})*
            extract_plain(value)
        }
        pub(crate) fn to_python(py:Python<'_>,value:model::AnyMessage)->PyResult<Py<PyAny>> {
            match value {$(model::AnyMessage::$name(value)=>Ok(Py::new(py,PyClassInitializer::from(Message::default()).add_subclass($name{value}))?.into_any()),)*value=>plain_to_python(py,value)}
        }
        pub(crate) fn register(module:&Bound<'_,PyModule>)->PyResult<()> {register_plain(module)?; $(module.add_class::<$name>()?;)*Ok(())}
    };
}
controller_values!(family_values);
