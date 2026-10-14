#![cfg(target_os = "macos")]

use meedee::transport::{
    Input, InputOptions, Output, OutputOptions, Packet, Protocol, TimestampMode,
};
use meedee_coremidi_fixture::{emit, enable, endpoint_exists, fail_disposal, fail_send_after};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::Duration;

#[test]
fn ffi_round_trip_preserves_bytes_ump_clocks_and_client_ownership() {
    enable();
    for (protocol, packet) in [
        (Protocol::Midi1, Packet::Bytes(vec![0x90, 60, 100])),
        (Protocol::Ump, Packet::Ump(vec![0x40903c00, 0xc0000000])),
    ] {
        for timestamps in [TimestampMode::Absolute, TimestampMode::Off] {
            let options = InputOptions {
                protocol,
                timestamps,
                ..InputOptions::default()
            };
            let name = format!("fixture-{protocol:?}-{timestamps:?}");
            let (sender, receiver) = mpsc::channel();
            let input = Input::new(options)
                .unwrap()
                .create_virtual(&name, move |packet| {
                    sender.send(packet).unwrap();
                })
                .unwrap();
            let output = Output::new(OutputOptions {
                protocol: options.protocol,
                backend: options.backend,
            })
            .unwrap();
            let port = output
                .ports()
                .unwrap()
                .into_iter()
                .find(|port| port.name() == name)
                .unwrap();
            let output = output.connect(&port).unwrap();
            drop(Input::new(options).unwrap());
            drop(
                Output::new(OutputOptions {
                    protocol: options.protocol,
                    backend: options.backend,
                })
                .unwrap(),
            );
            output.send(&packet).unwrap();
            let received = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
            assert_eq!(received.packet, packet);
            assert_eq!(received.clock, input.clock());
            assert_eq!(
                received.timestamp_ns.is_some(),
                timestamps == TimestampMode::Absolute
            );
            drop(output.close().unwrap());
            let client = input.close().unwrap();
            let reopened = client.create_virtual(&name, |_| {}).unwrap();
            assert_ne!(received.clock, reopened.clock());
            drop(reopened.close().unwrap());
        }
    }
}

#[test]
fn ffi_input_connect_and_virtual_output_use_cloned_source_metadata() {
    enable();
    for protocol in [Protocol::Midi1, Protocol::Ump] {
        let options = InputOptions {
            protocol,
            ..InputOptions::default()
        };
        let name = format!("fixture-source-{protocol:?}");
        let output = Output::new(OutputOptions {
            protocol: options.protocol,
            backend: options.backend,
        })
        .unwrap()
        .create_virtual(&name)
        .unwrap();
        let input = Input::new(options).unwrap();
        let port = input
            .ports()
            .unwrap()
            .into_iter()
            .find(|port| port.name() == name)
            .unwrap();
        let (sender, receiver) = mpsc::channel();
        let input = input
            .connect(&port, move |packet| {
                sender.send(packet).unwrap();
            })
            .unwrap();
        let packet = if protocol == Protocol::Midi1 {
            Packet::Bytes(vec![0x90, 60, 100])
        } else {
            Packet::Ump(vec![0x40903c00, 0xc0000000])
        };
        output.send(&packet).unwrap();
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .packet,
            packet
        );
        drop(input.close().unwrap());
        drop(output.close().unwrap());
    }
}

#[test]
fn failed_input_disposal_retains_callback_context_and_suppresses_delivery() {
    enable();
    let dropped = Arc::new(AtomicBool::new(false));
    struct Probe(Arc<AtomicBool>);
    impl Drop for Probe {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let probe = Probe(Arc::clone(&dropped));
    let (sender, receiver) = mpsc::channel();
    let name = "fixture-retained-input";
    let input = Input::new(InputOptions::default())
        .unwrap()
        .create_virtual(name, move |packet| {
            std::hint::black_box(&probe);
            sender.send(packet).unwrap();
        })
        .unwrap();
    let output = Output::new(OutputOptions::default()).unwrap();
    let port = output
        .ports()
        .unwrap()
        .into_iter()
        .find(|port| port.name() == name)
        .unwrap();
    let endpoint = u32::try_from(port.backend_id()).unwrap();
    fail_disposal(endpoint);
    assert!(input.close().is_err());
    assert!(endpoint_exists(endpoint));
    assert_eq!(emit(endpoint, &[0x90, 60, 100]), 0);
    assert!(receiver.try_recv().is_err());
    assert!(!dropped.load(Ordering::SeqCst));
    drop(output);
}

#[test]
fn ump_send_reports_an_accepted_prefix_and_does_not_retry_after_failure() {
    enable();
    let options = InputOptions {
        protocol: Protocol::Ump,
        ..InputOptions::default()
    };
    let (sender, receiver) = mpsc::channel();
    let name = "fixture-partial-ump-send";
    let input = Input::new(options)
        .unwrap()
        .create_virtual(name, move |packet| {
            sender.send(packet).unwrap();
        })
        .unwrap();
    let output = Output::new(OutputOptions {
        protocol: options.protocol,
        backend: options.backend,
    })
    .unwrap();
    let port = output
        .ports()
        .unwrap()
        .into_iter()
        .find(|port| port.name() == name)
        .unwrap();
    fail_send_after(u32::try_from(port.backend_id()).unwrap(), 1);
    let output = output.connect(&port).unwrap();
    let result = output.send(&Packet::Ump(vec![
        0x40903c00, 0xc0000000, 0x40903d00, 0x80000000,
    ]));
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("after 2 of 4 words"));
    assert_eq!(
        receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .packet,
        Packet::Ump(vec![0x40903c00, 0xc0000000])
    );
    assert!(output
        .send(&Packet::Ump(vec![0x40903d00, 0x80000000]))
        .is_err());
    assert!(receiver.try_recv().is_err());
    assert!(output.close().is_err());
    drop(input.close().unwrap());
}

#[test]
fn failed_output_disposal_retains_the_endpoint() {
    enable();
    let name = "fixture-retained-output";
    let output = Output::new(OutputOptions::default())
        .unwrap()
        .create_virtual(name)
        .unwrap();
    let input = Input::new(InputOptions::default()).unwrap();
    let port = input
        .ports()
        .unwrap()
        .into_iter()
        .find(|port| port.name() == name)
        .unwrap();
    let endpoint = u32::try_from(port.backend_id()).unwrap();
    fail_disposal(endpoint);
    assert!(output.close().is_err());
    assert!(endpoint_exists(endpoint));
}

#[test]
fn refreshed_and_foreign_discovery_snapshots_cannot_open_a_connection() {
    enable();
    let name = "fixture-stale-port";
    let input = Input::new(InputOptions::default())
        .unwrap()
        .create_virtual(name, |_| {})
        .unwrap();
    let output = Output::new(OutputOptions::default()).unwrap();
    let port = output
        .ports()
        .unwrap()
        .into_iter()
        .find(|port| port.name() == name)
        .unwrap();
    output.ports().unwrap();
    assert!(output.connect(&port).is_err());
    let other = Output::new(OutputOptions::default()).unwrap();
    assert!(other.connect(&port).is_err());
    drop(input.close().unwrap());
}
