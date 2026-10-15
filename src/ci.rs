//! Frozen MIDI-CI management values and the shared caller-driven session.
use crate::files::{bytes, frozen, limit, uint};
use crate::messages::{integer, Message};
use meedee::messages::PrimitiveMessage;
use meedee::{ci as model, ci_session as session};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyTuple};

#[pyclass(frozen, skip_from_py_object, module = "meedee.ci")]
#[derive(Clone)]
struct Address {
    value: model::Address,
}
#[pymethods]
impl Address {
    #[staticmethod]
    #[pyo3(signature=(*,channel))]
    fn channel(channel: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self {
            value: model::Address::channel(int(channel)?).map_err(error)?,
        })
    }
    #[classattr]
    const GROUP: Self = Self {
        value: model::Address::GROUP,
    };
    #[classattr]
    const FUNCTION_BLOCK: Self = Self {
        value: model::Address::FUNCTION_BLOCK,
    };
    #[getter]
    fn channel_number(&self) -> Option<u8> {
        self.value.channel_number()
    }
    fn __repr__(&self) -> String {
        match self.value.channel_number() {
            Some(n) => format!("Address.channel(channel={n})"),
            None if self.value == model::Address::GROUP => "Address.GROUP".into(),
            None => "Address.FUNCTION_BLOCK".into(),
        }
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|v| v.value == self.value)
    }
    fn __hash__(&self) -> u64 {
        crate::tuning::hash(&self.value)
    }
}
#[pyclass(frozen, skip_from_py_object, module = "meedee.ci")]
#[derive(Clone)]
struct Header {
    value: model::Header,
}
frozen!(Header;source_muid,destination_muid,version,address;{
    #[new]#[pyo3(signature=(*,source_muid,destination_muid,version=2,address=None))]
    fn new(source_muid:&Bound<'_,PyAny>,destination_muid:&Bound<'_,PyAny>,#[pyo3(from_py_with=int)]version:i64,address:Option<PyRef<'_,Address>>)->PyResult<Self>{Ok(Self{value:model::Header::new(int(source_muid)?,int(destination_muid)?,version,address.map_or(model::Address::FUNCTION_BLOCK,|v|v.value)).map_err(error)?})}
    #[getter]fn source_muid(&self)->u32{self.value.source_muid()}
    #[getter]fn destination_muid(&self)->u32{self.value.destination_muid()}
    #[getter]fn version(&self)->u8{self.value.version()}
    #[getter]fn address(&self)->Address{Address{value:self.value.address()}}
});
#[pyclass(frozen, skip_from_py_object, module = "meedee.ci")]
#[derive(Clone)]
struct Identity {
    value: model::Identity,
}
frozen!(Identity;manufacturer,family,model,revision;{
    #[new]#[pyo3(signature=(*,manufacturer,family,model,revision))]
    fn new(manufacturer:&Bound<'_,PyAny>,family:&Bound<'_,PyAny>,model:&Bound<'_,PyAny>,revision:&Bound<'_,PyAny>)->PyResult<Self>{Ok(Self{value:model::Identity::new(bytes(manufacturer,3)?,int(family)?,int(model)?,bytes(revision,4)?).map_err(error)?})}
    #[getter]fn manufacturer<'py>(&self,py:Python<'py>)->Bound<'py,PyBytes>{PyBytes::new(py,self.value.manufacturer())}
    #[getter]fn family(&self)->u16{self.value.family()}
    #[getter]fn model(&self)->u16{self.value.model()}
    #[getter]fn revision<'py>(&self,py:Python<'py>)->Bound<'py,PyBytes>{PyBytes::new(py,self.value.revision())}
});
#[pyclass(frozen, skip_from_py_object, module = "meedee.ci")]
#[derive(Clone)]
struct DiscoveryInfo {
    value: model::DiscoveryInfo,
}
frozen!(DiscoveryInfo;identity,capabilities,max_sysex,output_path;{
    #[new]#[pyo3(signature=(*,identity,capabilities,max_sysex,output_path=None))]
    fn new(identity:PyRef<'_,Identity>,capabilities:&Bound<'_,PyAny>,max_sysex:&Bound<'_,PyAny>,output_path:Option<&Bound<'_,PyAny>>)->PyResult<Self>{Ok(Self{value:model::DiscoveryInfo::new(identity.value.clone(),int(capabilities)?,int(max_sysex)?,output_path.map(int).transpose()?).map_err(error)?})}
    #[getter]fn identity(&self)->Identity{Identity{value:self.value.identity().clone()}}
    #[getter]fn capabilities(&self)->u8{self.value.capabilities()}
    #[getter]fn max_sysex(&self)->u32{self.value.max_sysex()}
    #[getter]fn output_path(&self)->Option<u8>{self.value.output_path()}
});
#[pyclass(frozen, skip_from_py_object, module = "meedee.ci")]
#[derive(Clone)]
struct Notice {
    value: model::Notice,
}
frozen!(Notice;original,status,status_data,details,message_text;{
    #[new]#[pyo3(signature=(*,original,status,status_data,details,message_text))]
    fn new(original:&Bound<'_,PyAny>,status:&Bound<'_,PyAny>,status_data:&Bound<'_,PyAny>,details:&Bound<'_,PyAny>,message_text:&Bound<'_,PyAny>)->PyResult<Self>{Ok(Self{value:model::Notice::new(int(original)?,int(status)?,int(status_data)?,bytes(details,5)?,bytes(message_text,16383)?).map_err(error)?})}
    #[getter]fn original(&self)->u8{self.value.original()}
    #[getter]fn status(&self)->u8{self.value.status()}
    #[getter]fn status_data(&self)->u8{self.value.status_data()}
    #[getter]fn details<'py>(&self,py:Python<'py>)->Bound<'py,PyBytes>{PyBytes::new(py,self.value.details())}
    #[getter]fn message_text<'py>(&self,py:Python<'py>)->Bound<'py,PyBytes>{PyBytes::new(py,self.value.message_text())}
});
macro_rules! management_class {
    ($name:ident; $($field:ident),*; {$($method:item)*})=>{
        #[pyclass(frozen,extends=Message,module="meedee.ci")]
        struct $name {value:model::Management}
        frozen!($name;header,$($field,)*extensions;{
            $($method)*
            #[getter]fn header(&self)->Header{Header{value:self.value.header().clone()}}
            #[getter]fn extensions<'py>(&self,py:Python<'py>)->Bound<'py,PyBytes>{PyBytes::new(py,self.value.extensions())}
        });
    }
}
management_class!(Discovery;info;{
    #[new]#[pyo3(signature=(*,header,info,extensions=None))]
    fn new(header:PyRef<'_,Header>,info:PyRef<'_,DiscoveryInfo>,extensions:Option<&Bound<'_,PyAny>>)->PyResult<PyClassInitializer<Self>>{Ok(PyClassInitializer::from(Message).add_subclass(Self{value:build(&header,model::Body::Discovery(info.value.clone()),extensions)?}))}
    #[getter]fn info(&self)->DiscoveryInfo{let model::Body::Discovery(value)=self.value.body()else{unreachable!()};DiscoveryInfo{value:value.clone()}}
});
management_class!(DiscoveryReply;info,function_block;{
    #[new]#[pyo3(signature=(*,header,info,function_block=None,extensions=None))]
    fn new(header:PyRef<'_,Header>,info:PyRef<'_,DiscoveryInfo>,function_block:Option<&Bound<'_,PyAny>>,extensions:Option<&Bound<'_,PyAny>>)->PyResult<PyClassInitializer<Self>>{Ok(PyClassInitializer::from(Message).add_subclass(Self{value:build(&header,model::Body::DiscoveryReply{info:info.value.clone(),function_block:optional_byte(function_block)?},extensions)?}))}
    #[getter]fn info(&self)->DiscoveryInfo{let model::Body::DiscoveryReply{info,..}=self.value.body()else{unreachable!()};DiscoveryInfo{value:info.clone()}}
    #[getter]fn function_block(&self)->Option<u8>{let model::Body::DiscoveryReply{function_block,..}=self.value.body()else{unreachable!()};*function_block}
});
management_class!(EndpointInquiry;;{
    #[new]#[pyo3(signature=(*,header,extensions=None))]
    fn new(header:PyRef<'_,Header>,extensions:Option<&Bound<'_,PyAny>>)->PyResult<PyClassInitializer<Self>>{Ok(PyClassInitializer::from(Message).add_subclass(Self{value:build(&header,model::Body::EndpointInquiry,extensions)?}))}
});
management_class!(EndpointReply;product_instance_id;{
    #[new]#[pyo3(signature=(*,header,product_instance_id,extensions=None))]
    fn new(header:PyRef<'_,Header>,product_instance_id:String,extensions:Option<&Bound<'_,PyAny>>)->PyResult<PyClassInitializer<Self>>{Ok(PyClassInitializer::from(Message).add_subclass(Self{value:build(&header,model::Body::EndpointReply{product_instance_id},extensions)?}))}
    #[getter]fn product_instance_id(&self)->&str{let model::Body::EndpointReply{product_instance_id}=self.value.body()else{unreachable!()};product_instance_id}
});
management_class!(InvalidateMuid;target;{
    #[new]#[pyo3(signature=(*,header,target,extensions=None))]
    fn new(header:PyRef<'_,Header>,target:&Bound<'_,PyAny>,extensions:Option<&Bound<'_,PyAny>>)->PyResult<PyClassInitializer<Self>>{let target=uint(target)?.try_into().map_err(|_|PyValueError::new_err("target exceeds u32"))?;Ok(PyClassInitializer::from(Message).add_subclass(Self{value:build(&header,model::Body::InvalidateMuid{target},extensions)?}))}
    #[getter]fn target(&self)->u32{let model::Body::InvalidateMuid{target}=self.value.body()else{unreachable!()};*target}
});
management_class!(Ack;notice;{
    #[new]#[pyo3(signature=(*,header,notice,extensions=None))]
    fn new(header:PyRef<'_,Header>,notice:PyRef<'_,Notice>,extensions:Option<&Bound<'_,PyAny>>)->PyResult<PyClassInitializer<Self>>{Ok(PyClassInitializer::from(Message).add_subclass(Self{value:build(&header,model::Body::Ack(notice.value.clone()),extensions)?}))}
    #[getter]fn notice(&self)->Notice{let model::Body::Ack(value)=self.value.body()else{unreachable!()};Notice{value:value.clone()}}
});
management_class!(Nak;notice;{
    #[new]#[pyo3(signature=(*,header,notice=None,extensions=None))]
    fn new(header:PyRef<'_,Header>,notice:Option<PyRef<'_,Notice>>,extensions:Option<&Bound<'_,PyAny>>)->PyResult<PyClassInitializer<Self>>{Ok(PyClassInitializer::from(Message).add_subclass(Self{value:build(&header,model::Body::Nak(notice.map(|v|v.value.clone())),extensions)?}))}
    #[getter]fn notice(&self)->Option<Notice>{let model::Body::Nak(value)=self.value.body()else{unreachable!()};value.clone().map(|value|Notice{value})}
});
#[pyclass(frozen, skip_from_py_object, module = "meedee.ci")]
#[derive(Clone)]
struct Route {
    value: session::Route,
}
frozen!(Route;input,output,group;{
    #[new]#[pyo3(signature=(*,input,output,group=None))]
    fn new(input:String,output:String,group:Option<&Bound<'_,PyAny>>)->PyResult<Self>{Ok(Self{value:session::Route::new(input,output,group.map(int).transpose()?).map_err(error)?})}
    #[getter]fn input(&self)->&str{self.value.input()}
    #[getter]fn output(&self)->&str{self.value.output()}
    #[getter]fn group(&self)->Option<u8>{self.value.group()}
});
#[pyclass(frozen, module = "meedee.ci")]
struct Peer {
    value: session::Peer,
}
#[pymethods]
impl Peer {
    #[getter]
    fn muid(&self) -> u32 {
        self.value.muid
    }
    #[getter]
    fn version(&self) -> u8 {
        self.value.version
    }
    #[getter]
    fn info(&self) -> DiscoveryInfo {
        DiscoveryInfo {
            value: self.value.info.clone(),
        }
    }
    #[getter]
    fn function_block(&self) -> Option<u8> {
        self.value.function_block
    }
    #[getter]
    fn product_instance_id(&self) -> Option<&str> {
        self.value.product_instance_id.as_deref()
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|v| v.value == self.value)
    }
    fn __hash__(&self) -> u64 {
        crate::tuning::hash(&self.value)
    }
}
#[pyclass(frozen, module = "meedee.ci")]
struct Action {
    value: session::Action,
}
#[pymethods]
impl Action {
    #[getter]
    fn token(&self) -> u64 {
        self.value.token
    }
    #[getter]
    fn route(&self) -> Route {
        Route {
            value: self.value.route.clone(),
        }
    }
    #[getter]
    fn message(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        to_python(py, self.value.message.clone())
    }
    fn __repr__(&self) -> String {
        format!(
            "Action(token={}, output={:?}, group={:?}, kind={})",
            self.value.token,
            self.value.route.output(),
            self.value.route.group(),
            self.value.message.kind()
        )
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|v| v.value == self.value)
    }
    fn __hash__(&self) -> u64 {
        crate::tuning::hash(&self.value)
    }
}
#[pyclass(frozen, module = "meedee.ci")]
struct Observation {
    value: session::Observation,
}
#[pymethods]
impl Observation {
    #[getter]
    fn kind(&self) -> &str {
        &self.value.kind
    }
    #[getter]
    fn peer_muid(&self) -> Option<u32> {
        self.value.peer_muid
    }
    #[getter]
    fn token(&self) -> Option<u64> {
        self.value.token
    }
    #[getter]
    fn message(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.value
            .message
            .clone()
            .map(|v| to_python(py, v))
            .transpose()
    }
    #[getter]
    fn reason(&self) -> &str {
        &self.value.reason
    }
    fn __repr__(&self) -> String {
        format!(
            "Observation(kind={:?}, peer_muid={:?}, token={:?}, reason={:?})",
            self.value.kind, self.value.peer_muid, self.value.token, self.value.reason
        )
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Self>>()
            .is_ok_and(|v| v.value == self.value)
    }
    fn __hash__(&self) -> u64 {
        crate::tuning::hash(&self.value)
    }
}
#[pyclass(module = "meedee.ci")]
struct Session {
    value: session::Session,
}
#[pymethods]
impl Session {
    #[new]
    #[pyo3(signature=(*,local_muid,identity,route,output_path=0,function_block=None,max_sysex=1_048_576,peer_limit=16,exchange_limit=16,reply_byte_limit=8_388_608))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        local_muid: &Bound<'_, PyAny>,
        identity: PyRef<'_, Identity>,
        route: PyRef<'_, Route>,
        #[pyo3(from_py_with=int)] output_path: i64,
        function_block: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with=int)] max_sysex: i64,
        #[pyo3(from_py_with=limit)] peer_limit: usize,
        #[pyo3(from_py_with=limit)] exchange_limit: usize,
        #[pyo3(from_py_with=limit)] reply_byte_limit: usize,
    ) -> PyResult<Self> {
        Ok(Self {
            value: session::Session::new(
                int(local_muid)?,
                identity.value.clone(),
                route.value.clone(),
                output_path,
                function_block.map(int).transpose()?,
                max_sysex,
                peer_limit,
                exchange_limit,
                reply_byte_limit,
            )
            .map_err(error)?,
        })
    }
    #[getter]
    fn local_muid(&self) -> u32 {
        self.value.local_muid()
    }
    #[getter]
    fn active(&self) -> bool {
        self.value.active()
    }
    #[getter]
    fn route(&self) -> Route {
        Route {
            value: self.value.route().clone(),
        }
    }
    #[getter]
    fn peers(&self) -> Vec<Peer> {
        self.value
            .peers()
            .iter()
            .cloned()
            .map(|value| Peer { value })
            .collect()
    }
    #[getter]
    fn pending_count(&self) -> usize {
        self.value.pending_count()
    }
    #[pyo3(signature=(*,now_ns,timeout_ns=3_000_000_000))]
    fn discover(
        &mut self,
        #[pyo3(from_py_with=uint)] now_ns: u64,
        #[pyo3(from_py_with=uint)] timeout_ns: u64,
    ) -> PyResult<Action> {
        Ok(Action {
            value: self.value.discover(now_ns, timeout_ns).map_err(error)?,
        })
    }
    #[pyo3(signature=(*,peer_muid,now_ns,timeout_ns))]
    fn inquire_endpoint(
        &mut self,
        peer_muid: &Bound<'_, PyAny>,
        #[pyo3(from_py_with=uint)] now_ns: u64,
        #[pyo3(from_py_with=uint)] timeout_ns: u64,
    ) -> PyResult<Action> {
        let peer = uint(peer_muid)?
            .try_into()
            .map_err(|_| PyValueError::new_err("peer_muid exceeds u32"))?;
        Ok(Action {
            value: self
                .value
                .inquire_endpoint(peer, now_ns, timeout_ns)
                .map_err(error)?,
        })
    }
    #[pyo3(signature=(*,token,sent,now_ns))]
    fn report_send(
        &mut self,
        py: Python<'_>,
        #[pyo3(from_py_with=uint)] token: u64,
        #[pyo3(from_py_with=crate::ump::boolean)] sent: bool,
        #[pyo3(from_py_with=uint)] now_ns: u64,
    ) -> PyResult<Vec<Py<PyAny>>> {
        updates(
            py,
            self.value.report_send(token, sent, now_ns).map_err(error)?,
        )
    }
    #[pyo3(signature=(message,*,input,group=None,now_ns))]
    fn feed(
        &mut self,
        py: Python<'_>,
        message: &Bound<'_, PyAny>,
        input: &str,
        group: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with=uint)] now_ns: u64,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let group = optional_byte(group)?;
        let result = if let Ok(v) = message.extract::<PyRef<'_, crate::messages::SysEx>>() {
            self.value.feed(&v.value, input, group, now_ns)
        } else {
            self.value
                .feed(&extract(message)?.sysex(), input, group, now_ns)
        };
        updates(py, result.map_err(error)?)
    }
    #[pyo3(signature=(*,now_ns))]
    fn poll(
        &mut self,
        py: Python<'_>,
        #[pyo3(from_py_with=uint)] now_ns: u64,
    ) -> PyResult<Vec<Py<PyAny>>> {
        updates(py, self.value.poll(now_ns).map_err(error)?)
    }
    #[pyo3(signature=(*,reason))]
    fn reset(&mut self, py: Python<'_>, reason: String) -> PyResult<Vec<Py<PyAny>>> {
        updates(py, self.value.reset(reason))
    }
    #[pyo3(signature=(*,local_muid,now_ns))]
    fn restart(
        &mut self,
        local_muid: &Bound<'_, PyAny>,
        #[pyo3(from_py_with=uint)] now_ns: u64,
    ) -> PyResult<()> {
        self.value.restart(int(local_muid)?, now_ns).map_err(error)
    }
    #[pyo3(signature=(message,*,now_ns,timeout_ns))]
    fn respond(
        &mut self,
        message: &Bound<'_, PyAny>,
        #[pyo3(from_py_with=uint)] now_ns: u64,
        #[pyo3(from_py_with=uint)] timeout_ns: u64,
    ) -> PyResult<Action> {
        Ok(Action {
            value: self
                .value
                .respond(extract(message)?, now_ns, timeout_ns)
                .map_err(error)?,
        })
    }
}
#[pyfunction]
fn decode(
    py: Python<'_>,
    message: PyRef<'_, crate::messages::SysEx>,
) -> PyResult<Option<Py<PyAny>>> {
    model::decode(&message.value)
        .map_err(error)?
        .map(|v| to_python(py, v))
        .transpose()
}
pub fn encode_message(message: &Bound<'_, PyAny>) -> Option<Vec<u8>> {
    extract(message).ok().map(|v| v.encode())
}
fn extract(message: &Bound<'_, PyAny>) -> PyResult<model::Management> {
    macro_rules! cases{($($name:ident),*)=>{$(if let Ok(v)=message.extract::<PyRef<'_,$name>>(){return Ok(v.value.clone());})*};}
    cases!(
        Discovery,
        DiscoveryReply,
        EndpointInquiry,
        EndpointReply,
        InvalidateMuid,
        Ack,
        Nak
    );
    Err(PyTypeError::new_err(
        "expected a supported MIDI-CI management message",
    ))
}
fn to_python(py: Python<'_>, value: model::Management) -> PyResult<Py<PyAny>> {
    macro_rules! object {
        ($name:ident) => {
            Ok(Py::new(
                py,
                PyClassInitializer::from(Message).add_subclass($name { value }),
            )?
            .into_any())
        };
    }
    match value.body() {
        model::Body::Discovery(_) => object!(Discovery),
        model::Body::DiscoveryReply { .. } => object!(DiscoveryReply),
        model::Body::EndpointInquiry => object!(EndpointInquiry),
        model::Body::EndpointReply { .. } => object!(EndpointReply),
        model::Body::InvalidateMuid { .. } => object!(InvalidateMuid),
        model::Body::Ack(_) => object!(Ack),
        model::Body::Nak(_) => object!(Nak),
    }
}
fn updates(py: Python<'_>, values: Vec<session::Update>) -> PyResult<Vec<Py<PyAny>>> {
    values
        .into_iter()
        .map(|v| match v {
            session::Update::Send(value) => Ok(Py::new(py, Action { value })?.into_any()),
            session::Update::Observe(value) => Ok(Py::new(py, Observation { value })?.into_any()),
        })
        .collect()
}
fn build(
    header: &Header,
    body: model::Body,
    extensions: Option<&Bound<'_, PyAny>>,
) -> PyResult<model::Management> {
    model::Management::new(
        header.value.clone(),
        body,
        extensions
            .map(|v| bytes(v, 1_048_576))
            .transpose()?
            .unwrap_or_default(),
    )
    .map_err(error)
}
fn optional_byte(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<u8>> {
    value
        .map(|v| {
            uint(v)?
                .try_into()
                .map_err(|_| PyValueError::new_err("value exceeds byte range"))
        })
        .transpose()
}
fn int(value: &Bound<'_, PyAny>) -> PyResult<i64> {
    integer(value, "value")
}
fn error(e: meedee::messages::ValueError) -> PyErr {
    PyValueError::new_err(e.to_string())
}
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("BROADCAST_MUID", model::BROADCAST_MUID)?;
    m.add("MAX_MUID", model::MAX_MUID)?;
    m.add_class::<Address>()?;
    m.add_class::<Header>()?;
    m.add_class::<Identity>()?;
    m.add_class::<DiscoveryInfo>()?;
    m.add_class::<Notice>()?;
    m.add_class::<Discovery>()?;
    m.add_class::<DiscoveryReply>()?;
    m.add_class::<EndpointInquiry>()?;
    m.add_class::<EndpointReply>()?;
    m.add_class::<InvalidateMuid>()?;
    m.add_class::<Ack>()?;
    m.add_class::<Nak>()?;
    m.add_class::<Route>()?;
    m.add_class::<Peer>()?;
    m.add_class::<Action>()?;
    m.add_class::<Observation>()?;
    m.add_class::<Session>()?;
    m.add_function(wrap_pyfunction!(decode, m)?)?;
    Ok(())
}
