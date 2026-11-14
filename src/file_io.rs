//! Unified file API; format detection, validation and transformation stay in Rust.
use crate::files::{closed, os_error, Stream};
use middy::file_io as model;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use std::path::PathBuf;

#[pyclass(module = "middy.file_io")]
struct Reader {
    value: Option<model::Reader<Stream>>,
    complete: bool,
    problem: Option<String>,
}
#[pymethods]
impl Reader {
    #[new]
    #[pyo3(signature=(source,*,payload_limit=1_048_576,recover=false,preserve_unknown=false))]
    fn new(
        source: &Bound<'_, PyAny>,
        #[pyo3(from_py_with=crate::units::bytes)] payload_limit: usize,
        #[pyo3(from_py_with=crate::ump::boolean)] recover: bool,
        #[pyo3(from_py_with=crate::ump::boolean)] preserve_unknown: bool,
    ) -> PyResult<Self> {
        Ok(Self {
            value: Some(
                model::Reader::new(
                    Stream::input(source)?,
                    payload_limit,
                    recover,
                    preserve_unknown,
                )
                .map_err(os_error)?,
            ),
            complete: false,
            problem: None,
        })
    }
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        let reader = self.value.as_mut().ok_or_else(closed)?;
        let result = py.detach(|| reader.next().transpose());
        self.complete = reader.complete();
        self.problem = reader.problem().map(str::to_owned);
        result
            .map_err(os_error)?
            .map(|record| match record {
                model::Record::Archive(record) => crate::archive::to_python(py, record),
                model::Record::Header(value) => {
                    Ok(Py::new(py, crate::files::Header { value })?.into_any())
                }
                model::Record::Event(value) => {
                    Ok(Py::new(py, crate::files::Event { value })?.into_any())
                }
            })
            .transpose()
    }
    fn __enter__(slf: PyRef<'_, Self>) -> PyResult<PyRef<'_, Self>> {
        slf.value.as_ref().ok_or_else(closed)?;
        Ok(slf)
    }
    fn __exit__(
        &mut self,
        _kind: &Bound<'_, PyAny>,
        _error: &Bound<'_, PyAny>,
        _traceback: &Bound<'_, PyAny>,
    ) {
        self.close();
    }
    fn close(&mut self) {
        self.value = None;
    }
    #[getter]
    fn complete(&self) -> bool {
        self.complete
    }
    #[getter]
    fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }
}

#[pyclass(module = "middy.file_io")]
struct Writer {
    value: Option<model::Writer<Stream>>,
}
impl Writer {
    fn open(
        destination: &Bound<'_, PyAny>,
        format: Option<&str>,
        payload_limit: usize,
        overwrite: bool,
    ) -> PyResult<Self> {
        let extension = if destination.hasattr("write")? {
            format.unwrap_or("middy").to_ascii_lowercase()
        } else {
            if format.is_some() {
                return Err(PyValueError::new_err(
                    "format applies only to stream destinations; use the filename extension",
                ));
            }
            let path = destination.extract::<PathBuf>()?;
            path.extension()
                .and_then(|v| v.to_str())
                .unwrap_or("")
                .to_ascii_lowercase()
        };
        if !matches!(extension.as_str(), "mid" | "midi" | "middy" | "syx") {
            return Err(PyValueError::new_err(
                "expected .mid, .midi, .syx or .middy destination",
            ));
        }
        if payload_limit == 0 {
            return Err(PyValueError::new_err("payload limit must be positive"));
        }
        Ok(Self {
            value: Some(
                model::Writer::new(
                    if overwrite {
                        Stream::output(destination)?
                    } else {
                        Stream::exclusive_output(destination)?
                    },
                    &extension,
                    payload_limit,
                )
                .map_err(os_error)?,
            ),
        })
    }
}
#[pymethods]
impl Writer {
    #[new]
    #[pyo3(signature=(destination,*,format=None,payload_limit=1_048_576))]
    fn new(
        destination: &Bound<'_, PyAny>,
        format: Option<&str>,
        #[pyo3(from_py_with=crate::units::bytes)] payload_limit: usize,
    ) -> PyResult<Self> {
        Self::open(destination, format, payload_limit, false)
    }
    fn write(&mut self, py: Python<'_>, record: &Bound<'_, PyAny>) -> PyResult<()> {
        let record = if let Ok(header) = record.extract::<PyRef<'_, crate::files::Header>>() {
            model::Record::Header(header.value.clone())
        } else if let Ok(event) = record.extract::<PyRef<'_, crate::files::Event>>() {
            model::Record::Event(event.value.clone())
        } else {
            match crate::archive::extract(record) {
                Ok(value) => model::Record::Archive(value),
                Err(_) if record.is_instance_of::<crate::messages::Message>() => {
                    let wire = crate::messages::encode_message(record)?;
                    if wire.first() != Some(&240) || wire.last() != Some(&247) {
                        return Err(PyValueError::new_err("expected a structured SysEx message"));
                    }
                    let message = middy::messages::SysEx::new(wire[1..wire.len() - 1].to_vec())
                        .map_err(|e| PyValueError::new_err(e.to_string()))?;
                    let payload = crate::event_values::with_metadata(
                        record,
                        middy::events::Payload::Message(middy::messages::AnyMessage::SysEx(
                            message,
                        )),
                    )?;
                    model::Record::Archive(middy::archive::Record::Event(Box::new(payload)))
                }
                Err(error) => return Err(error),
            }
        };
        let writer = self.value.as_mut().ok_or_else(closed)?;
        py.detach(|| writer.write(&record)).map_err(os_error)
    }
    #[pyo3(signature=(*,complete=true,reason=""))]
    fn finish(
        &mut self,
        py: Python<'_>,
        #[pyo3(from_py_with=crate::ump::boolean)] complete: bool,
        reason: &str,
    ) -> PyResult<()> {
        let writer = self.value.as_mut().ok_or_else(closed)?;
        py.detach(|| writer.finish_with_status(complete, reason))
            .map_err(os_error)?;
        self.value = None;
        Ok(())
    }
    fn __enter__(slf: PyRef<'_, Self>) -> PyResult<PyRef<'_, Self>> {
        slf.value.as_ref().ok_or_else(closed)?;
        Ok(slf)
    }
    fn __exit__(
        &mut self,
        py: Python<'_>,
        kind: &Bound<'_, PyAny>,
        _error: &Bound<'_, PyAny>,
        _traceback: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        if kind.is_none() && self.value.is_some() {
            let result = self.finish(py, true, "");
            self.value = None;
            result
        } else {
            self.value = None;
            Ok(())
        }
    }
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Reader>()?;
    module.add_class::<Writer>()?;
    module.add_function(wrap_pyfunction!(write, module)?)
}

/// Write an iterable of records using the filename's format and finish on success.
/// Preserve existing files unless overwrite is true; failures can leave a partial file.
#[pyfunction]
#[pyo3(signature=(records,filename,overwrite=false))]
fn write(
    py: Python<'_>,
    records: &Bound<'_, PyAny>,
    filename: PathBuf,
    #[pyo3(from_py_with=crate::ump::boolean)] overwrite: bool,
) -> PyResult<()> {
    let records = records.try_iter()?;
    let filename = filename.into_pyobject(py)?;
    let mut writer = Writer::open(filename.as_any(), None, model::DEFAULT_LIMIT, overwrite)?;
    for record in records {
        writer.write(py, &record?)?;
    }
    writer.finish(py, true, "")
}
