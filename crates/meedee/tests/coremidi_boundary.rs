#![cfg(target_os = "macos")]

use meedee::transport::{
    Error, Input, InputOptions, Output, OutputOptions, Packet, Protocol, TimestampMode,
};
use meedee_coremidi_fixture::{
    emit, enable, endpoint_exists, fail_disposal, fail_query, fail_send_after,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::Duration;

#[test]
fn unopened_diagnostics_survive_close_without_opening_native_clients() {
    use meedee::midi;
    let mut input = midi::Input::new();
    let mut output = midi::Output::new();
    assert!(!input.closed() && !output.closed());
    assert_eq!(input.dropped_packets(), 0);
    assert!(input.failure().is_none());
    input.close().unwrap();
    output.close().unwrap();
    assert!(input.closed() && output.closed());
    assert!(input.stream().is_err() && input.clock().is_err());
    assert!(input.failure().is_none());
    assert!(input.check_health().is_err());
    assert_eq!(input.dropped_packets(), 0);
}

#[test]
fn high_level_raw_input_preserves_fragments_and_reports_queue_loss() {
    use meedee::{events::InputItem, midi};
    enable();
    for protocol in [Protocol::Midi1, Protocol::Ump] {
        let name = format!("raw-input-{protocol:?}");
        let mut input = midi::Input::new()
            .raw()
            .unwrap()
            .with_transport(InputOptions {
                protocol,
                ..InputOptions::default()
            })
            .unwrap()
            .with_receive_limits(1, 16, 1024)
            .unwrap()
            .create_virtual(&name)
            .unwrap();
        let output = Output::new(OutputOptions {
            protocol,
            ..OutputOptions::default()
        })
        .unwrap();
        let port = output
            .ports()
            .unwrap()
            .into_iter()
            .find(|p| p.name() == name)
            .unwrap();
        let output = output.connect(&port).unwrap();
        let packet = if protocol == Protocol::Midi1 {
            Packet::Bytes(vec![0x90, 60])
        } else {
            Packet::Ump(vec![0x40903c00, 0xc0000000])
        };
        output.send(&packet).unwrap();
        output.send(&packet).unwrap();
        let InputItem::Message(first) = input
            .receive(Some(Duration::from_secs(1)), None)
            .unwrap()
            .unwrap()
        else {
            panic!("packet expected")
        };
        assert_eq!(first.packet, packet);
        assert_eq!(first.sequence, 0);
        assert!(first.timestamp_ns.is_some());
        let InputItem::Diagnostic(loss) = input
            .receive(Some(Duration::from_secs(1)), None)
            .unwrap()
            .unwrap()
        else {
            panic!("loss expected")
        };
        assert_eq!(loss.lost, 1);
        output.send(&packet).unwrap();
        let InputItem::Message(next) = input
            .receive(Some(Duration::from_secs(1)), None)
            .unwrap()
            .unwrap()
        else {
            panic!("packet expected")
        };
        assert_eq!(next.sequence, 2);
        assert_eq!(next.clock, first.clock);
        input.close().unwrap();
        assert!(matches!(
            input.receive(Some(Duration::ZERO), None),
            Err(midi::Error::Closed)
        ));
        assert_eq!(input.dropped_packets(), 1);
        drop(output.close().unwrap());
    }
    assert!(midi::Input::new().with_channel(1).unwrap().raw().is_err());
}

#[test]
fn high_level_callbacks_are_serial_guard_blocking_io_and_drain_on_close() {
    use meedee::{events::InputItem, midi};
    enable();
    let (sender, receiver) = mpsc::channel();
    let mut count = 0;
    let mut input = midi::Input::new()
        .with_callback(move |item| {
            let InputItem::Message(_) = item.unwrap() else {
                return;
            };
            count += 1;
            let error = midi::Output::new()
                .send(meedee::messages::NoteOn::new(60).unwrap())
                .unwrap_err();
            assert!(matches!(
                error.cause,
                meedee::events::PlaybackCause::Send(midi::Error::Transport(Error::CallbackThread))
            ));
            sender.send((count, std::thread::current().id())).unwrap();
        })
        .unwrap()
        .create_virtual("high-level-callback")
        .unwrap();
    assert!(input.receive(Some(Duration::ZERO), None).is_err());
    assert!(input.with_errors().next().unwrap().is_err());
    let output = Output::new(OutputOptions::default()).unwrap();
    let port = output
        .ports()
        .unwrap()
        .into_iter()
        .find(|p| p.name() == "high-level-callback")
        .unwrap();
    let output = output.connect(&port).unwrap();
    output.send(&Packet::Bytes(vec![0x90, 60, 90])).unwrap();
    output.send(&Packet::Bytes(vec![0x90, 61, 90])).unwrap();
    input.close().unwrap();
    let first = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
    let second = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!((first.0, second.0), (1, 2));
    assert_eq!(first.1, second.1);
    assert_ne!(first.1, std::thread::current().id());
    input.close().unwrap();
    drop(output.close().unwrap());
}

#[test]
fn attached_recording_with_callbacks_retains_activity_and_marks_panics_incomplete() {
    use meedee::{archive, events::InputItem, midi};
    enable();
    for panic in [false, true] {
        let name = format!("callback-recording-{panic}");
        let path = std::env::temp_dir().join(format!(
            "meedee-callback-recording-{}-{panic}-{}.meedee",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let (sent, received) = mpsc::channel();
        let mut input = midi::Input::new()
            .with_callback(move |item| {
                let item = item.unwrap();
                let message = matches!(item, InputItem::Message(_));
                sent.send(item).unwrap();
                assert!(!(panic && message), "intentional recording callback panic");
            })
            .unwrap()
            .create_virtual(&name)
            .unwrap();
        input
            .start(Some(&path), &midi::RecordingOptions::default())
            .unwrap();
        let output = Output::new(OutputOptions::default()).unwrap();
        let port = output
            .ports()
            .unwrap()
            .into_iter()
            .find(|p| p.name() == name)
            .unwrap();
        let output = output.connect(&port).unwrap();
        let start = std::time::Instant::now();
        output.send(&Packet::Bytes(vec![0x90, 60, 90])).unwrap();
        received.recv_timeout(Duration::from_secs(2)).unwrap();
        if panic {
            let deadline = start + Duration::from_secs(2);
            while !input.closed() {
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert!(input.close().is_err());
        } else {
            input.close().unwrap();
        }
        assert_eq!(input.recording_report().unwrap().complete, !panic);
        let activity = input.last_message_time().unwrap();
        assert!(activity >= start);
        assert_eq!(input.last_message_time(), Some(activity));
        let records =
            archive::Reader::new(std::fs::File::open(&path).unwrap(), 1_048_576, true, false)
                .unwrap()
                .collect::<std::io::Result<Vec<_>>>()
                .unwrap();
        assert_eq!(
            records
                .iter()
                .filter(|r| matches!(r, archive::Record::Event(_)))
                .count(),
            1
        );
        input.close().unwrap();
        drop(output.close().unwrap());
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn high_level_raw_callback_panics_stop_delivery_and_remain_observable() {
    use meedee::{events::InputItem, midi};
    enable();
    let (sender, receiver) = mpsc::channel();
    let mut input = midi::Input::new()
        .raw()
        .unwrap()
        .with_callback(move |item| {
            let InputItem::Message(packet) = item.unwrap() else {
                panic!("packet expected")
            };
            sender.send(packet.packet).unwrap();
            panic!("intentional callback panic");
        })
        .unwrap()
        .create_virtual("high-level-callback-panic")
        .unwrap();
    let output = Output::new(OutputOptions::default()).unwrap();
    let port = output
        .ports()
        .unwrap()
        .into_iter()
        .find(|p| p.name() == "high-level-callback-panic")
        .unwrap();
    let output = output.connect(&port).unwrap();
    let packet = Packet::Bytes(vec![0x90, 60]);
    output.send(&packet).unwrap();
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
        packet
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !input.closed() && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(input.closed());
    let failure = input.failure().unwrap();
    input.close().unwrap();
    assert!(Arc::ptr_eq(&failure, &input.failure().unwrap()));
    assert!(input
        .check_health()
        .unwrap_err()
        .to_string()
        .contains("callback panicked"));
    assert!(receiver.try_recv().is_err());
    drop(output.close().unwrap());
}

#[test]
fn high_level_callbacks_report_native_loss_during_silence_and_close_failure() {
    use meedee::{events::InputItem, midi};
    enable();
    let (sender, receiver) = mpsc::channel();
    let mut input = midi::Input::new()
        .raw()
        .unwrap()
        .with_transport(InputOptions {
            receive_byte_limit: 1,
            ..InputOptions::default()
        })
        .unwrap()
        .with_callback(move |item| {
            sender.send(item).unwrap();
        })
        .unwrap()
        .create_virtual("callback-native-loss")
        .unwrap();
    let output = Output::new(OutputOptions::default()).unwrap();
    let port = output
        .ports()
        .unwrap()
        .into_iter()
        .find(|p| p.name() == "callback-native-loss")
        .unwrap();
    let endpoint = u32::try_from(port.backend_id()).unwrap();
    let output = output.connect(&port).unwrap();
    output.send(&Packet::Bytes(vec![0x90, 60, 90])).unwrap();
    let InputItem::Diagnostic(loss) = receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap()
    else {
        panic!("loss expected")
    };
    assert_eq!(loss.lost, 1);
    fail_disposal(endpoint);
    let close_error = input.close().unwrap_err();
    assert!(input.closed());
    let delivered = receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap_err();
    let failure = input.failure().unwrap();
    assert!(matches!(
        failure.as_ref(),
        midi::Error::Transport(Error::Native(_))
    ));
    let midi::Error::Shared(delivered) = delivered else {
        panic!("shared error expected")
    };
    let midi::Error::Shared(closed) = close_error else {
        panic!("shared error expected")
    };
    assert!(Arc::ptr_eq(&failure, &delivered));
    assert!(Arc::ptr_eq(&failure, &closed));
    assert!(input.check_health().is_err());
    assert_eq!(input.dropped_packets(), 1);
    input.close().unwrap();
    drop(output);
}

#[test]
fn high_level_virtual_ports_preserve_messages_and_terminal_close() {
    use meedee::{events::InputItem, messages::Message, midi};
    enable();
    for protocol in [Protocol::Midi1, Protocol::Ump] {
        for virtual_input in [true, false] {
            let name = format!("high-level-{protocol:?}-{virtual_input}");
            let mut input = midi::Input::new()
                .with_transport(InputOptions {
                    protocol,
                    ..InputOptions::default()
                })
                .unwrap();
            let mut output = midi::Output::new()
                .with_transport(OutputOptions {
                    protocol,
                    ..OutputOptions::default()
                })
                .unwrap();
            if virtual_input {
                input = input.create_virtual(&name).unwrap();
                let port = output
                    .ports()
                    .unwrap()
                    .into_iter()
                    .find(|p| p.name() == name)
                    .unwrap();
                output = output.connect(&port).unwrap();
            } else {
                output = output.create_virtual(&name).unwrap();
                let port = input
                    .ports()
                    .unwrap()
                    .into_iter()
                    .find(|p| p.name() == name)
                    .unwrap();
                input = input.connect(&port).unwrap();
            }
            assert!(!input.closed() && !output.closed());
            let stream = input.stream().unwrap().to_owned();
            let clock = input.clock().unwrap().clone();
            let packet = match protocol {
                Protocol::Midi1 => Packet::Bytes(vec![0x90, 60, 100]),
                Protocol::Ump => Packet::Ump(vec![0x40903c00, 0xc0000000]),
            };
            output.send(packet.clone()).unwrap();
            let received = input
                .receive(Some(Duration::from_secs(2)), None)
                .unwrap()
                .unwrap();
            let InputItem::Message(message) = received else {
                panic!("expected decoded message")
            };
            assert!(message.timestamp().is_some());
            assert_eq!(message.stream(), Some(input.stream().unwrap()));
            match message.as_ref() {
                meedee::events::Payload::Midi1(meedee::messages::AnyMessage::NoteOn(v)) => {
                    assert_eq!(v.velocity(), 100)
                }
                meedee::events::Payload::Ump(meedee::ump::UmpMessage::NoteOn(v)) => {
                    assert_eq!(v.velocity(), 0xc000)
                }
                _ => panic!("unexpected decoded message"),
            }
            output.close().unwrap();
            output.close().unwrap();
            input.close().unwrap();
            input.close().unwrap();
            assert!(input.closed() && output.closed());
            assert_eq!(input.stream().unwrap(), stream);
            assert_eq!(input.clock().unwrap(), &clock);
            assert!(input.failure().is_none());
            assert!(input.check_health().is_err());
            assert!(input.create_virtual(&name).is_err());
            assert!(output.send(packet).is_err());
            assert!(output.create_virtual(&name).is_err());
        }
    }
}

#[test]
fn failed_discovery_returns_no_snapshot_and_allows_explicit_rediscovery() {
    enable();
    for protocol in [Protocol::Midi1, Protocol::Ump] {
        let name = format!("discovery-recovery-{protocol:?}");
        let input = Input::new(InputOptions {
            protocol,
            ..InputOptions::default()
        })
        .unwrap()
        .create_virtual(&name, |_| {})
        .unwrap();
        let output = Output::new(OutputOptions {
            protocol,
            ..OutputOptions::default()
        })
        .unwrap();
        let port = output
            .ports()
            .unwrap()
            .into_iter()
            .find(|p| p.name() == name)
            .unwrap();
        fail_query(port.backend_id() as u32);
        assert!(matches!(output.ports(), Err(Error::Native(_))));
        let port = output
            .ports()
            .unwrap()
            .into_iter()
            .find(|p| p.name() == name)
            .unwrap();
        let connection = output.connect(&port).unwrap();
        drop(connection.close().unwrap());
        drop(input.close().unwrap());
    }
}

#[test]
fn compound_send_stops_at_each_failed_boundary_and_reports_known_progress() {
    enable();
    for command in [
        meedee::messages::AnyCommand::NrpnSet(
            meedee::messages::NrpnSet::new(1234, 10000)
                .and_then(|m| m.with_resolution(14))
                .and_then(|m| m.with_channel(1))
                .unwrap(),
        ),
        meedee::messages::AnyCommand::BankProgramSelect(
            meedee::gm::BankProgramSelect::new(128)
                .and_then(|m| m.with_bank_msb(121))
                .and_then(|m| m.with_bank_lsb(3))
                .and_then(|m| m.with_channel(1))
                .unwrap(),
        ),
    ] {
        let expected = meedee::codecs::expand(&command).unwrap();
        let total = expected.len();
        for accepted in 0..expected.len() {
            let name = format!("compound-failure-{accepted}");
            let (sender, receiver) = mpsc::channel();
            let input = Input::new(InputOptions::default())
                .unwrap()
                .create_virtual(&name, move |packet| {
                    sender.send(packet.packet).unwrap();
                })
                .unwrap();
            let output = Output::new(OutputOptions::default()).unwrap();
            let port = output
                .ports()
                .unwrap()
                .into_iter()
                .find(|p| p.name() == name)
                .unwrap();
            fail_send_after(u32::try_from(port.backend_id()).unwrap(), accepted as i32);
            let output = output.connect(&port).unwrap();
            assert!(
                matches!(output.send_command(&command), Err(Error::CompoundSend { completed_messages: Some(n), total_messages, .. }) if n == accepted && total_messages == total)
            );
            let actual: Vec<_> = receiver.try_iter().collect();
            let expected: Vec<_> = expected[..accepted]
                .iter()
                .map(|m| Packet::Bytes(meedee::codecs::encode(m).unwrap()))
                .collect();
            assert_eq!(actual, expected);
            drop(output);
            drop(input.close().unwrap());
        }
    }
}

#[test]
fn simultaneous_raw_and_compound_sends_do_not_interleave_command_messages() {
    use std::sync::{Barrier, Mutex};
    enable();
    let received = Arc::new(Mutex::new(Vec::new()));
    let capture = Arc::clone(&received);
    let name = "compound-serialization";
    let input = Input::new(InputOptions::default())
        .unwrap()
        .create_virtual(name, move |packet| {
            capture.lock().unwrap().push(packet.packet);
        })
        .unwrap();
    let output = Output::new(OutputOptions::default()).unwrap();
    let port = output
        .ports()
        .unwrap()
        .into_iter()
        .find(|p| p.name() == name)
        .unwrap();
    let output = output.connect(&port).unwrap();
    let barrier = Barrier::new(2);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            barrier.wait();
            for _ in 0..50 {
                loop {
                    match output.send_command(
                        &meedee::messages::RpnSet::new(0, 128)
                            .and_then(|m| m.with_resolution(14))
                            .and_then(|m| m.with_channel(1))
                            .unwrap(),
                    ) {
                        Ok(()) => break,
                        Err(Error::CompoundSend {
                            completed_messages: Some(0),
                            cause,
                            ..
                        }) if *cause == Error::Busy => std::thread::yield_now(),
                        result => panic!("unexpected fixture send result {result:?}"),
                    }
                }
            }
        });
        scope.spawn(|| {
            barrier.wait();
            for _ in 0..50 {
                loop {
                    match output.send(&Packet::Bytes(vec![0xf8])) {
                        Ok(()) => break,
                        Err(Error::Busy) => std::thread::yield_now(),
                        result => panic!("unexpected fixture send result {result:?}"),
                    }
                }
            }
        });
    });
    let expected: Vec<_> = meedee::codecs::expand(
        &meedee::messages::RpnSet::new(0, 128)
            .and_then(|m| m.with_resolution(14))
            .and_then(|m| m.with_channel(1))
            .unwrap(),
    )
    .unwrap()
    .iter()
    .map(|m| Packet::Bytes(meedee::codecs::encode(m).unwrap()))
    .collect();
    let mut received = received.lock().unwrap().clone().into_iter().peekable();
    let mut commands = 0;
    let mut clocks = 0;
    while let Some(packet) = received.next() {
        if packet == Packet::Bytes(vec![0xf8]) {
            clocks += 1;
        } else {
            assert_eq!(packet, expected[0]);
            for packet in &expected[1..] {
                assert_eq!(received.next().as_ref(), Some(packet));
            }
            commands += 1;
        }
    }
    assert_eq!((commands, clocks), (50, 50));
    drop(output.close().unwrap());
    drop(input.close().unwrap());
}

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
