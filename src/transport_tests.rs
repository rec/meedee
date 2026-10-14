//! Exercise the actual Python adapter and C++ backend with fake OS endpoints.
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList};

use crate::packets::{Protocol, TimestampMode, UmpPacket};

#[test]
fn python_adapter_preserves_native_bytes_ump_clocks_and_owner_reuse() {
    meedee_coremidi_fixture::enable();
    Python::initialize();
    Python::attach(|py| {
        crate::lifecycle::initialize(py).unwrap();
        let messages = PyModule::new(py, "meedee.messages").unwrap();
        crate::messages::register(&messages).unwrap();
        for protocol in [Protocol::Midi1, Protocol::Ump] {
            for mode in [TimestampMode::Absolute, TimestampMode::Off] {
                let received = PyList::empty(py);
                let options = PyDict::new(py);
                options
                    .set_item("identity", "native-binding-input")
                    .unwrap();
                options.set_item("protocol", protocol).unwrap();
                options.set_item("timestamps", mode).unwrap();
                let input = py
                    .get_type::<crate::input::Input>()
                    .call((), Some(&options))
                    .unwrap();
                let options = PyDict::new(py);
                options
                    .set_item("identity", "native-binding-output")
                    .unwrap();
                options.set_item("protocol", protocol).unwrap();
                let output = py
                    .get_type::<crate::output::Output>()
                    .call((), Some(&options))
                    .unwrap();
                let name = format!("python-ffi-{protocol:?}-{mode:?}");
                let incoming = input
                    .call_method1(
                        "create_virtual",
                        (&name, received.getattr("append").unwrap(), &name),
                    )
                    .unwrap();
                let ports = output.call_method0("ports").unwrap();
                let port = ports
                    .try_iter()
                    .unwrap()
                    .map(Result::unwrap)
                    .find(|p| p.getattr("name").unwrap().extract::<String>().unwrap() == name)
                    .unwrap();
                let outgoing = output.call_method1("connect", (&port,)).unwrap();
                let data = if protocol == Protocol::Midi1 {
                    PyBytes::new(py, &[0x90, 60, 100]).into_any().unbind()
                } else {
                    Py::new(
                        py,
                        UmpPacket {
                            words: vec![0x40903c00, 0xffffffff],
                        },
                    )
                    .unwrap()
                    .into_any()
                };
                outgoing.call_method1("send", (&data,)).unwrap();
                assert_eq!(received.len(), 1);
                let packet = received.get_item(0).unwrap();
                assert!(packet.getattr("packet").unwrap().eq(data.bind(py)).unwrap());
                assert!(packet
                    .getattr("clock")
                    .unwrap()
                    .eq(incoming.getattr("clock").unwrap())
                    .unwrap());
                assert_eq!(
                    packet.getattr("timestamp_ns").unwrap().is_none(),
                    mode == TimestampMode::Off
                );
                let fields = PyDict::new(py);
                fields.set_item("channel", 1).unwrap();
                fields.set_item("controller", 7).unwrap();
                fields.set_item("value", 10000).unwrap();
                let command = messages
                    .getattr("ControlChange14")
                    .unwrap()
                    .call((), Some(&fields))
                    .unwrap();
                if protocol == Protocol::Midi1 {
                    outgoing.call_method1("send_command", (command,)).unwrap();
                    assert_eq!(received.len(), 3);
                    for (index, bytes) in [(1, [0xb0, 7, 78]), (2, [0xb0, 39, 16])] {
                        assert!(received
                            .get_item(index)
                            .unwrap()
                            .getattr("packet")
                            .unwrap()
                            .eq(PyBytes::new(py, &bytes))
                            .unwrap());
                    }
                } else {
                    assert!(outgoing.call_method1("send_command", (command,)).is_err());
                    assert!(!outgoing.getattr("closed").unwrap().is_truthy().unwrap());
                }
                outgoing.call_method0("close").unwrap();
                incoming.call_method0("close").unwrap();
                assert!(incoming.getattr("closed").unwrap().is_truthy().unwrap());
                assert_eq!(
                    incoming
                        .getattr("dropped_packets")
                        .unwrap()
                        .extract::<u64>()
                        .unwrap(),
                    0
                );
                assert_eq!(input.call_method0("ports").unwrap().len().unwrap(), 0);
                assert_eq!(output.call_method0("ports").unwrap().len().unwrap(), 0);
            }
        }
    });
}

#[test]
fn python_compound_error_retains_progress_and_invalidates_the_connection() {
    meedee_coremidi_fixture::enable();
    Python::initialize();
    Python::attach(|py| {
        // Exercise the real Python exception helper alongside the built package,
        // as the pytest harness and uv-backed workspace checks do.
        PyModule::import(py, "sys")
            .unwrap()
            .getattr("path")
            .unwrap()
            .call_method1("insert", (0, env!("CARGO_MANIFEST_DIR")))
            .unwrap();
        crate::lifecycle::initialize(py).unwrap();
        let messages = PyModule::new(py, "meedee.messages").unwrap();
        crate::messages::register(&messages).unwrap();
        let options = PyDict::new(py);
        options
            .set_item("identity", "compound-error-input")
            .unwrap();
        let input = py
            .get_type::<crate::input::Input>()
            .call((), Some(&options))
            .unwrap();
        options
            .set_item("identity", "compound-error-output")
            .unwrap();
        let output = py
            .get_type::<crate::output::Output>()
            .call((), Some(&options))
            .unwrap();
        let received = PyList::empty(py);
        let incoming = input
            .call_method1(
                "create_virtual",
                (
                    "compound-binding-failure",
                    received.getattr("append").unwrap(),
                    "compound-clock",
                ),
            )
            .unwrap();
        let ports = output.call_method0("ports").unwrap();
        let port = ports
            .try_iter()
            .unwrap()
            .map(Result::unwrap)
            .find(|p| {
                p.getattr("name").unwrap().extract::<String>().unwrap()
                    == "compound-binding-failure"
            })
            .unwrap();
        let endpoint = port
            .getattr("backend_id")
            .unwrap()
            .extract::<u32>()
            .unwrap();
        let outgoing = output.call_method1("connect", (port,)).unwrap();
        let fields = PyDict::new(py);
        fields.set_item("channel", 1).unwrap();
        fields.set_item("controller", 7).unwrap();
        fields.set_item("value", 10000).unwrap();
        let command = messages
            .getattr("ControlChange14")
            .unwrap()
            .call((), Some(&fields))
            .unwrap();
        meedee_coremidi_fixture::fail_send_after(endpoint, 1);
        let error = outgoing
            .call_method1("send_command", (command,))
            .unwrap_err();
        assert_eq!(
            error.get_type(py).name().unwrap(),
            "CompoundSendError",
            "{error}"
        );
        assert_eq!(
            error
                .value(py)
                .getattr("completed_messages")
                .unwrap()
                .extract::<usize>()
                .unwrap(),
            1
        );
        assert_eq!(
            error
                .value(py)
                .getattr("total_messages")
                .unwrap()
                .extract::<usize>()
                .unwrap(),
            2
        );
        assert_eq!(received.len(), 1);
        // Failed is not the same as closed: native ownership is retained until
        // teardown finishes, but subsequent use must already be rejected.
        assert!(!outgoing.getattr("closed").unwrap().is_truthy().unwrap());
        assert!(outgoing
            .call_method1("send", (PyBytes::new(py, &[0xf8]),))
            .is_err());
        assert!(outgoing.call_method0("close").is_err());
        assert!(outgoing.getattr("closed").unwrap().is_truthy().unwrap());
        incoming.call_method0("close").unwrap();
    });
}
