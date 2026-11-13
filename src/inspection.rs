//! Python inspection reports. Parsing and aggregation stay in the Rust toolkit.
use crate::files::{limit, os_error, Stream};
use middy::inspection;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use std::collections::BTreeMap;

#[pyclass(frozen, module = "middy.inspection")]
struct Report {
    value: inspection::Report,
}

#[pymethods]
impl Report {
    #[getter]
    fn note_state(&self) -> crate::notes::Report {
        crate::notes::Report {
            value: self.value.note_state(),
        }
    }
    #[getter]
    fn source_counts(&self) -> BTreeMap<String, u64> {
        self.value.source_counts().clone()
    }
    #[getter]
    fn timing_segments(&self) -> Vec<TimingSegment> {
        self.value
            .timing_segments()
            .iter()
            .map(|v| TimingSegment { value: v.clone() })
            .collect()
    }
    #[getter]
    fn timing_diagnostics(&self) -> Vec<TimingDiagnostic> {
        self.value
            .timing_diagnostics()
            .iter()
            .map(|v| TimingDiagnostic { value: v.clone() })
            .collect()
    }
    #[getter]
    fn timestamp_regressions(&self) -> u64 {
        self.value.timestamp_regressions()
    }
    #[getter]
    fn omitted_segments(&self) -> u64 {
        self.value.omitted_segments()
    }
    #[getter]
    fn omitted_diagnostics(&self) -> u64 {
        self.value.omitted_diagnostics()
    }
    #[getter]
    fn complete(&self) -> bool {
        self.value.complete()
    }
    #[getter]
    fn problem(&self) -> Option<&str> {
        self.value.problem().as_deref()
    }
    #[getter]
    fn records(&self) -> u64 {
        self.value.records()
    }
    #[getter]
    fn message_counts(&self) -> BTreeMap<&str, u64> {
        self.value.message_counts().clone()
    }
    #[getter]
    fn clocks(&self) -> Vec<crate::packets::Clock> {
        self.value
            .clocks()
            .iter()
            .map(|c| crate::packets::Clock { value: c.clone() })
            .collect()
    }
    #[getter]
    fn streams(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.value
            .streams()
            .iter()
            .map(|r| crate::archive::to_python(py, r.clone()))
            .collect()
    }
    #[getter]
    fn smf_header(&self) -> Option<crate::files::Header> {
        self.value
            .smf_header()
            .clone()
            .map(|value| crate::files::Header { value })
    }
    #[getter]
    fn track_records(&self) -> BTreeMap<u16, u64> {
        self.value.track_records().clone()
    }
    #[getter]
    fn gap_records(&self) -> u64 {
        self.value.gap_records()
    }
    #[getter]
    fn lost_events(&self) -> u128 {
        self.value.lost_events()
    }
    #[getter]
    fn discontinuities(&self) -> u64 {
        self.value.discontinuities()
    }
    #[getter]
    fn untimed_events(&self) -> u64 {
        self.value.untimed_events()
    }
    #[getter]
    fn opaque_records(&self) -> u64 {
        self.value.opaque_records()
    }
    fn __repr__(&self) -> String {
        format!(
            "Report(records={}, complete={}, problem={:?})",
            self.value.records(),
            self.value.complete(),
            self.value.problem()
        )
    }
}

#[pyfunction]
#[pyo3(signature=(source, *, format, payload_limit=1_048_576, preserve_unknown=false, detail_limit=128))]
fn inspect_file(
    py: Python<'_>,
    source: &Bound<'_, PyAny>,
    format: &str,
    #[pyo3(from_py_with=crate::units::bytes)] payload_limit: usize,
    #[pyo3(from_py_with=crate::ump::boolean)] preserve_unknown: bool,
    #[pyo3(from_py_with=limit)] detail_limit: usize,
) -> PyResult<Report> {
    let format = parse_format(format)?;
    let source = Stream::input(source)?;
    let value = py
        .detach(move || {
            inspection::inspect(
                source,
                format,
                payload_limit,
                preserve_unknown,
                detail_limit,
            )
        })
        .map_err(os_error)?;
    Ok(Report { value })
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    crate::comparison::register(module)?;
    module.add_class::<Report>()?;
    module.add_class::<Observer>()?;
    module.add_class::<Monitor>()?;
    module.add_class::<MonitorEntry>()?;
    module.add_class::<MonitorReport>()?;
    module.add_class::<TimingSegment>()?;
    module.add_class::<TimingDiagnostic>()?;
    module.add_function(wrap_pyfunction!(inspect_file, module)?)
}

#[pyclass(module = "middy.inspection")]
struct Observer {
    value: inspection::Observer,
}
#[pymethods]
impl Observer {
    #[pyo3(signature=(*,channel,group=None))]
    fn channel_states(
        &self,
        channel: i64,
        group: Option<i64>,
    ) -> PyResult<Vec<crate::notes::ChannelState>> {
        if !(1..=16).contains(&channel) || group.is_some_and(|v| !(1..=16).contains(&v)) {
            return Err(PyValueError::new_err("channel and group must be 1..16"));
        }
        Ok(self
            .value
            .channel_states(group.map(|v| v as u8), channel as u8)
            .into_iter()
            .map(|value| crate::notes::ChannelState { value })
            .collect())
    }
    #[new]
    #[pyo3(signature=(*,stream,clock,detail_limit=128))]
    fn new(
        stream: String,
        clock: PyRef<'_, crate::packets::Clock>,
        #[pyo3(from_py_with=limit)] detail_limit: usize,
    ) -> PyResult<Self> {
        Ok(Self {
            value: inspection::Observer::new(stream, clock.value.clone(), detail_limit)
                .map_err(|e| PyValueError::new_err(e.to_string()))?,
        })
    }
    fn feed(&mut self, item: &Bound<'_, PyAny>) -> PyResult<()> {
        let item = if let Ok(e) = crate::event_values::extract(item) {
            middy::events::InputItem::Message(Box::new(e))
        } else if let Ok(d) = item.extract::<PyRef<'_, crate::live::StreamDiagnostic>>() {
            middy::events::InputItem::Diagnostic(d.value.clone())
        } else {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "expected Message or StreamDiagnostic",
            ));
        };
        self.value
            .feed(&item)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }
    fn loss_counter_changed(&mut self) {
        self.value.loss_counter_changed();
    }
    fn snapshot(&self) -> Report {
        Report {
            value: self.value.snapshot(),
        }
    }
}

pub fn parse_format(format: &str) -> PyResult<inspection::Format> {
    match format {
        "archive" => Ok(inspection::Format::Archive),
        "smf" => Ok(inspection::Format::Smf),
        "syx" => Ok(inspection::Format::Syx),
        "clip" => Ok(inspection::Format::Clip),
        _ => Err(PyValueError::new_err(
            "format must be archive, smf, syx or clip",
        )),
    }
}

#[pyclass(frozen, module = "middy.inspection")]
struct TimingSegment {
    value: middy::inspection::TimingSegment,
}
#[pymethods]
impl TimingSegment {
    #[getter]
    fn source(&self) -> &str {
        &self.value.source
    }
    #[getter]
    fn clock_identity(&self) -> Option<&str> {
        self.value.clock_identity.as_deref()
    }
    #[getter]
    fn number(&self) -> u64 {
        self.value.number
    }
    #[getter]
    fn events(&self) -> u64 {
        self.value.events
    }
    #[getter]
    fn untimed(&self) -> u64 {
        self.value.untimed
    }
    #[getter]
    fn first(&self) -> Option<i128> {
        self.value.first
    }
    #[getter]
    fn last(&self) -> Option<i128> {
        self.value.last
    }
    #[getter]
    fn minimum(&self) -> Option<i128> {
        self.value.minimum
    }
    #[getter]
    fn maximum(&self) -> Option<i128> {
        self.value.maximum
    }
    #[getter]
    fn regressions(&self) -> u64 {
        self.value.regressions
    }
}
#[pyclass(frozen, module = "middy.inspection")]
struct TimingDiagnostic {
    value: middy::inspection::TimingDiagnostic,
}
#[pymethods]
impl TimingDiagnostic {
    #[getter]
    fn source(&self) -> &str {
        &self.value.source
    }
    #[getter]
    fn record(&self) -> u64 {
        self.value.record
    }
    #[getter]
    fn kind(&self) -> &str {
        self.value.kind
    }
}

#[pyclass(module = "middy.inspection")]
pub(crate) struct Monitor {
    pub(crate) value: inspection::Monitor,
}
#[pymethods]
impl Monitor {
    #[new]
    #[pyo3(signature=(*,stream,clock,encoding="bytes",types=Vec::new(),channel=None,group=None,detail_limit=128))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        stream: String,
        clock: PyRef<'_, crate::packets::Clock>,
        encoding: &str,
        types: Vec<String>,
        channel: Option<u8>,
        group: Option<u8>,
        #[pyo3(from_py_with=limit)] detail_limit: usize,
    ) -> PyResult<Self> {
        let encoding = match encoding {
            "bytes" => middy::transport::Encoding::Bytes,
            "ump" => middy::transport::Encoding::Ump,
            _ => return Err(PyValueError::new_err("encoding must be bytes or ump")),
        };
        let options = inspection::MonitorOptions {
            encoding,
            types,
            channel,
            group,
            detail_limit,
        };
        Ok(Self {
            value: inspection::Monitor::new(stream, clock.value.clone(), options)
                .map_err(|e| PyValueError::new_err(e.to_string()))?,
        })
    }
    fn feed(&mut self, item: &Bound<'_, PyAny>) -> PyResult<Option<MonitorEntry>> {
        let item = if let Ok(d) = item.extract::<PyRef<'_, crate::live::StreamDiagnostic>>() {
            middy::events::InputItem::Diagnostic(d.value.clone())
        } else {
            middy::events::InputItem::Message(Box::new(crate::event_values::extract(item)?))
        };
        self.value
            .feed(&item)
            .map(|v| v.map(|value| MonitorEntry { value }))
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }
    fn counter(&mut self, total: u64) -> Option<MonitorEntry> {
        self.value
            .counter(total)
            .map(|value| MonitorEntry { value })
    }
    fn snapshot(&self) -> MonitorReport {
        MonitorReport {
            value: self.value.snapshot(),
        }
    }
}
#[pyclass(frozen, module = "middy.inspection")]
pub(crate) struct MonitorEntry {
    pub(crate) value: inspection::MonitorEntry,
}
#[pymethods]
impl MonitorEntry {
    #[getter]
    fn kind(&self) -> &'static str {
        match self.value {
            inspection::MonitorEntry::Message { .. } => "event",
            inspection::MonitorEntry::Diagnostic(_) => "diagnostic",
            inspection::MonitorEntry::LossCounter(_) => "loss_counter",
        }
    }
    #[getter]
    fn message(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        match &self.value {
            inspection::MonitorEntry::Message { message, .. } => {
                crate::event_values::to_python(py, *message.clone()).map(Some)
            }
            _ => Ok(None),
        }
    }
    #[getter]
    fn diagnostic(&self) -> Option<crate::live::StreamDiagnostic> {
        match &self.value {
            inspection::MonitorEntry::Diagnostic(value) => Some(crate::live::StreamDiagnostic {
                value: value.clone(),
            }),
            _ => None,
        }
    }
    #[getter]
    fn transport_drop_total(&self) -> Option<u64> {
        match self.value {
            inspection::MonitorEntry::LossCounter(total) => Some(total),
            _ => None,
        }
    }
    #[getter]
    fn observations(&self) -> Vec<crate::event_views::Observation> {
        match &self.value {
            inspection::MonitorEntry::Message { observations, .. } => observations
                .iter()
                .map(|value| crate::event_views::Observation {
                    value: value.clone(),
                })
                .collect(),
            _ => Vec::new(),
        }
    }
    #[getter]
    fn note_state(&self) -> Vec<crate::notes::ChannelState> {
        match &self.value {
            inspection::MonitorEntry::Message { note_state, .. } => note_state
                .iter()
                .map(|value| crate::notes::ChannelState {
                    value: value.clone(),
                })
                .collect(),
            _ => Vec::new(),
        }
    }
    fn __repr__(&self) -> String {
        format!("MonitorEntry(kind={:?})", self.kind())
    }
}
#[pyclass(frozen, module = "middy.inspection")]
pub(crate) struct MonitorReport {
    pub(crate) value: inspection::MonitorReport,
}
#[pymethods]
impl MonitorReport {
    #[getter]
    fn inspection(&self) -> Report {
        Report {
            value: self.value.inspection.clone(),
        }
    }
    #[getter]
    fn filtered(&self) -> u64 {
        self.value.filtered
    }
    #[getter]
    fn diagnostics(&self) -> u64 {
        self.value.diagnostics
    }
    #[getter]
    fn transport_drops(&self) -> u64 {
        self.value.transport_drops
    }
    #[getter]
    fn interrupted(&self) -> bool {
        self.value.interrupted
    }
    #[getter]
    fn delivery(&self) -> Option<crate::metrics::DeliveryReport> {
        self.value
            .delivery
            .clone()
            .map(|value| crate::metrics::DeliveryReport { value })
    }
    fn __repr__(&self) -> String {
        format!("MonitorReport(records={}, filtered={}, diagnostics={}, transport_drops={}, interrupted={})",
            self.value.inspection.records(), self.value.filtered, self.value.diagnostics,
            self.value.transport_drops, self.value.interrupted)
    }
}
