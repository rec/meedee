//! File performance views and explicit export through the shared Rust clocks/codecs.
use crate::event_values::{Rational, Timestamp};
use crate::files::{closed, limit, os_error, uint, Event as SmfEvent, Header, Stream};
use crate::packets::Clock;
use crate::playback::ClockMapping;
use middy::{clip, smf};
use pyo3::prelude::*;
#[pyclass(name = "Timeline", module = "middy.smf")]
struct SmfTimeline {
    value: smf::Timeline,
}
#[pymethods]
impl SmfTimeline {
    #[new]
    #[pyo3(signature=(header,*,stream,sequence=None,payload_limit=1_048_576))]
    fn new(
        header: PyRef<'_, Header>,
        stream: String,
        sequence: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with=crate::units::bytes)] payload_limit: usize,
    ) -> PyResult<Self> {
        let sequence = sequence
            .map(uint)
            .transpose()?
            .map(|v| {
                u16::try_from(v)
                    .map_err(|_| pyo3::exceptions::PyValueError::new_err("sequence exceeds u16"))
            })
            .transpose()?;
        Ok(Self {
            value: smf::Timeline::new(header.value.clone(), stream, sequence, payload_limit)
                .map_err(os_error)?,
        })
    }
    #[getter]
    fn clock(&self) -> Clock {
        Clock {
            value: self.value.clock().clone(),
        }
    }
    #[getter]
    fn origin(&self) -> Timestamp {
        Timestamp {
            value: self.value.origin(),
        }
    }
    fn feed(&mut self, py: Python<'_>, event: PyRef<'_, SmfEvent>) -> PyResult<Vec<Py<PyAny>>> {
        self.value
            .feed(&event.value)
            .map_err(os_error)?
            .into_iter()
            .map(|v| crate::live::item(py, v))
            .collect()
    }
    fn finish(&mut self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.value
            .finish()
            .into_iter()
            .map(|v| crate::live::item(py, v))
            .collect()
    }
}
#[pyclass(name = "Timeline", module = "middy.clip")]
struct ClipTimeline {
    value: clip::Timeline,
}
#[pymethods]
impl ClipTimeline {
    #[new]
    #[pyo3(signature=(*,stream,include_configuration=false))]
    fn new(
        stream: String,
        #[pyo3(from_py_with=crate::ump::boolean)] include_configuration: bool,
    ) -> PyResult<Self> {
        Ok(Self {
            value: clip::Timeline::new(stream, include_configuration).map_err(os_error)?,
        })
    }
    #[getter]
    fn clock(&self) -> Option<Clock> {
        self.value.clock().cloned().map(|value| Clock { value })
    }
    #[getter]
    fn origin(&self) -> Option<Timestamp> {
        self.value.origin().map(|value| Timestamp { value })
    }
    fn feed(
        &mut self,
        py: Python<'_>,
        entry: PyRef<'_, crate::files::ClipEntry>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        self.value
            .feed(&entry.value)
            .map_err(os_error)?
            .into_iter()
            .map(|v| crate::live::item(py, v))
            .collect()
    }
    fn finish(&mut self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.value
            .finish()
            .into_iter()
            .map(|v| crate::live::item(py, v))
            .collect()
    }
}

#[pyclass(module = "middy.smf")]
struct MergedReader {
    value: Option<smf::MergedReader<Stream>>,
}
#[pymethods]
impl MergedReader {
    #[new]
    #[pyo3(signature=(source,*,sequence=None,payload_limit=1_048_576,track_limit=128,buffer_limit=8_388_608))]
    fn new(
        py: Python<'_>,
        source: &Bound<'_, PyAny>,
        sequence: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with=crate::units::bytes)] payload_limit: usize,
        #[pyo3(from_py_with=limit)] track_limit: usize,
        #[pyo3(from_py_with=crate::units::bytes)] buffer_limit: usize,
    ) -> PyResult<Self> {
        let sequence = sequence
            .map(uint)
            .transpose()?
            .map(|v| {
                v.try_into()
                    .map_err(|_| pyo3::exceptions::PyValueError::new_err("sequence exceeds u16"))
            })
            .transpose()?;
        let stream = Stream::input(source)?;
        Ok(Self {
            value: Some(
                py.detach(move || {
                    smf::MergedReader::new(
                        stream,
                        payload_limit,
                        track_limit,
                        buffer_limit,
                        sequence,
                    )
                })
                .map_err(os_error)?,
            ),
        })
    }
    #[getter]
    fn header(&self) -> PyResult<Header> {
        Ok(Header {
            value: self.value.as_ref().ok_or_else(closed)?.header.clone(),
        })
    }
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<SmfEvent>> {
        py.detach(|| {
            self.value
                .as_mut()
                .ok_or_else(|| std::io::Error::other("reader closed"))?
                .next()
                .transpose()
        })
        .map_err(os_error)
        .map(|v| v.map(|value| SmfEvent { value }))
    }
    fn close(&mut self) {
        self.value = None;
    }
}
#[pyclass(name = "TimedReader", module = "middy.smf")]
struct SmfTimedReader {
    value: Option<smf::TimedReader<Stream>>,
}
#[pymethods]
impl SmfTimedReader {
    #[new]
    #[pyo3(signature=(source,*,stream,sequence=None,payload_limit=1_048_576,track_limit=128,buffer_limit=8_388_608))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        source: &Bound<'_, PyAny>,
        stream: String,
        sequence: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with=crate::units::bytes)] payload_limit: usize,
        #[pyo3(from_py_with=limit)] track_limit: usize,
        #[pyo3(from_py_with=crate::units::bytes)] buffer_limit: usize,
    ) -> PyResult<Self> {
        let sequence = sequence
            .map(uint)
            .transpose()?
            .map(|v| {
                v.try_into()
                    .map_err(|_| pyo3::exceptions::PyValueError::new_err("sequence exceeds u16"))
            })
            .transpose()?;
        let source = Stream::input(source)?;
        let value = py
            .detach(move || {
                smf::TimedReader::new(
                    smf::MergedReader::new(
                        source,
                        payload_limit,
                        track_limit,
                        buffer_limit,
                        sequence,
                    )?,
                    stream,
                    sequence,
                    payload_limit,
                )
            })
            .map_err(os_error)?;
        Ok(Self { value: Some(value) })
    }
    #[getter]
    fn clock(&self) -> PyResult<Clock> {
        Ok(Clock {
            value: self.value.as_ref().ok_or_else(closed)?.clock().clone(),
        })
    }
    #[getter]
    fn origin(&self) -> PyResult<Timestamp> {
        Ok(Timestamp {
            value: self.value.as_ref().ok_or_else(closed)?.origin(),
        })
    }
    #[getter]
    fn header(&self) -> PyResult<Header> {
        Ok(Header {
            value: self.value.as_ref().ok_or_else(closed)?.header().clone(),
        })
    }
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        let item = py
            .detach(|| {
                self.value
                    .as_mut()
                    .ok_or_else(|| std::io::Error::other("reader closed"))?
                    .next()
                    .transpose()
            })
            .map_err(os_error)?;
        item.map(|v| crate::live::item(py, v)).transpose()
    }
    fn close(&mut self) {
        self.value = None;
    }
}
#[pyclass(name = "TimedReader", module = "middy.clip")]
struct ClipTimedReader {
    value: Option<clip::TimedReader<Stream>>,
}
#[pymethods]
impl ClipTimedReader {
    #[new]
    #[pyo3(signature=(source,*,stream,include_configuration=false))]
    fn new(
        py: Python<'_>,
        source: &Bound<'_, PyAny>,
        stream: String,
        #[pyo3(from_py_with=crate::ump::boolean)] include_configuration: bool,
    ) -> PyResult<Self> {
        let source = Stream::input(source)?;
        let value = py
            .detach(move || {
                clip::TimedReader::new(
                    middy::clip::Reader::new(source, false)?,
                    stream,
                    include_configuration,
                )
            })
            .map_err(os_error)?;
        Ok(Self { value: Some(value) })
    }
    #[getter]
    fn clock(&self) -> PyResult<Clock> {
        Ok(Clock {
            value: self.value.as_ref().ok_or_else(closed)?.clock().clone(),
        })
    }
    #[getter]
    fn origin(&self) -> PyResult<Timestamp> {
        Ok(Timestamp {
            value: self.value.as_ref().ok_or_else(closed)?.origin(),
        })
    }
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        let item = py
            .detach(|| {
                self.value
                    .as_mut()
                    .ok_or_else(|| std::io::Error::other("reader closed"))?
                    .next()
                    .transpose()
            })
            .map_err(os_error)?;
        item.map(|v| crate::live::item(py, v)).transpose()
    }
    fn close(&mut self) {
        self.value = None;
    }
}
#[pyclass(frozen, module = "middy.smf")]
struct ExportReport {
    value: smf::ExportReport,
}
#[pymethods]
impl ExportReport {
    #[getter]
    fn status(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(PyModule::import(py, "middy.translation")?
            .getattr("TranslationStatus")?
            .getattr(format!("{:?}", self.value.status))?
            .unbind())
    }
    #[getter]
    fn reason(&self) -> &str {
        &self.value.reason
    }
    #[getter]
    fn rounding_error_ticks(&self) -> Rational {
        Rational {
            value: self.value.rounding_error_ticks,
        }
    }
    #[getter]
    fn events(&self) -> Vec<SmfEvent> {
        self.value
            .events
            .iter()
            .cloned()
            .map(|value| SmfEvent { value })
            .collect()
    }
}
#[pyfunction]
#[pyo3(signature=(event,*,header,origin,track=0,tempo=500_000,mapping=None,allow_loss=false), text_signature="(event, *, header, origin, track=0, tempo=120.0, mapping=None, allow_loss=False)")]
#[allow(clippy::too_many_arguments)]
fn export_event(
    event: &Bound<'_, PyAny>,
    header: PyRef<'_, Header>,
    origin: PyRef<'_, Timestamp>,
    #[pyo3(from_py_with=uint)] track: u64,
    #[pyo3(from_py_with=crate::units::tempo)] tempo: u64,
    mapping: Option<PyRef<'_, ClockMapping>>,
    #[pyo3(from_py_with=crate::ump::boolean)] allow_loss: bool,
) -> PyResult<ExportReport> {
    let event = crate::event_values::extract(event)?;
    let track = track
        .try_into()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("track exceeds u16"))?;
    let tempo = tempo
        .try_into()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("tempo exceeds u32"))?;
    Ok(ExportReport {
        value: smf::export_event(
            &event,
            &header.value,
            &origin.value,
            track,
            tempo,
            mapping.as_ref().map(|m| &m.value),
            allow_loss,
        )
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
    })
}
pub fn register_smf(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<SmfTimeline>()?;
    m.add_class::<MergedReader>()?;
    m.add_class::<SmfTimedReader>()?;
    m.add_class::<ExportReport>()?;
    m.add_function(wrap_pyfunction!(export_event, m)?)?;
    Ok(())
}
pub fn register_clip(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ClipTimeline>()?;
    m.add_class::<ClipTimedReader>()
}
