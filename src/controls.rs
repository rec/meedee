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
    register_helpers(module)?;
    register_enums(module)?;
    register_observations(module)?;
    module.add_class::<Interpreter>()?;
    Ok(())
}

macro_rules! controllers {
    ($($name:ident),*) => {
        #[pyclass(eq,frozen,from_py_object,module="meedee.controls")]
        #[derive(Clone,Copy,PartialEq,Eq)]
        enum Controller {$($name),*}
        #[pymethods]
        impl Controller {
            #[getter]
            fn number(&self)->u8 {match self {$(Self::$name=>controls::Controller::$name.number()),*}}
            #[pyo3(signature=(*,channel,value))]
            fn message(&self,py:Python<'_>,channel:&Bound<'_,PyAny>,value:&Bound<'_,PyAny>)->PyResult<Py<PyAny>> {
                let value=meedee::messages::ControlChange::new(crate::messages::integer(channel,"channel")?,self.number().into(),crate::messages::integer(value,"value")?).map_err(|error|pyo3::exceptions::PyValueError::new_err(error.to_string()))?;
                crate::messages::to_python(py,meedee::messages::AnyMessage::ControlChange(value))
            }
        }
    };
}
controllers!(
    BankSelectMsb,
    Modulation,
    Breath,
    Foot,
    PortamentoTime,
    DataEntryMsb,
    ChannelVolume,
    Balance,
    Pan,
    Expression,
    EffectControl1,
    EffectControl2,
    BankSelectLsb,
    DataEntryLsb,
    Hold1,
    Portamento,
    Sostenuto,
    SoftPedal,
    LegatoFootswitch,
    Hold2,
    SoundVariation,
    FilterResonance,
    ReleaseTime,
    AttackTime,
    Brightness,
    DecayTime,
    VibratoRate,
    VibratoDepth,
    VibratoDelay,
    PortamentoControl,
    ReverbSend,
    ChorusSend,
    DataIncrement,
    DataDecrement,
    NrpnLsb,
    NrpnMsb,
    RpnLsb,
    RpnMsb,
    AllSoundOff,
    ResetAllControllers,
    LocalControl,
    AllNotesOff,
    OmniOff,
    OmniOn,
    MonoMode,
    PolyMode
);

macro_rules! helpers {
    (rpn: {$($rpn:ident($($field:ident),*));*}; cc: {$($cc:ident($($arg:ident),*));*})=>{
        $(#[pyfunction]
        #[pyo3(signature=(*,$($field),*))]
        fn $rpn(py:Python<'_>,$($field:&Bound<'_,PyAny>),*)->PyResult<Py<PyAny>> {
            let value=controls::$rpn($(crate::messages::integer($field,stringify!($field))?),*)
                .map_err(|error|pyo3::exceptions::PyValueError::new_err(error.to_string()))?;
            crate::messages::rpn_to_python(py,value)
        })*
        $(#[pyfunction]
        #[pyo3(signature=(*,$($arg),*))]
        fn $cc(py:Python<'_>,$($arg:&Bound<'_,PyAny>),*)->PyResult<Py<PyAny>> {
            let value=controls::$cc($(crate::messages::integer($arg,stringify!($arg))?),*)
                .map_err(|error|pyo3::exceptions::PyValueError::new_err(error.to_string()))?;
            crate::messages::to_python(py,meedee::messages::AnyMessage::ControlChange(value))
        })*
        fn register_helpers(module:&Bound<'_,PyModule>)->PyResult<()> {
            $(module.add_function(wrap_pyfunction!($rpn,module)?)?;)*
            $(module.add_function(wrap_pyfunction!($cc,module)?)?;)*
            module.add_class::<Controller>()?;
            module.add_function(wrap_pyfunction!(local_control,module)?)?;
            Ok(())
        }
    };
}
helpers! {
    rpn: {pitch_bend_sensitivity(channel,semitones,cents);channel_fine_tuning(channel,value);
    channel_coarse_tuning(channel,semitones);tuning_program(channel,program);tuning_bank(channel,bank);modulation_depth_range(channel,value)};
    cc: {all_sound_off(channel);reset_all_controllers(channel);all_notes_off(channel);omni_off(channel);omni_on(channel);mono_mode(channel,channels);poly_mode(channel)}
}
#[pyfunction]
#[pyo3(signature=(*,channel,enabled))]
fn local_control(
    py: Python<'_>,
    channel: &Bound<'_, PyAny>,
    enabled: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    if !enabled.is_exact_instance_of::<pyo3::types::PyBool>() {
        return Err(pyo3::exceptions::PyTypeError::new_err(
            "enabled must be a bool",
        ));
    }
    let value = controls::local_control(
        crate::messages::integer(channel, "channel")?,
        enabled.extract()?,
    )
    .map_err(|error| pyo3::exceptions::PyValueError::new_err(error.to_string()))?;
    crate::messages::to_python(py, meedee::messages::AnyMessage::ControlChange(value))
}
