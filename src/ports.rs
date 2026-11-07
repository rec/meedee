use meedee::transport;
use pyo3::prelude::*;

use crate::packets::Backend;

#[pyfunction]
#[pyo3(signature=(ports, pattern, direction))]
pub(crate) fn select_port(
    py: Python<'_>,
    ports: Vec<Py<Port>>,
    pattern: Option<&str>,
    direction: &str,
) -> PyResult<Py<Port>> {
    let names: Vec<_> = ports.iter().map(|p| p.borrow(py).name.clone()).collect();
    let index = meedee::midi::select_port_index(names.iter().map(String::as_str), pattern)
        .map_err(|e| {
            if pattern.is_none() {
                crate::errors::ConnectError::new_err(format!("No MIDI {direction} ports available"))
            } else {
                pyo3::exceptions::PyValueError::new_err(format!("{e}; expected one {direction}"))
            }
        })?;
    Ok(ports.into_iter().nth(index).expect("selected port index"))
}

#[pyclass(frozen, module = "meedee.midi")]
pub struct Port {
    pub native: Option<transport::Port>,
    pub owner: String,
    pub snapshot: u64,
    pub index: usize,
    #[pyo3(get)]
    pub name: String,
    #[pyo3(get)]
    pub backend: Backend,
    #[pyo3(get)]
    pub backend_id: u64,
    pub input: bool,
}

#[pymethods]
impl Port {
    #[staticmethod]
    fn _from_wire(
        owner: String,
        snapshot: u64,
        index: usize,
        name: String,
        backend: Backend,
        backend_id: u64,
        input: bool,
    ) -> Self {
        Self {
            native: None,
            owner,
            snapshot,
            index,
            name,
            backend,
            backend_id,
            input,
        }
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other.extract::<PyRef<'_, Self>>().is_ok_and(|other| {
            self.owner == other.owner
                && self.snapshot == other.snapshot
                && self.index == other.index
                && self.input == other.input
        })
    }
    fn __repr__(&self) -> String {
        format!(
            "Port(name={:?}, backend=Backend.{:?}, backend_id={})",
            self.name, self.backend, self.backend_id
        )
    }
    #[classattr]
    const __hash__: Option<Py<PyAny>> = None;
}

impl Port {
    pub fn new(
        native: transport::Port,
        owner: &str,
        snapshot: u64,
        index: usize,
        input: bool,
    ) -> Self {
        Self {
            name: native.name().into(),
            backend: Backend::from_native(native.backend()),
            backend_id: native.backend_id(),
            native: Some(native),
            owner: owner.into(),
            snapshot,
            index,
            input,
        }
    }
}
