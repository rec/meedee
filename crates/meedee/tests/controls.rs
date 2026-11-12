use meedee::codecs::{encode, expand};
use meedee::controls::{
    ControlInterpreter, DiagnosticKind, Observation, ParameterFamily, StepDirection,
};
use meedee::messages::*;

#[test]
fn controller_resolution_preserves_wire_values_and_rejects_incompatible_formats() {
    let coarse = ControlChange::new(1, 90).unwrap().with_channel(2).unwrap();
    let fine = ControlChange::new_with_resolution(1, 90, 14)
        .unwrap()
        .with_channel(2)
        .unwrap();
    let native = ControlChange::new_midi2(1, 90)
        .unwrap()
        .with_channel(2)
        .unwrap();
    assert_eq!(
        (coarse.resolution(), fine.resolution(), native.resolution()),
        (7, 14, 32)
    );
    assert_ne!(coarse, fine);
    assert_eq!(fine.clone().with_resolution(7).unwrap(), coarse);
    assert!(encode(&fine).is_err());
    let pair = expand(&fine).unwrap();
    assert_eq!(
        pair,
        vec![
            AnyMessage::ControlChange(ControlChange::new(1, 0).unwrap().with_channel(2).unwrap()),
            AnyMessage::ControlChange(ControlChange::new(33, 90).unwrap().with_channel(2).unwrap())
        ]
    );
    assert!(native.clone().with_resolution(14).is_err());
    assert!(expand(&native).is_err());
    assert!(fine.clone().with_controller(32).is_err());
    assert!(fine.clone().with_value(16384).is_err());
    assert!(fine.with_value(128).unwrap().with_resolution(7).is_err());
    assert!(ControlChange::new_with_resolution(1, 128, 14)
        .unwrap()
        .expand()
        .is_err());
    assert!(ControlChange::new_with_resolution(1, 0, 32).is_err());
}

#[test]
fn commands_expand_to_independent_wire_vectors() {
    let rpn = ParameterSet::new(1234, 90).unwrap();
    assert_eq!(rpn.family(), ParameterFamily::Rpn);
    assert_eq!(rpn.resolution(), 14);
    assert_eq!(rpn.channel(), None);
    let nrpn = rpn.clone().with_family(ParameterFamily::Nrpn).unwrap();
    assert_ne!(rpn, nrpn);
    let boundary = nrpn.with_parameter(16383).unwrap();
    assert!(boundary.clone().with_family(ParameterFamily::Rpn).is_err());
    assert!(boundary
        .with_parameter(0)
        .unwrap()
        .with_family(ParameterFamily::Rpn)
        .is_ok());
    for line in include_str!("fixtures/commands.txt").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let (fields, wire) = line.split_once('|').unwrap();
        let mut fields = fields.split_whitespace();
        let name = fields.next().unwrap();
        let fields: Vec<_> = fields.collect();
        let v: Vec<i64> = fields
            .iter()
            .take(if name == "ParameterStep" {
                3
            } else if name == "ParameterSet" {
                4
            } else {
                fields.len()
            })
            .map(|v| v.parse().unwrap())
            .collect();
        let command = match name {
            "ControlChange" => AnyCommand::ControlChange(
                ControlChange::new_with_resolution(v[1], v[2], 14)
                    .and_then(|m| m.with_channel(v[0]))
                    .unwrap(),
            ),
            "ParameterSet" => AnyCommand::ParameterSet(
                ParameterSet::new(0, v[2])
                    .and_then(|m| {
                        m.with_family(if fields[4] == "rpn" {
                            ParameterFamily::Rpn
                        } else {
                            ParameterFamily::Nrpn
                        })
                    })
                    .and_then(|m| m.with_parameter(v[1]))
                    .and_then(|m| m.with_resolution(v[3]))
                    .and_then(|m| m.with_channel(v[0]))
                    .unwrap(),
            ),
            "ParameterStep" => AnyCommand::ParameterStep(
                ParameterStep::new(0)
                    .and_then(|m| {
                        m.with_family(if fields[3] == "rpn" {
                            ParameterFamily::Rpn
                        } else {
                            ParameterFamily::Nrpn
                        })
                    })
                    .and_then(|m| m.with_parameter(v[1]))
                    .map(|m| {
                        m.with_direction(if v[2] == 1 {
                            StepDirection::Increment
                        } else {
                            StepDirection::Decrement
                        })
                    })
                    .and_then(|m| m.with_channel(v[0]))
                    .unwrap(),
            ),
            _ => panic!("unknown fixture command {name}"),
        };
        let expected: Vec<u8> = wire
            .split_whitespace()
            .map(|v| u8::from_str_radix(v, 16).unwrap())
            .collect();
        assert_eq!(
            expand(&command)
                .unwrap()
                .iter()
                .flat_map(|m| encode(m).unwrap())
                .collect::<Vec<_>>(),
            expected,
            "{line}"
        );
    }
    assert!(ParameterSet::new(16383, 0)
        .and_then(|m| m.with_resolution(7))
        .and_then(|m| m.with_channel(1))
        .is_err());
    assert!(ParameterSet::new(0, 127)
        .and_then(|m| m.with_family(ParameterFamily::Nrpn))
        .and_then(|m| m.with_parameter(16383))
        .and_then(|m| m.with_resolution(7))
        .and_then(|m| m.with_channel(1))
        .is_ok());
    for resolution in [0, 8, 13, 15] {
        assert!(ParameterSet::new(0, 0)
            .and_then(|m| m.with_resolution(resolution))
            .and_then(|m| m.with_channel(1))
            .is_err());
    }
    assert!(ParameterSet::new(0, 128)
        .and_then(|m| m.with_family(meedee::controls::ParameterFamily::Nrpn))
        .and_then(|m| m.with_resolution(7))
        .and_then(|m| m.with_channel(1))
        .is_err());
    assert!(ControlChange::new_with_resolution(0, 0, 14)
        .and_then(|m| m.with_channel(0))
        .is_err());
    assert!(ControlChange::new_with_resolution(32, 0, 14)
        .and_then(|m| m.with_channel(1))
        .is_err());
    assert!(ControlChange::new_with_resolution(0, 16384, 14)
        .and_then(|m| m.with_channel(1))
        .is_err());
}

#[test]
fn fine_controls_follow_wire_order_and_keep_only_the_known_coarse_component() {
    let mut interpreter = ControlInterpreter::new(None).unwrap();
    assert!(
        matches!(&cc(&mut interpreter, 1, 39, 7)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
    );
    assert!(
        matches!(&cc(&mut interpreter, 1, 7, 2)[0], Observation::Control(v) if v.value == 2 && v.resolution == 7 && v.first_index == 1)
    );
    cc(&mut interpreter, 2, 7, 9);
    interpreter
        .feed(&meedee::events::Payload::Midi1(AnyMessage::TimingClock(
            TimingClock::new().unwrap(),
        )))
        .unwrap();
    assert!(
        matches!(&cc(&mut interpreter, 1, 39, 1)[0], Observation::Control(v) if v.value == 257 && v.resolution == 14 && v.first_index == 1 && v.last_index == 4)
    );
    assert!(
        matches!(&cc(&mut interpreter, 1, 39, 2)[0], Observation::Control(v) if v.value == 258)
    );
    cc(&mut interpreter, 1, 7, 3);
    assert!(
        matches!(&cc(&mut interpreter, 1, 39, 4)[0], Observation::Control(v) if v.value == 388 && v.first_index == 6)
    );
}

#[test]
fn selector_edits_retain_the_other_half_but_not_data() {
    for (family, msb, lsb, other_msb, other_lsb) in [
        (ParameterFamily::Nrpn, 99, 98, 101, 100),
        (ParameterFamily::Rpn, 101, 100, 99, 98),
    ] {
        let mut interpreter = ControlInterpreter::new(None).unwrap();
        cc(&mut interpreter, 1, msb, 1);
        assert!(
            matches!(&cc(&mut interpreter, 1, 6, 2)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::IncompleteSelection)
        );
        cc(&mut interpreter, 1, lsb, 2);
        cc(&mut interpreter, 1, 6, 3);
        assert!(
            matches!(&cc(&mut interpreter, 1, 38, 4)[0], Observation::Parameter(v) if v.parameter == 130 && v.value == 388 && v.first_index == 0 && v.last_index == 4)
        );
        assert!(cc(&mut interpreter, 1, lsb, 3).is_empty());
        assert!(
            matches!(&cc(&mut interpreter, 1, 38, 4)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
        );
        assert!(
            matches!(&cc(&mut interpreter, 1, 6, 7)[0], Observation::Parameter(v) if v.family == family && v.parameter == 131 && v.value == 7 && v.resolution == 7 && v.first_index == 0 && v.last_index == 7)
        );
        assert!(cc(&mut interpreter, 1, msb, 2).is_empty());
        assert!(
            matches!(&cc(&mut interpreter, 1, 38, 4)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
        );
        assert!(
            matches!(&cc(&mut interpreter, 1, 6, 8)[0], Observation::Parameter(v) if v.family == family && v.parameter == 259 && v.value == 8 && v.first_index == 5 && v.last_index == 10)
        );
        assert!(
            matches!(&cc(&mut interpreter, 1, other_lsb, 2)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::SelectionInvalidated)
        );
        assert!(
            matches!(&cc(&mut interpreter, 1, 38, 3)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::IncompleteSelection)
        );
        cc(&mut interpreter, 1, other_msb, 0);
        assert!(
            matches!(&cc(&mut interpreter, 1, 38, 1)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
        );
    }
}

#[test]
fn repeated_selectors_preserve_data_and_its_source_range() {
    for (family, msb, lsb) in [
        (ParameterFamily::Nrpn, 99, 98),
        (ParameterFamily::Rpn, 101, 100),
    ] {
        let mut interpreter = ControlInterpreter::new(None).unwrap();
        cc(&mut interpreter, 1, msb, 1);
        cc(&mut interpreter, 1, lsb, 2);
        cc(&mut interpreter, 1, 6, 10);
        for (controller, value, first, last) in [(msb, 1, 1, 4), (lsb, 2, 2, 6)] {
            assert!(cc(&mut interpreter, 1, controller, value).is_empty());
            assert!(
                matches!(&cc(&mut interpreter, 1, 38, 3)[0], Observation::Parameter(v) if v.family == family && v.parameter == 130 && v.value == 1283 && v.resolution == 14 && v.first_index == first && v.last_index == last)
            );
        }
        cc(&mut interpreter, 1, lsb, 3);
        cc(&mut interpreter, 1, lsb, 2);
        assert!(
            matches!(&cc(&mut interpreter, 1, 38, 3)[0], Observation::Parameter(v) if v.parameter == 130 && v.value == 1283 && v.first_index == 2)
        );
    }
}

#[test]
fn family_switches_retain_complete_and_partial_selectors() {
    for (family, msb, lsb, other_msb, other_lsb) in [
        (ParameterFamily::Nrpn, 99, 98, 101, 100),
        (ParameterFamily::Rpn, 101, 100, 99, 98),
    ] {
        let mut interpreter = ControlInterpreter::new(None).unwrap();
        cc(&mut interpreter, 1, msb, 1);
        cc(&mut interpreter, 1, lsb, 2);
        cc(&mut interpreter, 1, 6, 10);
        cc(&mut interpreter, 1, other_msb, 3);
        assert!(
            matches!(&cc(&mut interpreter, 1, 6, 20)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::IncompleteSelection)
        );
        cc(&mut interpreter, 1, lsb, 3);
        assert!(
            matches!(&cc(&mut interpreter, 1, 38, 4)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
        );
        assert!(
            matches!(&cc(&mut interpreter, 1, 6, 30)[0], Observation::Parameter(v) if v.family == family && v.parameter == 131 && v.value == 30)
        );
        cc(&mut interpreter, 1, other_lsb, 4);
        assert!(
            matches!(&cc(&mut interpreter, 1, 6, 40)[0], Observation::Parameter(v) if v.family != family && v.parameter == 388 && v.value == 40)
        );
        cc(&mut interpreter, 1, msb, 2);
        assert!(
            matches!(&cc(&mut interpreter, 1, 6, 50)[0], Observation::Parameter(v) if v.family == family && v.parameter == 259 && v.value == 50)
        );
        interpreter.reset();
        for controller in [lsb, other_lsb] {
            cc(&mut interpreter, 1, controller, 5);
            assert!(
                matches!(&cc(&mut interpreter, 1, 6, 60)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::IncompleteSelection)
            );
        }
    }
}

#[test]
fn parameter_caches_are_independent_across_selections_and_families() {
    for (msb, lsb, other_msb, other_lsb) in [(99, 98, 101, 100), (101, 100, 99, 98)] {
        let mut interpreter = ControlInterpreter::new(None).unwrap();
        for (controller, value) in [
            (msb, 1),
            (lsb, 2),
            (6, 10),
            (other_msb, 1),
            (other_lsb, 2),
            (6, 20),
        ] {
            cc(&mut interpreter, 1, controller, value);
        }
        for (controller, value) in [(msb, 1), (lsb, 2), (6, 60)] {
            cc(&mut interpreter, 2, controller, value);
        }
        cc(&mut interpreter, 1, lsb, 2);
        assert!(
            matches!(&cc(&mut interpreter, 1, 38, 3)[0], Observation::Parameter(v) if v.parameter == 130 && v.value == 1283 && v.resolution == 14 && v.first_index == 0)
        );
        for (parameter_lsb, coarse) in [(3, 11), (4, 12)] {
            cc(&mut interpreter, 1, lsb, parameter_lsb);
            cc(&mut interpreter, 1, 6, coarse);
        }
        cc(&mut interpreter, 1, lsb, 2);
        assert!(
            matches!(&cc(&mut interpreter, 1, 38, 4)[0], Observation::Parameter(v) if v.parameter == 130 && v.value == 1284)
        );
        cc(&mut interpreter, 1, 96, 0);
        cc(&mut interpreter, 1, lsb, 3);
        assert!(
            matches!(&cc(&mut interpreter, 1, 38, 5)[0], Observation::Parameter(v) if v.parameter == 131 && v.value == 1413)
        );
        cc(&mut interpreter, 1, other_lsb, 2);
        assert!(
            matches!(&cc(&mut interpreter, 1, 38, 5)[0], Observation::Parameter(v) if v.parameter == 130 && v.value == 2565)
        );
        cc(&mut interpreter, 1, lsb, 2);
        assert!(
            matches!(&cc(&mut interpreter, 1, 38, 6)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
        );
        cc(&mut interpreter, 1, 6, 30);
        cc(&mut interpreter, 1, other_msb, 5);
        assert!(
            matches!(&cc(&mut interpreter, 1, 38, 6)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
        );
        cc(&mut interpreter, 1, 6, 40);
        cc(&mut interpreter, 1, other_msb, 1);
        assert!(
            matches!(&cc(&mut interpreter, 1, 38, 8)[0], Observation::Parameter(v) if v.parameter == 130 && v.value == 2568)
        );
        cc(&mut interpreter, 1, msb, 1);
        assert!(
            matches!(&cc(&mut interpreter, 1, 38, 7)[0], Observation::Parameter(v) if v.parameter == 130 && v.value == 3847)
        );
        cc(&mut interpreter, 1, 121, 0);
        assert!(
            matches!(&cc(&mut interpreter, 2, 38, 3)[0], Observation::Parameter(v) if v.channel == 2 && v.parameter == 130 && v.value == 7683)
        );
        interpreter.reset();
        for (selector_msb, selector_lsb) in [(msb, lsb), (other_msb, other_lsb)] {
            cc(&mut interpreter, 1, selector_msb, 1);
            cc(&mut interpreter, 1, selector_lsb, 2);
            assert!(
                matches!(&cc(&mut interpreter, 1, 38, 1)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
            );
        }
    }
}

#[test]
fn parameter_state_preserves_partial_knowledge_and_returns_snapshots() {
    use meedee::controls::ParameterState;
    for (family, msb, lsb, other_family, other_msb, other_lsb) in [
        (
            ParameterFamily::Nrpn,
            99,
            98,
            ParameterFamily::Rpn,
            101,
            100,
        ),
        (
            ParameterFamily::Rpn,
            101,
            100,
            ParameterFamily::Nrpn,
            99,
            98,
        ),
    ] {
        let mut interpreter = ControlInterpreter::new(None).unwrap();
        assert_eq!(interpreter.parameter_state(1, family, 0).unwrap(), None);
        cc(&mut interpreter, 1, msb, 0);
        cc(&mut interpreter, 1, lsb, 0);
        assert!(
            matches!(&cc(&mut interpreter, 1, 38, 7)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
        );
        let fine_only = interpreter.parameter_state(1, family, 0).unwrap();
        assert_eq!(
            fine_only,
            Some(ParameterState {
                coarse: None,
                fine: Some(7)
            })
        );
        assert!(
            matches!(&cc(&mut interpreter, 1, 6, 10)[0], Observation::Parameter(v) if v.resolution == 7 && v.last_index == 3)
        );
        assert_eq!(
            interpreter.parameter_state(1, family, 0).unwrap(),
            Some(ParameterState {
                coarse: Some(10),
                fine: None
            })
        );
        assert_eq!(fine_only.unwrap().fine, Some(7));
        cc(&mut interpreter, 1, 38, 5);
        let complete = interpreter.parameter_state(1, family, 0).unwrap();
        assert_eq!(
            complete,
            Some(ParameterState {
                coarse: Some(10),
                fine: Some(5)
            })
        );
        cc(&mut interpreter, 1, 6, 10);
        assert_eq!(
            interpreter.parameter_state(1, family, 0).unwrap(),
            Some(ParameterState {
                coarse: Some(10),
                fine: None
            })
        );
        assert_eq!(complete.unwrap().fine, Some(5));
        cc(&mut interpreter, 1, 38, 5);
        cc(&mut interpreter, 1, lsb, 1);
        cc(&mut interpreter, 1, 38, 9);
        let sibling = interpreter.parameter_state(1, family, 1).unwrap();
        assert_eq!(
            sibling,
            Some(ParameterState {
                coarse: None,
                fine: Some(9)
            })
        );
        for (controller, value) in [
            (other_msb, 0),
            (other_lsb, 0),
            (6, 20),
            (38, 6),
            (101, 127),
            (100, 127),
        ] {
            cc(&mut interpreter, 1, controller, value);
        }
        assert_eq!(interpreter.parameter_state(1, family, 0).unwrap(), complete);
        let other = interpreter.parameter_state(1, other_family, 0).unwrap();
        assert_eq!(
            other,
            Some(ParameterState {
                coarse: Some(20),
                fine: Some(6)
            })
        );
        assert_eq!(
            interpreter
                .parameter_state(1, ParameterFamily::Rpn, 16383)
                .unwrap(),
            None
        );
        assert_eq!(interpreter.parameter_state(2, family, 0).unwrap(), None);
        cc(&mut interpreter, 1, msb, 0);
        cc(&mut interpreter, 1, lsb, 0);
        cc(&mut interpreter, 1, 96, 0);
        assert_eq!(interpreter.parameter_state(1, family, 0).unwrap(), None);
        assert_eq!(interpreter.parameter_state(1, family, 1).unwrap(), sibling);
        assert_eq!(
            interpreter.parameter_state(1, other_family, 0).unwrap(),
            other
        );
        cc(&mut interpreter, 1, 121, 0);
        assert_eq!(interpreter.parameter_state(1, family, 1).unwrap(), None);
        assert_eq!(
            interpreter.parameter_state(1, other_family, 0).unwrap(),
            None
        );
        cc(&mut interpreter, 1, msb, 0);
        cc(&mut interpreter, 1, lsb, 0);
        cc(&mut interpreter, 1, 38, 3);
        interpreter.reset();
        assert_eq!(interpreter.parameter_state(1, family, 0).unwrap(), None);
    }
}

#[test]
fn parameter_state_lookup_validates_ranges() {
    let interpreter = ControlInterpreter::new(None).unwrap();
    for (channel, parameter) in [(0, 0), (17, 0), (1, 16384)] {
        assert!(interpreter
            .parameter_state(channel, ParameterFamily::Nrpn, parameter)
            .is_err());
    }
    assert_eq!(
        interpreter
            .parameter_state(16, ParameterFamily::Nrpn, 16383)
            .unwrap(),
        None
    );
}

#[test]
fn relative_updates_are_direction_only_and_invalidate_absolute_context() {
    let mut interpreter = ControlInterpreter::new(None).unwrap();
    cc(&mut interpreter, 1, 101, 0);
    cc(&mut interpreter, 1, 100, 1);
    assert!(
        matches!(&cc(&mut interpreter, 1, 96, 127)[0], Observation::Step(v) if v.direction == StepDirection::Increment && v.parameter == 1 && v.first_index == 0 && v.last_index == 2)
    );
    cc(&mut interpreter, 1, 6, 2);
    assert!(
        matches!(&cc(&mut interpreter, 1, 97, 99)[0], Observation::Step(v) if v.direction == StepDirection::Decrement)
    );
    assert!(
        matches!(&cc(&mut interpreter, 1, 38, 3)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
    );
}

#[test]
fn rpn_null_deselects_both_families_without_reserving_nrpn_16383() {
    let mut interpreter = ControlInterpreter::new(None).unwrap();
    cc(&mut interpreter, 1, 101, 0);
    cc(&mut interpreter, 1, 100, 0);
    cc(&mut interpreter, 1, 6, 5);
    cc(&mut interpreter, 1, 99, 127);
    cc(&mut interpreter, 1, 98, 127);
    assert!(
        matches!(&cc(&mut interpreter, 1, 6, 127)[0], Observation::Parameter(v) if v.family == ParameterFamily::Nrpn && v.parameter == 16383)
    );
    cc(&mut interpreter, 1, 101, 127);
    cc(&mut interpreter, 1, 100, 127);
    assert!(
        matches!(&cc(&mut interpreter, 1, 96, 0)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingSelection)
    );
    cc(&mut interpreter, 1, 98, 127);
    assert!(
        matches!(&cc(&mut interpreter, 1, 38, 1)[0], Observation::Parameter(v) if v.parameter == 16383 && v.value == 16257)
    );
    assert!(
        matches!(&cc(&mut interpreter, 1, 6, 2)[0], Observation::Parameter(v) if v.family == ParameterFamily::Nrpn && v.parameter == 16383)
    );
    cc(&mut interpreter, 1, 101, 0);
    cc(&mut interpreter, 1, 100, 0);
    assert!(
        matches!(&cc(&mut interpreter, 1, 38, 3)[0], Observation::Parameter(v) if v.family == ParameterFamily::Rpn && v.parameter == 0 && v.value == 643)
    );
}

#[test]
fn resets_and_source_boundaries_invalidate_only_the_applicable_state() {
    let mut one = ControlInterpreter::new(None).unwrap();
    let mut two = ControlInterpreter::new(None).unwrap();
    cc(&mut one, 1, 7, 2);
    cc(&mut one, 2, 7, 3);
    assert!(
        matches!(&cc(&mut two, 1, 39, 1)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
    );
    assert!(
        matches!(&cc(&mut one, 1, 121, 0)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::StateReset && d.channel == Some(1))
    );
    assert!(
        matches!(&cc(&mut one, 1, 39, 1)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
    );
    assert!(matches!(&cc(&mut one, 2, 39, 1)[0], Observation::Control(v) if v.value == 385));
    one.feed(&meedee::events::Payload::Midi1(AnyMessage::SystemReset(
        SystemReset::new().unwrap(),
    )))
    .unwrap();
    assert!(
        matches!(&cc(&mut one, 2, 39, 1)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
    );
    cc(&mut one, 1, 7, 2);
    one.reset();
    assert!(
        matches!(&cc(&mut one, 1, 39, 1)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
    );
    assert!(cc(&mut one, 1, 0, 1).is_empty());
    assert!(cc(&mut one, 1, 32, 1).is_empty());
}

fn cc(
    interpreter: &mut ControlInterpreter,
    channel: i64,
    controller: i64,
    value: i64,
) -> Vec<Observation> {
    interpreter
        .feed(&meedee::events::Payload::Midi1(AnyMessage::ControlChange(
            ControlChange::new(controller, value)
                .and_then(|m| m.with_channel(channel))
                .unwrap(),
        )))
        .unwrap()
        .into_iter()
        .map(|v| match v.value {
            meedee::events::ObservationValue::Control(v) => v,
            _ => panic!("expected control observation"),
        })
        .collect()
}

#[test]
fn parameter_step_builders_validate_family_and_preserve_defaults() {
    let step = ParameterStep::new(7).unwrap();
    assert_eq!(step.family(), ParameterFamily::Rpn);
    assert_eq!(step.direction(), StepDirection::Increment);
    assert_eq!(step.channel(), None);
    assert!(step.expand().is_err());
    assert!(ParameterStep::new(16383).is_err());
    let step = step
        .with_family(ParameterFamily::Nrpn)
        .unwrap()
        .with_parameter(16383)
        .unwrap()
        .with_direction(StepDirection::Decrement)
        .with_channel(16)
        .unwrap();
    assert!(step.clone().with_family(ParameterFamily::Rpn).is_err());
    assert!(step.clone().with_parameter(-1).is_err());
    assert!(step.clone().with_parameter(16384).is_err());
    assert!(step.clone().with_channel(17).is_err());
    assert_eq!(step.expand().unwrap().len(), 5);
}
