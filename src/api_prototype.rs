//! Device-free representation prototype. Promote these values once in slice 2;
//! this module is deliberately absent from production wheels.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use meedee::messages;
use pyo3::buffer::PyBuffer;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyInt};

#[pyclass(frozen, subclass, module = "meedee.messages")]
struct Message;

#[pymethods]
impl Message {
    #[new]
    fn new() -> PyResult<Self> {
        Err(PyTypeError::new_err("Message is abstract"))
    }
}

#[pyclass(frozen, extends = Message, module = "meedee.messages")]
struct NoteOn {
    value: messages::NoteOn,
}

#[pymethods]
impl NoteOn {
    #[new]
    #[pyo3(signature = (*, channel, note, velocity))]
    fn new(
        channel: &Bound<'_, PyAny>,
        note: &Bound<'_, PyAny>,
        velocity: &Bound<'_, PyAny>,
    ) -> PyResult<PyClassInitializer<Self>> {
        Ok(PyClassInitializer::from(Message).add_subclass(Self {
            value: messages::NoteOn::new(
                integer(channel, "channel")?,
                integer(note, "note")?,
                integer(velocity, "velocity")?,
            )
            .map_err(value_error)?,
        }))
    }

    #[getter]
    fn channel(&self) -> u8 {
        self.value.channel()
    }

    #[getter]
    fn note(&self) -> u8 {
        self.value.note()
    }

    #[getter]
    fn velocity(&self) -> u8 {
        self.value.velocity()
    }

    fn __repr__(&self) -> String {
        format!(
            "NoteOn(channel={}, note={}, velocity={})",
            self.value.channel(),
            self.value.note(),
            self.value.velocity()
        )
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|other| self.value == other.value))
    }

    fn __hash__(&self) -> u64 {
        value_hash(&self.value)
    }

    #[pyo3(signature = (**changes))]
    fn replace(&self, py: Python<'_>, changes: Option<&Bound<'_, PyDict>>) -> PyResult<Py<Self>> {
        let mut channel = i64::from(self.value.channel());
        let mut note = i64::from(self.value.note());
        let mut velocity = i64::from(self.value.velocity());
        if let Some(changes) = changes {
            for (name, value) in changes.iter() {
                match name.extract::<String>()?.as_str() {
                    "channel" => channel = integer(&value, "channel")?,
                    "note" => note = integer(&value, "note")?,
                    "velocity" => velocity = integer(&value, "velocity")?,
                    name => return Err(PyTypeError::new_err(format!("unknown field: {name}"))),
                }
            }
        }
        Py::new(
            py,
            PyClassInitializer::from(Message).add_subclass(Self {
                value: messages::NoteOn::new(channel, note, velocity).map_err(value_error)?,
            }),
        )
    }

    #[classattr]
    fn __annotations__(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
        let annotations = PyDict::new(py);
        for name in ["channel", "note", "velocity"] {
            annotations.set_item(name, py.get_type::<PyInt>())?;
        }
        Ok(annotations)
    }

    #[classattr]
    fn __match_args__() -> (&'static str, &'static str, &'static str) {
        ("channel", "note", "velocity")
    }
}

#[pyclass(frozen, extends = Message, module = "meedee.messages")]
struct SysEx {
    value: messages::SysEx,
}

#[pymethods]
impl SysEx {
    #[new]
    #[pyo3(signature = (*, data))]
    fn new(data: &Bound<'_, PyAny>) -> PyResult<PyClassInitializer<Self>> {
        Ok(PyClassInitializer::from(Message).add_subclass(Self {
            value: payload(data)?,
        }))
    }

    #[getter]
    fn data<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, self.value.data())
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!("SysEx(data={})", self.data(py).repr()?))
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|other| self.value == other.value))
    }

    fn __hash__(&self) -> u64 {
        value_hash(&self.value)
    }

    #[pyo3(signature = (**changes))]
    fn replace(&self, py: Python<'_>, changes: Option<&Bound<'_, PyDict>>) -> PyResult<Py<Self>> {
        let mut value = self.value.clone();
        if let Some(changes) = changes {
            for (name, data) in changes.iter() {
                if name.extract::<String>()? != "data" {
                    return Err(PyTypeError::new_err(format!("unknown field: {name}")));
                }
                value = payload(&data)?;
            }
        }
        Py::new(
            py,
            PyClassInitializer::from(Message).add_subclass(Self { value }),
        )
    }

    #[classattr]
    fn __annotations__(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
        let annotations = PyDict::new(py);
        annotations.set_item("data", py.get_type::<PyBytes>())?;
        Ok(annotations)
    }

    #[classattr]
    fn __match_args__() -> (&'static str,) {
        ("data",)
    }
}

fn integer(value: &Bound<'_, PyAny>, field: &str) -> PyResult<i64> {
    if !value.get_type().is(value.py().get_type::<PyInt>()) {
        return Err(PyTypeError::new_err(format!("{field} must be an int")));
    }
    value
        .extract::<i64>()
        .map_err(|_| PyValueError::new_err(format!("{field} is out of range")))
}

fn payload(data: &Bound<'_, PyAny>) -> PyResult<messages::SysEx> {
    let buffer = PyBuffer::<u8>::get(data)?;
    if buffer.dimensions() != 1 || !buffer.is_c_contiguous() {
        return Err(PyTypeError::new_err(
            "data must be a one-dimensional contiguous unsigned-byte buffer",
        ));
    }
    messages::SysEx::new(buffer.to_vec(data.py())?).map_err(value_error)
}

fn value_error(error: messages::ValueError) -> PyErr {
    PyValueError::new_err(error.to_string())
}

fn value_hash(value: &impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn python_representation_contract_and_measurements() {
    Python::initialize();
    Python::attach(|py| {
        let module = PyModule::new(py, "meedee.messages").unwrap();
        module.add_class::<Message>().unwrap();
        module.add_class::<NoteOn>().unwrap();
        module.add_class::<SysEx>().unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("test/native");
        PyModule::import(py, "sys")
            .unwrap()
            .getattr("path")
            .unwrap()
            .call_method1("insert", (0, path.to_str().unwrap()))
            .unwrap();
        let fixture = PyModule::import(py, "meedee_messages").unwrap();
        fixture
            .getattr("verify")
            .unwrap()
            .call1((&module,))
            .map_err(|error| error.display(py))
            .unwrap();
        let results = fixture
            .getattr("measure")
            .unwrap()
            .call1((&module,))
            .unwrap();
        println!("prototype: {results}");
        println!(
            "Rust value sizes: NoteOn={}, SysEx={}",
            std::mem::size_of::<messages::NoteOn>(),
            std::mem::size_of::<messages::SysEx>()
        );
    });
}
