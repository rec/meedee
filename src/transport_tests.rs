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
                let outgoing = output.call_method1("connect", (port,)).unwrap();
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
