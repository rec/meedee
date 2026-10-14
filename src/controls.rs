//! Frozen observations and a pure, per-source control interpreter.
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use meedee::controls;
use pyo3::prelude::*;
use pyo3::types::PyTuple;

macro_rules! enums {
    ($($name:ident { $($variant:ident),* };)*) => {
        $(
            #[pyclass(eq, frozen, from_py_object, module = "meedee.controls")]
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            enum $name { $($variant),* }
            impl From<controls::$name> for $name {
                fn from(value: controls::$name) -> Self {
                    match value { $(controls::$name::$variant => Self::$variant),* }
                }
            }
        )*
        fn register_enums(module: &Bound<'_, PyModule>) -> PyResult<()> {
            $(module.add_class::<$name>()?;)*
            Ok(())
        }
    };
}

enums! {
    ParameterFamily { Rpn, Nrpn };
    StepDirection { Increment, Decrement };
    DiagnosticKind { MissingCoarse, MissingSelection, IncompleteSelection, SelectionInvalidated, StateReset };
}

macro_rules! observations {
    ($($name:ident { $($field:ident: $ty:ty),* };)*) => {
        $(
            #[pyclass(frozen, module = "meedee.controls")]
            struct $name { value: controls::$name }
            #[pymethods]
            impl $name {
                $(#[getter] fn $field(&self) -> $ty { self.value.$field.into() })*
                fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
                    let fields = vec![$(format!("{}={}", stringify!($field), self.$field().into_pyobject(py)?.as_any().repr()?)),*];
                    Ok(format!("{}({})", stringify!($name), fields.join(", ")))
                }
                fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
                    other.extract::<PyRef<'_, Self>>().is_ok_and(|other| self.value == other.value)
                }
                fn __hash__(&self) -> u64 {
                    let mut hasher = DefaultHasher::new();
                    self.value.hash(&mut hasher);
                    hasher.finish()
                }
                #[classattr]
                fn __match_args__(py: Python<'_>) -> PyResult<Bound<'_, PyTuple>> {
                    PyTuple::new(py, [$(stringify!($field)),*])
                }
            }
        )*
        fn register_observations(module: &Bound<'_, PyModule>) -> PyResult<()> {
            $(module.add_class::<$name>()?;)*
            Ok(())
        }
    };
}

observations! {
    ControlObservation { channel: u8, controller: u8, value: u16, resolution: u8, first_index: usize, last_index: usize };
    ParameterObservation { channel: u8, family: ParameterFamily, parameter: u16, value: u16, resolution: u8, first_index: usize, last_index: usize };
    ParameterStep { channel: u8, family: ParameterFamily, parameter: u16, direction: StepDirection, first_index: usize, last_index: usize };
    Diagnostic { kind: DiagnosticKind, channel: Option<u8>, controller: Option<u8>, first_index: usize, last_index: usize };
}

#[pyclass(module = "meedee.controls")]
struct Interpreter {
    value: controls::Interpreter,
}

#[pymethods]
impl Interpreter {
    #[new]
    fn new() -> Self {
        Self {
            value: controls::Interpreter::new(),
        }
    }
    fn feed(&mut self, py: Python<'_>, message: &Bound<'_, PyAny>) -> PyResult<Vec<Py<PyAny>>> {
        let message = crate::messages::control_input(message)?;
        to_python(py, self.value.feed(message))
    }
    fn reset(&mut self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        to_python(py, self.value.reset())
    }
}

fn to_python(py: Python<'_>, observations: Vec<controls::Observation>) -> PyResult<Vec<Py<PyAny>>> {
    observations
        .into_iter()
        .map(|observation| match observation {
            controls::Observation::Control(value) => {
                Ok(Py::new(py, ControlObservation { value })?.into_any())
            }
            controls::Observation::Parameter(value) => {
                Ok(Py::new(py, ParameterObservation { value })?.into_any())
            }
            controls::Observation::Step(value) => {
                Ok(Py::new(py, ParameterStep { value })?.into_any())
            }
            controls::Observation::Diagnostic(value) => {
                Ok(Py::new(py, Diagnostic { value })?.into_any())
            }
        })
        .collect()
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_enums(module)?;
    register_observations(module)?;
    module.add_class::<Interpreter>()?;
    Ok(())
}
