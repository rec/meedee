use meedee::codecs::{encode, expand};
use meedee::controls::{
    DiagnosticKind, InputMessage, Interpreter, Observation, ParameterFamily, StepDirection,
};
use meedee::messages::*;

#[test]
fn commands_expand_to_independent_wire_vectors() {
    for line in include_str!("fixtures/commands.txt").lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let (fields, wire) = line.split_once('|').unwrap();
        let mut fields = fields.split_whitespace();
        let name = fields.next().unwrap();
        let v: Vec<i64> = fields.map(|v| v.parse().unwrap()).collect();
        let command = match name {
            "ControlChange14" => {
                AnyCommand::ControlChange14(ControlChange14::new(v[0], v[1], v[2]).unwrap())
            }
            "RpnSet" => AnyCommand::RpnSet(RpnSet::new(v[0], v[1], v[2], v[3]).unwrap()),
            "NrpnSet" => AnyCommand::NrpnSet(NrpnSet::new(v[0], v[1], v[2], v[3]).unwrap()),
            "RpnIncrement" => AnyCommand::RpnIncrement(RpnIncrement::new(v[0], v[1]).unwrap()),
            "RpnDecrement" => AnyCommand::RpnDecrement(RpnDecrement::new(v[0], v[1]).unwrap()),
            "NrpnIncrement" => AnyCommand::NrpnIncrement(NrpnIncrement::new(v[0], v[1]).unwrap()),
            "NrpnDecrement" => AnyCommand::NrpnDecrement(NrpnDecrement::new(v[0], v[1]).unwrap()),
            _ => panic!("unknown fixture command {name}"),
        };
        let expected: Vec<u8> = wire
            .split_whitespace()
            .map(|v| u8::from_str_radix(v, 16).unwrap())
            .collect();
        assert_eq!(
            expand(&command).iter().flat_map(encode).collect::<Vec<_>>(),
            expected,
            "{line}"
        );
    }
    assert!(RpnSet::new(1, 16383, 0, 7).is_err());
    assert!(NrpnSet::new(1, 16383, 127, 7).is_ok());
    for resolution in [0, 8, 13, 15] {
        assert!(RpnSet::new(1, 0, 0, resolution).is_err());
    }
    assert!(NrpnSet::new(1, 0, 128, 7).is_err());
    assert!(ControlChange14::new(0, 0, 0).is_err());
    assert!(ControlChange14::new(1, 32, 0).is_err());
    assert!(ControlChange14::new(1, 0, 16384).is_err());
}

#[test]
fn fine_controls_follow_wire_order_and_keep_only_the_known_coarse_component() {
    let mut interpreter = Interpreter::new();
    assert!(
        matches!(&cc(&mut interpreter, 1, 39, 7)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
    );
    assert!(
        matches!(&cc(&mut interpreter, 1, 7, 2)[0], Observation::Control(v) if v.value == 2 && v.resolution == 7 && v.first_index == 1)
    );
    cc(&mut interpreter, 2, 7, 9);
    interpreter.feed(&AnyMessage::TimingClock(TimingClock::new().unwrap()));
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
fn edited_and_switched_selectors_require_fresh_halves_and_never_donate_data() {
    let mut interpreter = Interpreter::new();
    cc(&mut interpreter, 1, 99, 1);
    assert!(
        matches!(&cc(&mut interpreter, 1, 6, 2)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::IncompleteSelection)
    );
    cc(&mut interpreter, 1, 98, 2);
    cc(&mut interpreter, 1, 6, 3);
    assert!(
        matches!(&cc(&mut interpreter, 1, 38, 4)[0], Observation::Parameter(v) if v.parameter == 130 && v.value == 388 && v.first_index == 0 && v.last_index == 4)
    );
    assert!(
        matches!(&cc(&mut interpreter, 1, 98, 3)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::SelectionInvalidated)
    );
    assert!(
        matches!(&cc(&mut interpreter, 1, 6, 7)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::IncompleteSelection)
    );
    cc(&mut interpreter, 1, 99, 1);
    assert!(
        matches!(&cc(&mut interpreter, 1, 38, 4)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
    );
    cc(&mut interpreter, 1, 6, 7);
    assert!(
        matches!(&cc(&mut interpreter, 1, 100, 2)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::SelectionInvalidated)
    );
    assert!(
        matches!(&cc(&mut interpreter, 1, 38, 3)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::IncompleteSelection)
    );
    cc(&mut interpreter, 1, 101, 0);
    assert!(
        matches!(&cc(&mut interpreter, 1, 38, 1)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
    );
}

#[test]
fn relative_updates_are_direction_only_and_invalidate_absolute_context() {
    let mut interpreter = Interpreter::new();
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
    let mut interpreter = Interpreter::new();
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
}

#[test]
fn resets_and_source_boundaries_invalidate_only_the_applicable_state() {
    let mut one = Interpreter::new();
    let mut two = Interpreter::new();
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
    one.feed(InputMessage::SystemReset);
    assert!(
        matches!(&cc(&mut one, 2, 39, 1)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
    );
    cc(&mut one, 1, 7, 2);
    assert!(
        matches!(&one.reset()[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::StateReset && d.first_index == 8)
    );
    assert!(
        matches!(&cc(&mut one, 1, 39, 1)[0], Observation::Diagnostic(d) if d.kind == DiagnosticKind::MissingCoarse)
    );
    assert!(cc(&mut one, 1, 0, 1).is_empty());
    assert!(cc(&mut one, 1, 32, 1).is_empty());
}

fn cc(
    interpreter: &mut Interpreter,
    channel: i64,
    controller: i64,
    value: i64,
) -> Vec<Observation> {
    interpreter.feed(&AnyMessage::ControlChange(
        ControlChange::new(channel, controller, value).unwrap(),
    ))
}
