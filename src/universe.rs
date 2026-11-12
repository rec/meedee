use std::collections::BTreeMap;
use std::time::Duration;

use middy::universe as model;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

fn error(e: std::io::Error) -> PyErr {
    PyValueError::new_err(e.to_string())
}

macro_rules! enums {
    ($($name:ident { $($variant:ident),* };)*) => {$(
        #[pyclass(eq,hash,frozen,from_py_object,module="middy.universe")]
        #[derive(Clone,Copy,PartialEq,Eq,Hash)]
        enum $name { $($variant),* }
        impl From<$name> for model::$name { fn from(v:$name)->Self { match v { $($name::$variant=>Self::$variant),* } } }
        impl From<model::$name> for $name { fn from(v:model::$name)->Self { match v { $(model::$name::$variant=>Self::$variant),* } } }
    )*};
}
enums! { Retention { Full, Minimum }; Category { Controls, Performance, Notes, System }; }

#[pyclass(frozen, skip_from_py_object, module = "middy.universe")]
#[derive(Clone)]
struct Scope {
    value: model::Scope,
}
#[pymethods]
impl Scope {
    #[new]
    #[pyo3(signature=(*,group=None,version=1))]
    fn new(group: Option<u8>, version: u8) -> PyResult<Self> {
        Ok(Self {
            value: model::Scope::new(group, version).map_err(error)?,
        })
    }
    #[getter]
    fn group(&self) -> Option<u8> {
        self.value.group
    }
    #[getter]
    fn version(&self) -> u8 {
        self.value.version
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|other| self.value == other.value)
    }
}

#[pyclass(frozen, module = "middy.universe")]
struct CacheOptions {
    value: model::CacheOptions,
}
#[pymethods]
impl CacheOptions {
    #[new]
    #[pyo3(signature=(*,default=Retention::Full,universe=None,categories=None))]
    fn new(
        default: Retention,
        universe: Option<Retention>,
        categories: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        let mut values = BTreeMap::new();
        if let Some(categories) = categories {
            for (k, v) in categories {
                values.insert(
                    model::Category::from(k.extract::<Category>()?),
                    model::Retention::from(v.extract::<Retention>()?),
                );
            }
        }
        Ok(Self {
            value: model::CacheOptions {
                default: default.into(),
                universe: universe.map(Into::into),
                categories: values,
            },
        })
    }
    fn effective(&self, category: Category) -> Retention {
        self.value.effective(category.into()).into()
    }
}

macro_rules! states {
    ($($name:ident {$($field:ident:$ty:ty),*} {$($extra:item)*});* $(;)?) => {$(
        #[pyclass(frozen,module="middy.universe")]
        struct $name { value:model::$name }
        #[pymethods]
        impl $name {
            $(
                #[getter]
                fn $field(&self) -> $ty {
                    self.value.$field.clone()
                }
            )*
            fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
                other.extract::<PyRef<'_, Self>>()
                    .is_ok_and(|other| self.value == other.value)
            }
            fn __repr__(&self) -> String {
                format!("{:?}", self.value)
            }
            $($extra)*
        }
    )*};
}
states! {
    ControllerState {channel:u8,controller:u8,coarse:Option<u8>,fine:Option<u8>,native:Option<u32>} {
        #[getter] fn scope(&self)->Scope{Scope{value:self.value.scope}}
    };
    CachedParameter { channel: u8, parameter: u16, native: Option<u32> } {
        #[getter]
        fn scope(&self) -> Scope { Scope { value: self.value.scope } }
        #[getter]
        fn family(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
            family(py, self.value.family)
        }
        #[getter]
        fn components(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
            self.value.components
                .map(|v| crate::controls::parameter_state_object(py, v))
                .transpose()
        }
    };
    SelectionState { channel: u8 } {
        #[getter]
        fn scope(&self) -> Scope { Scope { value: self.value.scope } }
        #[getter]
        fn active(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
            self.value.active.map(|v| family(py, v)).transpose()
        }
        #[getter]
        fn rpn(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
            crate::controls::parameter_state_object(py, self.value.rpn)
        }
        #[getter]
        fn nrpn(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
            crate::controls::parameter_state_object(py, self.value.nrpn)
        }
    };
    ProgramState {channel:u8,program:Option<u8>,pending_msb:Option<u8>,pending_lsb:Option<u8>,bank_msb:Option<u8>,bank_lsb:Option<u8>} {
        #[getter] fn scope(&self)->Scope{Scope{value:self.value.scope}}
    };
    PerformanceState {channel:u8,pitch_bend:Option<i32>,pressure:Option<u32>} {
        #[getter] fn scope(&self)->Scope{Scope{value:self.value.scope}}
    };
    NoteState {
        channel: u8, note: u8, held: Option<bool>,
        velocity: Option<u16>, velocity_resolution: Option<u8>,
        pressure: Option<u32>, pitch_bend: Option<i32>,
        registered: BTreeMap<u8,u32>, assignable: BTreeMap<u8,u32>
    } {
        #[getter] fn scope(&self)->Scope{Scope{value:self.value.scope}}
    };
    SystemState {
        running: Option<bool>, song: Option<u8>, position: Option<u16>,
        master_volume: BTreeMap<u8,u16>, master_fine_tuning: BTreeMap<u8,i16>,
        master_coarse_tuning: BTreeMap<u8,i8>
    } {
        #[getter]
        fn scope(&self) -> Scope { Scope { value: self.value.scope } }
        #[getter]
        fn timecode(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
            self.value.timecode.clone().map(|observation| {
                let value = middy::timecode::AssemblyResult {
                    observation: Some(observation),
                    diagnostic: None,
                };
                let result = crate::event_views::value_object(
                    py, middy::events::ObservationValue::Timecode(value),
                )?;
                Ok(result.bind(py).getattr("observation")?.unbind())
            }).transpose()
        }
    };
    TuningState {group:Option<u8>,device:u8,bank:Option<u8>,program:u8} {
        #[getter]
        fn notes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
            let result = PyDict::new(py);
            for (&note, &value) in &self.value.notes {
                result.set_item(note, crate::tuning::pitch_object(py, value)?)?;
            }
            Ok(result)
        }
        #[getter]
        fn offsets(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
            self.value.offsets.as_ref()
                .map(|v| crate::tuning::offsets_object(py, v).map(Bound::unbind))
                .transpose()
        }
    };
    ChannelTuningState {group:Option<u8>,device:u8,channel:u8} {
        #[getter]
        fn offsets(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
            Ok(crate::tuning::offsets_object(py, &self.value.offsets)?.unbind())
        }
    }
}
fn family(py: Python<'_>, v: middy::controls::ParameterFamily) -> PyResult<Py<PyAny>> {
    let name = match v {
        middy::controls::ParameterFamily::Rpn => "Rpn",
        middy::controls::ParameterFamily::Nrpn => "Nrpn",
    };
    Ok(py
        .import("middy.controls")?
        .getattr("ParameterFamily")?
        .getattr(name)?
        .unbind())
}
#[pyclass(frozen, module = "middy.universe")]
struct Snapshot {
    value: model::Snapshot,
}
macro_rules! snapshot_fields { ($($field:ident:$ty:ident),*; $($extra:item)*) => {
    #[pymethods] impl Snapshot { $(
        #[getter] fn $field<'py>(&self,py:Python<'py>)->PyResult<Bound<'py,PyTuple>>{
            let values = self.value.$field.iter().cloned()
                .map(|value| Py::new(py, $ty { value }))
                .collect::<PyResult<Vec<_>>>()?;
            PyTuple::new(py, values)
        }
    )* $($extra)* }
}; }
snapshot_fields!(
    controllers: ControllerState, parameters: CachedParameter,
    selections: SelectionState, programs: ProgramState, performance: PerformanceState,
    notes: NoteState, system: SystemState, tuning: TuningState,
    channel_tuning: ChannelTuningState;
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other.extract::<PyRef<'_, Self>>().is_ok_and(|other| self.value == other.value)
    }
    fn __repr__(&self) -> String { format!("{:?}", self.value) }
    #[getter]
    fn mpe(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let values = self.value.mpe.iter().map(|(group, state)| {
            let value = crate::event_views::value_object(
                py, middy::events::ObservationValue::Mpe(state.clone()),
            )?;
            Ok((*group, value))
        }).collect::<PyResult<Vec<_>>>()?;
        Ok(PyTuple::new(py, values)?.into_any().unbind())
    }
);

#[pyclass(module = "middy.universe")]
struct Universe {
    value: model::Universe,
}
#[pymethods]
impl Universe {
    #[new]
    #[pyo3(signature=(*,options=None,timecode_timeout=1_000_000_000),text_signature="(*, options=None, timecode_timeout=1.0)")]
    fn new(
        options: Option<PyRef<'_, CacheOptions>>,
        #[pyo3(from_py_with=crate::units::duration)] timecode_timeout: u64,
    ) -> PyResult<Self> {
        let mut value = model::Universe::new(options.map(|v| v.value.clone()).unwrap_or_default());
        value
            .set_timecode_timeout(Duration::from_nanos(timecode_timeout))
            .map_err(error)?;
        Ok(Self { value })
    }
    fn effective(&self, category: Category) -> Retention {
        self.value.effective(category.into()).into()
    }
    fn reset(&mut self) {
        self.value.reset();
    }
    fn set_timecode_timeout(
        &mut self,
        #[pyo3(from_py_with=crate::units::duration)] timeout: u64,
    ) -> PyResult<()> {
        self.value
            .set_timecode_timeout(Duration::from_nanos(timeout))
            .map_err(error)
    }
    fn configure_mpe(&mut self, zone: &Bound<'_, PyAny>) -> PyResult<()> {
        let command = crate::profiles::extract_command(zone)?
            .ok_or_else(|| PyValueError::new_err("MPE ZoneConfiguration required"))?;
        let middy::messages::AnyCommand::ZoneConfiguration(zone) = command else {
            return Err(PyValueError::new_err("MPE ZoneConfiguration required"));
        };
        self.value.configure_mpe(zone).map_err(error)
    }
    #[pyo3(signature=(event,*,now_ns=None))]
    fn feed(
        &mut self,
        event: &Bound<'_, PyAny>,
        now_ns: Option<u64>,
    ) -> PyResult<Vec<crate::event_views::Observation>> {
        let out = if let Ok(d) = event.extract::<PyRef<'_, crate::live::StreamDiagnostic>>() {
            self.value.feed_diagnostic(&d.value)
        } else {
            self.value
                .feed(&crate::event_values::extract(event)?, now_ns)
        };
        Ok(out
            .map_err(error)?
            .into_iter()
            .map(|value| crate::event_views::Observation { value })
            .collect())
    }
    #[pyo3(signature=(*,now_ns=None))]
    fn poll(&mut self, now_ns: Option<u64>) -> PyResult<Vec<crate::event_views::Observation>> {
        Ok(self
            .value
            .poll(now_ns)
            .map_err(error)?
            .into_iter()
            .map(|value| crate::event_views::Observation { value })
            .collect())
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            value: self.value.snapshot(),
        }
    }
    fn parameter_state(
        &self,
        scope: &Scope,
        channel: u8,
        family: &Bound<'_, PyAny>,
        parameter: u16,
    ) -> PyResult<Option<CachedParameter>> {
        Ok(self
            .value
            .parameter_state(
                scope.value,
                channel,
                crate::controls::parameter_family(family)?,
                parameter,
            )
            .map_err(error)?
            .map(|value| CachedParameter { value }))
    }
    fn controller_state(
        &self,
        scope: &Scope,
        channel: u8,
        controller: u8,
    ) -> PyResult<Option<ControllerState>> {
        Ok(self
            .value
            .controller_state(scope.value, channel, controller)
            .map_err(error)?
            .map(|value| ControllerState { value }))
    }
    fn note_state(&self, scope: &Scope, channel: u8, note: u8) -> PyResult<Option<NoteState>> {
        Ok(self
            .value
            .note_state(scope.value, channel, note)
            .map_err(error)?
            .map(|value| NoteState { value }))
    }
    fn program_state(&self, scope: &Scope, channel: u8) -> PyResult<Option<ProgramState>> {
        Ok(self
            .value
            .program_state(scope.value, channel)
            .map_err(error)?
            .map(|value| ProgramState { value }))
    }
    fn performance_state(&self, scope: &Scope, channel: u8) -> PyResult<Option<PerformanceState>> {
        Ok(self
            .value
            .performance_state(scope.value, channel)
            .map_err(error)?
            .map(|value| PerformanceState { value }))
    }
    fn selection_state(&self, scope: &Scope, channel: u8) -> PyResult<Option<SelectionState>> {
        Ok(self
            .value
            .selection_state(scope.value, channel)
            .map_err(error)?
            .map(|value| SelectionState { value }))
    }
    fn system_state(&self, scope: &Scope) -> PyResult<Option<SystemState>> {
        Ok(self
            .value
            .system_state(scope.value)
            .map_err(error)?
            .map(|value| SystemState { value }))
    }
    fn mpe_state(
        &self,
        py: Python<'_>,
        group: Option<u8>,
        channel: u8,
    ) -> PyResult<Option<Py<PyAny>>> {
        self.value
            .mpe_state(group, channel)
            .map_err(error)?
            .map(|v| crate::event_views::value_object(py, middy::events::ObservationValue::Mpe(v)))
            .transpose()
    }
    fn channel_tuning_state(
        &self,
        group: Option<u8>,
        device: u8,
        channel: u8,
    ) -> PyResult<Option<ChannelTuningState>> {
        Ok(self
            .value
            .channel_tuning_state(group, device, channel)
            .map_err(error)?
            .map(|value| ChannelTuningState { value }))
    }
    fn tuning_state(
        &self,
        group: Option<u8>,
        device: u8,
        bank: Option<u8>,
        program: u8,
    ) -> PyResult<Option<TuningState>> {
        Ok(self
            .value
            .tuning_state(group, device, bank, program)
            .map_err(error)?
            .map(|value| TuningState { value }))
    }
}
pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Retention>()?;
    module.add_class::<Category>()?;
    module.add_class::<Scope>()?;
    module.add_class::<CacheOptions>()?;
    module.add_class::<Universe>()?;
    module.add_class::<Snapshot>()?;
    module.add_class::<ControllerState>()?;
    module.add_class::<CachedParameter>()?;
    module.add_class::<SelectionState>()?;
    module.add_class::<ProgramState>()?;
    module.add_class::<PerformanceState>()?;
    module.add_class::<NoteState>()?;
    module.add_class::<SystemState>()?;
    module.add_class::<TuningState>()?;
    module.add_class::<ChannelTuningState>()
}
