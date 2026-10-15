//! Frozen MTS values and pure request/reply tracking. No native MIDI setup.
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use meedee::messages::PrimitiveMessage;
use meedee::tuning as model;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyString, PyTuple};
use pyo3::IntoPyObjectExt;

use crate::messages::{integer, Message};

#[pyclass(frozen, skip_from_py_object, module = "meedee.tuning")]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct NoChange;
#[pymethods]
impl NoChange {
    #[new]
    fn new() -> Self {
        Self
    }
    fn __repr__(&self) -> &'static str {
        "NoChange()"
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other.is_instance_of::<Self>()
    }
    fn __hash__(&self) -> u64 {
        hash(self)
    }
}

// The same field list drives strict keyword constructors, read-only properties,
// checked replacement, annotations and pattern matching for all frozen values.
macro_rules! value_classes {
    ($($name:ident { $($field:ident : $read:ident => $write:ident : $annotation:ty),* } {$($method:item)*};)*) => {$(
        #[pyclass(frozen, module = "meedee.tuning")]
        struct $name { value: model::$name }
        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature = (*, $($field),*))]
            fn new($($field: &Bound<'_, PyAny>),*) -> PyResult<Self> {
                Ok(Self { value: model::$name::new($(self::$read($field)?),*)
                    .map_err(value_error)? })
            }
            $(#[getter]
            fn $field<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
                $write(py, self.value.$field())
            })*
            fn __repr__(slf: PyRef<'_, Self>) -> PyResult<String> {
                let py = slf.py();
                render(slf.into_pyobject(py)?.as_any(), stringify!($name),
                    &[$(stringify!($field)),*])
            }
            fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
                other.extract::<PyRef<'_, Self>>()
                    .is_ok_and(|other| self.value == other.value)
            }
            fn __hash__(&self) -> u64 { hash(&self.value) }
            #[pyo3(signature = (**changes))]
            fn replace<'py>(slf: PyRef<'py, Self>, changes: Option<&Bound<'py, PyDict>>)
                -> PyResult<Bound<'py, PyAny>> {
                let py = slf.py();
                replacement(slf.into_pyobject(py)?.as_any(),
                    &[$(stringify!($field)),*], changes)
            }
            #[classattr]
            fn __match_args__(py: Python<'_>) -> PyResult<Bound<'_, PyTuple>> {
                PyTuple::new(py, [$(stringify!($field)),*])
            }
            #[classattr]
            fn __annotations__(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
                let fields = PyDict::new(py);
                $(fields.set_item(stringify!($field), py.get_type::<$annotation>())?;)*
                Ok(fields)
            }
            $($method)*
        }
    )*};
}
value_classes! {
    Pitch { semitone: int => number: PyInt, fraction: int => number: PyInt } {
        #[getter]
        fn frequency_hz(&self) -> f64 { self.value.frequency_hz() }
        #[staticmethod]
        #[pyo3(signature = (*, frequency_hz))]
        fn from_frequency(frequency_hz: &Bound<'_, PyAny>) -> PyResult<PitchConversion> {
            Ok(PitchConversion { value: model::Pitch::from_frequency(real(frequency_hz)?)
                .map_err(value_error)? })
        }
        #[staticmethod]
        #[pyo3(signature = (*, note, cents))]
        fn from_cents(note: &Bound<'_, PyAny>, cents: &Bound<'_, PyAny>) -> PyResult<PitchConversion> {
            Ok(PitchConversion { value: model::Pitch::from_cents(int(note)?, real(cents)?)
                .map_err(value_error)? })
        }
    };
    TuningTable { values: pitches => pitch_tuple: PyTuple } {};
    NoteTuning { note: int => number: PyInt, pitch: pitch => pitch_object: PyAny } {};
    ScaleOffsets { resolution: int => number: PyInt, values: integers => integer_tuple: PyTuple } {
        #[staticmethod]
        #[pyo3(signature = (*, resolution, cents))]
        fn from_cents(resolution: &Bound<'_, PyAny>, cents: &Bound<'_, PyAny>) -> PyResult<ScaleConversion> {
            let cents = sequence(cents, 12)?.into_iter()
                .map(|value| real(&value)).collect::<PyResult<Vec<_>>>()?;
            Ok(ScaleConversion { value: model::ScaleOffsets::from_cents(int(resolution)?, cents)
                .map_err(value_error)? })
        }
    };
}
#[pyclass(frozen, module = "meedee.tuning")]
struct PitchConversion {
    value: model::PitchConversion,
}
#[pymethods]
impl PitchConversion {
    #[getter]
    fn pitch(&self) -> Pitch {
        Pitch {
            value: self.value.pitch,
        }
    }
    #[getter]
    fn error_cents(&self) -> f64 {
        self.value.error_cents
    }
    fn __repr__(&self) -> String {
        format!(
            "PitchConversion(pitch=Pitch(semitone={}, fraction={}), error_cents={:?})",
            self.value.pitch.semitone(),
            self.value.pitch.fraction(),
            self.value.error_cents
        )
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|other| self.value == other.value)
    }
    fn __hash__(&self) -> u64 {
        hash(&(self.value.pitch, error_bits(self.value.error_cents)))
    }
    #[classattr]
    fn __match_args__() -> (&'static str, &'static str) {
        ("pitch", "error_cents")
    }
    #[classattr]
    fn __annotations__(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
        let fields = PyDict::new(py);
        fields.set_item("pitch", py.get_type::<Pitch>())?;
        fields.set_item("error_cents", py.get_type::<PyFloat>())?;
        Ok(fields)
    }
}
#[pyclass(frozen, module = "meedee.tuning")]
struct ScaleConversion {
    value: model::ScaleConversion,
}
#[pymethods]
impl ScaleConversion {
    #[getter]
    fn offsets(&self) -> ScaleOffsets {
        ScaleOffsets {
            value: self.value.offsets.clone(),
        }
    }
    #[getter]
    fn error_cents<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, self.value.error_cents)
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "ScaleConversion(offsets={}, error_cents={})",
            offsets_object(py, &self.value.offsets)?.repr()?,
            self.error_cents(py)?.repr()?
        ))
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|other| self.value == other.value)
    }
    fn __hash__(&self) -> u64 {
        hash(&(&self.value.offsets, self.value.error_cents.map(error_bits)))
    }
    #[classattr]
    fn __match_args__() -> (&'static str, &'static str) {
        ("offsets", "error_cents")
    }
    #[classattr]
    fn __annotations__(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
        let fields = PyDict::new(py);
        fields.set_item("offsets", py.get_type::<ScaleOffsets>())?;
        fields.set_item("error_cents", py.get_type::<PyTuple>())?;
        Ok(fields)
    }
}

macro_rules! message_classes {
    ($($name:ident { $($field:ident : $read:ident => $write:ident : $annotation:ty),* };)*) => {
        $(
            #[pyclass(frozen, extends = Message, module = "meedee.tuning")]
            struct $name { value: model::$name }
            #[pymethods]
            impl $name {
                #[new]
                #[pyo3(signature = (*, $($field),*))]
                fn new($($field: &Bound<'_, PyAny>),*) -> PyResult<PyClassInitializer<Self>> {
                    Ok(PyClassInitializer::from(Message).add_subclass(Self {
                        value: model::$name::new($(self::$read($field)?),*)
                            .map_err(value_error)?,
                    }))
                }
                $(#[getter]
                fn $field<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
                    $write(py, self.value.$field())
                })*
                fn __repr__(slf: PyRef<'_, Self>) -> PyResult<String> {
                    let py = slf.py();
                    render(slf.into_pyobject(py)?.as_any(), stringify!($name),
                        &[$(stringify!($field)),*])
                }
                fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
                    other.extract::<PyRef<'_, Self>>()
                        .is_ok_and(|other| self.value == other.value)
                }
                fn __hash__(&self) -> u64 { hash(&self.value) }
                #[pyo3(signature = (**changes))]
                fn replace<'py>(slf: PyRef<'py, Self>, changes: Option<&Bound<'py, PyDict>>)
                    -> PyResult<Bound<'py, PyAny>> {
                    let py = slf.py();
                    replacement(slf.into_pyobject(py)?.as_any(),
                        &[$(stringify!($field)),*], changes)
                }
                #[classattr]
                fn __match_args__(py: Python<'_>) -> PyResult<Bound<'_, PyTuple>> {
                    PyTuple::new(py, [$(stringify!($field)),*])
                }
                #[classattr]
                fn __annotations__(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
                    let fields = PyDict::new(py);
                    $(fields.set_item(stringify!($field), py.get_type::<$annotation>())?;)*
                    Ok(fields)
                }
            }
        )*
        pub fn encode_message(message: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
            $(if let Ok(value) = message.extract::<PyRef<'_, $name>>() {
                return Ok(value.value.encode());
            })*
            Err(PyTypeError::new_err("message must be a supported MIDI 1.0 Message"))
        }
        pub fn is_message(message: &Bound<'_, PyAny>) -> bool {
            false $(|| message.is_instance_of::<$name>())*
        }
        fn extract_message(message: &Bound<'_, PyAny>) -> PyResult<model::TuningMessage> {
            $(if let Ok(value) = message.extract::<PyRef<'_, $name>>() {
                return Ok(value.value.clone().into());
            })*
            Err(PyTypeError::new_err("message must be a typed MTS message"))
        }
        fn message_object(py: Python<'_>, message: model::TuningMessage) -> PyResult<Py<PyAny>> {
            match message {
                $(model::TuningMessage::$name(value) => Ok(Py::new(py,
                    PyClassInitializer::from(Message).add_subclass($name { value }))?.into_any())),*
            }
        }
        fn register_messages(module: &Bound<'_, PyModule>) -> PyResult<()> {
            $(module.add_class::<$name>()?;)*
            Ok(())
        }
    };
}
message_classes! {
    BulkDumpRequest {device_id:int=>number:PyInt,program:int=>number:PyInt};
    BankedDumpRequest {device_id:int=>number:PyInt,bank:int=>number:PyInt,program:int=>number:PyInt};
    BulkDump {device_id:int=>number:PyInt,program:int=>number:PyInt,name:text=>string:PyString,table:table=>table_object:TuningTable};
    KeyBasedDump {device_id:int=>number:PyInt,bank:int=>number:PyInt,program:int=>number:PyInt,name:text=>string:PyString,table:table=>table_object:TuningTable};
    ScaleOctaveDump {device_id:int=>number:PyInt,bank:int=>number:PyInt,program:int=>number:PyInt,name:text=>string:PyString,offsets:offsets=>offsets_object:ScaleOffsets};
    SingleNoteChange {device_id:int=>number:PyInt,program:int=>number:PyInt,changes:changes=>changes_tuple:PyTuple};
    BankedNoteChange {device_id:int=>number:PyInt,bank:int=>number:PyInt,program:int=>number:PyInt,realtime:flag=>boolean:PyBool,changes:changes=>changes_tuple:PyTuple};
    ScaleOctaveChange {device_id:int=>number:PyInt,realtime:flag=>boolean:PyBool,channels:channels=>integer_tuple:PyTuple,offsets:offsets=>offsets_object:ScaleOffsets};
}
#[pyclass(eq, frozen, from_py_object, module = "meedee.tuning")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChecksumStatus {
    NotPresent,
    Matches,
    LegacyMismatch,
}
#[pyclass(frozen, module = "meedee.tuning")]
struct Decoded {
    value: model::Decoded,
}
#[pymethods]
impl Decoded {
    #[getter]
    fn message(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        message_object(py, self.value.message.clone())
    }
    #[getter]
    fn checksum(&self) -> ChecksumStatus {
        match self.value.checksum {
            model::ChecksumStatus::NotPresent => ChecksumStatus::NotPresent,
            model::ChecksumStatus::Matches => ChecksumStatus::Matches,
            model::ChecksumStatus::LegacyMismatch => ChecksumStatus::LegacyMismatch,
        }
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|other| self.value == other.value)
    }
    fn __hash__(&self) -> u64 {
        hash(&self.value)
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Decoded(message={}, checksum=ChecksumStatus.{:?})",
            self.message(py)?.bind(py).repr()?,
            self.checksum()
        ))
    }
}
#[pyfunction]
fn decode(message: &Bound<'_, PyAny>) -> PyResult<Option<Decoded>> {
    let message = message.extract::<PyRef<'_, crate::messages::SysEx>>()?;
    Ok(model::decode(&message.value)
        .map_err(value_error)?
        .map(|value| Decoded { value }))
}
#[pyclass(eq, frozen, from_py_object, module = "meedee.tuning")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RequestStatus {
    Pending,
    Completed,
    TimedOut,
    Cancelled,
}
impl From<model::RequestStatus> for RequestStatus {
    fn from(value: model::RequestStatus) -> Self {
        match value {
            model::RequestStatus::Pending => Self::Pending,
            model::RequestStatus::Completed => Self::Completed,
            model::RequestStatus::TimedOut => Self::TimedOut,
            model::RequestStatus::Cancelled => Self::Cancelled,
        }
    }
}
#[pyclass(module = "meedee.tuning")]
struct ReplyTracker {
    value: model::ReplyTracker,
}
#[pymethods]
impl ReplyTracker {
    #[new]
    #[pyo3(signature=(*,request,now_ns,timeout_ns))]
    fn new(
        request: &Bound<'_, PyAny>,
        now_ns: &Bound<'_, PyAny>,
        timeout_ns: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        Ok(Self {
            value: model::ReplyTracker::new(
                &extract_message(request)?,
                nanoseconds(now_ns)?,
                nanoseconds(timeout_ns)?,
            )
            .map_err(value_error)?,
        })
    }
    #[getter]
    fn status(&self) -> RequestStatus {
        self.value.status().into()
    }
    #[getter]
    fn deadline_ns(&self) -> u64 {
        self.value.deadline_ns()
    }
    #[pyo3(signature=(*,now_ns))]
    fn poll(&mut self, now_ns: &Bound<'_, PyAny>) -> PyResult<RequestStatus> {
        Ok(self
            .value
            .poll(nanoseconds(now_ns)?)
            .map_err(value_error)?
            .into())
    }
    fn cancel(&mut self) -> RequestStatus {
        self.value.cancel().into()
    }
    #[pyo3(signature=(message,*,now_ns))]
    fn observe(&mut self, message: &Bound<'_, PyAny>, now_ns: &Bound<'_, PyAny>) -> PyResult<bool> {
        self.value
            .observe(&extract_message(message)?, nanoseconds(now_ns)?)
            .map_err(value_error)
    }
}
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_messages(module)?;
    module.add_class::<Pitch>()?;
    module.add_class::<NoChange>()?;
    module.add_class::<TuningTable>()?;
    module.add_class::<NoteTuning>()?;
    module.add_class::<ScaleOffsets>()?;
    module.add_class::<PitchConversion>()?;
    module.add_class::<ScaleConversion>()?;
    module.add_class::<ChecksumStatus>()?;
    module.add_class::<Decoded>()?;
    module.add_class::<RequestStatus>()?;
    module.add_class::<ReplyTracker>()?;
    module.add_function(wrap_pyfunction!(decode, module)?)?;
    Ok(())
}
fn value_error(error: meedee::messages::ValueError) -> PyErr {
    PyValueError::new_err(error.to_string())
}
pub(crate) fn hash(value: &impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
fn error_bits(value: f64) -> u64 {
    // Equal positive/negative zero must hash identically.
    if value == 0.0 {
        0
    } else {
        value.to_bits()
    }
}
fn int(value: &Bound<'_, PyAny>) -> PyResult<i64> {
    integer(value, "value")
}
fn real(value: &Bound<'_, PyAny>) -> PyResult<f64> {
    if !value.is_exact_instance_of::<PyFloat>() && !value.is_exact_instance_of::<PyInt>() {
        return Err(PyTypeError::new_err(
            "value must be a float or int, not bool or a coercible object",
        ));
    }
    value.extract()
}
fn nanoseconds(value: &Bound<'_, PyAny>) -> PyResult<u64> {
    if !value.is_exact_instance_of::<PyInt>() {
        return Err(PyTypeError::new_err("nanoseconds must be an int"));
    }
    value
        .extract::<u64>()
        .map_err(|_| PyValueError::new_err("nanoseconds must be in 0..=2**64-1"))
}
fn flag(value: &Bound<'_, PyAny>) -> PyResult<bool> {
    if !value.is_exact_instance_of::<PyBool>() {
        return Err(PyTypeError::new_err("realtime must be a bool"));
    }
    value.extract()
}
fn text(value: &Bound<'_, PyAny>) -> PyResult<String> {
    value.extract()
}
fn sequence<'py>(value: &Bound<'py, PyAny>, limit: usize) -> PyResult<Vec<Bound<'py, PyAny>>> {
    // Snapshot only bounded list/tuple inputs; never consume an unbounded iterator.
    if !value.is_exact_instance_of::<PyTuple>()
        && !value.is_exact_instance_of::<pyo3::types::PyList>()
    {
        return Err(PyTypeError::new_err("expected a built-in list or tuple"));
    }
    if value.len()? > limit {
        return Err(PyValueError::new_err("too many values"));
    }
    value.try_iter()?.collect()
}
fn pitch(value: &Bound<'_, PyAny>) -> PyResult<model::TuningValue> {
    if let Ok(value) = value.extract::<PyRef<'_, Pitch>>() {
        Ok(model::TuningValue::Pitch(value.value))
    } else if value.is_instance_of::<NoChange>() {
        Ok(model::TuningValue::NoChange)
    } else {
        Err(PyTypeError::new_err("pitch must be Pitch or NoChange"))
    }
}
fn pitches(value: &Bound<'_, PyAny>) -> PyResult<Vec<model::TuningValue>> {
    sequence(value, 128)?.iter().map(pitch).collect()
}
fn integers(value: &Bound<'_, PyAny>) -> PyResult<Vec<i64>> {
    sequence(value, 12)?.iter().map(int).collect()
}
fn channels(value: &Bound<'_, PyAny>) -> PyResult<Vec<i64>> {
    sequence(value, 16)?.iter().map(int).collect()
}
fn table(value: &Bound<'_, PyAny>) -> PyResult<model::TuningTable> {
    Ok(value.extract::<PyRef<'_, TuningTable>>()?.value.clone())
}
fn offsets(value: &Bound<'_, PyAny>) -> PyResult<model::ScaleOffsets> {
    Ok(value.extract::<PyRef<'_, ScaleOffsets>>()?.value.clone())
}
fn changes(value: &Bound<'_, PyAny>) -> PyResult<Vec<model::NoteTuning>> {
    sequence(value, 127)?
        .into_iter()
        .map(|value| Ok(value.extract::<PyRef<'_, NoteTuning>>()?.value.clone()))
        .collect()
}
fn number<T: for<'py> IntoPyObject<'py>>(py: Python<'_>, value: T) -> PyResult<Bound<'_, PyAny>> {
    value.into_bound_py_any(py)
}
fn boolean(py: Python<'_>, value: bool) -> PyResult<Bound<'_, PyAny>> {
    Ok(PyBool::new(py, value).to_owned().into_any())
}
fn string<'py>(py: Python<'py>, value: &str) -> PyResult<Bound<'py, PyAny>> {
    Ok(PyString::new(py, value).into_any())
}
fn pitch_object(py: Python<'_>, value: model::TuningValue) -> PyResult<Bound<'_, PyAny>> {
    match value {
        model::TuningValue::Pitch(value) => {
            Ok(Py::new(py, Pitch { value })?.into_bound(py).into_any())
        }
        model::TuningValue::NoChange => Ok(Py::new(py, NoChange)?.into_bound(py).into_any()),
    }
}
fn pitch_tuple<'py>(
    py: Python<'py>,
    values: &[model::TuningValue; 128],
) -> PyResult<Bound<'py, PyAny>> {
    let values = values
        .iter()
        .map(|value| pitch_object(py, *value))
        .collect::<PyResult<Vec<_>>>()?;
    Ok(PyTuple::new(py, values)?.into_any())
}
fn integer_tuple<'py, T: Copy + for<'p> IntoPyObject<'p>>(
    py: Python<'py>,
    values: &[T],
) -> PyResult<Bound<'py, PyAny>> {
    let values = values
        .iter()
        .map(|value| number(py, *value))
        .collect::<PyResult<Vec<_>>>()?;
    Ok(PyTuple::new(py, values)?.into_any())
}
fn table_object<'py>(py: Python<'py>, value: &model::TuningTable) -> PyResult<Bound<'py, PyAny>> {
    Ok(Py::new(
        py,
        TuningTable {
            value: value.clone(),
        },
    )?
    .into_bound(py)
    .into_any())
}
fn offsets_object<'py>(
    py: Python<'py>,
    value: &model::ScaleOffsets,
) -> PyResult<Bound<'py, PyAny>> {
    Ok(Py::new(
        py,
        ScaleOffsets {
            value: value.clone(),
        },
    )?
    .into_bound(py)
    .into_any())
}
fn changes_tuple<'py>(
    py: Python<'py>,
    values: &[model::NoteTuning],
) -> PyResult<Bound<'py, PyAny>> {
    let values = values
        .iter()
        .map(|value| {
            Py::new(
                py,
                NoteTuning {
                    value: value.clone(),
                },
            )
        })
        .collect::<PyResult<Vec<_>>>()?;
    Ok(PyTuple::new(py, values)?.into_any())
}
pub(crate) fn render(object: &Bound<'_, PyAny>, name: &str, fields: &[&str]) -> PyResult<String> {
    let values = fields
        .iter()
        .map(|field| Ok(format!("{field}={}", object.getattr(*field)?.repr()?)))
        .collect::<PyResult<Vec<_>>>()?;
    Ok(format!("{name}({})", values.join(", ")))
}
pub(crate) fn replacement<'py>(
    object: &Bound<'py, PyAny>,
    fields: &[&str],
    changes: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyAny>> {
    let values = PyDict::new(object.py());
    for field in fields {
        values.set_item(*field, object.getattr(*field)?)?;
    }
    if let Some(changes) = changes {
        for (name, value) in changes {
            let name = name.extract::<String>()?;
            if !fields.contains(&name.as_str()) {
                return Err(PyTypeError::new_err(format!("unknown field: {name}")));
            }
            values.set_item(name, value)?;
        }
    }
    object.get_type().call((), Some(&values))
}

pub(crate) fn decoded_object(py: Python<'_>, value: model::Decoded) -> PyResult<Py<PyAny>> {
    Ok(Py::new(py, Decoded { value })?.into_any())
}
