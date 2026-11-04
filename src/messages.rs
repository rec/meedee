//! Frozen Python message classes backed by the shared Rust toolkit.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use meedee::messages;
use meedee::messages::PrimitiveMessage;
use pyo3::buffer::PyBuffer;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyInt, PyTuple};

#[pyclass(frozen, subclass, module = "meedee.messages")]
#[derive(Default)]
pub(crate) struct Message {
    metadata: std::sync::OnceLock<Metadata>,
}
#[derive(Clone, Default)]
pub(crate) struct Metadata {
    pub timestamp: Option<meedee::events::Timestamp>,
    pub stream: Option<String>,
    pub span: Option<meedee::events::SourceSpan>,
}
impl Message {
    pub(crate) fn metadata(&self) -> Metadata {
        self.metadata.get().cloned().unwrap_or_default()
    }
    pub(crate) fn attach(&self, metadata: Metadata) -> PyResult<()> {
        if metadata.stream.as_ref().is_some_and(|s| s.is_empty()) {
            return Err(PyValueError::new_err("empty stream identity"));
        }
        if let Some(span) = &metadata.span {
            span.validate(metadata.timestamp.as_ref())
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
        }
        self.metadata
            .set(metadata)
            .map_err(|_| PyValueError::new_err("message metadata is already set"))
    }
}

#[pymethods]
impl Message {
    #[getter]
    fn timestamp(&self) -> Option<crate::event_values::Timestamp> {
        self.metadata()
            .timestamp
            .map(|value| crate::event_values::Timestamp { value })
    }
    #[getter]
    fn stream(&self) -> Option<String> {
        self.metadata().stream
    }
    #[getter]
    fn span(&self) -> Option<crate::event_values::SourceSpan> {
        self.metadata()
            .span
            .map(|value| crate::event_values::SourceSpan { value })
    }
    #[new]
    fn new() -> PyResult<Self> {
        Err(PyTypeError::new_err("Message is abstract"))
    }
}

#[pyclass(frozen, extends = Message, module = "meedee.messages")]
pub(crate) struct SysEx {
    pub(crate) value: messages::SysEx,
}

#[pymethods]
impl SysEx {
    #[new]
    #[pyo3(signature = (data))]
    fn new(data: &Bound<'_, PyAny>) -> PyResult<PyClassInitializer<Self>> {
        Ok(
            PyClassInitializer::from(Message::default()).add_subclass(Self {
                value: payload(data)?,
            }),
        )
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
    fn replace<'py>(
        slf: PyRef<'py, Self>,
        changes: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let py = slf.py();
        crate::tuning::replacement(slf.into_pyobject(py)?.as_any(), &["data"], changes)
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

macro_rules! channel_value {
    (NoteOn, $note:expr, $velocity:expr) => { messages::NoteOn::new($note).and_then(|m| m.with_velocity($velocity)) };
    (NoteOff, $note:expr, $velocity:expr) => { messages::NoteOff::new($note).and_then(|m| m.with_velocity($velocity)) };
    (PolyPressure, $note:expr, $pressure:expr) => { messages::PolyPressure::new($note).and_then(|m| m.with_pressure($pressure)) };
    (ChannelPressure, $pressure:expr) => { messages::ChannelPressure::new().and_then(|m| m.with_pressure($pressure)) };
    (PitchBend, $value:expr) => { messages::PitchBend::new().and_then(|m| m.with_value($value)) };
    (RpnSet, $parameter:expr, $value:expr, $resolution:expr) => { messages::RpnSet::new($parameter,$value).and_then(|m| m.with_resolution($resolution)) };
    (NrpnSet, $parameter:expr, $value:expr, $resolution:expr) => { messages::NrpnSet::new($parameter,$value).and_then(|m| m.with_resolution($resolution)) };
    ($name:ident, $($field:expr),*) => { messages::$name::new($($field),*) };
}
macro_rules! wire_field {
    ($value:ident, channel) => {
        i64::from($value.channel().expect("resolved command channel"))
    };
    ($value:ident, $field:ident) => {
        i64::from($value.$field())
    };
}

macro_rules! channel_message {
    ($name:ident $signature:tt { $($field:ident),* } $(; $($method:item)*)?) => {
        #[pyclass(frozen, extends = Message, module = "meedee.messages")]
        struct $name { value: messages::$name }
        impl $name {
            fn from_value(value: messages::$name) -> Self { Self { value } }
            fn resolved(&self) -> PyResult<&messages::$name> {
                if self.value.channel().is_none() { return Err(PyValueError::new_err("message channel is unassigned")); }
                Ok(&self.value)
            }
        }
        #[pymethods]
        impl $name {
            $($($method)*)?
            #[new]
            #[pyo3(signature = $signature)]
            fn new($(#[pyo3(from_py_with=strict_integer)] $field: i64,)* channel: Option<&Bound<'_, PyAny>>) -> PyResult<PyClassInitializer<Self>> {
                let channel = channel.map(|v| integer(v, "channel")).transpose()?;
                Ok(PyClassInitializer::from(Message::default()).add_subclass(Self {
                    value: {
                        let value = channel_value!($name, $($field),*).map_err(value_error)?;
                        if let Some(channel) = channel { value.with_channel(channel).map_err(value_error)? } else { value }
                    },
                }))
            }
            #[getter]
            fn channel(&self) -> Option<i64> { self.value.channel().map(i64::from) }
            $(#[getter] fn $field(&self) -> i64 { i64::from(self.value.$field()) })*
            fn __repr__(&self) -> String {
                let fields = vec![$(format!("{}={}", stringify!($field), self.value.$field())),*];
                format!("{}({}, channel={})", stringify!($name), fields.join(", "), self.channel().map_or_else(|| "None".into(), |v| v.to_string()))
            }
            fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
                other.extract::<PyRef<'_, Self>>().is_ok_and(|v| self.value == v.value)
            }
            fn __hash__(&self) -> u64 { value_hash(&self.value) }
            #[pyo3(signature=(**changes))]
            fn replace<'py>(slf: PyRef<'py, Self>, changes: Option<&Bound<'py, PyDict>>) -> PyResult<Bound<'py, PyAny>> {
        let py = slf.py();
        crate::tuning::replacement(slf.into_pyobject(py)?.as_any(), &["channel", $(stringify!($field)),*], changes)
    }
            #[classattr]
            fn __annotations__(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
                let annotations = PyDict::new(py);
                $(annotations.set_item(stringify!($field), py.get_type::<PyInt>())?;)*
                annotations.set_item("channel", py.import("typing")?.getattr("Optional")?.get_item(py.get_type::<PyInt>())?)?;
                Ok(annotations)
            }
            #[classattr]
            fn __match_args__(py: Python<'_>) -> PyResult<Bound<'_, PyTuple>> { PyTuple::new(py, [$(stringify!($field),)* "channel"]) }
        }
    };
}

macro_rules! note_message {
    ($name:ident $signature:tt) => {
        channel_message! { $name $signature { note, velocity };
            fn transpose(slf: PyRef<'_, Self>, #[pyo3(from_py_with=strict_integer)] semitones: i64) -> PyResult<Py<PyAny>> {
                let py = slf.py();
                let changes = PyDict::new(py);
                changes.set_item("note", i64::from(slf.value.note()).saturating_add(semitones).clamp(0,127))?;
                Ok(crate::tuning::replacement(slf.into_pyobject(py)?.as_any(), &["note", "velocity", "channel"], Some(&changes))?.unbind())
            }
        }
    };
}

macro_rules! message_classes {
    (NoteOn $signature:tt { channel, note, velocity }; $($rest:tt)*) => {
        note_message! { NoteOn $signature }
        message_classes! { $($rest)* }
    };
    (NoteOff $signature:tt { channel, note, velocity }; $($rest:tt)*) => {
        note_message! { NoteOff $signature }
        message_classes! { $($rest)* }
    };
    ($name:ident $signature:tt { channel, $($field:ident),* }; $($rest:tt)*) => {
        channel_message! { $name $signature { $($field),* } }
        message_classes! { $($rest)* }
    };
    ($name:ident $signature:tt { $($field:ident),* }; $($rest:tt)*) => {
            #[pyclass(frozen, extends = Message, module = "meedee.messages")]
            struct $name { value: messages::$name }
            impl $name {
                fn from_value(value: messages::$name) -> Self { Self { value } }
                fn resolved(&self) -> PyResult<&messages::$name> { Ok(&self.value) }
            }

            #[pymethods]
            impl $name {
                #[new]
                #[pyo3(signature = $signature)]
                fn new($($field: &Bound<'_, PyAny>),*) -> PyResult<PyClassInitializer<Self>> {
                    Ok(PyClassInitializer::from(Message::default()).add_subclass(Self {
                        value: messages::$name::new($(integer($field, stringify!($field))?),*)
                            .map_err(value_error)?,
                    }))
                }
                $(#[getter]
                fn $field(&self) -> i64 { i64::from(self.value.$field()) })*

                fn __repr__(&self) -> String {
                    let fields: Vec<String> = vec![$(format!("{}={}", stringify!($field), self.value.$field())),*];
                    format!("{}({})", stringify!($name), fields.join(", "))
                }
                fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
                    other.extract::<PyRef<'_, Self>>().is_ok_and(|other| self.value == other.value)
                }
                fn __hash__(&self) -> u64 { value_hash(&self.value) }

                #[pyo3(signature = (**changes))]
                fn replace<'py>(slf: PyRef<'py, Self>, changes: Option<&Bound<'py, PyDict>>) -> PyResult<Bound<'py, PyAny>> {
        let py = slf.py();
        crate::tuning::replacement(slf.into_pyobject(py)?.as_any(), &[$(stringify!($field)),*], changes)
    }
                #[classattr]
                fn __annotations__(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
                    let annotations = PyDict::new(py);
                    $(annotations.set_item(stringify!($field), py.get_type::<PyInt>())?;)*
                    Ok(annotations)
                }
                #[classattr]
                fn __match_args__(py: Python<'_>) -> PyResult<Bound<'_, PyTuple>> {
                    let names: &[&str] = &[$(stringify!($field)),*];
                    PyTuple::new(py, names)
                }
            }
        message_classes! { $($rest)* }
    };
    () => {};
}

macro_rules! short_messages {
    ($($name:ident $signature:tt { $($field:ident),* };)*) => {
        message_classes! { $($name $signature { $($field),* };)* }

        fn register_short(module: &Bound<'_, PyModule>) -> PyResult<()> {
            $(module.add_class::<$name>()?;)*
            Ok(())
        }

        pub fn encode_message(message: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
            if let Ok(message) = message.extract::<PyRef<'_, SysEx>>() {
                return message.value.encode().map_err(value_error);
            }
            $(if let Ok(message) = message.extract::<PyRef<'_, $name>>() {
                return message.resolved()?.encode().map_err(value_error);
            })*
            if let Some(bytes) = crate::profiles::encode_message(message) { return Ok(bytes); }
            if let Some(bytes) = crate::ci::encode_message(message) { return Ok(bytes); }
            crate::tuning::encode_message(message)
        }

        pub(crate) fn extract_primitive(message: &Bound<'_, PyAny>) -> PyResult<messages::AnyMessage> {
            if let Ok(value) = message.extract::<PyRef<'_, SysEx>>() {return Ok(messages::AnyMessage::SysEx(value.value.clone()));}
            $(if let Ok(value) = message.extract::<PyRef<'_, $name>>() {return Ok(messages::AnyMessage::$name(value.resolved()?.clone()));})*
            Err(PyTypeError::new_err("expected a primitive MIDI 1.0 message"))
        }

        pub fn to_python(py: Python<'_>, message: messages::AnyMessage) -> PyResult<Py<PyAny>> {
            match message {
                messages::AnyMessage::SysEx(value) => Ok(Py::new(py,
                    PyClassInitializer::from(Message::default()).add_subclass(SysEx { value }))?.into_any()),
                $(messages::AnyMessage::$name(value) => Ok(Py::new(py,
                    PyClassInitializer::from(Message::default()).add_subclass($name::from_value(value)))?.into_any()),)*
            }
        }

        pub fn control_input(message: &Bound<'_, PyAny>) -> PyResult<meedee::controls::InputMessage> {
            use meedee::controls::InputMessage;
            if let Ok(message) = message.extract::<PyRef<'_, ControlChange>>() {
                return Ok(InputMessage::ControlChange(message.resolved()?.clone()));
            }
            if message.is_instance_of::<SystemReset>() || crate::profiles::changes_mode(message) {
                return Ok(InputMessage::SystemReset);
            }
            if message.is_instance_of::<NoteOn>() || message.is_instance_of::<SysEx>()
                || crate::tuning::is_message(message)
                || crate::profiles::is_message(message)
                $(|| message.is_instance_of::<$name>())* {
                return Ok(InputMessage::Other);
            }
            Err(PyTypeError::new_err("interpreter input must be a primitive MIDI 1.0 message"))
        }
    };
}

short_messages! {
    NoteOn (note, velocity=90, channel=None) { channel, note, velocity };
    NoteOff (note, velocity=0, channel=None) { channel, note, velocity };
    PolyPressure (note, pressure=0, channel=None) { channel, note, pressure };
    ControlChange (controller, value, channel=None) { channel, controller, value };
    ProgramChange (program, channel=None) { channel, program };
    ChannelPressure (pressure=0, channel=None) { channel, pressure };
    PitchBend (value=0, channel=None) { channel, value };
    QuarterFrame ( message_type, value) { message_type, value };
    SongPosition ( position) { position };
    SongSelect ( song) { song };
    TuneRequest () {};
    TimingClock () {};
    Start () {};
    Continue () {};
    Stop () {};
    ActiveSensing () {};
    SystemReset () {};
}

macro_rules! command_messages {
    ($($name:ident $signature:tt { $($field:ident),* };)*) => {
        message_classes! { $($name $signature { $($field),* };)* }

        fn register_commands(module: &Bound<'_, PyModule>) -> PyResult<()> {
            module.add_class::<ParameterStep>()?;
            $(module.add_class::<$name>()?;)*
            Ok(())
        }
        pub fn extract_command(command: &Bound<'_, PyAny>) -> PyResult<messages::AnyCommand> {
            if let Ok(command) = command.extract::<PyRef<'_, ParameterStep>>() {
                if command.value.channel().is_none() { return Err(PyValueError::new_err("message channel is unassigned")); }
                return Ok(messages::AnyCommand::ParameterStep(command.value.clone()));
            }
            $(if let Ok(command) = command.extract::<PyRef<'_, $name>>() {
                return Ok(messages::AnyCommand::$name(command.resolved()?.clone()));
            })*
            crate::profiles::extract_command(command)?
                .ok_or_else(|| PyTypeError::new_err("command must be a supported compound command"))
        }
        pub fn command_wire(py: Python<'_>, command: &messages::AnyCommand) -> PyResult<(String, Py<PyDict>)> {
            let kwargs = PyDict::new(py);
            if let messages::AnyCommand::ParameterStep(value) = command {
                let value = ParameterStep { value: value.clone() };
                kwargs.set_item("parameter", value.parameter())?;
                kwargs.set_item("direction", value.direction())?;
                kwargs.set_item("channel", value.channel())?;
                kwargs.set_item("family", value.family())?;
                return Ok(("ParameterStep".into(), kwargs.unbind()));
            }
            let (name, fields): (String, std::collections::HashMap<String, i64>) = match command {
                $(messages::AnyCommand::$name(value) => (stringify!($name).into(),
                    [$( (stringify!($field).into(), wire_field!(value, $field)) ),*].into_iter().collect()),)*
                _ => crate::profiles::command_wire(command),
            };
            for (key, value) in fields { kwargs.set_item(key, value)?; }
            Ok((name, kwargs.unbind()))
        }
    };
}

command_messages! {
    ControlChange14 (controller, value, channel=None) { channel, controller, value };
    RpnSet (parameter, value, resolution=14, channel=None) { channel, parameter, value, resolution };
    NrpnSet (parameter, value, resolution=14, channel=None) { channel, parameter, value, resolution };
}

#[pyclass(frozen, extends = Message, module = "meedee.messages")]
struct ParameterStep {
    value: messages::ParameterStep,
}
#[pymethods]
impl ParameterStep {
    #[new]
    #[pyo3(signature=(parameter, direction=1, channel=None, *, family="rpn"))]
    fn new(
        #[pyo3(from_py_with=strict_integer)] parameter: i64,
        #[pyo3(from_py_with=strict_integer)] direction: i64,
        channel: Option<&Bound<'_, PyAny>>,
        family: &str,
    ) -> PyResult<PyClassInitializer<Self>> {
        use meedee::controls::{ParameterFamily, StepDirection};
        let family = match family {
            "rpn" => ParameterFamily::Rpn,
            "nrpn" => ParameterFamily::Nrpn,
            _ => return Err(PyValueError::new_err("family must be rpn or nrpn")),
        };
        let direction = match direction {
            1 => StepDirection::Increment,
            -1 => StepDirection::Decrement,
            _ => return Err(PyValueError::new_err("direction must be 1 or -1")),
        };
        let mut value = messages::ParameterStep::new(0)
            .and_then(|v| v.with_family(family))
            .and_then(|v| v.with_parameter(parameter))
            .map_err(value_error)?
            .with_direction(direction);
        if let Some(channel) = channel {
            value = value
                .with_channel(integer(channel, "channel")?)
                .map_err(value_error)?;
        }
        Ok(PyClassInitializer::from(Message::default()).add_subclass(Self { value }))
    }
    #[getter]
    fn parameter(&self) -> u16 {
        self.value.parameter()
    }
    #[getter]
    fn direction(&self) -> i8 {
        match self.value.direction() {
            meedee::controls::StepDirection::Increment => 1,
            meedee::controls::StepDirection::Decrement => -1,
        }
    }
    #[getter]
    fn family(&self) -> &'static str {
        match self.value.family() {
            meedee::controls::ParameterFamily::Rpn => "rpn",
            meedee::controls::ParameterFamily::Nrpn => "nrpn",
        }
    }
    #[getter]
    fn channel(&self) -> Option<u8> {
        self.value.channel()
    }
    fn __repr__(&self) -> String {
        format!(
            "ParameterStep(parameter={}, direction={}, channel={}, family={:?})",
            self.parameter(),
            self.direction(),
            self.channel()
                .map_or_else(|| "None".into(), |v| v.to_string()),
            self.family()
        )
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|v| self.value == v.value)
    }
    fn __hash__(&self) -> u64 {
        value_hash(&self.value)
    }
    #[pyo3(signature=(**changes))]
    fn replace<'py>(
        slf: PyRef<'py, Self>,
        changes: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let py = slf.py();
        crate::tuning::replacement(
            slf.into_pyobject(py)?.as_any(),
            &["parameter", "direction", "channel", "family"],
            changes,
        )
    }
    #[classattr]
    fn __annotations__(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
        let annotations = PyDict::new(py);
        annotations.set_item("parameter", py.get_type::<PyInt>())?;
        annotations.set_item("direction", py.get_type::<PyInt>())?;
        annotations.set_item(
            "channel",
            py.import("typing")?
                .getattr("Optional")?
                .get_item(py.get_type::<PyInt>())?,
        )?;
        annotations.set_item("family", py.get_type::<pyo3::types::PyString>())?;
        Ok(annotations)
    }
    #[classattr]
    fn __match_args__() -> (&'static str, &'static str, &'static str) {
        ("parameter", "direction", "channel")
    }
}

pub(crate) fn rpn_to_python(py: Python<'_>, value: messages::RpnSet) -> PyResult<Py<PyAny>> {
    Ok(Py::new(
        py,
        PyClassInitializer::from(Message::default()).add_subclass(RpnSet::from_value(value)),
    )?
    .into_any())
}

pub fn integer(value: &Bound<'_, PyAny>, field: &str) -> PyResult<i64> {
    let value = crate::units::field(value, field)?;
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

/// A performed note: the existing NoteOn value plus an authored duration.
#[pyclass(frozen, module = "meedee.messages")]
struct Note {
    duration: Py<PyAny>,
    attack: Py<NoteOn>,
}
#[pymethods]
impl Note {
    #[new]
    #[pyo3(signature=(duration, note, velocity=90, channel=None))]
    fn new(
        py: Python<'_>,
        duration: &Bound<'_, PyAny>,
        #[pyo3(from_py_with=strict_integer)] note: i64,
        #[pyo3(from_py_with=strict_integer)] velocity: i64,
        channel: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        validate_duration(duration)?;
        Ok(Self {
            duration: duration.clone().unbind(),
            attack: Py::new(py, NoteOn::new(note, velocity, channel)?)?,
        })
    }
    #[getter]
    fn duration(&self, py: Python<'_>) -> Py<PyAny> {
        self.duration.clone_ref(py)
    }
    #[getter]
    fn note(&self, py: Python<'_>) -> i64 {
        self.attack.borrow(py).note()
    }
    #[getter]
    fn velocity(&self, py: Python<'_>) -> i64 {
        self.attack.borrow(py).velocity()
    }
    #[getter]
    fn channel(&self, py: Python<'_>) -> Option<i64> {
        self.attack.borrow(py).channel()
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Note(duration={}, note={}, velocity={}, channel={})",
            self.duration.bind(py).repr()?,
            self.note(py),
            self.velocity(py),
            self.channel(py)
                .map_or_else(|| "None".into(), |v| v.to_string())
        ))
    }
}

#[pyclass(frozen, module = "meedee.messages")]
struct Rest {
    duration: Py<PyAny>,
}
#[pymethods]
impl Rest {
    #[new]
    #[pyo3(signature=(duration))]
    fn new(duration: &Bound<'_, PyAny>) -> PyResult<Self> {
        validate_duration(duration)?;
        Ok(Self {
            duration: duration.clone().unbind(),
        })
    }
    #[getter]
    fn duration(&self, py: Python<'_>) -> Py<PyAny> {
        self.duration.clone_ref(py)
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!("Rest(duration={})", self.duration.bind(py).repr()?))
    }
}

fn validate_duration(value: &Bound<'_, PyAny>) -> PyResult<()> {
    value
        .py()
        .import("meedee._units")?
        .getattr("musical_duration")?
        .call1((value,))?;
    Ok(())
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Message>()?;
    module.add_class::<Note>()?;
    module.add_class::<Rest>()?;
    module.add_class::<SysEx>()?;
    register_short(module)?;
    register_commands(module)?;
    Ok(())
}

#[cfg(test)]
#[test]
fn python_representation_contract_and_measurements() {
    Python::initialize();
    Python::attach(|py| {
        let module = PyModule::new(py, "meedee.messages").unwrap();
        register(&module).unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test/native");
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

fn strict_integer(value: &Bound<'_, PyAny>) -> PyResult<i64> {
    integer(value, "value")
}
