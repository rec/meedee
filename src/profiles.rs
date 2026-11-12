//! Pure profile bindings; the public loaders select their own namespace.
use crate::messages::{integer, Message};
use crate::tuning::{render, replacement};
use middi::messages::PrimitiveMessage;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyInt, PyString, PyTuple};
use pyo3::IntoPyObjectExt;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
#[path = "profile_engines.rs"]
pub(crate) mod engines;

macro_rules! classes {
    ($namespace:literal; $($name:ident { $($field:ident : $read:ident => $write:ident : $annotation:ty),* };)*) => {$(
        #[pyclass(frozen,extends=Message,module=$namespace)]
        pub(super) struct $name {pub(super) value:model::$name}
        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature=(*,$($field),*))]
            fn new($($field:&Bound<'_,PyAny>),*)->PyResult<PyClassInitializer<Self>> {
                Ok(PyClassInitializer::from(Message::default()).add_subclass(Self {
                    value:model::$name::new($(self::$read(&crate::units::field($field,stringify!($field))?)?),*).map_err(value_error)?,
                }))
            }
            $(#[getter]
            fn $field<'py>(&self,py:Python<'py>)->PyResult<Bound<'py,PyAny>> {$write(py,self.value.$field())})*
            fn __repr__(slf:PyRef<'_,Self>)->PyResult<String> {
                let py=slf.py();render(slf.into_pyobject(py)?.as_any(),stringify!($name),&[$(stringify!($field)),*])
            }
            fn __eq__(&self,other:&Bound<'_,PyAny>)->bool {other.extract::<PyRef<'_,Self>>().is_ok_and(|other|self.value==other.value)}
            fn __hash__(&self)->u64 {hash(&self.value)}
            #[pyo3(signature=(**changes))]
            fn replace<'py>(slf:PyRef<'py,Self>,changes:Option<&Bound<'py,PyDict>>)->PyResult<Bound<'py,PyAny>> {
                let py=slf.py();replacement(slf.into_pyobject(py)?.as_any(),&[$(stringify!($field)),*],changes)
            }
            #[classattr]
            fn __match_args__(py:Python<'_>)->PyResult<Bound<'_,PyTuple>> {PyTuple::new(py,[$(stringify!($field)),*])}
            #[classattr]
            fn __annotations__(py:Python<'_>)->PyResult<Bound<'_,PyDict>> {
                let fields=PyDict::new(py);$(fields.set_item(stringify!($field),py.get_type::<$annotation>())?;)*Ok(fields)
            }
        }
    )*};
}
macro_rules! entry_annotation {
    (OptionalInt,$py:ident) => {
        $py.import("typing")?
            .getattr("Optional")?
            .get_item($py.get_type::<PyInt>())?
    };
    ($type:ident,$py:ident) => {
        $py.get_type::<$type>().into_any()
    };
}
macro_rules! entries {
    ($($name:ident { $($field:ident : $ty:ty => $annotation:ident),* };)*)=>{$(
        #[pyclass(frozen,module="middi.gm")]
        pub(super) struct $name {value:model::$name}
        #[pymethods]
        impl $name {
            $(#[getter] fn $field(&self)->$ty {self.value.$field()})*
            fn __repr__(slf:PyRef<'_,Self>)->PyResult<String> {let py=slf.py();render(slf.into_pyobject(py)?.as_any(),stringify!($name),&[$(stringify!($field)),*])}
            fn __eq__(&self,other:&Bound<'_,PyAny>)->bool {other.extract::<PyRef<'_,Self>>().is_ok_and(|other|self.value==other.value)}
            fn __hash__(&self)->u64 {hash(&self.value)}
            #[classattr] fn __match_args__(py:Python<'_>)->PyResult<Bound<'_,PyTuple>> {PyTuple::new(py,[$(stringify!($field)),*])}
            #[classattr] fn __annotations__(py:Python<'_>)->PyResult<Bound<'_,PyDict>> {let fields=PyDict::new(py);$(fields.set_item(stringify!($field),entry_annotation!($annotation,py))?;)*Ok(fields)}
        }
    )*};
}
mod gm {
    use super::*;
    use middi::gm as model;
    #[pyclass(frozen, extends=Message, module="middi.gm")]
    pub(super) struct GmMode {
        pub(super) value: model::GmMode,
    }
    #[pymethods]
    impl GmMode {
        #[new]
        #[pyo3(signature=(mode,*,device_id))]
        fn new(mode: &str, device_id: &Bound<'_, PyAny>) -> PyResult<PyClassInitializer<Self>> {
            let mode = match mode {
                "gm1" => model::Mode::Gm1,
                "gm2" => model::Mode::Gm2,
                "off" => model::Mode::Off,
                _ => {
                    return Err(pyo3::exceptions::PyValueError::new_err(
                        "mode must be gm1, gm2 or off",
                    ))
                }
            };
            let value = model::GmMode::new(mode, int(device_id)?).map_err(value_error)?;
            Ok(PyClassInitializer::from(Message::default()).add_subclass(Self { value }))
        }
        #[getter]
        fn mode(&self) -> &'static str {
            match self.value.mode() {
                model::Mode::Gm1 => "gm1",
                model::Mode::Gm2 => "gm2",
                model::Mode::Off => "off",
            }
        }
        #[getter]
        fn device_id(&self) -> u8 {
            self.value.device_id()
        }
        fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
            other
                .extract::<PyRef<'_, Self>>()
                .is_ok_and(|v| self.value == v.value)
        }
        fn __hash__(&self) -> u64 {
            hash(&self.value)
        }
        fn __repr__(slf: PyRef<'_, Self>) -> PyResult<String> {
            let py = slf.py();
            render(
                slf.into_pyobject(py)?.as_any(),
                "GmMode",
                &["mode", "device_id"],
            )
        }
        #[pyo3(signature=(**changes))]
        fn replace<'py>(
            slf: PyRef<'py, Self>,
            changes: Option<&Bound<'py, PyDict>>,
        ) -> PyResult<Bound<'py, PyAny>> {
            let py = slf.py();
            replacement(
                slf.into_pyobject(py)?.as_any(),
                &["mode", "device_id"],
                changes,
            )
        }
        #[classattr]
        fn __match_args__() -> (&'static str,) {
            ("mode",)
        }
        #[classattr]
        fn __annotations__(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
            let fields = PyDict::new(py);
            fields.set_item("mode", py.get_type::<PyString>())?;
            fields.set_item("device_id", py.get_type::<PyInt>())?;
            Ok(fields)
        }
    }
    entries! {
        Program {program:u8=>PyInt,bank_msb:Option<u8> =>OptionalInt,bank_lsb:Option<u8> =>OptionalInt,name:&'static str=>PyString,family:&'static str=>PyString};
        DrumKit {program:u8=>PyInt,bank_msb:u8=>PyInt,bank_lsb:u8=>PyInt,name:&'static str=>PyString};
        Percussion {note:u8=>PyInt,name:&'static str=>PyString};
    }
    #[pyfunction]
    #[pyo3(signature=(*,version=1))]
    fn programs(#[pyo3(from_py_with=int)] version: i64) -> PyResult<Vec<Program>> {
        Ok(model::programs(version)
            .map_err(value_error)?
            .into_iter()
            .map(|value| Program { value })
            .collect())
    }
    #[pyfunction]
    #[pyo3(signature=(*,program,bank_lsb=None))]
    fn program(
        program: &Bound<'_, PyAny>,
        bank_lsb: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Option<Program>> {
        Ok(
            model::program(int(program)?, bank_lsb.map(int).transpose()?)
                .map_err(value_error)?
                .map(|value| Program { value }),
        )
    }
    #[pyfunction]
    fn find_programs(name: &str) -> Vec<Program> {
        model::find_programs(name)
            .into_iter()
            .map(|value| Program { value })
            .collect()
    }
    #[pyfunction]
    fn drum_kits() -> Vec<DrumKit> {
        model::drum_kits()
            .into_iter()
            .map(|value| DrumKit { value })
            .collect()
    }
    #[pyfunction]
    fn find_drum_kits(name: &str) -> Vec<DrumKit> {
        model::find_drum_kits(name)
            .into_iter()
            .map(|value| DrumKit { value })
            .collect()
    }
    #[pyfunction]
    #[pyo3(signature=(*,note,program=None))]
    fn percussion(
        note: &Bound<'_, PyAny>,
        program: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Option<Percussion>> {
        Ok(model::percussion(int(note)?, program.map(int).transpose()?)
            .map_err(value_error)?
            .map(|value| Percussion { value }))
    }
    #[pyfunction]
    #[pyo3(signature=(message,*,max_entries=1024))]
    fn decode(
        py: Python<'_>,
        message: &Bound<'_, PyAny>,
        #[pyo3(from_py_with=engines::limit)] max_entries: usize,
    ) -> PyResult<Option<Py<PyAny>>> {
        let raw = message.extract::<PyRef<'_, crate::messages::SysEx>>()?;
        model::decode(&raw.value, max_entries)
            .map_err(value_error)?
            .map(|message| engines::gm_to_python(py, message, max_entries))
            .transpose()
    }
    pub(super) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
        module.add_class::<GmMode>()?;
        module.add_class::<Program>()?;
        module.add_class::<DrumKit>()?;
        module.add_class::<Percussion>()?;
        module.add("GM1_DRUM_CHANNEL", model::GM1_DRUM_CHANNEL)?;
        module.add("GM2_ON_SETTLING_NS", model::GM2_ON_SETTLING_NS)?;
        module.add_function(wrap_pyfunction!(programs, module)?)?;
        module.add_function(wrap_pyfunction!(program, module)?)?;
        module.add_function(wrap_pyfunction!(find_programs, module)?)?;
        module.add_function(wrap_pyfunction!(drum_kits, module)?)?;
        module.add_function(wrap_pyfunction!(find_drum_kits, module)?)?;
        module.add_function(wrap_pyfunction!(percussion, module)?)?;
        module.add_function(wrap_pyfunction!(decode, module)?)?;
        Ok(())
    }
}
mod mpe {
    use super::*;
    use middi::mpe as model;
    #[pyclass(frozen, extends=Message, module="middi.mpe")]
    pub(super) struct ZoneConfiguration {
        pub(super) value: model::ZoneConfiguration,
    }
    #[pymethods]
    impl ZoneConfiguration {
        #[new]
        #[pyo3(signature=(*,manager_channel,members,group=None))]
        fn new(
            manager_channel: &Bound<'_, PyAny>,
            members: &Bound<'_, PyAny>,
            group: Option<&Bound<'_, PyAny>>,
        ) -> PyResult<PyClassInitializer<Self>> {
            let mut value = model::ZoneConfiguration::new(int(manager_channel)?, int(members)?)
                .map_err(value_error)?;
            if let Some(g) = group {
                value = value.with_group(int(g)?).map_err(value_error)?;
            }
            Ok(PyClassInitializer::from(Message::default()).add_subclass(Self { value }))
        }
        #[getter]
        fn manager_channel(&self) -> u8 {
            self.value.manager_channel()
        }
        #[getter]
        fn members(&self) -> u8 {
            self.value.members()
        }
        #[getter]
        fn group(&self) -> Option<u8> {
            self.value.group()
        }
        fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
            other
                .extract::<PyRef<'_, Self>>()
                .is_ok_and(|v| self.value == v.value)
        }
        fn __hash__(&self) -> u64 {
            hash(&self.value)
        }
        fn __repr__(slf: PyRef<'_, Self>) -> PyResult<String> {
            let py = slf.py();
            render(
                slf.into_pyobject(py)?.as_any(),
                "ZoneConfiguration",
                &["manager_channel", "members", "group"],
            )
        }
        #[pyo3(signature=(**changes))]
        fn replace<'py>(
            slf: PyRef<'py, Self>,
            changes: Option<&Bound<'py, PyDict>>,
        ) -> PyResult<Bound<'py, PyAny>> {
            let py = slf.py();
            replacement(
                slf.into_pyobject(py)?.as_any(),
                &["manager_channel", "members", "group"],
                changes,
            )
        }
        #[classattr]
        fn __match_args__() -> (&'static str, &'static str) {
            ("manager_channel", "members")
        }
        #[classattr]
        fn __annotations__(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
            let d = PyDict::new(py);
            d.set_item("manager_channel", py.get_type::<PyInt>())?;
            d.set_item("members", py.get_type::<PyInt>())?;
            d.set_item(
                "group",
                py.import("typing")?
                    .getattr("Optional")?
                    .get_item(py.get_type::<PyInt>())?,
            )?;
            Ok(d)
        }
    }
    pub(super) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
        module.add_class::<ZoneConfiguration>()
    }
}
mod universal {
    use super::*;
    use middi::universal as model;
    classes! {"middi.universal";
        IdentityRequest {device_id:int=>number:PyInt};
        IdentityReply {device_id:int=>number:PyInt,manufacturer:manufacturer=>integer_tuple:PyTuple,family:int=>number:PyInt,model:int=>number:PyInt,revision:revision=>integer_tuple:PyTuple};
        MasterVolume {device_id:int=>number:PyInt,value:int=>number:PyInt};
        MasterFineTuning {device_id:int=>number:PyInt,value:int=>number:PyInt};
        MasterCoarseTuning {device_id:int=>number:PyInt,semitones:int=>number:PyInt};
    }
    #[pyfunction]
    fn decode(py: Python<'_>, message: &Bound<'_, PyAny>) -> PyResult<Option<Py<PyAny>>> {
        let raw = message.extract::<PyRef<'_, crate::messages::SysEx>>()?;
        model::decode(&raw.value)
            .map_err(value_error)?
            .map(|message| match message {
                model::UniversalMessage::IdentityRequest(value) => {
                    object(py, IdentityRequest { value })
                }
                model::UniversalMessage::IdentityReply(value) => {
                    object(py, IdentityReply { value })
                }
                model::UniversalMessage::MasterVolume(value) => object(py, MasterVolume { value }),
                model::UniversalMessage::MasterFineTuning(value) => {
                    object(py, MasterFineTuning { value })
                }
                model::UniversalMessage::MasterCoarseTuning(value) => {
                    object(py, MasterCoarseTuning { value })
                }
            })
            .transpose()
    }
    pub(super) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
        module.add_class::<IdentityRequest>()?;
        module.add_class::<IdentityReply>()?;
        module.add_class::<MasterVolume>()?;
        module.add_class::<MasterFineTuning>()?;
        module.add_class::<MasterCoarseTuning>()?;
        module.add_function(wrap_pyfunction!(decode, module)?)?;
        Ok(())
    }
}
mod timecode {
    use super::*;
    use middi::timecode as model;
    #[pyclass(eq, frozen, from_py_object, module = "middi.timecode")]
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum FrameRate {
        Fps24,
        Fps25,
        DropFrame30,
        Fps30,
    }
    pub(super) fn rate(value: &Bound<'_, PyAny>) -> PyResult<model::FrameRate> {
        Ok(match value.extract::<FrameRate>()? {
            FrameRate::Fps24 => model::FrameRate::Fps24,
            FrameRate::Fps25 => model::FrameRate::Fps25,
            FrameRate::DropFrame30 => model::FrameRate::DropFrame30,
            FrameRate::Fps30 => model::FrameRate::Fps30,
        })
    }
    pub(super) fn rate_object(
        py: Python<'_>,
        value: model::FrameRate,
    ) -> PyResult<Bound<'_, PyAny>> {
        Ok(Py::new(
            py,
            match value {
                model::FrameRate::Fps24 => FrameRate::Fps24,
                model::FrameRate::Fps25 => FrameRate::Fps25,
                model::FrameRate::DropFrame30 => FrameRate::DropFrame30,
                model::FrameRate::Fps30 => FrameRate::Fps30,
            },
        )?
        .into_bound(py)
        .into_any())
    }
    classes! {"middi.timecode"; FullFrame {device_id:int=>number:PyInt,rate:rate=>rate_object:FrameRate,hours:int=>number:PyInt,minutes:int=>number:PyInt,seconds:int=>number:PyInt,frames:int=>number:PyInt};}
    #[pyfunction]
    fn decode(py: Python<'_>, message: &Bound<'_, PyAny>) -> PyResult<Option<Py<PyAny>>> {
        let raw = message.extract::<PyRef<'_, crate::messages::SysEx>>()?;
        model::decode(&raw.value)
            .map_err(value_error)?
            .map(|value| object(py, FullFrame { value }))
            .transpose()
    }
    pub(super) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
        module.add_class::<FrameRate>()?;
        module.add_class::<FullFrame>()?;
        module.add_function(wrap_pyfunction!(decode, module)?)?;
        Ok(())
    }
}

pub fn register_gm(module: &Bound<'_, PyModule>) -> PyResult<()> {
    gm::register(module)?;
    engines::register_gm(module)
}
pub fn register_mpe(module: &Bound<'_, PyModule>) -> PyResult<()> {
    mpe::register(module)?;
    engines::register_mpe(module)
}
pub fn register_universal(module: &Bound<'_, PyModule>) -> PyResult<()> {
    universal::register(module)
}
pub fn register_timecode(module: &Bound<'_, PyModule>) -> PyResult<()> {
    timecode::register(module)?;
    engines::register_timecode(module)
}

macro_rules! primitives {
    ($($module:ident :: $name:ident),*)=>{
        pub fn encode_message(message:&Bound<'_,PyAny>)->Option<Vec<u8>> {$(if let Ok(value)=message.extract::<PyRef<'_,$module::$name>>() {return Some(value.value.encode().expect("validated system message"));})*engines::encode_message(message)}
    };
}
primitives!(
    gm::GmMode,
    universal::IdentityRequest,
    universal::IdentityReply,
    universal::MasterVolume,
    universal::MasterFineTuning,
    universal::MasterCoarseTuning,
    timecode::FullFrame
);
pub fn extract_command(
    command: &Bound<'_, PyAny>,
) -> PyResult<Option<middi::messages::AnyCommand>> {
    if let Ok(value) = command.extract::<PyRef<'_, mpe::ZoneConfiguration>>() {
        return Ok(Some(middi::messages::AnyCommand::ZoneConfiguration(
            value.value.clone(),
        )));
    }
    Ok(None)
}
pub fn command_wire(
    command: &middi::messages::AnyCommand,
) -> (String, std::collections::HashMap<String, i64>) {
    use middi::messages::AnyCommand;
    match command {
        AnyCommand::ZoneConfiguration(value) => (
            "ZoneConfiguration".into(),
            [
                ("manager_channel", value.manager_channel()),
                ("members", value.members()),
            ]
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect(),
        ),
        _ => unreachable!("existing commands are handled by messages::command_wire"),
    }
}
fn object<T>(py: Python<'_>, value: T) -> PyResult<Py<PyAny>>
where
    T: pyo3::PyClass<BaseType = Message>,
{
    Ok(Py::new(
        py,
        PyClassInitializer::from(Message::default()).add_subclass(value),
    )?
    .into_any())
}
fn value_error(error: middi::messages::ValueError) -> PyErr {
    PyValueError::new_err(error.to_string())
}
fn int(value: &Bound<'_, PyAny>) -> PyResult<i64> {
    integer(value, "value")
}
fn number<T: for<'py> IntoPyObject<'py>>(py: Python<'_>, value: T) -> PyResult<Bound<'_, PyAny>> {
    value.into_bound_py_any(py)
}
fn integer_tuple<'py>(py: Python<'py>, values: &[u8]) -> PyResult<Bound<'py, PyAny>> {
    Ok(PyTuple::new(py, values.iter().copied())?.into_any())
}
fn manufacturer(value: &Bound<'_, PyAny>) -> PyResult<Vec<i64>> {
    sequence(value, 3)
}
fn revision(value: &Bound<'_, PyAny>) -> PyResult<Vec<i64>> {
    sequence(value, 4)
}
fn sequence(value: &Bound<'_, PyAny>, limit: usize) -> PyResult<Vec<i64>> {
    if !value.is_exact_instance_of::<PyTuple>()
        && !value.is_exact_instance_of::<pyo3::types::PyList>()
    {
        return Err(PyTypeError::new_err("expected a built-in list or tuple"));
    }
    if value.len()? > limit {
        return Err(PyValueError::new_err("too many bytes"));
    }
    value.try_iter()?.map(|value| int(&value?)).collect()
}
fn hash(value: &impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn register_control_engines(module: &Bound<'_, PyModule>) -> PyResult<()> {
    engines::register_controls(module)
}
