use meedee::transport;
use pyo3::prelude::*;

use crate::packets::Backend;

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
    #[getter]
    fn _wire(&self) -> (String, u64, usize, String, String, u64, bool) {
        (
            self.owner.clone(),
            self.snapshot,
            self.index,
            self.name.clone(),
            format!("{:?}", self.backend),
            self.backend_id,
            self.input,
        )
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
