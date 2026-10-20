use pyo3::prelude::*;

mod errors;
mod input;
mod inspection;
mod notes;
mod output;
mod routing;
mod state;
mod topology;
mod transfer;

mod callback;
mod lifecycle;

mod archive;
mod ci;
mod codecs;
mod comparison;
mod controls;
mod event_values;
mod event_views;
mod file_timing;
mod files;
mod live;
mod messages;
mod packets;
mod playback;
mod ports;
mod profiles;
mod recording;
mod tuning;
mod ump;

#[cfg(all(test, target_os = "macos"))]
mod transport_tests;

/// One private binary. Public loaders select which definitions to register.
#[pymodule]
fn _core(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(populate_messages, module)?)?;
    module.add_function(wrap_pyfunction!(populate_ci, module)?)?;
    module.add_function(wrap_pyfunction!(populate_inspection, module)?)?;
    module.add_function(wrap_pyfunction!(populate_notes, module)?)?;
    module.add_function(wrap_pyfunction!(populate_routing, module)?)?;
    module.add_function(wrap_pyfunction!(populate_topology, module)?)?;
    module.add_function(wrap_pyfunction!(populate_transfer, module)?)?;
    module.add_function(wrap_pyfunction!(populate_midi, module)?)?;
    module.add_function(wrap_pyfunction!(populate_events, module)?)?;
    module.add_function(wrap_pyfunction!(populate_codecs, module)?)?;
    module.add_function(wrap_pyfunction!(populate_controls, module)?)?;
    module.add_function(wrap_pyfunction!(populate_tuning, module)?)?;
    module.add_function(wrap_pyfunction!(populate_gm, module)?)?;
    module.add_function(wrap_pyfunction!(populate_universal, module)?)?;
    module.add_function(wrap_pyfunction!(populate_mpe, module)?)?;
    module.add_function(wrap_pyfunction!(populate_timecode, module)?)?;
    module.add_function(wrap_pyfunction!(populate_ump, module)?)?;
    module.add_function(wrap_pyfunction!(populate_smf, module)?)?;
    module.add_function(wrap_pyfunction!(populate_syx, module)?)?;
    module.add_function(wrap_pyfunction!(populate_clip, module)?)?;
    module.add_function(wrap_pyfunction!(populate_archive, module)?)?;
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
    profiles::register_mpe(module)?;
    event_views::register_mpe(module)
}
#[pyfunction]
fn populate_timecode(module: &Bound<'_, PyModule>) -> PyResult<()> {
    profiles::register_timecode(module)?;
    event_views::register_timecode(module)
}

#[pyfunction]
fn populate_tuning(module: &Bound<'_, PyModule>) -> PyResult<()> {
    tuning::register(module)?;
    event_views::register_tuning(module)
}

#[pyfunction]
fn populate_controls(module: &Bound<'_, PyModule>) -> PyResult<()> {
    controls::register(module)?;
    profiles::register_control_engines(module)?;
    event_views::register_controls(module)
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
    packets::register_events(module)?;
    event_values::register(module)?;
    live::register(module)?;
    playback::register(module)?;
    event_views::register_events(module)
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
    native.add_function(wrap_pyfunction!(live::prepare_message, &native)?)?;
    native.add_function(wrap_pyfunction!(live::live_size, &native)?)?;
    playback::register_native(&native)?;
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
    let typed = PyModule::import(py, "meedee._typed")?;
    for name in [
        "TypedInput",
        "TypedOutput",
        "TypedInputConnection",
        "TypedOutputConnection",
    ] {
        let class = typed.getattr(name)?;
        class.setattr("__module__", "meedee.midi")?;
        module.add(name, class)?;
    }
    let error = PyModule::import(py, "meedee._errors")?.getattr("CompoundSendError")?;
    error.setattr("__module__", "meedee.midi")?;
    module.add("CompoundSendError", error)?;
    Ok(())
}

#[pyfunction]
fn populate_ump(module: &Bound<'_, PyModule>) -> PyResult<()> {
    ump::register(module)?;
    event_views::register_ump(module)
}

#[pyfunction]
fn populate_smf(module: &Bound<'_, PyModule>) -> PyResult<()> {
    files::register_smf(module)?;
    file_timing::register_smf(module)
}
#[pyfunction]
fn populate_syx(module: &Bound<'_, PyModule>) -> PyResult<()> {
    files::register_syx(module)
}
#[pyfunction]
fn populate_clip(module: &Bound<'_, PyModule>) -> PyResult<()> {
    files::register_clip(module)?;
    file_timing::register_clip(module)
}

#[pyfunction]
fn populate_archive(module: &Bound<'_, PyModule>) -> PyResult<()> {
    archive::register(module)?;
    recording::register(module)
}

#[pyfunction]
fn populate_ci(module: &Bound<'_, PyModule>) -> PyResult<()> {
    ci::register(module)
}

#[pyfunction]
fn populate_inspection(module: &Bound<'_, PyModule>) -> PyResult<()> {
    inspection::register(module)
}

#[pyfunction]
fn populate_notes(module: &Bound<'_, PyModule>) -> PyResult<()> {
    notes::register(module)
}

#[pyfunction]
fn populate_routing(module: &Bound<'_, PyModule>) -> PyResult<()> {
    routing::register(module)
}

#[pyfunction]
fn populate_topology(module: &Bound<'_, PyModule>) -> PyResult<()> {
    topology::register(module)
}

#[pyfunction]
fn populate_transfer(module: &Bound<'_, PyModule>) -> PyResult<()> {
    transfer::register(module)
}
