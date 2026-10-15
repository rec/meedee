//! Pure profile bindings; the public loaders select their own namespace.
use crate::messages::{integer, Message};
use crate::tuning::{render, replacement};
use meedee::messages::PrimitiveMessage;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyInt, PyString, PyTuple};
use pyo3::IntoPyObjectExt;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
#[path = "profile_engines.rs"]
mod engines;

macro_rules! classes {
    ($namespace:literal; $($name:ident { $($field:ident : $read:ident => $write:ident : $annotation:ty),* };)*) => {$(
        #[pyclass(frozen,extends=Message,module=$namespace)]
        pub(super) struct $name {pub(super) value:model::$name}
        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature=(*,$($field),*))]
            fn new($($field:&Bound<'_,PyAny>),*)->PyResult<PyClassInitializer<Self>> {
                Ok(PyClassInitializer::from(Message).add_subclass(Self {
                    value:model::$name::new($(self::$read($field)?),*).map_err(value_error)?,
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
macro_rules! entries {
    ($($name:ident { $($field:ident : $ty:ty => $annotation:ty),* };)*)=>{$(
        #[pyclass(frozen,module="meedee.gm")]
        pub(super) struct $name {value:model::$name}
        #[pymethods]
        impl $name {
            $(#[getter] fn $field(&self)->$ty {self.value.$field()})*
            fn __repr__(slf:PyRef<'_,Self>)->PyResult<String> {let py=slf.py();render(slf.into_pyobject(py)?.as_any(),stringify!($name),&[$(stringify!($field)),*])}
            fn __eq__(&self,other:&Bound<'_,PyAny>)->bool {other.extract::<PyRef<'_,Self>>().is_ok_and(|other|self.value==other.value)}
            fn __hash__(&self)->u64 {hash(&self.value)}
            #[classattr] fn __match_args__(py:Python<'_>)->PyResult<Bound<'_,PyTuple>> {PyTuple::new(py,[$(stringify!($field)),*])}
            #[classattr] fn __annotations__(py:Python<'_>)->PyResult<Bound<'_,PyDict>> {let fields=PyDict::new(py);$(fields.set_item(stringify!($field),py.get_type::<$annotation>())?;)*Ok(fields)}
        }
    )*};
}
mod gm {
    use super::*;
    use meedee::gm as model;
    classes! {"meedee.gm";
        BankProgramSelect {channel:int=>number:PyInt,bank_msb:int=>number:PyInt,bank_lsb:int=>number:PyInt,program:int=>number:PyInt};
        Gm1On {device_id:int=>number:PyInt};
        GmOff {device_id:int=>number:PyInt};
        Gm2On {device_id:int=>number:PyInt};
    }
    entries! {
        Program {program:u8=>PyInt,name:&'static str=>PyString,family:&'static str=>PyString};
        BankProgram {program:u8=>PyInt,bank_msb:u8=>PyInt,bank_lsb:u8=>PyInt,name:&'static str=>PyString,family:&'static str=>PyString};
        DrumKit {program:u8=>PyInt,bank_msb:u8=>PyInt,bank_lsb:u8=>PyInt,name:&'static str=>PyString};
        Percussion {note:u8=>PyInt,name:&'static str=>PyString};
    }
    #[pyfunction]
    fn programs() -> Vec<Program> {
        model::programs()
            .into_iter()
            .map(|value| Program { value })
            .collect()
    }
    #[pyfunction]
    #[pyo3(signature=(*,program))]
    fn program(program: &Bound<'_, PyAny>) -> PyResult<Program> {
        Ok(Program {
            value: model::program(int(program)?).map_err(value_error)?,
        })
    }
    #[pyfunction]
    fn gm2_programs() -> Vec<BankProgram> {
        model::gm2_programs()
            .into_iter()
            .map(|value| BankProgram { value })
            .collect()
    }
    #[pyfunction]
    #[pyo3(signature=(*,bank_lsb,program))]
    fn gm2_program(
        bank_lsb: &Bound<'_, PyAny>,
        program: &Bound<'_, PyAny>,
    ) -> PyResult<Option<BankProgram>> {
        Ok(model::gm2_program(int(bank_lsb)?, int(program)?)
            .map_err(value_error)?
            .map(|value| BankProgram { value }))
    }
    #[pyfunction]
    fn find_programs(py: Python<'_>, name: &str) -> PyResult<Vec<Py<PyAny>>> {
        model::find_programs(name)
            .into_iter()
            .map(|entry| match entry {
                model::ProgramMatch::Gm1(value) => Ok(Py::new(py, Program { value })?.into_any()),
                model::ProgramMatch::Gm2(value) => {
                    Ok(Py::new(py, BankProgram { value })?.into_any())
                }
            })
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
    #[pyo3(signature=(*,note))]
    fn percussion(note: &Bound<'_, PyAny>) -> PyResult<Option<Percussion>> {
        Ok(model::percussion(int(note)?)
            .map_err(value_error)?
            .map(|value| Percussion { value }))
    }
    #[pyfunction]
    #[pyo3(signature=(*,program,note))]
    fn gm2_percussion(
        program: &Bound<'_, PyAny>,
        note: &Bound<'_, PyAny>,
    ) -> PyResult<Option<Percussion>> {
        Ok(model::gm2_percussion(int(program)?, int(note)?)
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
        let mode = model::decode(&raw.value).map_err(value_error)?;
        if mode.is_none() {
            return engines::decode_gm(py, &raw.value, max_entries);
        }
        mode.map(|message| match message {
            model::ModeMessage::Gm1On(value) => object(py, Gm1On { value }),
            model::ModeMessage::GmOff(value) => object(py, GmOff { value }),
            model::ModeMessage::Gm2On(value) => object(py, Gm2On { value }),
        })
        .transpose()
    }
    pub(super) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
        module.add_class::<BankProgramSelect>()?;
        module.add_class::<Gm1On>()?;
        module.add_class::<GmOff>()?;
        module.add_class::<Gm2On>()?;
        module.add_class::<Program>()?;
        module.add_class::<BankProgram>()?;
        module.add_class::<DrumKit>()?;
        module.add_class::<Percussion>()?;
        module.add("GM1_DRUM_CHANNEL", model::GM1_DRUM_CHANNEL)?;
        module.add("GM2_ON_SETTLING_NS", model::GM2_ON_SETTLING_NS)?;
        module.add_function(wrap_pyfunction!(programs, module)?)?;
        module.add_function(wrap_pyfunction!(program, module)?)?;
        module.add_function(wrap_pyfunction!(gm2_programs, module)?)?;
        module.add_function(wrap_pyfunction!(gm2_program, module)?)?;
        module.add_function(wrap_pyfunction!(find_programs, module)?)?;
        module.add_function(wrap_pyfunction!(drum_kits, module)?)?;
        module.add_function(wrap_pyfunction!(find_drum_kits, module)?)?;
        module.add_function(wrap_pyfunction!(percussion, module)?)?;
        module.add_function(wrap_pyfunction!(gm2_percussion, module)?)?;
        module.add_function(wrap_pyfunction!(decode, module)?)?;
        Ok(())
    }
}
mod mpe {
    use super::*;
    use meedee::mpe as model;
    classes! {"meedee.mpe"; ZoneConfiguration {manager_channel:int=>number:PyInt,members:int=>number:PyInt};}
    pub(super) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
        module.add_class::<ZoneConfiguration>()
    }
}
mod universal {
    use super::*;
    use meedee::universal as model;
    classes! {"meedee.universal";
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
    use meedee::timecode as model;
    #[pyclass(eq, frozen, from_py_object, module = "meedee.timecode")]
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
    classes! {"meedee.timecode"; FullFrame {device_id:int=>number:PyInt,rate:rate=>rate_object:FrameRate,hours:int=>number:PyInt,minutes:int=>number:PyInt,seconds:int=>number:PyInt,frames:int=>number:PyInt};}
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

pub fn changes_mode(message: &Bound<'_, PyAny>) -> bool {
    message.is_instance_of::<gm::Gm1On>()
        || message.is_instance_of::<gm::Gm2On>()
        || message.is_instance_of::<gm::GmOff>()
}
macro_rules! primitives {
    ($($module:ident :: $name:ident),*)=>{
        pub fn encode_message(message:&Bound<'_,PyAny>)->Option<Vec<u8>> {$(if let Ok(value)=message.extract::<PyRef<'_,$module::$name>>() {return Some(value.value.encode());})*engines::encode_message(message)}
        pub fn is_message(message:&Bound<'_,PyAny>)->bool {engines::is_message(message) $(||message.is_instance_of::<$module::$name>())*}
    };
}
primitives!(
    gm::Gm1On,
    gm::GmOff,
    gm::Gm2On,
    universal::IdentityRequest,
    universal::IdentityReply,
    universal::MasterVolume,
    universal::MasterFineTuning,
    universal::MasterCoarseTuning,
    timecode::FullFrame
);
pub fn extract_command(command: &Bound<'_, PyAny>) -> Option<meedee::messages::AnyCommand> {
    if let Ok(value) = command.extract::<PyRef<'_, gm::BankProgramSelect>>() {
        return Some(meedee::messages::AnyCommand::BankProgramSelect(
            value.value.clone(),
        ));
    }
    if let Ok(value) = command.extract::<PyRef<'_, mpe::ZoneConfiguration>>() {
        return Some(meedee::messages::AnyCommand::ZoneConfiguration(
            value.value.clone(),
        ));
    }
    None
}
pub fn command_wire(
    command: &meedee::messages::AnyCommand,
) -> (String, std::collections::HashMap<String, i64>) {
    use meedee::messages::AnyCommand;
    match command {
        AnyCommand::BankProgramSelect(value) => (
            "BankProgramSelect".into(),
            [
                ("channel", value.channel()),
                ("bank_msb", value.bank_msb()),
                ("bank_lsb", value.bank_lsb()),
                ("program", value.program()),
            ]
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect(),
        ),
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
    Ok(Py::new(py, PyClassInitializer::from(Message).add_subclass(value))?.into_any())
}
fn value_error(error: meedee::messages::ValueError) -> PyErr {
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
