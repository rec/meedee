#![cfg(target_os = "macos")]

use meedee::transport::{
    Input, InputOptions, Output, OutputOptions, Packet, Protocol, TimestampMode,
};
use std::sync::mpsc;
use std::time::Duration;

// Explicitly opt in: these tests create temporary software endpoints only.
#[test]
#[ignore = "requires authorization to create CoreMIDI virtual ports"]
fn virtual_ports_preserve_byte_and_native_ump_packets() {
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
            let name = format!(
                "meedee-test-{}-{protocol:?}-{timestamps:?}",
                std::process::id()
            );
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
            output.send(&packet).unwrap();
            let received = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
            assert_eq!(received.packet, packet);
            assert_eq!(received.clock, input.clock());
            assert_eq!(
                received.timestamp_ns.is_some(),
                timestamps == TimestampMode::Absolute
            );
            assert!(!input.callback_panicked());
            drop(output.close().unwrap());
            let client = input.close().unwrap();
            let old_clock = received.clock;
            let reopened = client.create_virtual(&name, |_| {}).unwrap();
            assert_ne!(old_clock, reopened.clock());
            drop(reopened.close().unwrap());
        }
    }
}

#[test]
#[ignore = "requires authorization to create CoreMIDI virtual ports"]
fn refreshing_ports_rejects_old_discovery_snapshots() {
    let name = format!("meedee-stale-test-{}", std::process::id());
    let input = Input::new(InputOptions::default())
        .unwrap()
        .create_virtual(&name, |_| {})
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
    drop(input.close().unwrap());
}
