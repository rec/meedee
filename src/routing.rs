//! Python routing surfaces; policies and admission are owned by the Rust core.
use crate::files::{limit, os_error, Stream};
use meedee::routing;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use std::collections::BTreeMap;

#[pyclass(frozen, module = "meedee.routing")]
struct Route {
    value: routing::Route,
}
#[pymethods]
impl Route {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature=(*, name, source, destination, protocol, channel=None, group=None, remap_channel=None, remap_group=None, include_nonchannel=false))]
    fn new(
        name: String,
        source: String,
        destination: String,
        protocol: &str,
        channel: Option<u8>,
        group: Option<u8>,
        remap_channel: Option<u8>,
        remap_group: Option<u8>,
        #[pyo3(from_py_with=crate::ump::boolean)] include_nonchannel: bool,
    ) -> PyResult<Self> {
        Ok(Self {
            value: routing::Route::new(
                name,
                source,
                destination,
                match protocol {
                    "midi1" => meedee::transport::Protocol::Midi1,
                    "ump" => meedee::transport::Protocol::Ump,
                    _ => return Err(PyValueError::new_err("protocol must be midi1 or ump")),
                },
                channel,
                group,
                remap_channel,
                remap_group,
                include_nonchannel,
            )
            .map_err(error)?,
        })
    }
    #[getter]
    fn name(&self) -> &str {
        self.value.name()
    }
    #[getter]
    fn source(&self) -> &str {
        self.value.source()
    }
    #[getter]
    fn destination(&self) -> &str {
        self.value.destination()
    }
    #[getter]
    fn protocol(&self) -> &str {
        match self.value.protocol() {
            meedee::transport::Protocol::Midi1 => "midi1",
            meedee::transport::Protocol::Ump => "ump",
        }
    }
    #[getter]
    fn channel(&self) -> Option<u8> {
        self.value.channel()
    }
    #[getter]
    fn group(&self) -> Option<u8> {
        self.value.group()
    }
    #[getter]
    fn remap_channel(&self) -> Option<u8> {
        self.value.remap_channel()
    }
    #[getter]
    fn remap_group(&self) -> Option<u8> {
        self.value.remap_group()
    }
    #[getter]
    fn include_nonchannel(&self) -> bool {
        self.value.include_nonchannel()
    }
}

#[pyclass(module = "meedee.routing")]
struct Router {
    value: routing::Router,
}
#[pymethods]
impl Router {
    #[new]
    #[pyo3(signature=(*,routes,event_limit=128,byte_limit=8_388_608))]
    fn new(
        routes: Vec<PyRef<'_, Route>>,
        #[pyo3(from_py_with=limit)] event_limit: usize,
        #[pyo3(from_py_with=limit)] byte_limit: usize,
    ) -> PyResult<Self> {
        Ok(Self {
            value: routing::Router::new(
                routes.iter().map(|r| r.value.clone()).collect(),
                event_limit,
                byte_limit,
            )
            .map_err(error)?,
        })
    }
    fn feed(&mut self, item: &Bound<'_, PyAny>) -> PyResult<()> {
        if let Ok(e) = item.extract::<PyRef<'_, crate::event_values::Event>>() {
            self.value.feed(&e.value).map_err(error)
        } else if let Ok(d) = item.extract::<PyRef<'_, crate::live::StreamDiagnostic>>() {
            self.value.boundary(
                &d.value.stream,
                &format!("input diagnostic: {}", d.value.reason),
            );
            Ok(())
        } else {
            Err(PyTypeError::new_err("expected Event or StreamDiagnostic"))
        }
    }
    fn peek(&self) -> Option<Delivery> {
        self.value.peek().cloned().map(|value| Delivery { value })
    }
    fn confirm(&mut self, id: u64) -> PyResult<()> {
        self.value.confirm(id).map_err(error)
    }
    fn fail(&mut self, id: u64, reason: &str) -> PyResult<()> {
        self.value.fail(id, reason).map_err(error)
    }
    fn stop(&mut self, reason: &str) {
        self.value.stop(reason);
    }
    fn snapshot(&self) -> Report {
        Report {
            value: self.value.snapshot(),
        }
    }
}

#[pyclass(frozen, module = "meedee.routing")]
struct Delivery {
    value: routing::Delivery,
}
#[pymethods]
impl Delivery {
    #[getter]
    fn id(&self) -> u64 {
        self.value.id
    }
    #[getter]
    fn route(&self) -> &str {
        &self.value.route
    }
    #[getter]
    fn destination(&self) -> &str {
        &self.value.destination
    }
    #[getter]
    fn event(&self) -> crate::event_values::Event {
        crate::event_values::Event {
            value: self.value.event.clone(),
        }
    }
    #[getter]
    fn wire_bytes(&self) -> usize {
        self.value.wire_bytes
    }
}
#[pyclass(frozen, module = "meedee.routing")]
struct Report {
    value: routing::Report,
}
#[pymethods]
impl Report {
    #[getter]
    fn received(&self) -> u64 {
        self.value.received
    }
    #[getter]
    fn filtered(&self) -> u64 {
        self.value.filtered
    }
    #[getter]
    fn admitted(&self) -> u64 {
        self.value.admitted
    }
    #[getter]
    fn confirmed(&self) -> u64 {
        self.value.confirmed
    }
    #[getter]
    fn capacity_rejections(&self) -> u64 {
        self.value.capacity_rejections
    }
    #[getter]
    fn pending(&self) -> usize {
        self.value.pending
    }
    #[getter]
    fn queued_bytes(&self) -> usize {
        self.value.queued_bytes
    }
    #[getter]
    fn stopped(&self) -> bool {
        self.value.stopped
    }
    #[getter]
    fn reason(&self) -> Option<&str> {
        self.value.reason.as_deref()
    }
    #[getter]
    fn uncertain_delivery(&self) -> Option<u64> {
        self.value.uncertain_delivery
    }
    #[getter]
    fn uncertain_route(&self) -> Option<&str> {
        self.value.uncertain_route.as_deref()
    }
    #[getter]
    fn uncertain_destination(&self) -> Option<&str> {
        self.value.uncertain_destination.as_deref()
    }
    #[getter]
    fn routes(&self) -> BTreeMap<String, BTreeMap<&str, u64>> {
        self.value
            .routes
            .iter()
            .map(|(n, r)| {
                (
                    n.clone(),
                    BTreeMap::from([
                        ("admitted", r.admitted),
                        ("confirmed", r.confirmed),
                        ("capacity_rejections", r.capacity_rejections),
                    ]),
                )
            })
            .collect()
    }
}
#[pyclass(frozen, module = "meedee.routing")]
struct Preview {
    value: routing::Preview,
}
#[pymethods]
impl Preview {
    #[getter]
    fn complete(&self) -> bool {
        self.value.complete
    }
    #[getter]
    fn problem(&self) -> Option<&str> {
        self.value.problem.as_deref()
    }
    #[getter]
    fn records(&self) -> u64 {
        self.value.records
    }
    #[getter]
    fn routing(&self) -> Report {
        Report {
            value: self.value.routing.clone(),
        }
    }
    #[getter]
    fn examples(&self) -> Vec<Delivery> {
        self.value
            .examples
            .iter()
            .map(|v| Delivery { value: v.clone() })
            .collect()
    }
    #[getter]
    fn omitted_examples(&self) -> u64 {
        self.value.omitted_examples
    }
}

#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature=(source,*,routes,event_limit=128,byte_limit=8_388_608,payload_limit=1_048_576,detail_limit=128))]
fn preview_file(
    py: Python<'_>,
    source: &Bound<'_, PyAny>,
    routes: Vec<PyRef<'_, Route>>,
    #[pyo3(from_py_with=limit)] event_limit: usize,
    #[pyo3(from_py_with=limit)] byte_limit: usize,
    #[pyo3(from_py_with=limit)] payload_limit: usize,
    #[pyo3(from_py_with=limit)] detail_limit: usize,
) -> PyResult<Preview> {
    let source = Stream::input(source)?;
    let routes = routes.iter().map(|r| r.value.clone()).collect();
    let value = py
        .detach(move || {
            routing::preview(
                source,
                routes,
                event_limit,
                byte_limit,
                payload_limit,
                detail_limit,
            )
        })
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::InvalidInput {
                error(e)
            } else {
                os_error(e)
            }
        })?;
    Ok(Preview { value })
}
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Route>()?;
    module.add_class::<Router>()?;
    module.add_class::<Delivery>()?;
    module.add_class::<Report>()?;
    module.add_class::<Preview>()?;
    module.add_function(wrap_pyfunction!(preview_file, module)?)
}
fn error(e: std::io::Error) -> PyErr {
    PyValueError::new_err(e.to_string())
}
