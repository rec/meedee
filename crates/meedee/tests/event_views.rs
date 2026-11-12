use meedee::messages::Message;
use meedee::{
    events::{Clock, Payload, Timestamp},
    events::{ObservationValue as Value, View},
    events::{StreamDiagnostic as Diagnostic, StreamDiagnosticKind as DiagnosticKind},
    messages::{AnyMessage, ControlChange, NoteOn},
    ump,
};
fn event(message: AnyMessage, ticks: i64) -> Payload {
    (Payload::Midi1(message))
        .with_metadata(meedee::messages::Metadata {
            stream: Some("s".into()),
            timestamp: Some(Timestamp {
                ticks,
                clock: Clock::new("c".into(), 1, 1_000_000_000, "test".into(), None).unwrap(),
            }),
            span: None,
        })
        .unwrap()
}
#[test]
fn anonymous_controls_preserve_timing_and_require_reset_before_source_changes() {
    use meedee::controls::{ControlInterpreter, DiagnosticKind as ControlKind, Observation};
    let mut interpreter = ControlInterpreter::new(None).unwrap();
    let cc = |controller| {
        Payload::Midi1(AnyMessage::ControlChange(
            ControlChange::new(controller, 1)
                .unwrap()
                .with_channel(1)
                .unwrap(),
        ))
    };
    interpreter.feed(&cc(7)).unwrap();
    assert!(interpreter
        .feed(&cc(39).with_stream("keyboard").unwrap())
        .is_err());
    let result = interpreter.feed(&cc(39)).unwrap();
    assert_eq!(result[0].stream, None);
    assert!(
        matches!(&result[0].value, Value::Control(Observation::Control(v)) if v.resolution == 14)
    );
    interpreter.reset();
    let timestamp = |identity: &str| Timestamp {
        ticks: 10,
        clock: Clock::new(identity.into(), 1, 1_000_000_000, "test".into(), None).unwrap(),
    };
    interpreter
        .feed(&cc(7).with_timestamp(timestamp("a")).unwrap())
        .unwrap();
    let result = interpreter
        .feed(&cc(39).with_timestamp(timestamp("b")).unwrap())
        .unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].stream, None);
    assert_eq!(result[0].last_timestamp, Some(timestamp("b")));
    assert!(
        matches!(&result[0].value, Value::Control(Observation::Diagnostic(v)) if v.kind == ControlKind::StateReset)
    );
    assert!(
        matches!(&result[1].value, Value::Control(Observation::Diagnostic(v)) if v.kind == ControlKind::MissingCoarse)
    );
    interpreter.reset();
    interpreter
        .feed(&cc(7).with_stream("keyboard").unwrap())
        .unwrap();
    assert!(interpreter.feed(&cc(39)).is_err());
    let loss = Diagnostic {
        stream: "keyboard".into(),
        packet: 1,
        kind: DiagnosticKind::Loss,
        lost: 2,
        reason: "lost".into(),
    };
    assert_eq!(
        interpreter.feed_diagnostic(&loss).unwrap()[0].value,
        Value::Boundary(loss)
    );
}

#[test]
fn inactive_parameter_selectors_retain_their_contributor_times() {
    use meedee::controls::{ControlInterpreter, Observation, ParameterFamily};
    let mut interpreter = ControlInterpreter::new(None).unwrap();
    let mut result = Vec::new();
    for (index, (controller, value)) in [(99, 1), (98, 2), (101, 0), (100, 0), (98, 3), (6, 30)]
        .into_iter()
        .enumerate()
    {
        result = interpreter
            .feed(&event(
                AnyMessage::ControlChange(
                    ControlChange::new(controller, value)
                        .unwrap()
                        .with_channel(1)
                        .unwrap(),
                ),
                index as i64 * 10,
            ))
            .unwrap();
    }
    assert!(
        matches!(&result[0].value, Value::Control(Observation::Parameter(v)) if v.family == ParameterFamily::Nrpn && v.parameter == 131)
    );
    assert_eq!(
        (result[0].first_index, result[0].last_index),
        (Some(0), Some(5))
    );
    assert_eq!(result[0].first_timestamp.as_ref().unwrap().ticks, 0);
    assert_eq!(result[0].last_timestamp.as_ref().unwrap().ticks, 50);
    for (index, (controller, value)) in [
        (98, 4),
        (6, 40),
        (101, 0),
        (100, 0),
        (99, 1),
        (98, 3),
        (38, 3),
    ]
    .into_iter()
    .enumerate()
    {
        result = interpreter
            .feed(&event(
                AnyMessage::ControlChange(
                    ControlChange::new(controller, value)
                        .unwrap()
                        .with_channel(1)
                        .unwrap(),
                ),
                (index as i64 + 6) * 10,
            ))
            .unwrap();
    }
    assert!(
        matches!(&result[0].value, Value::Control(Observation::Parameter(v)) if v.value == 3843)
    );
    assert_eq!(
        (result[0].first_index, result[0].last_index),
        (Some(5), Some(12))
    );
    assert_eq!(result[0].first_timestamp.as_ref().unwrap().ticks, 50);
    interpreter
        .feed_diagnostic(&Diagnostic {
            stream: "s".into(),
            packet: 1,
            kind: DiagnosticKind::Loss,
            lost: 1,
            reason: "known loss".into(),
        })
        .unwrap();
    assert_eq!(
        interpreter
            .parameter_state(1, ParameterFamily::Nrpn, 131)
            .unwrap(),
        None
    );
    assert_eq!(
        interpreter
            .parameter_state(1, ParameterFamily::Nrpn, 132)
            .unwrap(),
        None
    );
    for (controller, value) in [(99, 1), (98, 3), (101, 0), (100, 0)] {
        interpreter
            .feed(&event(
                AnyMessage::ControlChange(
                    ControlChange::new(controller, value)
                        .unwrap()
                        .with_channel(1)
                        .unwrap(),
                ),
                110,
            ))
            .unwrap();
        if controller == 98 || controller == 100 {
            let missing = interpreter
                .feed(&event(
                    AnyMessage::ControlChange(
                        ControlChange::new(38, 3).unwrap().with_channel(1).unwrap(),
                    ),
                    120,
                ))
                .unwrap();
            assert!(
                matches!(&missing[0].value, Value::Control(Observation::Diagnostic(d)) if d.kind == meedee::controls::DiagnosticKind::MissingCoarse)
            );
        }
    }
}

#[test]
fn ump_finish_retains_group_and_last_contributing_timestamp() {
    let mut assembler = ump::Assembler::new(1024, 2048, 4).unwrap();
    let packets = ump::packetize_sysex7(3, &(0..20).collect::<Vec<u8>>()).unwrap();
    let packet = |index: usize, ticks| {
        Payload::Ump(ump::UmpMessage::SysEx7(packets[index].clone()))
            .with_timestamp(Timestamp {
                ticks,
                clock: Clock::new("capture".into(), 1, 1_000_000_000, "test".into(), None).unwrap(),
            })
            .unwrap()
    };
    assembler.feed(&packet(0, 10)).unwrap();
    assembler
        .feed(&Payload::Ump(ump::UmpMessage::Noop(
            ump::Noop::new().unwrap(),
        )))
        .unwrap();
    assembler.feed(&packet(1, 20)).unwrap();
    let out = assembler.finish();
    assert_eq!(out.len(), 1);
    assert_eq!(
        (out[0].group, out[0].first_index, out[0].last_index),
        (Some(3), Some(0), Some(2))
    );
    assert_eq!(out[0].first_timestamp.as_ref().unwrap().ticks, 10);
    assert_eq!(out[0].last_timestamp.as_ref().unwrap().ticks, 20);
    assert_eq!(
        out[0].value,
        Value::Ump(ump::AssemblyItem::Diagnostic(
            ump::AssemblyDiagnostic::Truncated
        ))
    );
    assert_eq!(
        (assembler.active_streams(), assembler.retained_bytes()),
        (0, 0)
    );
    assert!(assembler.finish().is_empty());
}
#[test]
fn timed_controls_retain_only_active_contributors_and_reset_on_loss() {
    let mut view = meedee::controls::ControlInterpreter::new(None).unwrap();
    view.feed(&event(
        AnyMessage::ControlChange(
            ControlChange::new(7, 64)
                .and_then(|m| m.with_channel(1))
                .unwrap(),
        ),
        10,
    ))
    .unwrap();
    for i in 0..2000 {
        view.feed(&event(
            AnyMessage::NoteOn(
                NoteOn::new(60)
                    .and_then(|m| m.with_velocity(100))
                    .and_then(|m| m.with_channel(1))
                    .unwrap(),
            ),
            i + 11,
        ))
        .unwrap();
    }
    let out = view
        .feed(&event(
            AnyMessage::ControlChange(
                ControlChange::new(39, 1)
                    .and_then(|m| m.with_channel(1))
                    .unwrap(),
            ),
            3000,
        ))
        .unwrap();
    assert_eq!(out[0].first_index, Some(0));
    assert_eq!(out[0].first_timestamp.as_ref().unwrap().ticks, 10);
    assert_eq!(out[0].last_timestamp.as_ref().unwrap().ticks, 3000);
    assert!(
        matches!(&out[0].value,Value::Control(meedee::controls::Observation::Control(v))if v.value==8193)
    );
    view.boundary(&Diagnostic {
        stream: "s".into(),
        packet: 2002,
        kind: DiagnosticKind::Loss,
        lost: 1,
        reason: "loss".into(),
    })
    .unwrap();
    let out = view
        .feed(&event(
            AnyMessage::ControlChange(
                ControlChange::new(39, 2)
                    .and_then(|m| m.with_channel(1))
                    .unwrap(),
            ),
            4000,
        ))
        .unwrap();
    assert!(matches!(
        &out[0].value,
        Value::Control(meedee::controls::Observation::Diagnostic(_))
    ));
    let mut wrong = event(
        AnyMessage::NoteOn(
            NoteOn::new(60)
                .and_then(|m| m.with_velocity(100))
                .and_then(|m| m.with_channel(1))
                .unwrap(),
        ),
        5000,
    );
    wrong = wrong.with_stream("another").unwrap();
    assert!(view.feed(&wrong).is_err());
}
#[test]
fn velocity_and_ump_assembly_keep_exact_event_ranges() {
    let mut v = meedee::controls::VelocityInterpreter::new(None).unwrap();
    v.feed(&event(
        AnyMessage::ControlChange(
            ControlChange::new(88, 127)
                .and_then(|m| m.with_channel(1))
                .unwrap(),
        ),
        1,
    ))
    .unwrap();
    let out = v
        .feed(&event(
            AnyMessage::NoteOn(
                NoteOn::new(60)
                    .and_then(|m| m.with_velocity(127))
                    .and_then(|m| m.with_channel(1))
                    .unwrap(),
            ),
            2,
        ))
        .unwrap();
    assert_eq!((out[0].first_index, out[0].last_index), (Some(0), Some(1)));
    assert!(matches!(&out[0].value,Value::Velocity(v)if v.velocity==16383));
    let mut assembler = meedee::ump::Assembler::new(1024, 8192, 64).unwrap();
    let packets = ump::packetize_sysex7(2, &[125, 1, 2, 3, 4, 5, 6, 7]).unwrap();
    let mut a = event(
        AnyMessage::NoteOn(
            NoteOn::new(60)
                .and_then(|m| m.with_velocity(100))
                .and_then(|m| m.with_channel(1))
                .unwrap(),
        ),
        10,
    );
    a = Payload::Ump(ump::UmpMessage::SysEx7(packets[0].clone()))
        .with_metadata(a.metadata().unwrap().clone())
        .unwrap();
    assert!(assembler.feed(&a).unwrap().is_empty());
    a = a
        .clone()
        .with_timestamp(Timestamp {
            ticks: 20,
            ..a.timestamp().unwrap().clone()
        })
        .unwrap();
    a = Payload::Ump(ump::UmpMessage::SysEx7(packets[1].clone()))
        .with_metadata(a.metadata().unwrap().clone())
        .unwrap();
    let out = assembler.feed(&a).unwrap();
    assert_eq!(
        (out[0].group, out[0].first_index, out[0].last_index),
        (Some(2), Some(0), Some(1))
    );
    assert_eq!(out[0].first_timestamp.as_ref().unwrap().ticks, 10);
    assert_eq!(out[0].last_timestamp.as_ref().unwrap().ticks, 20);
}
