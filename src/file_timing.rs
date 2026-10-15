//! File performance views and explicit export through the shared Rust clocks/codecs.
use crate::event_values::{Event, Rational, Timestamp};
use crate::files::{closed, limit, os_error, uint, Event as SmfEvent, Header, Stream};
use crate::packets::Clock;
use crate::playback::ClockMapping;
use meedee::{file_timing as model, smf};
use pyo3::prelude::*;
#[pyclass(module = "meedee.smf")]
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
        #[pyo3(from_py_with=limit)] payload_limit: usize,
        #[pyo3(from_py_with=limit)] track_limit: usize,
        #[pyo3(from_py_with=limit)] buffer_limit: usize,
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
#[pyclass(name = "TimedReader", module = "meedee.smf")]
struct SmfTimedReader {
    value: Option<model::SmfReader<Stream>>,
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
        #[pyo3(from_py_with=limit)] payload_limit: usize,
        #[pyo3(from_py_with=limit)] track_limit: usize,
        #[pyo3(from_py_with=limit)] buffer_limit: usize,
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
                model::SmfReader::new(
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
#[pyclass(name = "TimedReader", module = "meedee.clip")]
struct ClipTimedReader {
    value: Option<model::ClipReader<Stream>>,
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
                model::ClipReader::new(
                    meedee::clip::Reader::new(source, false)?,
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
#[pyclass(frozen, module = "meedee.smf")]
struct ExportReport {
    value: model::ExportReport,
}
#[pymethods]
impl ExportReport {
    #[getter]
    fn status(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(PyModule::import(py, "meedee.ump")?
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
#[pyo3(signature=(event,*,header,origin,track=0,tempo=500_000,mapping=None,allow_loss=false))]
#[allow(clippy::too_many_arguments)]
fn export_event(
    event: PyRef<'_, Event>,
    header: PyRef<'_, Header>,
    origin: PyRef<'_, Timestamp>,
    #[pyo3(from_py_with=uint)] track: u64,
    #[pyo3(from_py_with=uint)] tempo: u64,
    mapping: Option<PyRef<'_, ClockMapping>>,
    #[pyo3(from_py_with=crate::ump::boolean)] allow_loss: bool,
) -> PyResult<ExportReport> {
    let track = track
        .try_into()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("track exceeds u16"))?;
    let tempo = tempo
        .try_into()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("tempo exceeds u32"))?;
    Ok(ExportReport {
        value: model::export_smf(
            &event.value,
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
    m.add_class::<MergedReader>()?;
    m.add_class::<SmfTimedReader>()?;
    m.add_class::<ExportReport>()?;
    m.add_function(wrap_pyfunction!(export_event, m)?)?;
    Ok(())
}
pub fn register_clip(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ClipTimedReader>()
}
