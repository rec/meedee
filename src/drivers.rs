//! Python connection, clock and exception adapters for shared Rust workflow loops.
use meedee::{
    drivers::{Receiver, Sender},
    events::{Cancellation, InputItem, Payload, PlaybackClock},
    midi,
};
use pyo3::exceptions::{PyKeyboardInterrupt, PyOSError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::{cell::RefCell, io, time::Duration};

struct Calls<'py> {
    py: Python<'py>,
    failure: RefCell<Option<PyErr>>,
}
impl<'py> Calls<'py> {
    fn new(py: Python<'py>) -> Self {
        Self {
            py,
            failure: RefCell::new(None),
        }
    }
    fn convert<T>(&self, result: PyResult<T>) -> midi::Result<T> {
        result.map_err(|error| {
            let kind = if error.is_instance_of::<PyKeyboardInterrupt>(self.py) {
                io::ErrorKind::Interrupted
            } else {
                io::ErrorKind::Other
            };
            let message = error
                .value(self.py)
                .str()
                .map(|s| s.to_string())
                .unwrap_or_else(|_| error.to_string());
            if self.failure.borrow().is_none() {
                *self.failure.borrow_mut() = Some(error);
            }
            io::Error::new(kind, message).into()
        })
    }
    fn check(&self) -> midi::Result<()> {
        if let Some(error) = self.failure.borrow().as_ref() {
            let kind = if error.is_instance_of::<PyKeyboardInterrupt>(self.py) {
                io::ErrorKind::Interrupted
            } else {
                io::ErrorKind::Other
            };
            return Err(io::Error::new(kind, error.to_string()).into());
        }
        self.convert(self.py.check_signals())
    }
    fn finish<T>(&self, result: midi::Result<T>, propagate_expected: bool) -> PyResult<T> {
        if let Some(error) = self.failure.borrow_mut().take() {
            if (propagate_expected || !expected(self.py, &error))
                && !error.is_instance_of::<PyKeyboardInterrupt>(self.py)
            {
                return Err(error);
            }
        }
        result.map_err(|e| PyValueError::new_err(e.to_string()))
    }
}
fn expected(py: Python<'_>, error: &PyErr) -> bool {
    error.is_instance_of::<crate::errors::MidiError>(py)
        || error.is_instance_of::<crate::errors::StateError>(py)
        || error.is_instance_of::<crate::errors::CallbackThreadError>(py)
        || error.is_instance_of::<crate::errors::NativePanicError>(py)
        || error.is_instance_of::<crate::errors::ResourceError>(py)
        || error.is_instance_of::<crate::errors::WorkerError>(py)
        || error.is_instance_of::<crate::errors::WorkerTimeoutError>(py)
        || error.is_instance_of::<PyOSError>(py)
        || error.is_instance_of::<PyValueError>(py)
}
struct Output<'a, 'py> {
    calls: &'a Calls<'py>,
    object: Bound<'py, PyAny>,
}
struct Input<'a, 'py> {
    calls: &'a Calls<'py>,
    object: Bound<'py, PyAny>,
}
impl Receiver for Input<'_, '_> {
    fn dropped_packets(&self) -> midi::Result<u64> {
        self.calls.convert(
            self.object
                .getattr("dropped_packets")
                .and_then(|v| v.extract()),
        )
    }
    fn check_monitor(&self) -> midi::Result<()> {
        self.calls.check()?;
        let failure = self.calls.convert(self.object.getattr("failure"))?;
        if !failure.is_none() {
            return self.calls.convert(Err(PyErr::from_value(failure)));
        }
        Ok(())
    }
    fn check(&self) -> midi::Result<()> {
        self.calls.check()?;
        let dropped: u64 = self.calls.convert(
            self.object
                .getattr("dropped_packets")
                .and_then(|v| v.extract()),
        )?;
        if dropped != 0 {
            return Err(midi::Error::Transport(meedee::transport::Error::Resource(
                "input packet loss reported by transport counter".into(),
            )));
        }
        self.check_monitor()
    }
    fn receive(&mut self, timeout: Duration) -> midi::Result<Option<InputItem>> {
        self.calls.check()?;
        let kwargs = PyDict::new(self.calls.py);
        self.calls
            .convert(kwargs.set_item("timeout", timeout.as_secs_f64()))?;
        let value = self
            .calls
            .convert(self.object.call_method("receive", (), Some(&kwargs)))?;
        if value.is_none() {
            return Ok(None);
        }
        if let Ok(d) = value.extract::<PyRef<'_, crate::live::StreamDiagnostic>>() {
            return Ok(Some(InputItem::Diagnostic(d.value.clone())));
        }
        self.calls
            .convert(crate::event_values::extract(&value))
            .map(|v| Some(InputItem::Message(Box::new(v))))
    }
}
impl Sender for Output<'_, '_> {
    fn send_management(&mut self, message: meedee::ci::Management) -> midi::Result<()> {
        self.calls.check()?;
        let message = self
            .calls
            .convert(crate::ci::to_python(self.calls.py, message))?;
        self.calls
            .convert(self.object.call_method1("send", (message,)))?;
        Ok(())
    }
    fn send(&mut self, message: Payload) -> midi::Result<()> {
        self.calls.check()?;
        let message = self
            .calls
            .convert(crate::event_values::to_python(self.calls.py, message))?;
        let report = self
            .calls
            .convert(self.object.call_method1("send", (message,)))?;
        if report
            .extract::<PyRef<'_, crate::playback::PlaybackReport>>()
            .is_ok_and(|report| report.value.cancelled)
        {
            return Err(midi::Error::Cancelled);
        }
        Ok(())
    }
}
struct Clock<'a, 'py> {
    calls: &'a Calls<'py>,
    now: Bound<'py, PyAny>,
    cancel: Bound<'py, PyAny>,
    last: u64,
}
impl PlaybackClock for Clock<'_, '_> {
    fn now_ns(&mut self) -> u64 {
        if let Ok(now) = self
            .calls
            .convert(self.now.call0().and_then(|n| n.extract()))
        {
            self.last = now;
        }
        self.last
    }
    fn wait_until(&mut self, deadline: u64, cancel: &Cancellation) -> io::Result<bool> {
        self.calls.check().map_err(io::Error::from)?;
        let seconds = (deadline.saturating_sub(self.last) as f64 / 1_000_000_000.0).min(0.05);
        self.calls
            .convert(self.cancel.call_method1("wait_for", (seconds,)))
            .map_err(io::Error::from)?;
        Ok(!cancel.cancelled())
    }
}

#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_sysex_dump(
    py: Python<'_>,
    receiver: Bound<'_, PyAny>,
    output: Bound<'_, PyAny>,
    model: Option<String>,
    scope: String,
    device_id: Option<u8>,
    #[pyo3(from_py_with=crate::units::duration)] timeout: u64,
    #[pyo3(from_py_with=crate::units::duration)] silence: u64,
    #[pyo3(from_py_with=crate::units::duration)] identity_timeout: u64,
    #[pyo3(from_py_with=crate::files::limit)] message_limit: usize,
    #[pyo3(from_py_with=crate::units::bytes)] byte_limit: usize,
    #[pyo3(from_py_with=crate::units::bytes)] payload_limit: usize,
    cancel: Bound<'_, PyAny>,
    now: Bound<'_, PyAny>,
) -> PyResult<crate::sysex::DumpReport> {
    let stream = receiver.getattr("stream")?.extract::<String>()?;
    let token = cancel
        .extract::<PyRef<'_, crate::playback::Cancellation>>()?
        .value
        .clone();
    let calls = Calls::new(py);
    let mut receiver = Input {
        calls: &calls,
        object: receiver,
    };
    let mut output = if output.is_none() {
        None
    } else {
        Some(Output {
            calls: &calls,
            object: output,
        })
    };
    let mut clock = Clock {
        calls: &calls,
        now,
        cancel,
        last: 0,
    };
    let options = meedee::sysex::DumpOptions {
        scope,
        device_id,
        timeout: Duration::from_nanos(timeout),
        silence: Duration::from_nanos(silence),
        identity_timeout: Duration::from_nanos(identity_timeout),
        message_limit,
        byte_limit,
        payload_limit,
        cancel: token,
    };
    let result = meedee::sysex::dump_with(
        &mut receiver,
        output.as_mut(),
        &stream,
        model.as_deref(),
        &options,
        &mut clock,
    );
    calls.finish(
        result.map(|value| crate::sysex::DumpReport { value }),
        false,
    )
}

#[pyfunction]
pub(crate) fn run_tuning(
    py: Python<'_>,
    mut query: PyRefMut<'_, crate::tuning::Query>,
    receiver: Bound<'_, PyAny>,
    output: Bound<'_, PyAny>,
    cancel: Bound<'_, PyAny>,
    now: Bound<'_, PyAny>,
) -> PyResult<crate::tuning::QueryReport> {
    if query.value.snapshot().status != "ready" {
        return Err(PyValueError::new_err("query must be fresh"));
    }
    let token = cancel
        .extract::<PyRef<'_, crate::playback::Cancellation>>()?
        .value
        .clone();
    let calls = Calls::new(py);
    let mut receiver = Input {
        calls: &calls,
        object: receiver,
    };
    let mut output = Output {
        calls: &calls,
        object: output,
    };
    let mut clock = Clock {
        calls: &calls,
        now,
        cancel,
        last: 0,
    };
    if let Err(error) = meedee::tuning::query_with(
        &mut query.value,
        &mut receiver,
        &mut output,
        &mut clock,
        &token,
    ) {
        query.value.fail(&error.to_string());
    }
    calls.finish(
        Ok(crate::tuning::QueryReport {
            value: query.value.snapshot(),
        }),
        false,
    )
}

#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_ci(
    py: Python<'_>,
    mut browser: PyRefMut<'_, crate::ci::Browser>,
    receiver: Bound<'_, PyAny>,
    output: Bound<'_, PyAny>,
    cancel: Bound<'_, PyAny>,
    now: Bound<'_, PyAny>,
    #[pyo3(from_py_with=crate::files::limit)] message_limit: usize,
    #[pyo3(from_py_with=crate::files::limit)] byte_limit: usize,
) -> PyResult<crate::ci::BrowseReport> {
    if browser.value.snapshot().status != "ready" {
        return Err(PyValueError::new_err("browser must be fresh"));
    }
    if receiver.getattr("stream")?.extract::<String>()? != browser.value.route().input() {
        return Err(PyValueError::new_err(
            "browser route must match the receiver stream",
        ));
    }
    let token = cancel
        .extract::<PyRef<'_, crate::playback::Cancellation>>()?
        .value
        .clone();
    let calls = Calls::new(py);
    let mut receiver = Input {
        calls: &calls,
        object: receiver,
    };
    let mut output = Output {
        calls: &calls,
        object: output,
    };
    let mut clock = Clock {
        calls: &calls,
        now,
        cancel,
        last: 0,
    };
    let options = meedee::ci::Options {
        cancel: token,
        message_limit,
        byte_limit,
    };
    let result = meedee::ci::browse_with(
        &mut browser.value,
        &mut receiver,
        &mut output,
        &mut clock,
        &options,
    );
    calls
        .finish(result, true)
        .map(|value| crate::ci::BrowseReport { value })
}

/// Forward messages using a router and caller-owned input/output connections.
#[pyfunction(name = "run")]
#[pyo3(signature=(router, receiver, outputs, *, duration=None, cancel=None))]
pub(crate) fn run_routing(
    py: Python<'_>,
    mut router: PyRefMut<'_, crate::routing::Router>,
    receiver: Bound<'_, PyAny>,
    outputs: Bound<'_, PyDict>,
    duration: Option<&Bound<'_, PyAny>>,
    cancel: Option<PyRef<'_, crate::playback::Cancellation>>,
) -> PyResult<crate::routing::Report> {
    let duration = duration
        .map(crate::units::duration)
        .transpose()?
        .map(Duration::from_nanos);
    let token = cancel.map(|c| c.value.clone()).unwrap_or_default();
    let calls = Calls::new(py);
    let mut receiver = Input {
        calls: &calls,
        object: receiver,
    };
    let result = meedee::routing::run_with(
        &mut router.value,
        &mut receiver,
        |delivery| {
            let object = calls.convert(outputs.get_item(&delivery.destination).and_then(|v| {
                v.ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(delivery.destination.clone()))
            }))?;
            Output {
                calls: &calls,
                object,
            }
            .send(delivery.event.clone())
        },
        &mut meedee::events::MonotonicClock::default(),
        duration,
        &token,
    );
    if let Some(error) = calls.failure.borrow_mut().take() {
        return Err(error);
    }
    result
        .map(|value| crate::routing::Report { value })
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyclass(module = "meedee._core")]
pub(crate) struct DiscoveryPollState {
    value: meedee::discovery::PollState,
    protocol: String,
}
#[pymethods]
impl DiscoveryPollState {
    #[new]
    fn new(
        protocol: String,
        #[pyo3(from_py_with=crate::files::limit)] port_limit: usize,
        #[pyo3(from_py_with=crate::files::limit)] detail_limit: usize,
        #[pyo3(from_py_with=crate::files::limit)] history_limit: usize,
    ) -> PyResult<Self> {
        Ok(Self {
            value: meedee::discovery::PollState::new(port_limit, detail_limit, history_limit)
                .map_err(|e| PyValueError::new_err(e.to_string()))?,
            protocol,
        })
    }
    fn poll(
        &mut self,
        py: Python<'_>,
        ports: Bound<'_, PyAny>,
    ) -> PyResult<crate::topology::Report> {
        if self.value.closed() {
            return Err(PyValueError::new_err("discovery poller is closed"));
        }
        let calls = Calls::new(py);
        let result = self.value.scan(|direction| {
            let values = calls.convert(ports.call1((direction,)))?;
            let mut endpoints = Vec::new();
            for value in calls.convert(values.try_iter())? {
                let port = calls.convert(value)?;
                let backend: crate::packets::Backend = calls.convert(
                    port.getattr("backend")
                        .and_then(|v| v.extract().map_err(Into::into)),
                )?;
                let id: u64 =
                    calls.convert(port.getattr("backend_id").and_then(|v| v.extract()))?;
                let name: String = calls.convert(port.getattr("name").and_then(|v| v.extract()))?;
                endpoints.push(meedee::topology::Endpoint::new(
                    format!("{:?}", backend.native()),
                    self.protocol.clone(),
                    direction.into(),
                    id,
                    name,
                )?);
            }
            Ok(endpoints)
        });
        if let Some(error) = calls.failure.borrow_mut().take() {
            if !expected(py, &error) {
                return Err(error);
            }
        }
        let value = match result {
            Ok(report) => report,
            Err(error) => self.value.fail(&error.to_string()),
        };
        Ok(crate::topology::Report { value })
    }
    fn snapshot(&self) -> crate::topology::Report {
        crate::topology::Report {
            value: self.value.snapshot(),
        }
    }
    #[getter]
    fn closed(&self) -> bool {
        self.value.closed()
    }
    fn close(&mut self) {
        self.value.close();
    }
}

#[pyfunction]
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_monitor(
    py: Python<'_>,
    mut monitor: PyRefMut<'_, crate::inspection::Monitor>,
    receiver: Bound<'_, PyAny>,
    callback: Bound<'_, PyAny>,
    duration: Bound<'_, PyAny>,
    cancel: Bound<'_, PyAny>,
    now: Bound<'_, PyAny>,
    metrics: bool,
) -> PyResult<crate::inspection::MonitorReport> {
    let duration = if duration.is_none() {
        None
    } else {
        Some(Duration::from_nanos(crate::units::duration(&duration)?))
    };
    let token = cancel
        .extract::<PyRef<'_, crate::playback::Cancellation>>()?
        .value
        .clone();
    let calls = Calls::new(py);
    let mut receiver = Input {
        calls: &calls,
        object: receiver,
    };
    let mut clock = Clock {
        calls: &calls,
        now,
        cancel,
        last: 0,
    };
    let result = meedee::inspection::run_with(
        &mut monitor.value,
        &mut receiver,
        |entry| {
            if !callback.is_none() {
                calls.convert(callback.call1((crate::inspection::MonitorEntry {
                    value: entry.clone(),
                },)))?;
            }
            Ok(())
        },
        &mut clock,
        duration,
        &token,
        metrics,
    );
    calls
        .finish(result, true)
        .map(|value| crate::inspection::MonitorReport { value })
}
