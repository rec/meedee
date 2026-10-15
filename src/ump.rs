//! One pure UMP namespace backed by Rust-owned values. No native MIDI runtime.
use crate::messages::{self, integer, Message};
use crate::tuning::{render, replacement};
use meedee::ump as model;
use pyo3::buffer::PyBuffer;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyDict, PyInt, PyList, PyTuple};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
include!("../crates/meedee/src/ump_values.rs");

macro_rules! value_methods {
    ($name:ident; $($field:ident),*; {$($methods:item)*})=>{
        #[pymethods]
        impl $name {
            $($methods)*
            fn __eq__(&self,other:&Bound<'_,PyAny>)->bool{other.extract::<PyRef<'_,Self>>().is_ok_and(|v|self.value==v.value)}
            fn __hash__(&self)->u64{hash(&self.value)}
            fn __repr__(slf:PyRef<'_,Self>)->PyResult<String>{let py=slf.py();render(slf.into_pyobject(py)?.as_any(),stringify!($name),&[$(stringify!($field)),*])}
            #[pyo3(signature=(**changes))]
            fn replace<'py>(slf:PyRef<'py,Self>,changes:Option<&Bound<'py,PyDict>>)->PyResult<Bound<'py,PyAny>>{let py=slf.py();replacement(slf.into_pyobject(py)?.as_any(),&[$(stringify!($field)),*],changes)}
            #[classattr]fn __match_args__(py:Python<'_>)->PyResult<Bound<'_,PyTuple>>{let fields:&[&str]=&[$(stringify!($field)),*];PyTuple::new(py,fields)}
        }
    };
}
macro_rules! scalar {
    (Noop [$first:expr,$second:expr] {}; $($name:ident [$a:expr,$b:expr] {$($field:ident:$ty:ty = $lo:literal ..= $hi:literal => [$word:literal,$shift:literal,$mask:literal,$bias:literal]),*};)*)=>{
        #[pyclass(frozen,extends=Message,module="meedee.ump")]
        struct Noop {value:model::Noop}
        value_methods!(Noop;;{#[new]fn new()->PyClassInitializer<Self>{PyClassInitializer::from(Message).add_subclass(Self{value:model::Noop::new().expect("constant Noop")})}});
        $(#[pyclass(frozen,extends=Message,module="meedee.ump")]
        struct $name{value:model::$name}
        value_methods!($name;$($field),*;{
            #[new]#[pyo3(signature=(*,$($field),*))]
            #[allow(clippy::too_many_arguments)]
            fn new($($field:&Bound<'_,PyAny>),*)->PyResult<PyClassInitializer<Self>>{Ok(PyClassInitializer::from(Message).add_subclass(Self{value:model::$name::new($(integer($field,stringify!($field))?),*).map_err(error)?}))}
            $(#[getter]fn $field(&self)->$ty{self.value.$field()})*
            #[classattr]fn __annotations__(py:Python<'_>)->PyResult<Bound<'_,PyDict>>{let d=PyDict::new(py);$(d.set_item(stringify!($field),py.get_type::<PyInt>())?;)*Ok(d)}
        });)*
        fn extract(value:&Bound<'_,PyAny>)->PyResult<model::UmpMessage>{
            if let Ok(v)=value.extract::<PyRef<'_,Noop>>(){return Ok(model::UmpMessage::Noop(v.value.clone()));}
            $(if let Ok(v)=value.extract::<PyRef<'_,$name>>(){return Ok(model::UmpMessage::$name(v.value.clone()));})*
            extract_packet(value)
        }
        fn to_python(py:Python<'_>,value:model::UmpMessage)->PyResult<Py<PyAny>>{match value{
            model::UmpMessage::Noop(value)=>object(py,Noop{value}),
            $(model::UmpMessage::$name(value)=>object(py,$name{value}),)*
            model::UmpMessage::Midi1(value)=>object(py,Midi1Packet{value}),
            model::UmpMessage::SysEx7(value)=>object(py,SysEx7Packet{value}),
            model::UmpMessage::SysEx8(value)=>object(py,SysEx8Packet{value}),
            model::UmpMessage::MixedDataSet(value)=>object(py,MixedDataSet{value}),
            model::UmpMessage::FlexData(value)=>object(py,FlexData{value}),
            model::UmpMessage::Stream(value)=>object(py,StreamMessage{value}),
            model::UmpMessage::Opaque(value)=>object(py,OpaquePacket{value}),
        }}
        pub(crate) fn register(module:&Bound<'_,PyModule>)->PyResult<()>{module.add_class::<Noop>()?;$(module.add_class::<$name>()?;)*register_packets(module)}
    };
}
scalar_values!(scalar);
#[pyclass(frozen,extends=Message,module="meedee.ump")]
struct Midi1Packet {
    value: model::Midi1Packet,
}
value_methods!(Midi1Packet;group,message;{
    #[new]#[pyo3(signature=(*,group,message))]
    fn new(group:&Bound<'_,PyAny>,message:&Bound<'_,PyAny>)->PyResult<PyClassInitializer<Self>>{Ok(PyClassInitializer::from(Message).add_subclass(Self{value:model::Midi1Packet::new(int(group)?,messages::extract_primitive(message)?).map_err(error)?}))}
    #[getter]fn group(&self)->u8{self.value.group()}
    #[getter]fn message(&self,py:Python<'_>)->PyResult<Py<PyAny>>{messages::to_python(py,self.value.message().clone())}
});
#[pyclass(frozen,extends=Message,module="meedee.ump")]
struct SysEx7Packet {
    value: model::SysEx7Packet,
}
value_methods!(SysEx7Packet;group,format,data;{
    #[new]#[pyo3(signature=(*,group,format,data))]
    fn new(py:Python<'_>,group:&Bound<'_,PyAny>,format:&Bound<'_,PyAny>,data:&Bound<'_,PyAny>)->PyResult<PyClassInitializer<Self>>{Ok(PyClassInitializer::from(Message).add_subclass(Self{value:model::SysEx7Packet::new(int(group)?,int(format)?,buffer(py,data,6)?).map_err(error)?}))}
    #[getter]fn group(&self)->u8{self.value.group()}#[getter]fn format(&self)->u8{self.value.format()}
    #[getter]fn data<'py>(&self,py:Python<'py>)->Bound<'py,PyBytes>{PyBytes::new(py,self.value.data())}
});
#[pyclass(frozen,extends=Message,module="meedee.ump")]
struct SysEx8Packet {
    value: model::SysEx8Packet,
}
value_methods!(SysEx8Packet;group,format,stream_id,data,aborted;{
    #[new]#[pyo3(signature=(*,group,format,stream_id,data,aborted=false))]
    fn new(py:Python<'_>,group:&Bound<'_,PyAny>,format:&Bound<'_,PyAny>,stream_id:&Bound<'_,PyAny>,data:&Bound<'_,PyAny>,#[pyo3(from_py_with=boolean)] aborted:bool)->PyResult<PyClassInitializer<Self>>{Ok(PyClassInitializer::from(Message).add_subclass(Self{value:model::SysEx8Packet::new(int(group)?,int(format)?,int(stream_id)?,buffer(py,data,13)?,aborted).map_err(error)?}))}
    #[getter]fn group(&self)->u8{self.value.group()}#[getter]fn format(&self)->u8{self.value.format()}#[getter]fn stream_id(&self)->u8{self.value.stream_id()}#[getter]fn aborted(&self)->bool{self.value.aborted()}
    #[getter]fn data<'py>(&self,py:Python<'py>)->Bound<'py,PyBytes>{PyBytes::new(py,self.value.data())}
});
#[pyclass(frozen,extends=Message,module="meedee.ump")]
struct MixedDataSet {
    value: model::MixedDataSet,
}
value_methods!(MixedDataSet;group,header,mds_id,data;{
    #[new]#[pyo3(signature=(*,group,header,mds_id,data))]
    fn new(py:Python<'_>,group:&Bound<'_,PyAny>,header:&Bound<'_,PyAny>,mds_id:&Bound<'_,PyAny>,data:&Bound<'_,PyAny>)->PyResult<PyClassInitializer<Self>>{Ok(PyClassInitializer::from(Message).add_subclass(Self{value:model::MixedDataSet::new(int(group)?,boolean(header)?,int(mds_id)?,buffer(py,data,14)?).map_err(error)?}))}
    #[getter]fn group(&self)->u8{self.value.group()}#[getter]fn header(&self)->bool{self.value.header()}#[getter]fn mds_id(&self)->u8{self.value.mds_id()}
    #[getter]fn data<'py>(&self,py:Python<'py>)->Bound<'py,PyBytes>{PyBytes::new(py,self.value.data())}
});
#[pyclass(frozen,extends=Message,module="meedee.ump")]
struct FlexData {
    value: model::FlexData,
}
value_methods!(FlexData;group,format,channel,status_bank,status,data;{
    #[new]#[pyo3(signature=(*,group,format,channel,status_bank,status,data))]
    fn new(group:&Bound<'_,PyAny>,format:&Bound<'_,PyAny>,channel:&Bound<'_,PyAny>,status_bank:&Bound<'_,PyAny>,status:&Bound<'_,PyAny>,data:&Bound<'_,PyAny>)->PyResult<PyClassInitializer<Self>>{Ok(PyClassInitializer::from(Message).add_subclass(Self{value:model::FlexData::new(int(group)?,int(format)?,optional_int(channel)?,int(status_bank)?,int(status)?,words(data,3)?).map_err(error)?}))}
    #[getter]fn group(&self)->u8{self.value.group()}#[getter]fn format(&self)->u8{self.value.format()}#[getter]fn channel(&self)->Option<u8>{self.value.channel()}#[getter]fn status_bank(&self)->u8{self.value.status_bank()}#[getter]fn status(&self)->u8{self.value.status()}
    #[getter]fn data<'py>(&self,py:Python<'py>)->PyResult<Bound<'py,PyTuple>>{tuple(py,self.value.data())}
});
#[pyclass(frozen,extends=Message,module="meedee.ump")]
struct StreamMessage {
    value: model::StreamMessage,
}
value_methods!(StreamMessage;format,status,data;{
    #[new]#[pyo3(signature=(*,format,status,data))]
    fn new(py:Python<'_>,format:&Bound<'_,PyAny>,status:&Bound<'_,PyAny>,data:&Bound<'_,PyAny>)->PyResult<PyClassInitializer<Self>>{Ok(PyClassInitializer::from(Message).add_subclass(Self{value:model::StreamMessage::new(int(format)?,int(status)?,buffer(py,data,14)?).map_err(error)?}))}
    #[getter]fn format(&self)->u8{self.value.format()}#[getter]fn status(&self)->u16{self.value.status()}
    #[getter]fn data<'py>(&self,py:Python<'py>)->Bound<'py,PyBytes>{PyBytes::new(py,self.value.data())}
});
#[pyclass(frozen,extends=Message,module="meedee.ump")]
struct OpaquePacket {
    value: model::OpaquePacket,
}
value_methods!(OpaquePacket;words;{
    #[new]#[pyo3(signature=(*,words))]
    fn new(words:&Bound<'_,PyAny>)->PyResult<PyClassInitializer<Self>>{Ok(PyClassInitializer::from(Message).add_subclass(Self{value:model::OpaquePacket::new(self::words(words,4)?).map_err(error)?}))}
    #[getter]fn words<'py>(&self,py:Python<'py>)->PyResult<Bound<'py,PyTuple>>{tuple(py,self.value.words())}
});
fn extract_packet(value: &Bound<'_, PyAny>) -> PyResult<model::UmpMessage> {
    if let Ok(v) = value.extract::<PyRef<'_, Midi1Packet>>() {
        return Ok(model::UmpMessage::Midi1(v.value.clone()));
    }
    if let Ok(v) = value.extract::<PyRef<'_, SysEx7Packet>>() {
        return Ok(model::UmpMessage::SysEx7(v.value.clone()));
    }
    if let Ok(v) = value.extract::<PyRef<'_, SysEx8Packet>>() {
        return Ok(model::UmpMessage::SysEx8(v.value.clone()));
    }
    if let Ok(v) = value.extract::<PyRef<'_, MixedDataSet>>() {
        return Ok(model::UmpMessage::MixedDataSet(v.value.clone()));
    }
    if let Ok(v) = value.extract::<PyRef<'_, FlexData>>() {
        return Ok(model::UmpMessage::FlexData(v.value.clone()));
    }
    if let Ok(v) = value.extract::<PyRef<'_, StreamMessage>>() {
        return Ok(model::UmpMessage::Stream(v.value.clone()));
    }
    if let Ok(v) = value.extract::<PyRef<'_, OpaquePacket>>() {
        return Ok(model::UmpMessage::Opaque(v.value.clone()));
    }
    Err(PyTypeError::new_err("expected a meedee.ump message"))
}
#[pyfunction]
fn encode<'py>(py: Python<'py>, message: &Bound<'_, PyAny>) -> PyResult<Bound<'py, PyTuple>> {
    tuple(py, &model::encode(&extract(message)?))
}
#[pyfunction]
#[pyo3(signature=(words,*,preserve_unknown=false))]
fn decode(
    py: Python<'_>,
    words: &Bound<'_, PyAny>,
    #[pyo3(from_py_with=boolean)] preserve_unknown: bool,
) -> PyResult<Py<PyAny>> {
    to_python(
        py,
        model::decode(&self::words(words, 4)?, preserve_unknown).map_err(error)?,
    )
}
#[pyfunction]
#[pyo3(signature=(data,*,byteorder))]
fn bytes_to_words<'py>(
    py: Python<'py>,
    data: &Bound<'_, PyAny>,
    byteorder: &str,
) -> PyResult<Bound<'py, PyTuple>> {
    tuple(
        py,
        &model::bytes_to_words(&buffer(py, data, usize::MAX)?, little_endian(byteorder)?)
            .map_err(error)?,
    )
}
#[pyfunction]
#[pyo3(signature=(words,*,byteorder))]
fn words_to_bytes<'py>(
    py: Python<'py>,
    words: &Bound<'_, PyAny>,
    byteorder: &str,
) -> PyResult<Bound<'py, PyBytes>> {
    Ok(PyBytes::new(
        py,
        &model::words_to_bytes(&self::words(words, usize::MAX)?, little_endian(byteorder)?),
    ))
}
#[pyfunction]
#[pyo3(signature=(*,group,data,message_limit=1_048_576))]
fn packetize_sysex7(
    py: Python<'_>,
    group: &Bound<'_, PyAny>,
    data: &Bound<'_, PyAny>,
    #[pyo3(from_py_with=limit)] message_limit: usize,
) -> PyResult<Vec<Py<PyAny>>> {
    model::packetize_sysex7(int(group)?, &buffer(py, data, message_limit)?)
        .map_err(error)?
        .into_iter()
        .map(|value| object(py, SysEx7Packet { value }))
        .collect()
}
#[pyfunction]
#[pyo3(signature=(*,group,stream_id,data,message_limit=1_048_576))]
fn packetize_sysex8(
    py: Python<'_>,
    group: &Bound<'_, PyAny>,
    stream_id: &Bound<'_, PyAny>,
    data: &Bound<'_, PyAny>,
    #[pyo3(from_py_with=limit)] message_limit: usize,
) -> PyResult<Vec<Py<PyAny>>> {
    model::packetize_sysex8(
        int(group)?,
        int(stream_id)?,
        &buffer(py, data, message_limit)?,
    )
    .map_err(error)?
    .into_iter()
    .map(|value| object(py, SysEx8Packet { value }))
    .collect()
}
#[pyfunction]
#[pyo3(signature=(*,group,channel,status_bank,status,text))]
fn packetize_flex_text(
    py: Python<'_>,
    group: &Bound<'_, PyAny>,
    channel: &Bound<'_, PyAny>,
    status_bank: &Bound<'_, PyAny>,
    status: &Bound<'_, PyAny>,
    text: &str,
) -> PyResult<Vec<Py<PyAny>>> {
    model::packetize_flex_text(
        int(group)?,
        optional_int(channel)?,
        int(status_bank)?,
        int(status)?,
        text,
    )
    .map_err(error)?
    .into_iter()
    .map(|value| object(py, FlexData { value }))
    .collect()
}
#[pyfunction]
#[pyo3(signature=(*,status,function_block,text))]
fn packetize_stream_text(
    py: Python<'_>,
    status: &Bound<'_, PyAny>,
    function_block: &Bound<'_, PyAny>,
    text: &str,
) -> PyResult<Vec<Py<PyAny>>> {
    model::packetize_stream_text(int(status)?, optional_int(function_block)?, text)
        .map_err(error)?
        .into_iter()
        .map(|value| object(py, StreamMessage { value }))
        .collect()
}

#[pyclass(frozen, module = "meedee.ump")]
struct DecodeDiagnostic {
    value: model::DecodeDiagnostic,
}
#[pymethods]
impl DecodeDiagnostic {
    #[getter]
    fn word_offset(&self) -> usize {
        self.value.word_offset
    }
    #[getter]
    fn reason(&self) -> &str {
        &self.value.reason
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|v| self.value == v.value)
    }
    fn __hash__(&self) -> u64 {
        hash(&self.value)
    }
}
#[pyclass(module = "meedee.ump")]
struct Decoder {
    value: model::Decoder,
}
#[pymethods]
impl Decoder {
    #[new]
    #[pyo3(signature=(*,preserve_unknown=false))]
    fn new(#[pyo3(from_py_with=boolean)] preserve_unknown: bool) -> Self {
        Self {
            value: model::Decoder::new(preserve_unknown),
        }
    }
    fn feed(&mut self, py: Python<'_>, words: &Bound<'_, PyAny>) -> PyResult<Vec<Py<PyAny>>> {
        decode_outputs(py, self.value.feed(&self::words(words, usize::MAX)?))
    }
    fn finish(&mut self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        decode_outputs(py, self.value.finish())
    }
    fn reset(&mut self) {
        self.value.reset();
    }
}
fn decode_outputs(py: Python<'_>, values: Vec<model::DecodeOutput>) -> PyResult<Vec<Py<PyAny>>> {
    values
        .into_iter()
        .map(|v| match v {
            model::DecodeOutput::Message(v) => to_python(py, v),
            model::DecodeOutput::Diagnostic(value) => {
                Ok(Py::new(py, DecodeDiagnostic { value })?.into_any())
            }
        })
        .collect()
}
macro_rules! enums {
    ($($name:ident{$($variant:ident),*};)*)=>{$(
        #[pyclass(eq,frozen,from_py_object,module="meedee.ump")]
        #[derive(Clone,Copy,PartialEq,Eq)]
        enum $name{$($variant),*}
        impl From<model::$name> for $name{fn from(v:model::$name)->Self{match v{$(model::$name::$variant=>Self::$variant),*}}}
    )*};
}
enums! {DataKind{SysEx7,SysEx8,FlexData,StreamText};AssemblyDiagnostic{Interrupted,UnexpectedContinuation,Aborted,PayloadLimit,StreamLimit,MalformedText,TooManyPackets,Truncated};TranslationStatus{Exact,Quantized,Unsupported,ProfileDependent,ContextRequired};}
#[pyclass(frozen, module = "meedee.ump")]
struct AssembledData {
    value: model::AssembledData,
}
#[pymethods]
impl AssembledData {
    #[getter]
    fn kind(&self) -> DataKind {
        self.value.kind.into()
    }
    #[getter]
    fn group(&self) -> Option<u8> {
        self.value.group
    }
    #[getter]
    fn channel(&self) -> Option<u8> {
        self.value.channel
    }
    #[getter]
    fn stream_id(&self) -> Option<u8> {
        self.value.stream_id
    }
    #[getter]
    fn function_block(&self) -> Option<u8> {
        self.value.function_block
    }
    #[getter]
    fn status_bank(&self) -> Option<u8> {
        self.value.status_bank
    }
    #[getter]
    fn status(&self) -> Option<u16> {
        self.value.status
    }
    #[getter]
    fn data<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.value.data)
    }
    #[getter]
    fn text(&self) -> Option<&str> {
        self.value.text()
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|v| self.value == v.value)
    }
    fn __hash__(&self) -> u64 {
        hash(&self.value)
    }
}
#[pyclass(module = "meedee.ump")]
struct Assembler {
    value: model::Assembler,
}
#[pymethods]
impl Assembler {
    #[new]
    #[pyo3(signature=(*,message_limit=1_048_576,total_limit=8_388_608,max_streams=64))]
    fn new(
        #[pyo3(from_py_with=limit)] message_limit: usize,
        #[pyo3(from_py_with=limit)] total_limit: usize,
        #[pyo3(from_py_with=limit)] max_streams: usize,
    ) -> PyResult<Self> {
        Ok(Self {
            value: model::Assembler::new(message_limit, total_limit, max_streams).map_err(error)?,
        })
    }
    #[getter]
    fn retained_bytes(&self) -> usize {
        self.value.retained_bytes()
    }
    #[getter]
    fn active_streams(&self) -> usize {
        self.value.active_streams()
    }
    fn feed(&mut self, py: Python<'_>, message: &Bound<'_, PyAny>) -> PyResult<Vec<Py<PyAny>>> {
        assembly_outputs(py, self.value.feed(&extract(message)?))
    }
    fn finish(&mut self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        assembly_outputs(py, self.value.finish())
    }
    fn reset(&mut self) {
        self.value.reset();
    }
}
fn assembly_outputs(py: Python<'_>, values: Vec<model::AssemblyItem>) -> PyResult<Vec<Py<PyAny>>> {
    values
        .into_iter()
        .map(|v| match v {
            model::AssemblyItem::Data(value) => {
                Ok(Py::new(py, AssembledData { value })?.into_any())
            }
            model::AssemblyItem::Diagnostic(v) => {
                Ok(Py::new(py, AssemblyDiagnostic::from(v))?.into_any())
            }
        })
        .collect()
}
#[derive(Clone)]
enum Translated {
    Midi1(model::Translation<meedee::messages::AnyMessage>),
    Midi2(model::Translation<model::UmpMessage>),
}
#[pyclass(frozen, module = "meedee.ump")]
struct Translation {
    value: Translated,
}
#[pymethods]
impl Translation {
    #[getter]
    fn status(&self) -> TranslationStatus {
        match &self.value {
            Translated::Midi1(v) => v.status.into(),
            Translated::Midi2(v) => v.status.into(),
        }
    }
    #[getter]
    fn reason(&self) -> &str {
        match &self.value {
            Translated::Midi1(v) => v.reason,
            Translated::Midi2(v) => v.reason,
        }
    }
    #[getter]
    fn group(&self) -> Option<u8> {
        match &self.value {
            Translated::Midi1(v) => v.group,
            Translated::Midi2(v) => v.group,
        }
    }
    #[getter]
    fn converted(&self) -> bool {
        match &self.value {
            Translated::Midi1(v) => v.converted(),
            Translated::Midi2(v) => v.converted(),
        }
    }
    #[getter]
    fn messages(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        match &self.value {
            Translated::Midi1(v) => v
                .messages
                .iter()
                .cloned()
                .map(|m| messages::to_python(py, m))
                .collect(),
            Translated::Midi2(v) => v
                .messages
                .iter()
                .cloned()
                .map(|m| to_python(py, m))
                .collect(),
        }
    }
}
#[pyfunction]
#[pyo3(signature=(message,*,allow_loss=false))]
fn to_midi1(
    message: &Bound<'_, PyAny>,
    #[pyo3(from_py_with=boolean)] allow_loss: bool,
) -> PyResult<Translation> {
    Ok(Translation {
        value: Translated::Midi1(model::to_midi1(&extract(message)?, allow_loss).map_err(error)?),
    })
}
#[pyfunction]
#[pyo3(signature=(message,*,group))]
fn to_midi2(message: &Bound<'_, PyAny>, group: &Bound<'_, PyAny>) -> PyResult<Translation> {
    Ok(Translation {
        value: Translated::Midi2(
            model::to_midi2(&midi1_message(message)?, int(group)?).map_err(error)?,
        ),
    })
}
#[pyclass(frozen, module = "meedee.ump")]
struct Capabilities {
    value: model::Capabilities,
}
value_methods!(Capabilities;midi1,midi2,sysex8_streams;{
    #[new]#[pyo3(signature=(*,midi1,midi2,sysex8_streams))]
    fn new(midi1:&Bound<'_,PyAny>,midi2:&Bound<'_,PyAny>,sysex8_streams:&Bound<'_,PyAny>)->PyResult<Self>{Ok(Self{value:model::Capabilities::new(boolean(midi1)?,boolean(midi2)?,int(sysex8_streams)?).map_err(error)?})}
    #[getter]fn midi1(&self)->bool{self.value.midi1}#[getter]fn midi2(&self)->bool{self.value.midi2}#[getter]fn sysex8_streams(&self)->u8{self.value.sysex8_streams}
    fn supports(&self,message:&Bound<'_,PyAny>)->PyResult<Option<bool>>{Ok(self.value.supports(&extract(message)?))}
});
fn register_packets(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Midi1Packet>()?;
    module.add_class::<SysEx7Packet>()?;
    module.add_class::<SysEx8Packet>()?;
    module.add_class::<MixedDataSet>()?;
    module.add_class::<FlexData>()?;
    module.add_class::<StreamMessage>()?;
    module.add_class::<OpaquePacket>()?;
    module.add_class::<DecodeDiagnostic>()?;
    module.add_class::<Decoder>()?;
    module.add_class::<DataKind>()?;
    module.add_class::<AssembledData>()?;
    module.add_class::<AssemblyDiagnostic>()?;
    module.add_class::<Assembler>()?;
    module.add_class::<TranslationStatus>()?;
    module.add_class::<Translation>()?;
    module.add_class::<Capabilities>()?;
    module.add_function(wrap_pyfunction!(encode, module)?)?;
    module.add_function(wrap_pyfunction!(decode, module)?)?;
    module.add_function(wrap_pyfunction!(bytes_to_words, module)?)?;
    module.add_function(wrap_pyfunction!(words_to_bytes, module)?)?;
    module.add_function(wrap_pyfunction!(packetize_sysex7, module)?)?;
    module.add_function(wrap_pyfunction!(packetize_sysex8, module)?)?;
    module.add_function(wrap_pyfunction!(packetize_flex_text, module)?)?;
    module.add_function(wrap_pyfunction!(packetize_stream_text, module)?)?;
    module.add_function(wrap_pyfunction!(to_midi1, module)?)?;
    module.add_function(wrap_pyfunction!(to_midi2, module)?)?;
    register_helpers(module)
}
fn object<T: pyo3::PyClass<BaseType = Message>>(py: Python<'_>, value: T) -> PyResult<Py<PyAny>> {
    Ok(Py::new(py, PyClassInitializer::from(Message).add_subclass(value))?.into_any())
}
fn error(v: meedee::messages::ValueError) -> PyErr {
    PyValueError::new_err(v.to_string())
}
fn int(v: &Bound<'_, PyAny>) -> PyResult<i64> {
    integer(v, "value")
}
fn optional_int(v: &Bound<'_, PyAny>) -> PyResult<Option<i64>> {
    if v.is_none() {
        Ok(None)
    } else {
        Ok(Some(int(v)?))
    }
}
fn boolean(v: &Bound<'_, PyAny>) -> PyResult<bool> {
    if !v.is_exact_instance_of::<PyBool>() {
        return Err(PyTypeError::new_err("expected bool"));
    }
    v.extract()
}
fn limit(v: &Bound<'_, PyAny>) -> PyResult<usize> {
    let n = int(v)?;
    if n <= 0 {
        return Err(PyValueError::new_err("limit must be positive"));
    }
    usize::try_from(n).map_err(|_| PyValueError::new_err("limit too large"))
}
fn words(v: &Bound<'_, PyAny>, max: usize) -> PyResult<Vec<u32>> {
    if !v.is_exact_instance_of::<PyTuple>() && !v.is_exact_instance_of::<PyList>() {
        return Err(PyTypeError::new_err(
            "words must be a built-in list or tuple",
        ));
    }
    if v.len()? > max {
        return Err(PyValueError::new_err("too many words"));
    }
    v.try_iter()?
        .map(|v| {
            let n = int(&v?)?;
            u32::try_from(n).map_err(|_| PyValueError::new_err("word must be 0..4294967295"))
        })
        .collect()
}
fn buffer(py: Python<'_>, v: &Bound<'_, PyAny>, max: usize) -> PyResult<Vec<u8>> {
    let b = PyBuffer::<u8>::get(v)?;
    if b.dimensions() != 1 {
        return Err(PyTypeError::new_err(
            "expected a one-dimensional unsigned-byte buffer",
        ));
    }
    if b.item_count() > max {
        return Err(PyValueError::new_err("payload limit exceeded"));
    }
    b.to_vec(py)
}
fn tuple<'py>(py: Python<'py>, v: &[u32]) -> PyResult<Bound<'py, PyTuple>> {
    PyTuple::new(py, v.iter().copied())
}
fn little_endian(value: &str) -> PyResult<bool> {
    match value {
        "big" => Ok(false),
        "little" => Ok(true),
        _ => Err(PyValueError::new_err("byteorder must be 'big' or 'little'")),
    }
}
fn hash(v: &impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

macro_rules! helpers {
    ($class:ident; $($name:ident($($field:ident:$read:ident),*);)*)=>{
        $(#[pyfunction]#[pyo3(signature=(*,$($field),*))]
        #[allow(clippy::too_many_arguments)]
        fn $name(py:Python<'_>,$($field:&Bound<'_,PyAny>),*)->PyResult<Py<PyAny>>{object(py,$class{value:model::$name($($read($field)?),*).map_err(error)?})})*
    };
}
helpers! {FlexData;
    tempo(group:int,ten_nanoseconds_per_quarter:int);
    time_signature(group:int,numerator:int,denominator_power:int,thirtyseconds_per_quarter:int);
    metronome(group:int,clocks_per_primary:int,accent1:int,accent2:int,accent3:int,subdivision1:int,subdivision2:int);
    key_signature(group:int,channel:optional_int,sharps_flats:int,tonic:int);
}
helpers! {StreamMessage;
    endpoint_discovery(major:int,minor:int,filter:int);
    endpoint_info(major:int,minor:int,static_blocks:boolean,function_blocks:int,midi1:boolean,midi2:boolean,rx_jr:boolean,tx_jr:boolean);
    stream_configuration(request:boolean,protocol:int,rx_jr:boolean,tx_jr:boolean);
    function_block_discovery(block:int,filter:int);
    function_block_info(active:boolean,block:int,ui_hint:int,midi1_port:int,direction:int,first_group:int,groups:int,midi_ci_version:int,sysex8_streams:int);
}
#[pyfunction]
#[pyo3(signature=(*,manufacturer,family,model,revision))]
fn device_identity(
    py: Python<'_>,
    manufacturer: &Bound<'_, PyAny>,
    family: &Bound<'_, PyAny>,
    model: &Bound<'_, PyAny>,
    revision: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    object(
        py,
        StreamMessage {
            value: model::device_identity(
                buffer(py, manufacturer, 3)?,
                int(family)?,
                int(model)?,
                buffer(py, revision, 4)?,
            )
            .map_err(error)?,
        },
    )
}
#[pyfunction]
fn start_of_clip(py: Python<'_>) -> PyResult<Py<PyAny>> {
    object(
        py,
        StreamMessage {
            value: model::start_of_clip(),
        },
    )
}
#[pyfunction]
fn end_of_clip(py: Python<'_>) -> PyResult<Py<PyAny>> {
    object(
        py,
        StreamMessage {
            value: model::end_of_clip(),
        },
    )
}
#[pyfunction]
#[pyo3(signature=(*,value,source_bits,destination_bits))]
fn scale_up(
    value: &Bound<'_, PyAny>,
    source_bits: &Bound<'_, PyAny>,
    destination_bits: &Bound<'_, PyAny>,
) -> PyResult<u32> {
    let value = u32::try_from(int(value)?)
        .map_err(|_| PyValueError::new_err("value must be unsigned 32-bit"))?;
    let source = u8::try_from(int(source_bits)?)
        .map_err(|_| PyValueError::new_err("invalid source_bits"))?;
    let destination = u8::try_from(int(destination_bits)?)
        .map_err(|_| PyValueError::new_err("invalid destination_bits"))?;
    model::scale_up(value, source, destination).map_err(error)
}
fn register_helpers(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(tempo, module)?)?;
    module.add_function(wrap_pyfunction!(time_signature, module)?)?;
    module.add_function(wrap_pyfunction!(metronome, module)?)?;
    module.add_function(wrap_pyfunction!(key_signature, module)?)?;
    module.add_function(wrap_pyfunction!(endpoint_discovery, module)?)?;
    module.add_function(wrap_pyfunction!(endpoint_info, module)?)?;
    module.add_function(wrap_pyfunction!(device_identity, module)?)?;
    module.add_function(wrap_pyfunction!(stream_configuration, module)?)?;
    module.add_function(wrap_pyfunction!(function_block_discovery, module)?)?;
    module.add_function(wrap_pyfunction!(function_block_info, module)?)?;
    module.add_function(wrap_pyfunction!(start_of_clip, module)?)?;
    module.add_function(wrap_pyfunction!(end_of_clip, module)?)?;
    module.add_function(wrap_pyfunction!(scale_up, module)?)?;
    Ok(())
}

fn midi1_message(message: &Bound<'_, PyAny>) -> PyResult<meedee::messages::AnyMessage> {
    if let Ok(value) = messages::extract_primitive(message) {
        return Ok(value);
    }
    let wire = messages::encode_message(message)?;
    if wire.first() == Some(&240) && wire.last() == Some(&247) {
        return Ok(meedee::messages::AnyMessage::SysEx(
            meedee::messages::SysEx::new(wire[1..wire.len() - 1].to_vec()).map_err(error)?,
        ));
    }
    Err(PyTypeError::new_err("expected a primitive MIDI 1 message"))
}
