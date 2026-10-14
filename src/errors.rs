use std::io::{self, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyRuntimeError, PyTimeoutError};
use pyo3::PyResult;

/// Callers must invalidate affected ownership when a native call unwinds.
pub fn native_call<T>(operation: &str, call: impl FnOnce() -> T) -> PyResult<T> {
    catch_unwind(AssertUnwindSafe(call)).map_err(|payload| {
        let detail = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("unknown panic");
        NativePanicError::new_err(format!(
            "native MIDI {operation} panicked; recreate affected resources: {detail}"
        ))
    })
}

pub fn failed_error() -> pyo3::PyErr {
    StateError::new_err("MIDI resource failed; create a new client")
}

/// Diagnostics must not turn cleanup failures into another unwind.
pub fn report_failure(message: &str) {
    let _ = writeln!(io::stderr().lock(), "meedee: {message}");
}

create_exception!(
    meedee.midi,
    MidiError,
    PyException,
    "A native MIDI operation failed."
);
create_exception!(
    meedee.midi,
    InitError,
    MidiError,
    "The MIDI backend could not initialize."
);
create_exception!(
    meedee.midi,
    CapabilityError,
    MidiError,
    "The requested transport capability is unavailable."
);

pub fn transport_error(error: meedee::transport::Error, operation: &str) -> pyo3::PyErr {
    use meedee::transport::Error;
    match error {
        Error::Capability(message) => CapabilityError::new_err(message),
        Error::Invalid(message) => pyo3::exceptions::PyValueError::new_err(message),
        Error::Busy | Error::Closed => StateError::new_err(error.to_string()),
        Error::CallbackThread => CallbackThreadError::new_err(error.to_string()),
        Error::Resource(message) => ResourceError::new_err(message),
        Error::Worker(message) => ResourceError::new_err(message),
        Error::Native(message) => match operation {
            "init" => InitError::new_err(message),
            "ports" => PortInfoError::new_err(message),
            "open" => ConnectError::new_err(message),
            "send" => SendError::new_err(message),
            _ => MidiError::new_err(message),
        },
    }
}
create_exception!(
    meedee.midi,
    PortInfoError,
    MidiError,
    "Port information could not be retrieved."
);
create_exception!(
    meedee.midi,
    ConnectError,
    MidiError,
    "A MIDI connection could not be opened."
);
create_exception!(
    meedee.midi,
    SendError,
    MidiError,
    "A MIDI message could not be sent."
);

create_exception!(
    meedee.midi,
    StateError,
    PyRuntimeError,
    "MIDI ownership state prevents this operation."
);
create_exception!(
    meedee.midi,
    CallbackThreadError,
    PyRuntimeError,
    "A blocking MIDI operation was attempted from a callback."
);
create_exception!(
    meedee.midi,
    NativePanicError,
    PyRuntimeError,
    "A recoverable native MIDI panic occurred."
);
create_exception!(
    meedee.midi,
    ResourceError,
    PyRuntimeError,
    "MIDI lifecycle resources could not be allocated."
);
create_exception!(
    meedee.midi,
    WorkerError,
    PyRuntimeError,
    "The MIDI worker exited or communication failed."
);
create_exception!(
    meedee.midi,
    WorkerTimeoutError,
    PyTimeoutError,
    "A MIDI worker operation exceeded its deadline."
);
