//! Python views of the native MIDI 2-only musical values.
use crate::messages::{integer, Message};
use meedee::messages as model;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyInt, PyTuple};
include!("../crates/meedee/src/midi2_values.rs");

macro_rules! values {
    ($($name:ident [$first:expr,$second:expr] {$($field:ident:$ty:ty = $lo:literal ..= $hi:literal => [$word:literal,$shift:literal,$mask:literal,$bias:literal]),*};)*) => {
        $(#[pyclass(frozen, extends=Message, module="meedee.messages")]
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
        pub(crate) fn extract(v: &Bound<'_, PyAny>) -> Option<model::AnyMessage> {
            $(if let Ok(v) = v.extract::<PyRef<'_, $name>>() { return Some(model::AnyMessage::$name(v.value.clone())); })*
            None
        }
        pub(crate) fn to_python(py: Python<'_>, value: model::AnyMessage) -> PyResult<Py<PyAny>> {
            match value {
                $(model::AnyMessage::$name(value) => Ok(Py::new(py, PyClassInitializer::from(Message::default()).add_subclass($name { value }))?.into_any()),)*
                _ => unreachable!("MIDI 2-only semantic type"),
            }
        }
        pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
            $(module.add_class::<$name>()?;)*
            Ok(())
        }
    };
}
midi2_values!(values);
