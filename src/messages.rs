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
    /// Return this message unchanged; note-addressed subclasses shift their note number.
    fn transpose(
        slf: PyRef<'_, Self>,
        #[pyo3(from_py_with = strict_integer)] semitones: i64,
    ) -> PyResult<Py<PyAny>> {
        let _ = semitones;
        let py = slf.py();
        Ok(slf.into_pyobject(py)?.into_any().unbind())
    }
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
    #[pyo3(signature = (data, *, group=None))]
    fn new(
        data: &Bound<'_, PyAny>,
        group: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let mut value = payload(data)?;
        if let Some(g) = group {
            value = value
                .with_group(integer(g, "group")?)
                .map_err(value_error)?;
        }
        Ok(PyClassInitializer::from(Message::default()).add_subclass(Self { value }))
    }

    #[getter]
    fn data<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, self.value.data())
    }
    #[getter]
    fn group(&self) -> Option<u8> {
        self.value.group()
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let suffix = self
            .value
            .group()
            .map_or(String::new(), |g| format!(", group={g}"));
        Ok(format!("SysEx(data={}{})", self.data(py).repr()?, suffix))
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
        crate::tuning::replacement(slf.into_pyobject(py)?.as_any(), &["data", "group"], changes)
    }

    #[classattr]
    fn __annotations__(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
        let annotations = PyDict::new(py);
        annotations.set_item("data", py.get_type::<PyBytes>())?;
        annotations.set_item(
            "group",
            py.import("typing")?
                .getattr("Optional")?
                .get_item(py.get_type::<PyInt>())?,
        )?;
        Ok(annotations)
    }

    #[classattr]
    fn __match_args__() -> (&'static str,) {
        ("data",)
    }
}

macro_rules! versioned_message {
    ($name:ident $signature:tt { $($required:ident),* } { $($optional:ident => $setter:ident),* } { $($extra:ident => $extra_setter:ident),* } $(; $($method:item)*)?) => {
        #[pyclass(frozen, extends=Message, module="meedee.messages")]
        struct $name { value: messages::$name }
        impl $name {
            fn from_value(value: messages::$name) -> Self { Self { value } }
            fn resolved(&self) -> PyResult<&messages::$name> {
                if self.value.version() != 1 { return Err(PyValueError::new_err("expected MIDI 1; convert explicitly")); }
                if self.value.channel().is_none() { return Err(PyValueError::new_err("message channel is unassigned")); }
                Ok(&self.value)
            }
        }
        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature=$signature)]
            #[allow(clippy::too_many_arguments)]
            fn new($($required: &Bound<'_, PyAny>,)* $($optional: Option<&Bound<'_, PyAny>>,)* channel: Option<&Bound<'_, PyAny>>, #[pyo3(from_py_with=strict_integer)] version: i64, group: Option<&Bound<'_, PyAny>>, $($extra: Option<&Bound<'_, PyAny>>),*) -> PyResult<PyClassInitializer<Self>> {
                let mut value = match version {
                    1 => messages::$name::new($(integer($required,stringify!($required))?),*),
                    2 => messages::$name::new_midi2($(integer($required,stringify!($required))?),*),
                    _ => return Err(PyValueError::new_err("version must be 1 or 2")),
                }.map_err(value_error)?;
                $(if let Some(v) = $optional { value = value.$setter(integer(v,stringify!($optional))?).map_err(value_error)?; })*
                if let Some(v) = channel { value = value.with_channel(integer(v,"channel")?).map_err(value_error)?; }
                if let Some(v) = group { value = value.with_group(integer(v,"group")?).map_err(value_error)?; }
                $(if let Some(v) = $extra { value = value.$extra_setter(integer(v,stringify!($extra))?).map_err(value_error)?; })*
                Ok(PyClassInitializer::from(Message::default()).add_subclass(Self { value }))
            }
            #[getter] fn channel(&self) -> Option<u8> { self.value.channel() }
            #[getter] fn version(&self) -> u8 { self.value.version() }
            #[getter] fn group(&self) -> Option<u8> { self.value.group() }
            $(#[getter] fn $required(&self) -> i64 { i64::from(self.value.$required()) })*
            $(#[getter] fn $optional(&self) -> i64 { i64::from(self.value.$optional()) })*
            $(#[getter] fn $extra(&self) -> Option<i64> { self.value.$extra().map(i64::from) })*
            fn __eq__(&self, other:&Bound<'_,PyAny>) -> bool { other.extract::<PyRef<'_,Self>>().is_ok_and(|v| self.value==v.value) }
            fn __hash__(&self) -> u64 { value_hash(&self.value) }
            fn __repr__(slf:PyRef<'_,Self>) -> PyResult<String> {
                let py=slf.py();
                let mut fields = if slf.value.version()==1 { vec![$(stringify!($required),)* $(stringify!($optional),)* "channel"] } else { vec![$(stringify!($required),)* $(stringify!($optional),)* "channel", "version", "group", $(stringify!($extra)),*] };
                if slf.value.version()==1 && slf.value.group().is_some() { fields.push("group"); }
                crate::tuning::render(slf.into_pyobject(py)?.as_any(), stringify!($name), &fields)
            }
            #[pyo3(signature=(**changes))]
            fn replace<'py>(slf:PyRef<'py,Self>, changes:Option<&Bound<'py,PyDict>>) -> PyResult<Bound<'py,PyAny>> {
                if let Some(v) = changes.map(|c| c.get_item("version")).transpose()?.flatten() {
                    if integer(&v,"version")? != i64::from(slf.value.version()) { return Err(PyValueError::new_err("version cannot change through replace; convert explicitly")); }
                }
                let py=slf.py();
                crate::tuning::replacement(slf.into_pyobject(py)?.as_any(), &[$(stringify!($required),)* $(stringify!($optional),)* "channel", "version", "group", $(stringify!($extra)),*],changes)
            }
            #[classattr] fn __annotations__(py:Python<'_>) -> PyResult<Bound<'_,PyDict>> {
                let d=PyDict::new(py);
                for field in [$(stringify!($required),)* $(stringify!($optional),)* "version"] { d.set_item(field,py.get_type::<PyInt>())?; }
                for field in ["channel", "group", $(stringify!($extra)),*] { d.set_item(field,py.import("typing")?.getattr("Optional")?.get_item(py.get_type::<PyInt>())?)?; }
                Ok(d)
            }
            #[classattr] fn __match_args__(py:Python<'_>) -> PyResult<Bound<'_,PyTuple>> { PyTuple::new(py,[$(stringify!($required),)* $(stringify!($optional),)* "channel"]) }
            $($($method)*)?
        }
    };
}
pub(crate) fn transpose_note<'py>(
    value: &Bound<'py, PyAny>,
    semitones: i64,
) -> PyResult<Py<PyAny>> {
    let changes = PyDict::new(value.py());
    let note: u8 = value.getattr("note")?.extract()?;
    changes.set_item(
        "note",
        i64::from(note).saturating_add(semitones).clamp(0, 127),
    )?;
    Ok(value.call_method("replace", (), Some(&changes))?.unbind())
}

macro_rules! versioned_note {
    ($name:ident) => {
        versioned_message! { $name (note, velocity=None, channel=None, *, version=1, group=None, attribute_type=None, attribute_data=None) {note} {velocity => with_velocity} {attribute_type => with_attribute_type, attribute_data => with_attribute_data};
            fn transpose(slf:PyRef<'_,Self>, #[pyo3(from_py_with=strict_integer)] semitones:i64) -> PyResult<Py<PyAny>> {
                let py=slf.py();
                transpose_note(slf.into_pyobject(py)?.as_any(), semitones)
            }
        }
    };
}
versioned_note!(NoteOn);
versioned_note!(NoteOff);
versioned_message! { PolyPressure (note, pressure=None, channel=None, *, version=1, group=None) {note} {pressure => with_pressure} {};
    fn transpose(slf:PyRef<'_,Self>, #[pyo3(from_py_with=strict_integer)] semitones:i64) -> PyResult<Py<PyAny>> {
        let py=slf.py();
        transpose_note(slf.into_pyobject(py)?.as_any(), semitones)
    }
}
#[pyclass(frozen, extends = Message, module = "meedee.messages")]
pub(crate) struct ControlChange {
    value: messages::ControlChange,
}
impl ControlChange {
    fn from_value(value: messages::ControlChange) -> Self {
        Self { value }
    }
    fn resolved(&self) -> PyResult<&messages::ControlChange> {
        if self.value.version() != 1 {
            return Err(PyValueError::new_err("expected MIDI 1; convert explicitly"));
        }
        if self.value.channel().is_none() {
            return Err(PyValueError::new_err("message channel is unassigned"));
        }
        if self.value.resolution() == 14 {
            return Err(PyValueError::new_err(
                "14-bit ControlChange requires expansion into two messages",
            ));
        }
        Ok(&self.value)
    }
}
#[pymethods]
impl ControlChange {
    #[new]
    #[pyo3(signature=(controller, value, channel=None, *, version=1, group=None, resolution=None))]
    fn new(
        controller: &Bound<'_, PyAny>,
        value: &Bound<'_, PyAny>,
        channel: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with=strict_integer)] version: i64,
        group: Option<&Bound<'_, PyAny>>,
        resolution: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let controller = integer(controller, "controller")?;
        let number = integer(value, "value")?;
        let resolution = resolution.map(|v| integer(v, "resolution")).transpose()?;
        let mut value = match version {
            1 => messages::ControlChange::new_with_resolution(
                controller,
                number,
                resolution.unwrap_or(7),
            ),
            2 => messages::ControlChange::new_midi2(controller, number)
                .and_then(|v| v.with_resolution(resolution.unwrap_or(32))),
            _ => return Err(PyValueError::new_err("version must be 1 or 2")),
        }
        .map_err(value_error)?;
        if let Some(v) = channel {
            value = value
                .with_channel(integer(v, "channel")?)
                .map_err(value_error)?;
        }
        if let Some(v) = group {
            value = value
                .with_group(integer(v, "group")?)
                .map_err(value_error)?;
        }
        Ok(PyClassInitializer::from(Message::default()).add_subclass(Self { value }))
    }
    #[getter]
    fn controller(&self) -> u8 {
        self.value.controller()
    }
    #[getter]
    fn value(&self) -> u32 {
        self.value.value()
    }
    #[getter]
    fn channel(&self) -> Option<u8> {
        self.value.channel()
    }
    #[getter]
    fn version(&self) -> u8 {
        self.value.version()
    }
    #[getter]
    fn group(&self) -> Option<u8> {
        self.value.group()
    }
    #[getter]
    fn resolution(&self) -> u8 {
        self.value.resolution()
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|v| self.value == v.value)
    }
    fn __hash__(&self) -> u64 {
        value_hash(&self.value)
    }
    fn __repr__(slf: PyRef<'_, Self>) -> PyResult<String> {
        let py = slf.py();
        let mut fields = if slf.value.version() == 2 {
            vec![
                "controller",
                "value",
                "channel",
                "version",
                "group",
                "resolution",
            ]
        } else {
            vec!["controller", "value", "channel", "resolution"]
        };
        if slf.value.version() == 1 && slf.value.group().is_some() {
            fields.push("group");
        }
        crate::tuning::render(slf.into_pyobject(py)?.as_any(), "ControlChange", &fields)
    }
    #[pyo3(signature=(**changes))]
    fn replace<'py>(
        slf: PyRef<'py, Self>,
        changes: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        if let Some(v) = changes
            .map(|c| c.get_item("version"))
            .transpose()?
            .flatten()
        {
            if integer(&v, "version")? != i64::from(slf.value.version()) {
                return Err(PyValueError::new_err(
                    "version cannot change through replace; convert explicitly",
                ));
            }
        }
        let py = slf.py();
        crate::tuning::replacement(
            slf.into_pyobject(py)?.as_any(),
            &[
                "controller",
                "value",
                "channel",
                "version",
                "group",
                "resolution",
            ],
            changes,
        )
    }
    #[classattr]
    fn __annotations__(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
        let fields = PyDict::new(py);
        for name in ["controller", "value", "version", "resolution"] {
            fields.set_item(name, py.get_type::<PyInt>())?;
        }
        for name in ["channel", "group"] {
            fields.set_item(
                name,
                py.import("typing")?
                    .getattr("Optional")?
                    .get_item(py.get_type::<PyInt>())?,
            )?;
        }
        Ok(fields)
    }
    #[classattr]
    fn __match_args__() -> (&'static str, &'static str, &'static str) {
        ("controller", "value", "channel")
    }
}
versioned_message! { ProgramChange (program, channel=None, *, version=1, group=None, options=None, bank_msb=None, bank_lsb=None) {program} {} {options => with_options, bank_msb => with_bank_msb, bank_lsb => with_bank_lsb} }
versioned_message! { ChannelPressure (pressure=None, channel=None, *, version=1, group=None) {} {pressure => with_pressure} {} }
versioned_message! { PitchBend (value=None, channel=None, *, version=1, group=None) {} {value => with_value} {} }

macro_rules! message_classes {
    (NoteOn $signature:tt { $($field:ident),* }; $($rest:tt)*) => { message_classes! { $($rest)* } };
    (NoteOff $signature:tt { $($field:ident),* }; $($rest:tt)*) => { message_classes! { $($rest)* } };
    (PolyPressure $signature:tt { $($field:ident),* }; $($rest:tt)*) => { message_classes! { $($rest)* } };
    (ControlChange $signature:tt { $($field:ident),* }; $($rest:tt)*) => { message_classes! { $($rest)* } };
    (ProgramChange $signature:tt { $($field:ident),* }; $($rest:tt)*) => { message_classes! { $($rest)* } };
    (ChannelPressure $signature:tt { $($field:ident),* }; $($rest:tt)*) => { message_classes! { $($rest)* } };
    (PitchBend $signature:tt { $($field:ident),* }; $($rest:tt)*) => { message_classes! { $($rest)* } };
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
                #[pyo3(signature = ($($field,)* *, group=None))]
                fn new($($field: &Bound<'_, PyAny>,)* group: Option<&Bound<'_, PyAny>>) -> PyResult<PyClassInitializer<Self>> {
                    let mut value = messages::$name::new($(integer($field, stringify!($field))?),*).map_err(value_error)?;
                    if let Some(g) = group { value = value.with_group(integer(g, "group")?).map_err(value_error)?; }
                    Ok(PyClassInitializer::from(Message::default()).add_subclass(Self {
                        value,
                    }))
                }
                $(#[getter]
                fn $field(&self) -> i64 { i64::from(self.value.$field()) })*
                #[getter] fn group(&self) -> Option<u8> { self.value.group() }

                fn __repr__(&self) -> String {
                    let mut fields: Vec<String> = vec![$(format!("{}={}", stringify!($field), self.value.$field())),*];
                    if let Some(g) = self.value.group() { fields.push(format!("group={g}")); }
                    format!("{}({})", stringify!($name), fields.join(", "))
                }
                fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
                    other.extract::<PyRef<'_, Self>>().is_ok_and(|other| self.value == other.value)
                }
                fn __hash__(&self) -> u64 { value_hash(&self.value) }

                #[pyo3(signature = (**changes))]
                fn replace<'py>(slf: PyRef<'py, Self>, changes: Option<&Bound<'py, PyDict>>) -> PyResult<Bound<'py, PyAny>> {
        let py = slf.py();
        crate::tuning::replacement(slf.into_pyobject(py)?.as_any(), &[$(stringify!($field),)* "group"], changes)
    }
                #[classattr]
                fn __annotations__(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
                    let annotations = PyDict::new(py);
                    $(annotations.set_item(stringify!($field), py.get_type::<PyInt>())?;)*
                    annotations.set_item("group", py.import("typing")?.getattr("Optional")?.get_item(py.get_type::<PyInt>())?)?;
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

        pub(crate) fn extract_message(message: &Bound<'_, PyAny>) -> PyResult<messages::AnyMessage> {
            if let Some(v) = crate::messages_midi2::extract(message) { return Ok(v); }
            if let Ok(value) = message.extract::<PyRef<'_, SysEx>>() {return Ok(messages::AnyMessage::SysEx(value.value.clone()));}
            $(if let Ok(value) = message.extract::<PyRef<'_, $name>>() {return Ok(messages::AnyMessage::$name(value.value.clone()));})*
            Err(PyTypeError::new_err("expected a primitive MIDI 1.0 message"))
        }

        pub fn to_python(py: Python<'_>, message: messages::AnyMessage) -> PyResult<Py<PyAny>> {
            match message {
                messages::AnyMessage::SysEx(value) => Ok(Py::new(py,
                    PyClassInitializer::from(Message::default()).add_subclass(SysEx { value }))?.into_any()),
                $(messages::AnyMessage::$name(value) => Ok(Py::new(py,
                    PyClassInitializer::from(Message::default()).add_subclass($name::from_value(value)))?.into_any()),)*
                v => crate::messages_midi2::to_python(py, v),
            }
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

fn register_commands(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<ParameterSet>()?;
    module.add_class::<ParameterStep>()
}
pub fn extract_command(command: &Bound<'_, PyAny>) -> PyResult<messages::AnyCommand> {
    macro_rules! extract { ($($name:ident),*) => { $(
        if let Ok(command) = command.extract::<PyRef<'_, $name>>() {
            return Ok(messages::AnyCommand::$name(command.value.clone()));
        }
    )* }; }
    if let Ok(command) = command.extract::<PyRef<'_, ControlChange>>() {
        if command.value.version() != 1 {
            return Err(PyTypeError::new_err(
                "MIDI 2 controller values require UMP encoding",
            ));
        }
        return Ok(messages::AnyCommand::ControlChange(command.value.clone()));
    }
    extract!(ParameterSet, ParameterStep);
    crate::profiles::extract_command(command)?
        .ok_or_else(|| PyTypeError::new_err("command must be a supported compound command"))
}
pub fn command_wire(
    py: Python<'_>,
    command: &messages::AnyCommand,
) -> PyResult<(String, Py<PyDict>)> {
    let kwargs = PyDict::new(py);
    kwargs.set_item("group", command.group())?;
    let name = match command {
        messages::AnyCommand::ControlChange(value) => {
            kwargs.set_item("controller", value.controller())?;
            kwargs.set_item("value", value.value())?;
            kwargs.set_item("channel", value.channel())?;
            kwargs.set_item("version", value.version())?;
            kwargs.set_item("group", value.group())?;
            kwargs.set_item("resolution", value.resolution())?;
            "ControlChange".into()
        }
        messages::AnyCommand::ParameterSet(value) => {
            kwargs.set_item("parameter", value.parameter())?;
            kwargs.set_item("value", value.value())?;
            kwargs.set_item("resolution", value.resolution())?;
            kwargs.set_item("channel", value.channel())?;
            kwargs.set_item("family", family_name(value.family()))?;
            "ParameterSet".into()
        }
        messages::AnyCommand::ParameterStep(value) => {
            let value = ParameterStep {
                value: value.clone(),
            };
            kwargs.set_item("parameter", value.parameter())?;
            kwargs.set_item("direction", value.direction())?;
            kwargs.set_item("channel", value.channel())?;
            kwargs.set_item("family", value.family())?;
            "ParameterStep".into()
        }
        _ => {
            let (name, fields) = crate::profiles::command_wire(command);
            for (key, value) in fields {
                kwargs.set_item(key, value)?;
            }
            name
        }
    };
    Ok((name, kwargs.unbind()))
}
fn family_name(family: meedee::controls::ParameterFamily) -> &'static str {
    match family {
        meedee::controls::ParameterFamily::Rpn => "rpn",
        meedee::controls::ParameterFamily::Nrpn => "nrpn",
    }
}
fn parameter_family(family: &str) -> PyResult<meedee::controls::ParameterFamily> {
    match family {
        "rpn" => Ok(meedee::controls::ParameterFamily::Rpn),
        "nrpn" => Ok(meedee::controls::ParameterFamily::Nrpn),
        _ => Err(PyValueError::new_err("family must be rpn or nrpn")),
    }
}

#[pyclass(frozen, extends = Message, module = "meedee.messages")]
struct ParameterSet {
    value: messages::ParameterSet,
}
#[pymethods]
impl ParameterSet {
    #[getter]
    fn group(&self) -> Option<u8> {
        self.value.group()
    }
    #[new]
    #[pyo3(signature=(parameter, value, resolution=14, channel=None, *, family="rpn", group=None))]
    fn new(
        #[pyo3(from_py_with=strict_integer)] parameter: i64,
        #[pyo3(from_py_with=strict_integer)] value: i64,
        #[pyo3(from_py_with=strict_integer)] resolution: i64,
        channel: Option<&Bound<'_, PyAny>>,
        family: &str,
        group: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let family = parameter_family(family)?;
        let mut value = messages::ParameterSet::new(0, value)
            .and_then(|v| v.with_family(family))
            .and_then(|v| v.with_parameter(parameter))
            .and_then(|v| v.with_resolution(resolution))
            .map_err(value_error)?;
        if let Some(g) = group {
            value = value
                .with_group(integer(g, "group")?)
                .map_err(value_error)?;
        }
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
    fn value(&self) -> u16 {
        self.value.value()
    }
    #[getter]
    fn resolution(&self) -> u8 {
        self.value.resolution()
    }
    #[getter]
    fn channel(&self) -> Option<u8> {
        self.value.channel()
    }
    #[getter]
    fn family(&self) -> &'static str {
        family_name(self.value.family())
    }
    fn __repr__(slf: PyRef<'_, Self>) -> PyResult<String> {
        let py = slf.py();
        crate::tuning::render(
            slf.into_pyobject(py)?.as_any(),
            "ParameterSet",
            &[
                "parameter",
                "value",
                "resolution",
                "channel",
                "family",
                "group",
            ],
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
            &[
                "parameter",
                "value",
                "resolution",
                "channel",
                "family",
                "group",
            ],
            changes,
        )
    }
    #[classattr]
    fn __annotations__(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
        let fields = PyDict::new(py);
        for name in ["parameter", "value", "resolution"] {
            fields.set_item(name, py.get_type::<PyInt>())?;
        }
        fields.set_item(
            "channel",
            py.import("typing")?
                .getattr("Optional")?
                .get_item(py.get_type::<PyInt>())?,
        )?;
        fields.set_item("family", py.get_type::<pyo3::types::PyString>())?;
        fields.set_item(
            "group",
            py.import("typing")?
                .getattr("Optional")?
                .get_item(py.get_type::<PyInt>())?,
        )?;
        Ok(fields)
    }
    #[classattr]
    fn __match_args__() -> (&'static str, &'static str, &'static str, &'static str) {
        ("parameter", "value", "resolution", "channel")
    }
}

#[pyclass(frozen, extends = Message, module = "meedee.messages")]
struct ParameterStep {
    value: messages::ParameterStep,
}
#[pymethods]
impl ParameterStep {
    #[getter]
    fn group(&self) -> Option<u8> {
        self.value.group()
    }
    #[new]
    #[pyo3(signature=(parameter, direction=1, channel=None, *, family="rpn", group=None))]
    fn new(
        #[pyo3(from_py_with=strict_integer)] parameter: i64,
        #[pyo3(from_py_with=strict_integer)] direction: i64,
        channel: Option<&Bound<'_, PyAny>>,
        family: &str,
        group: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyClassInitializer<Self>> {
        use meedee::controls::StepDirection;
        let family = parameter_family(family)?;
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
        if let Some(g) = group {
            value = value
                .with_group(integer(g, "group")?)
                .map_err(value_error)?;
        }
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
            &["parameter", "direction", "channel", "family", "group"],
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
        annotations.set_item(
            "group",
            py.import("typing")?
                .getattr("Optional")?
                .get_item(py.get_type::<PyInt>())?,
        )?;
        Ok(annotations)
    }
    #[classattr]
    fn __match_args__() -> (&'static str, &'static str, &'static str) {
        ("parameter", "direction", "channel")
    }
}

pub(crate) fn parameter_set_to_python(
    py: Python<'_>,
    value: messages::ParameterSet,
) -> PyResult<Py<PyAny>> {
    Ok(Py::new(
        py,
        PyClassInitializer::from(Message::default()).add_subclass(ParameterSet { value }),
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

pub(crate) fn value_hash(value: &impl Hash) -> u64 {
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
            attack: Py::new(
                py,
                NoteOn::new(
                    note.into_pyobject(py)?.as_any(),
                    Some(velocity.into_pyobject(py)?.as_any()),
                    channel,
                    1,
                    None,
                    None,
                    None,
                )?,
            )?,
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
        self.attack.borrow(py).channel().map(i64::from)
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
    crate::messages_midi2::register(module)?;
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

pub(crate) fn strict_integer(value: &Bound<'_, PyAny>) -> PyResult<i64> {
    integer(value, "value")
}
