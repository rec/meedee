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
pub(crate) struct Message;

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
pub(crate) struct SysEx {
    pub(crate) value: messages::SysEx,
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

macro_rules! message_classes {
    ($($name:ident $signature:tt { $($field:ident),* };)*) => {
        $(
            #[pyclass(frozen, extends = Message, module = "meedee.messages")]
            struct $name { value: messages::$name }

            #[pymethods]
            impl $name {
                #[new]
                #[pyo3(signature = $signature)]
                fn new($($field: &Bound<'_, PyAny>),*) -> PyResult<PyClassInitializer<Self>> {
                    Ok(PyClassInitializer::from(Message).add_subclass(Self {
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
                fn replace(&self, py: Python<'_>, changes: Option<&Bound<'_, PyDict>>) -> PyResult<Py<Self>> {
                    // Fixed-size field arrays also handle zero-field messages without
                    // special unchecked indexing or a second replacement code path.
                    let mut values = [$(i64::from(self.value.$field())),*];
                    let names: &[&str] = &[$(stringify!($field)),*];
                    if let Some(changes) = changes {
                        for (name, value) in changes.iter() {
                            let name = name.extract::<String>()?;
                            let index = names.iter().position(|field| *field == name)
                                .ok_or_else(|| PyTypeError::new_err(format!("unknown field: {name}")))?;
                            values[index] = integer(&value, &name)?;
                        }
                    }
                    let [$($field),*] = values;
                    Py::new(py, PyClassInitializer::from(Message).add_subclass(Self {
                        value: messages::$name::new($($field),*).map_err(value_error)?,
                    }))
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
        )*

    };
}

macro_rules! short_messages {
    ($($name:ident $signature:tt { $($field:ident),* };)*) => {
        message_classes! { $($name $signature { $($field),* };)* }

        fn register_short(module: &Bound<'_, PyModule>) -> PyResult<()> {
            $(module.add_class::<$name>()?;)*
            Ok(())
        }

        pub fn encode_message(message: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
            if let Ok(message) = message.extract::<PyRef<'_, NoteOn>>() {
                return Ok(message.value.encode());
            }
            if let Ok(message) = message.extract::<PyRef<'_, SysEx>>() {
                return Ok(message.value.encode());
            }
            $(if let Ok(message) = message.extract::<PyRef<'_, $name>>() {
                return Ok(message.value.encode());
            })*
            if let Some(bytes) = crate::profiles::encode_message(message) { return Ok(bytes); }
            crate::tuning::encode_message(message)
        }

        pub(crate) fn extract_primitive(message: &Bound<'_, PyAny>) -> PyResult<messages::AnyMessage> {
            if let Ok(value) = message.extract::<PyRef<'_, NoteOn>>() {return Ok(messages::AnyMessage::NoteOn(value.value.clone()));}
            if let Ok(value) = message.extract::<PyRef<'_, SysEx>>() {return Ok(messages::AnyMessage::SysEx(value.value.clone()));}
            $(if let Ok(value) = message.extract::<PyRef<'_, $name>>() {return Ok(messages::AnyMessage::$name(value.value.clone()));})*
            Err(PyTypeError::new_err("expected a primitive MIDI 1.0 message"))
        }

        pub fn to_python(py: Python<'_>, message: messages::AnyMessage) -> PyResult<Py<PyAny>> {
            match message {
                messages::AnyMessage::NoteOn(value) => Ok(Py::new(py,
                    PyClassInitializer::from(Message).add_subclass(NoteOn { value }))?.into_any()),
                messages::AnyMessage::SysEx(value) => Ok(Py::new(py,
                    PyClassInitializer::from(Message).add_subclass(SysEx { value }))?.into_any()),
                $(messages::AnyMessage::$name(value) => Ok(Py::new(py,
                    PyClassInitializer::from(Message).add_subclass($name { value }))?.into_any()),)*
            }
        }

        pub fn control_input(message: &Bound<'_, PyAny>) -> PyResult<meedee::controls::InputMessage> {
            use meedee::controls::InputMessage;
            if let Ok(message) = message.extract::<PyRef<'_, ControlChange>>() {
                return Ok(InputMessage::ControlChange(message.value.clone()));
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
    NoteOff (*, channel, note, velocity) { channel, note, velocity };
    PolyPressure (*, channel, note, pressure) { channel, note, pressure };
    ControlChange (*, channel, controller, value) { channel, controller, value };
    ProgramChange (*, channel, program) { channel, program };
    ChannelPressure (*, channel, pressure) { channel, pressure };
    PitchBend (*, channel, value) { channel, value };
    QuarterFrame (*, message_type, value) { message_type, value };
    SongPosition (*, position) { position };
    SongSelect (*, song) { song };
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
            $(module.add_class::<$name>()?;)*
            Ok(())
        }
        pub fn extract_command(command: &Bound<'_, PyAny>) -> PyResult<messages::AnyCommand> {
            $(if let Ok(command) = command.extract::<PyRef<'_, $name>>() {
                return Ok(messages::AnyCommand::$name(command.value.clone()));
            })*
            crate::profiles::extract_command(command)
                .ok_or_else(|| PyTypeError::new_err("command must be a supported compound command"))
        }
        pub fn command_wire(command: &messages::AnyCommand) -> (String, std::collections::HashMap<String, i64>) {
            match command {
                $(messages::AnyCommand::$name(value) => (stringify!($name).into(),
                    [$( (stringify!($field).into(), i64::from(value.$field())) ),*].into_iter().collect()),)*
                _ => crate::profiles::command_wire(command),
            }
        }
    };
}

command_messages! {
    ControlChange14 (*, channel, controller, value) { channel, controller, value };
    RpnSet (*, channel, parameter, value, resolution) { channel, parameter, value, resolution };
    NrpnSet (*, channel, parameter, value, resolution) { channel, parameter, value, resolution };
    RpnIncrement (*, channel, parameter) { channel, parameter };
    RpnDecrement (*, channel, parameter) { channel, parameter };
    NrpnIncrement (*, channel, parameter) { channel, parameter };
    NrpnDecrement (*, channel, parameter) { channel, parameter };
}

pub(crate) fn rpn_to_python(py: Python<'_>, value: messages::RpnSet) -> PyResult<Py<PyAny>> {
    Ok(Py::new(
        py,
        PyClassInitializer::from(Message).add_subclass(RpnSet { value }),
    )?
    .into_any())
}

pub fn integer(value: &Bound<'_, PyAny>, field: &str) -> PyResult<i64> {
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

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Message>()?;
    module.add_class::<NoteOn>()?;
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
