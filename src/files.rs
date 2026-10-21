//! Python stream adapters and Rust file codecs. Caller-owned streams stay open.
use crate::messages;
use meedee::{clip, smf, syx};
use pyo3::buffer::PyBuffer;
use pyo3::exceptions::{PyOSError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyTuple};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

pub enum Stream {
    File(File),
    Python(Py<PyAny>),
}
impl Stream {
    pub fn exclusive_output(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        if value.hasattr("write")? {
            Ok(Self::Python(value.clone().unbind()))
        } else {
            Ok(Self::File(
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(value.extract::<PathBuf>()?)
                    .map_err(os_error)?,
            ))
        }
    }
    pub fn input(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        if value.hasattr("read")? {
            Ok(Self::Python(value.clone().unbind()))
        } else {
            Ok(Self::File(
                File::open(value.extract::<PathBuf>()?).map_err(os_error)?,
            ))
        }
    }
    pub fn output(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        if value.hasattr("write")? {
            Ok(Self::Python(value.clone().unbind()))
        } else {
            Ok(Self::File(
                File::create(value.extract::<PathBuf>()?).map_err(os_error)?,
            ))
        }
    }
}
fn stream_error(e: PyErr) -> io::Error {
    io::Error::other(e.to_string())
}
impl Read for Stream {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::File(f) => f.read(b),
            Self::Python(o) => Python::attach(|py| {
                let value = o
                    .bind(py)
                    .call_method1("read", (b.len(),))
                    .map_err(stream_error)?;
                let buffer = PyBuffer::<u8>::get(&value).map_err(stream_error)?;
                if buffer.dimensions() != 1 || buffer.len_bytes() > b.len() {
                    return Err(io::Error::other("read returned more bytes than requested"));
                }
                let bytes = buffer.to_vec(py).map_err(stream_error)?;
                b[..bytes.len()].copy_from_slice(&bytes);
                Ok(bytes.len())
            }),
        }
    }
}
impl Write for Stream {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        match self {
            Self::File(f) => f.write(b),
            Self::Python(o) => Python::attach(|py| {
                let result = o
                    .bind(py)
                    .call_method1("write", (PyBytes::new(py, b),))
                    .map_err(stream_error)?;
                if !result.is_exact_instance_of::<pyo3::types::PyInt>() {
                    return Err(io::Error::other(
                        "write must return a built-in integer byte count",
                    ));
                }
                let n = result.extract::<usize>().map_err(stream_error)?;
                if n > b.len() {
                    return Err(io::Error::other("write returned an invalid byte count"));
                }
                Ok(n)
            }),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::File(f) => f.flush(),
            Self::Python(o) => Python::attach(|py| {
                o.bind(py)
                    .call_method0("flush")
                    .map(|_| ())
                    .map_err(stream_error)
            }),
        }
    }
}
impl Seek for Stream {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        match self {
            Self::File(f) => f.seek(p),
            Self::Python(o) => Python::attach(|py| {
                let (a, b) = match p {
                    SeekFrom::Start(n) => (i128::from(n), 0),
                    SeekFrom::Current(n) => (i128::from(n), 1),
                    SeekFrom::End(n) => (i128::from(n), 2),
                };
                let result = o
                    .bind(py)
                    .call_method1("seek", (a, b))
                    .map_err(stream_error)?;
                if !result.is_exact_instance_of::<pyo3::types::PyInt>() {
                    return Err(io::Error::other(
                        "seek must return a built-in integer position",
                    ));
                }
                result.extract().map_err(stream_error)
            }),
        }
    }
}
pub fn os_error(e: io::Error) -> PyErr {
    PyOSError::new_err(e.to_string())
}
pub fn bytes(value: &Bound<'_, PyAny>, limit: usize) -> PyResult<Vec<u8>> {
    let b = PyBuffer::<u8>::get(value)?;
    if b.dimensions() != 1 {
        return Err(PyTypeError::new_err("expected one-dimensional byte buffer"));
    }
    if b.len_bytes() > limit {
        return Err(PyValueError::new_err("payload limit exceeded"));
    }
    b.to_vec(value.py())
}
pub fn uint(value: &Bound<'_, PyAny>) -> PyResult<u64> {
    if !value.is_exact_instance_of::<pyo3::types::PyInt>() {
        return Err(PyTypeError::new_err("expected a built-in integer"));
    }
    value
        .extract()
        .map_err(|_| PyValueError::new_err("integer must fit unsigned 64 bits"))
}
pub fn limit(value: &Bound<'_, PyAny>) -> PyResult<usize> {
    let n = uint(value)?;
    if n == 0 {
        return Err(PyValueError::new_err("limit must be positive"));
    }
    n.try_into()
        .map_err(|_| PyValueError::new_err("limit exceeds platform size"))
}
macro_rules! frozen{($name:ident;$($field:ident),*;{$($method:item)*})=>{
#[pymethods]impl $name{$($method)*
fn __eq__(&self,other:&Bound<'_,PyAny>)->bool{other.extract::<PyRef<'_,Self>>().is_ok_and(|v|self.value==v.value)}
fn __hash__(&self)->u64{crate::tuning::hash(&self.value)}
fn __repr__(slf:PyRef<'_,Self>)->PyResult<String>{let py=slf.py();crate::tuning::render(slf.into_pyobject(py)?.as_any(),stringify!($name),&[$(stringify!($field)),*])}
#[pyo3(signature=(**changes))]fn replace<'py>(slf:PyRef<'py,Self>,changes:Option<&Bound<'py,PyDict>>)->PyResult<Bound<'py,PyAny>>{let py=slf.py();crate::tuning::replacement(slf.into_pyobject(py)?.as_any(),&[$(stringify!($field)),*],changes)}
#[classattr]fn __match_args__(py:Python<'_>)->PyResult<Bound<'_,PyTuple>>{let f:&[&str]=&[$(stringify!($field)),*];PyTuple::new(py,f)}
}};}

pub(crate) use frozen;
#[pyclass(frozen, module = "meedee.smf")]
pub struct Header {
    pub(crate) value: smf::Header,
}
frozen!(Header;format,tracks,division,extra;{
#[new]#[pyo3(signature=(*,format,tracks,division,extra=None))]fn new(format:&Bound<'_,PyAny>,tracks:&Bound<'_,PyAny>,division:&Bound<'_,PyAny>,extra:Option<&Bound<'_,PyAny>>)->PyResult<Self>{Ok(Self{value:smf::Header::new(uint(format)?.try_into().map_err(|_|PyValueError::new_err("format out of range"))?,uint(tracks)?.try_into().map_err(|_|PyValueError::new_err("tracks out of range"))?,uint(division)?.try_into().map_err(|_|PyValueError::new_err("division out of range"))?,extra.map(|v|bytes(v,usize::MAX)).transpose()?.unwrap_or_default()).map_err(|e|PyValueError::new_err(e.to_string()))?})}
#[getter]fn format(&self)->u8{self.value.format()}#[getter]fn tracks(&self)->u16{self.value.tracks()}#[getter]fn division(&self)->u16{self.value.division()}#[getter]fn extra<'py>(&self,py:Python<'py>)->Bound<'py,PyBytes>{PyBytes::new(py,self.value.extra())}
});
#[pyclass(frozen, module = "meedee.smf")]
pub struct Meta {
    value: (u8, Vec<u8>),
}
frozen!(Meta;kind,data;{
#[new]#[pyo3(signature=(*,kind,data))]fn new(kind:&Bound<'_,PyAny>,data:&Bound<'_,PyAny>)->PyResult<Self>{let kind=uint(kind)?.try_into().map_err(|_|PyValueError::new_err("meta kind out of range"))?;let data=bytes(data,usize::MAX)?;smf::validate_meta(kind,&data,0).map_err(|e|PyValueError::new_err(e.to_string()))?;Ok(Self{value:(kind,data)})}
#[getter]fn kind(&self)->u8{self.value.0}#[getter]fn data<'py>(&self,py:Python<'py>)->Bound<'py,PyBytes>{PyBytes::new(py,&self.value.1)}
});
#[pyclass(frozen, module = "meedee.smf")]
pub struct SysExFragment {
    value: (u8, Vec<u8>),
}
frozen!(SysExFragment;status,data;{
#[new]#[pyo3(signature=(*,status,data))]fn new(status:&Bound<'_,PyAny>,data:&Bound<'_,PyAny>)->PyResult<Self>{let status=uint(status)?;if status!=240&&status!=247{return Err(PyValueError::new_err("status must be F0 or F7"));}Ok(Self{value:(status as u8,bytes(data,usize::MAX)?)})}
#[getter]fn status(&self)->u8{self.value.0}#[getter]fn data<'py>(&self,py:Python<'py>)->Bound<'py,PyBytes>{PyBytes::new(py,&self.value.1)}
});
#[pyclass(frozen, module = "meedee.smf")]
pub struct UnknownChunk {
    value: ([u8; 4], Vec<u8>),
}
frozen!(UnknownChunk;id,data;{
#[new]#[pyo3(signature=(*,id,data))]fn new(id:&Bound<'_,PyAny>,data:&Bound<'_,PyAny>)->PyResult<Self>{let id=bytes(id,4)?.try_into().map_err(|_|PyValueError::new_err("chunk id must be four bytes"))?;if id==*b"MThd"||id==*b"MTrk"{return Err(PyValueError::new_err("known chunk ID"));}Ok(Self{value:(id,bytes(data,usize::MAX)?)})}
#[getter]fn id<'py>(&self,py:Python<'py>)->Bound<'py,PyBytes>{PyBytes::new(py,&self.value.0)}#[getter]fn data<'py>(&self,py:Python<'py>)->Bound<'py,PyBytes>{PyBytes::new(py,&self.value.1)}
});
#[pyclass(frozen, module = "meedee.smf")]
pub struct Event {
    pub value: smf::Event,
}
frozen!(Event;track,tick,record;{
#[new]#[pyo3(signature=(*,track,tick,record))]fn new(track:&Bound<'_,PyAny>,tick:&Bound<'_,PyAny>,record:&Bound<'_,PyAny>)->PyResult<Self>{Ok(Self{value:smf::Event{track:uint(track)?.try_into().map_err(|_|PyValueError::new_err("track out of range"))?,tick:uint(tick)?,record:extract_record(record)?}})}
#[getter]fn track(&self)->u16{self.value.track}#[getter]fn tick(&self)->u64{self.value.tick}#[getter]fn record(&self,py:Python<'_>)->PyResult<Py<PyAny>>{record_python(py,self.value.record.clone())}
});
fn extract_record(v: &Bound<'_, PyAny>) -> PyResult<smf::Record> {
    if let Ok(v) = v.extract::<PyRef<'_, Meta>>() {
        return Ok(smf::Record::Meta {
            kind: v.value.0,
            data: v.value.1.clone(),
        });
    }
    if let Ok(v) = v.extract::<PyRef<'_, SysExFragment>>() {
        return Ok(smf::Record::SysEx {
            status: v.value.0,
            data: v.value.1.clone(),
        });
    }
    if let Ok(v) = v.extract::<PyRef<'_, UnknownChunk>>() {
        return Ok(smf::Record::Chunk {
            id: v.value.0,
            data: v.value.1.clone(),
        });
    }
    Ok(smf::Record::Message(messages::extract_primitive(v)?))
}
fn record_python(py: Python<'_>, r: smf::Record) -> PyResult<Py<PyAny>> {
    match r {
        smf::Record::Message(v) => messages::to_python(py, v),
        smf::Record::Meta { kind, data } => Ok(Py::new(
            py,
            Meta {
                value: (kind, data),
            },
        )?
        .into_any()),
        smf::Record::SysEx { status, data } => Ok(Py::new(
            py,
            SysExFragment {
                value: (status, data),
            },
        )?
        .into_any()),
        smf::Record::Chunk { id, data } => {
            Ok(Py::new(py, UnknownChunk { value: (id, data) })?.into_any())
        }
    }
}
#[pyclass(module = "meedee.smf")]
struct Reader {
    value: Option<smf::Reader<Stream>>,
}
#[pymethods]
impl Reader {
    #[new]
    #[pyo3(signature=(source,*,payload_limit=1_048_576))]
    fn new(
        source: &Bound<'_, PyAny>,
        #[pyo3(from_py_with=limit)] payload_limit: usize,
    ) -> PyResult<Self> {
        Ok(Self {
            value: Some(smf::Reader::new(Stream::input(source)?, payload_limit).map_err(os_error)?),
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
    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<Event>> {
        let value = self.value.as_mut().ok_or_else(closed)?;
        py.detach(|| value.next().transpose())
            .map(|v| v.map(|value| Event { value }))
            .map_err(os_error)
    }
    fn close(&mut self) {
        self.value = None;
    }
}
#[pyclass(module = "meedee.smf")]
struct Writer {
    value: Option<smf::Writer<Stream>>,
}
#[pymethods]
impl Writer {
    #[new]
    #[pyo3(signature=(destination,*,header,payload_limit=1_048_576))]
    fn new(
        destination: &Bound<'_, PyAny>,
        header: PyRef<'_, Header>,
        #[pyo3(from_py_with=limit)] payload_limit: usize,
    ) -> PyResult<Self> {
        Ok(Self {
            value: Some(
                smf::Writer::new(
                    Stream::output(destination)?,
                    header.value.clone(),
                    payload_limit,
                )
                .map_err(os_error)?,
            ),
        })
    }
    fn write(&mut self, py: Python<'_>, event: PyRef<'_, Event>) -> PyResult<()> {
        let event = event.value.clone();
        let value = self.value.as_mut().ok_or_else(closed)?;
        py.detach(|| value.write(&event)).map_err(os_error)
    }
    fn finish(&mut self, py: Python<'_>) -> PyResult<()> {
        let value = self.value.as_mut().ok_or_else(closed)?;
        py.detach(|| value.finish()).map_err(os_error)?;
        self.value = None;
        Ok(())
    }
}
pub fn closed() -> PyErr {
    PyValueError::new_err("file codec is closed")
}
#[pyclass(name = "Reader", module = "meedee.syx")]
struct SyxReader {
    value: Option<syx::Reader<Stream>>,
}
#[pymethods]
impl SyxReader {
    #[new]
    #[pyo3(signature=(source,*,payload_limit=1_048_576))]
    fn new(
        source: &Bound<'_, PyAny>,
        #[pyo3(from_py_with=limit)] payload_limit: usize,
    ) -> PyResult<Self> {
        Ok(Self {
            value: Some(syx::Reader::new(Stream::input(source)?, payload_limit).map_err(os_error)?),
        })
    }
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        let value = self.value.as_mut().ok_or_else(closed)?;
        py.detach(|| value.next().transpose())
            .map_err(os_error)?
            .map(|v| messages::to_python(py, meedee::messages::AnyMessage::SysEx(v)))
            .transpose()
    }
    fn close(&mut self) {
        self.value = None;
    }
}
#[pyfunction]
#[pyo3(signature=(destination,messages,*,payload_limit=1_048_576))]
fn write_syx(
    destination: &Bound<'_, PyAny>,
    messages: &Bound<'_, PyAny>,
    #[pyo3(from_py_with=limit)] payload_limit: usize,
) -> PyResult<()> {
    let mut stream = Stream::output(destination)?;
    for v in messages.try_iter()? {
        let v = messages::extract_primitive(&v?)?;
        let meedee::messages::AnyMessage::SysEx(v) = v else {
            return Err(PyTypeError::new_err("SYX accepts only SysEx"));
        };
        destination
            .py()
            .detach(|| syx::write(&mut stream, &v, payload_limit))
            .map_err(os_error)?;
    }
    destination.py().detach(|| stream.flush()).map_err(os_error)
}
#[pyclass(name = "Entry", frozen, module = "meedee.clip")]
struct ClipEntry {
    value: clip::Entry,
}
#[pymethods]
impl ClipEntry {
    #[getter]
    fn tick(&self) -> Option<u64> {
        self.value.tick
    }
    #[getter]
    fn sequence(&self) -> bool {
        self.value.sequence
    }
    #[getter]
    fn message(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        crate::ump::to_python(py, self.value.message.clone())
    }
}
#[pyclass(name = "Reader", module = "meedee.clip")]
struct ClipReader {
    value: Option<clip::Reader<Stream>>,
}
#[pymethods]
impl ClipReader {
    #[new]
    #[pyo3(signature=(source,*,preserve_unknown=false))]
    fn new(
        source: &Bound<'_, PyAny>,
        #[pyo3(from_py_with=crate::ump::boolean)] preserve_unknown: bool,
    ) -> PyResult<Self> {
        Ok(Self {
            value: Some(
                clip::Reader::new(Stream::input(source)?, preserve_unknown).map_err(os_error)?,
            ),
        })
    }
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<ClipEntry>> {
        let value = self.value.as_mut().ok_or_else(closed)?;
        py.detach(|| value.next().transpose())
            .map_err(os_error)
            .map(|v| v.map(|value| ClipEntry { value }))
    }
    fn close(&mut self) {
        self.value = None;
    }
}
#[pyclass(name = "Writer", module = "meedee.clip")]
struct ClipWriter {
    value: Option<clip::Writer<Stream>>,
}
#[pymethods]
impl ClipWriter {
    #[new]
    fn new(destination: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self {
            value: Some(clip::Writer::new(Stream::output(destination)?).map_err(os_error)?),
        })
    }
    fn write(&mut self, py: Python<'_>, message: &Bound<'_, PyAny>) -> PyResult<()> {
        let message = crate::ump::extract(message)?;
        let value = self.value.as_mut().ok_or_else(closed)?;
        py.detach(|| value.write(&message)).map_err(os_error)
    }
    fn finish(&mut self, py: Python<'_>) -> PyResult<()> {
        let value = self.value.as_mut().ok_or_else(closed)?;
        py.detach(|| value.finish()).map_err(os_error)?;
        self.value = None;
        Ok(())
    }
}
pub fn register_smf(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Header>()?;
    m.add_class::<Meta>()?;
    m.add_class::<SysExFragment>()?;
    m.add_class::<UnknownChunk>()?;
    m.add_class::<Event>()?;
    m.add_class::<Reader>()?;
    m.add_class::<Writer>()
}
pub fn register_syx(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("Reader", m.py().get_type::<SyxReader>())?;
    m.add("write", wrap_pyfunction!(write_syx, m)?)?;
    Ok(())
}
pub fn register_clip(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("Entry", m.py().get_type::<ClipEntry>())?;
    m.add("Reader", m.py().get_type::<ClipReader>())?;
    m.add("Writer", m.py().get_type::<ClipWriter>())
}
