use pyo3::prelude::*;

mod errors;
mod input;
mod output;
mod state;

mod callback;
mod lifecycle;

mod codecs;
mod controls;
mod messages;
mod packets;
mod ports;
mod profiles;
mod tuning;

#[cfg(all(test, target_os = "macos"))]
mod transport_tests;

/// One private binary. Public loaders select which definitions to register.
#[pymodule]
fn _core(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(populate_messages, module)?)?;
    module.add_function(wrap_pyfunction!(populate_midi, module)?)?;
    module.add_function(wrap_pyfunction!(populate_events, module)?)?;
    module.add_function(wrap_pyfunction!(populate_codecs, module)?)?;
    module.add_function(wrap_pyfunction!(populate_controls, module)?)?;
    module.add_function(wrap_pyfunction!(populate_tuning, module)?)?;
    module.add_function(wrap_pyfunction!(populate_gm, module)?)?;
    module.add_function(wrap_pyfunction!(populate_universal, module)?)?;
    module.add_function(wrap_pyfunction!(populate_mpe, module)?)?;
    module.add_function(wrap_pyfunction!(populate_timecode, module)?)?;
    Ok(())
}

#[pyfunction]
fn populate_gm(module: &Bound<'_, PyModule>) -> PyResult<()> {
    profiles::register_gm(module)
}
#[pyfunction]
fn populate_universal(module: &Bound<'_, PyModule>) -> PyResult<()> {
    profiles::register_universal(module)
}
#[pyfunction]
fn populate_mpe(module: &Bound<'_, PyModule>) -> PyResult<()> {
    profiles::register_mpe(module)
}
#[pyfunction]
fn populate_timecode(module: &Bound<'_, PyModule>) -> PyResult<()> {
    profiles::register_timecode(module)
}

#[pyfunction]
fn populate_tuning(module: &Bound<'_, PyModule>) -> PyResult<()> {
    tuning::register(module)
}

#[pyfunction]
fn populate_controls(module: &Bound<'_, PyModule>) -> PyResult<()> {
    controls::register(module)?;
    profiles::register_control_engines(module)
}

#[pyfunction]
fn populate_codecs(module: &Bound<'_, PyModule>) -> PyResult<()> {
    codecs::register(module)
}

#[pyfunction]
fn populate_messages(module: &Bound<'_, PyModule>) -> PyResult<()> {
    messages::register(module)
}

#[pyfunction]
fn populate_events(module: &Bound<'_, PyModule>) -> PyResult<()> {
    packets::register_events(module)
}

#[pyfunction]
fn populate_midi(module: &Bound<'_, PyModule>) -> PyResult<()> {
    lifecycle::initialize(module.py())?;
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    packets::register_transport(module)?;
    let py = module.py();
    module.add("MidiError", py.get_type::<errors::MidiError>())?;
    module.add("InitError", py.get_type::<errors::InitError>())?;
    module.add("PortInfoError", py.get_type::<errors::PortInfoError>())?;
    module.add("ConnectError", py.get_type::<errors::ConnectError>())?;
    module.add("SendError", py.get_type::<errors::SendError>())?;
    module.add("CapabilityError", py.get_type::<errors::CapabilityError>())?;
    module.add("StateError", py.get_type::<errors::StateError>())?;
    module.add(
        "CallbackThreadError",
        py.get_type::<errors::CallbackThreadError>(),
    )?;
    module.add(
        "NativePanicError",
        py.get_type::<errors::NativePanicError>(),
    )?;
    module.add("ResourceError", py.get_type::<errors::ResourceError>())?;
    module.add("WorkerError", py.get_type::<errors::WorkerError>())?;
    module.add(
        "WorkerTimeoutError",
        py.get_type::<errors::WorkerTimeoutError>(),
    )?;
    let native = PyModule::new(py, "meedee._native")?;
    for name in [
        "MidiError",
        "InitError",
        "PortInfoError",
        "ConnectError",
        "SendError",
        "CapabilityError",
        "StateError",
        "CallbackThreadError",
        "NativePanicError",
        "ResourceError",
        "WorkerError",
        "WorkerTimeoutError",
    ] {
        native.add(name, module.getattr(name)?)?;
    }
    native.add_class::<input::Input>()?;
    native.add_class::<input::InputConnection>()?;
    native.add_class::<output::Output>()?;
    native.add_class::<output::OutputConnection>()?;
    module.add_class::<ports::Port>()?;
    native.add_class::<callback::CallbackBridge>()?;
    native.add_function(wrap_pyfunction!(callback::check_thread, &native)?)?;
    native.add_function(wrap_pyfunction!(packets::prepare_send, &native)?)?;
    native.add_function(wrap_pyfunction!(codecs::prepare_command, &native)?)?;
    PyModule::import(py, "sys")?
        .getattr("modules")?
        .set_item("meedee._native", native)?;
    let clients = PyModule::import(py, "meedee._clients")?;
    for name in ["Input", "Output", "InputConnection", "OutputConnection"] {
        let class = clients.getattr(name)?;
        class.setattr("__module__", "meedee.midi")?;
        module.add(name, class)?;
    }
    let error = PyModule::import(py, "meedee._errors")?.getattr("CompoundSendError")?;
    error.setattr("__module__", "meedee.midi")?;
    module.add("CompoundSendError", error)?;
    Ok(())
}
