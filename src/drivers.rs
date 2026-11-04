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
    fn check(&self) -> midi::Result<()> {
        self.calls.check()?;
        let dropped: u64 = self.calls.convert(
            self.object
                .getattr("dropped_packets")
                .and_then(|v| v.extract()),
        )?;
        if dropped != 0 {
            return Err(midi::Error::Transport(meedee::transport::Error::Resource(
                "input packet loss reported by transport counter",
            )));
        }
        let failure = self.calls.convert(self.object.getattr("failure"))?;
        if !failure.is_none() {
            return self.calls.convert(Err(PyErr::from_value(failure)));
        }
        Ok(())
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
        self.calls.convert(self.object.call1((message,)))?;
        Ok(())
    }
    fn send(&mut self, message: Payload) -> midi::Result<()> {
        self.calls.check()?;
        let message = self
            .calls
            .convert(crate::event_values::to_python(self.calls.py, message))?;
        self.calls.convert(self.object.call1((message,)))?;
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
pub(crate) fn run_transfer_output(
    py: Python<'_>,
    mut session: PyRefMut<'_, crate::transfer::Session>,
    output: Bound<'_, PyAny>,
    cancel: Bound<'_, PyAny>,
    now: Bound<'_, PyAny>,
) -> PyResult<crate::transfer::Report> {
    let token = cancel
        .extract::<PyRef<'_, crate::playback::Cancellation>>()?
        .value
        .clone();
    let calls = Calls::new(py);
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
    let result =
        meedee::transfer_output::run_with(&mut session.value, &mut output, &mut clock, &token);
    calls
        .finish(result, false)
        .map(|value| crate::transfer::Report { value })
}

#[pyfunction]
pub(crate) fn run_transfer_input(
    py: Python<'_>,
    mut capture: PyRefMut<'_, crate::transfer::Capture>,
    receiver: Bound<'_, PyAny>,
    cancel: Bound<'_, PyAny>,
    now: Bound<'_, PyAny>,
) -> PyResult<crate::transfer::CaptureReport> {
    if capture.value.snapshot().status != "ready" {
        return Err(PyValueError::new_err("capture must be fresh"));
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
    let mut clock = Clock {
        calls: &calls,
        now,
        cancel,
        last: 0,
    };
    if let Err(error) =
        meedee::transfer_input::run_with(&mut capture.value, &mut receiver, &mut clock, &token)
    {
        capture.value.fail(&error.to_string());
    }
    calls.finish(
        Ok(crate::transfer::CaptureReport {
            value: capture.value.snapshot(),
        }),
        false,
    )
}

#[pyfunction]
pub(crate) fn run_tuning(
    py: Python<'_>,
    mut query: PyRefMut<'_, crate::tuning_query::Query>,
    receiver: Bound<'_, PyAny>,
    output: Bound<'_, PyAny>,
    cancel: Bound<'_, PyAny>,
    now: Bound<'_, PyAny>,
) -> PyResult<crate::tuning_query::QueryReport> {
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
    if let Err(error) = meedee::tuning_live::run_with(
        &mut query.value,
        &mut receiver,
        &mut output,
        &mut clock,
        &token,
    ) {
        query.value.fail(&error.to_string());
    }
    calls.finish(
        Ok(crate::tuning_query::QueryReport {
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
    let options = meedee::ci_live::Options {
        cancel: token,
        message_limit,
        byte_limit,
    };
    let result = meedee::ci_live::run_with(
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
