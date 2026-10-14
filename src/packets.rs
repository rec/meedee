//! Immutable Python transport values; no native client initialization.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use meedee::transport;
use pyo3::buffer::PyBuffer;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyInt, PyList, PyTuple};

#[pyclass(eq, frozen, from_py_object, module = "meedee.midi")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Protocol {
    Midi1,
    Ump,
}

#[pyclass(eq, frozen, from_py_object, module = "meedee.midi")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Backend {
    CoreMidi,
    CoreMidiUmp,
    AlsaSeq,
    AlsaSeqUmp,
    AlsaRaw,
    AlsaRawUmp,
    WinMm,
    WindowsMidiServices,
}

#[pyclass(eq, frozen, from_py_object, module = "meedee.midi")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimestampMode {
    Absolute,
    Off,
}

#[pyclass(frozen, skip_from_py_object, module = "meedee.midi")]
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct UmpPacket {
    pub words: Vec<u32>,
}

#[pymethods]
impl UmpPacket {
    #[new]
    #[pyo3(signature = (*, words))]
    fn new(words: &Bound<'_, PyAny>) -> PyResult<Self> {
        if !words.is_instance_of::<PyTuple>() && !words.is_instance_of::<PyList>() {
            return Err(PyTypeError::new_err(
                "words must be a list or tuple of integers",
            ));
        }
        let mut owned = Vec::new();
        for word in words.try_iter()? {
            let word = word?;
            if !word.is_exact_instance_of::<PyInt>() {
                return Err(PyTypeError::new_err("UMP words must be built-in integers"));
            }
            owned.push(
                word.extract::<u32>()
                    .map_err(|_| PyValueError::new_err("UMP words must be in 0..4294967295"))?,
            );
        }
        Ok(Self { words: owned })
    }
    #[getter]
    fn words<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, &self.words)
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!("UmpPacket(words={})", self.words(py)?.repr()?))
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|other| self.words == other.words)
    }
    fn __hash__(&self) -> u64 {
        value_hash(self)
    }
}

#[pyclass(frozen, skip_from_py_object, module = "meedee.events")]
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Clock {
    #[pyo3(get)]
    pub identity: String,
    #[pyo3(get)]
    pub backend: Backend,
    #[pyo3(get)]
    pub mode: TimestampMode,
}

#[pymethods]
impl Clock {
    #[new]
    #[pyo3(signature = (*, identity, backend, mode))]
    fn new(identity: String, backend: Backend, mode: TimestampMode) -> PyResult<Self> {
        if identity.is_empty() {
            return Err(PyValueError::new_err("clock identity must not be empty"));
        }
        Ok(Self {
            identity,
            backend,
            mode,
        })
    }
    #[getter]
    fn tick_numerator(&self) -> u32 {
        1
    }
    #[getter]
    fn tick_denominator(&self) -> u32 {
        1_000_000_000
    }
    #[getter]
    fn resolution_ns(&self) -> Option<u64> {
        None
    }
    fn __repr__(&self) -> String {
        format!(
            "Clock(identity={:?}, backend=Backend.{:?}, mode=TimestampMode.{:?})",
            self.identity, self.backend, self.mode
        )
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|other| *self == *other)
    }
    fn __hash__(&self) -> u64 {
        value_hash(self)
    }
}

#[pyclass(frozen, skip_from_py_object, module = "meedee.events")]
#[derive(Clone)]
pub struct ReceivedPacket {
    #[pyo3(get)]
    pub timestamp_ns: Option<i64>,
    pub clock: Clock,
    pub packet: transport::Packet,
}

#[pymethods]
impl ReceivedPacket {
    #[new]
    #[pyo3(signature = (*, timestamp_ns, clock, packet))]
    fn new(
        timestamp_ns: &Bound<'_, PyAny>,
        clock: PyRef<'_, Clock>,
        packet: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        let timestamp_ns = if timestamp_ns.is_none() {
            None
        } else {
            if !timestamp_ns.is_exact_instance_of::<PyInt>() {
                return Err(PyTypeError::new_err(
                    "timestamp_ns must be a built-in integer or None",
                ));
            }
            Some(timestamp_ns.extract::<i64>().map_err(|_| {
                PyValueError::new_err("timestamp_ns must fit a signed 64-bit integer")
            })?)
        };
        if clock.mode == TimestampMode::Off && timestamp_ns.is_some() {
            return Err(PyValueError::new_err("disabled timestamps must be None"));
        }
        Ok(Self {
            timestamp_ns,
            clock: clock.clone(),
            packet: extract_packet(packet)?,
        })
    }
    #[getter]
    fn clock(&self) -> Clock {
        self.clock.clone()
    }
    #[getter]
    fn packet(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match &self.packet {
            transport::Packet::Bytes(bytes) => Ok(PyBytes::new(py, bytes).into_any().unbind()),
            transport::Packet::Ump(words) => Ok(Py::new(
                py,
                UmpPacket {
                    words: words.clone(),
                },
            )?
            .into_any()),
        }
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "ReceivedPacket(timestamp_ns={}, clock={}, packet={})",
            self.timestamp_ns
                .map_or_else(|| "None".into(), |n| n.to_string()),
            self.clock.__repr__(),
            self.packet(py)?.bind(py).repr()?
        ))
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other.extract::<PyRef<'_, Self>>().is_ok_and(|other| {
            self.timestamp_ns == other.timestamp_ns
                && self.clock == other.clock
                && self.packet == other.packet
        })
    }
    fn __hash__(&self) -> u64 {
        let mut hash = DefaultHasher::new();
        self.timestamp_ns.hash(&mut hash);
        self.clock.hash(&mut hash);
        match &self.packet {
            transport::Packet::Bytes(b) => {
                0_u8.hash(&mut hash);
                b.hash(&mut hash);
            }
            transport::Packet::Ump(w) => {
                1_u8.hash(&mut hash);
                w.hash(&mut hash);
            }
        }
        hash.finish()
    }
}

pub fn extract_packet(value: &Bound<'_, PyAny>) -> PyResult<transport::Packet> {
    if let Ok(packet) = value.extract::<PyRef<'_, UmpPacket>>() {
        return Ok(transport::Packet::Ump(packet.words.clone()));
    }
    let buffer = PyBuffer::<u8>::get(value)?;
    if buffer.dimensions() != 1 || buffer.item_size() != 1 {
        return Err(PyTypeError::new_err(
            "packet must be a one-dimensional unsigned-byte buffer or UmpPacket",
        ));
    }
    Ok(transport::Packet::Bytes(buffer.to_vec(value.py())?))
}

impl Protocol {
    pub fn native(self) -> transport::Protocol {
        match self {
            Self::Midi1 => transport::Protocol::Midi1,
            Self::Ump => transport::Protocol::Ump,
        }
    }
}
impl TimestampMode {
    pub fn native(self) -> transport::TimestampMode {
        match self {
            Self::Absolute => transport::TimestampMode::Absolute,
            Self::Off => transport::TimestampMode::Off,
        }
    }
}
impl Backend {
    pub fn native(self) -> transport::Backend {
        match self {
            Self::CoreMidi => transport::Backend::CoreMidi,
            Self::CoreMidiUmp => transport::Backend::CoreMidiUmp,
            Self::AlsaSeq => transport::Backend::AlsaSeq,
            Self::AlsaSeqUmp => transport::Backend::AlsaSeqUmp,
            Self::AlsaRaw => transport::Backend::AlsaRaw,
            Self::AlsaRawUmp => transport::Backend::AlsaRawUmp,
            Self::WinMm => transport::Backend::WinMm,
            Self::WindowsMidiServices => transport::Backend::WindowsMidiServices,
        }
    }
    pub fn from_native(backend: transport::Backend) -> Self {
        match backend {
            transport::Backend::CoreMidi => Self::CoreMidi,
            transport::Backend::CoreMidiUmp => Self::CoreMidiUmp,
            transport::Backend::AlsaSeq => Self::AlsaSeq,
            transport::Backend::AlsaSeqUmp => Self::AlsaSeqUmp,
            transport::Backend::AlsaRaw => Self::AlsaRaw,
            transport::Backend::AlsaRawUmp => Self::AlsaRawUmp,
            transport::Backend::WinMm => Self::WinMm,
            transport::Backend::WindowsMidiServices => Self::WindowsMidiServices,
        }
    }
}
pub fn register_transport(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Protocol>()?;
    module.add_class::<Backend>()?;
    module.add_class::<TimestampMode>()?;
    module.add_class::<UmpPacket>()?;
    module.add_function(wrap_pyfunction!(compiled_backends, module)?)?;
    Ok(())
}

#[pyfunction]
fn compiled_backends() -> PyResult<Vec<Backend>> {
    Ok(transport::compiled_backends()
        .map_err(|e| crate::errors::transport_error(e, "init"))?
        .into_iter()
        .map(Backend::from_native)
        .collect())
}

#[pyfunction]
pub fn prepare_send(
    py: Python<'_>,
    protocol: Protocol,
    packet: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let packet = extract_packet(packet)?;
    transport::validate_packet(protocol.native(), &packet)
        .map_err(|e| crate::errors::transport_error(e, "send"))?;
    match packet {
        transport::Packet::Bytes(bytes) => Ok(PyBytes::new(py, &bytes).into_any().unbind()),
        transport::Packet::Ump(words) => Ok(Py::new(py, UmpPacket { words })?.into_any()),
    }
}
pub fn register_events(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Clock>()?;
    module.add_class::<ReceivedPacket>()?;
    Ok(())
}
fn value_hash(value: &impl Hash) -> u64 {
    let mut hash = DefaultHasher::new();
    value.hash(&mut hash);
    hash.finish()
}
