use middy::controls::{ControlInterpreter, ParameterFamily};
use middy::events::Payload;
use middy::messages::{AnyMessage, ControlChange, ProgramChange};
use middy::messages::{Message, NoteOn, ParameterController, SysEx};
use middy::universe::{CacheOptions, Category, Retention, Scope, Universe};

fn cc(controller: i64, value: i64) -> Payload {
    Payload::Message(AnyMessage::ControlChange(
        ControlChange::new(controller, value)
            .unwrap()
            .with_channel(1)
            .unwrap(),
    ))
}

#[test]
fn retention_preserves_partial_parameters_and_observations() {
    let scope = Scope::new(None, 1).unwrap();
    for retention in [Retention::Full, Retention::Minimum] {
        let mut cache = Universe::new(CacheOptions {
            default: retention,
            ..Default::default()
        });
        let mut decoder = ControlInterpreter::new(None).unwrap();
        for (index, (controller, value)) in [
            (99, 1),
            (98, 2),
            (38, 7),
            (6, 10),
            (98, 3),
            (6, 20),
            (98, 2),
            (38, 5),
        ]
        .into_iter()
        .enumerate()
        {
            let event = cc(controller, value);
            assert_eq!(
                cache.feed(&event, Some(index as u64)).unwrap(),
                decoder.feed(&event).unwrap()
            );
            assert_eq!(
                cache
                    .parameter_state(scope, 1, ParameterFamily::Nrpn, 130)
                    .unwrap()
                    .map(|v| v.components.unwrap()),
                decoder
                    .parameter_state(1, ParameterFamily::Nrpn, 130)
                    .unwrap()
            );
        }
        let snapshot = cache.snapshot();
        assert_eq!(snapshot.parameters.len(), 2);
        assert_eq!(snapshot.parameters[0].components.unwrap().fine, Some(5));
        cache.reset();
        assert!(cache.snapshot().parameters.is_empty());
        assert_eq!(snapshot.parameters.len(), 2);
    }
}

#[test]
fn policies_resolve_and_optional_values_are_omitted() {
    let scope = Scope::new(None, 1).unwrap();
    let mut options = CacheOptions {
        default: Retention::Minimum,
        universe: Some(Retention::Full),
        ..Default::default()
    };
    options
        .categories
        .insert(Category::Performance, Retention::Minimum);
    let mut cache = Universe::new(options);
    assert_eq!(cache.effective(Category::Controls), Retention::Full);
    cache.feed(&cc(7, 0), Some(0)).unwrap();
    assert_eq!(
        cache.controller_state(scope, 1, 7).unwrap().unwrap().coarse,
        Some(0)
    );
    let program = Payload::Message(AnyMessage::ProgramChange(
        ProgramChange::new(8).unwrap().with_channel(1).unwrap(),
    ));
    cache.feed(&program, Some(1)).unwrap();
    assert!(cache.program_state(scope, 1).unwrap().is_none());
    assert!(cache.poll(Some(0)).is_err());
    assert!(cache.controller_state(scope, 0, 7).is_err());
}

#[test]
fn native_group_values_and_program_banks_remain_independent() {
    let mut cache = Universe::default();
    let plain = Scope::new(None, 1).unwrap();
    let grouped = Scope::new(Some(2), 1).unwrap();
    let native = Scope::new(Some(2), 2).unwrap();
    cache
        .feed(&cc(7, 10).with_stream("keyboard").unwrap(), Some(0))
        .unwrap();
    let grouped_cc = Payload::Message(AnyMessage::ControlChange(
        ControlChange::new(7, 20)
            .unwrap()
            .with_channel(1)
            .unwrap()
            .with_group(2)
            .unwrap(),
    ))
    .with_stream("keyboard")
    .unwrap();
    cache.feed(&grouped_cc, Some(1)).unwrap();
    let native_cc = Payload::Message(AnyMessage::ControlChange(
        ControlChange::new_midi2(7, 0xfedcba98)
            .unwrap()
            .with_channel(1)
            .unwrap()
            .with_group(2)
            .unwrap(),
    ))
    .with_stream("keyboard")
    .unwrap();
    cache.feed(&native_cc, Some(2)).unwrap();
    assert_eq!(
        cache.controller_state(plain, 1, 7).unwrap().unwrap().coarse,
        Some(10)
    );
    assert_eq!(
        cache
            .controller_state(grouped, 1, 7)
            .unwrap()
            .unwrap()
            .coarse,
        Some(20)
    );
    assert_eq!(
        cache
            .controller_state(native, 1, 7)
            .unwrap()
            .unwrap()
            .native,
        Some(0xfedcba98)
    );
    assert!(cache
        .feed(&cc(7, 99).with_stream("other").unwrap(), Some(3))
        .is_err());
    cache.reset();
    cache.feed(&cc(32, 4), Some(0)).unwrap();
    cache
        .feed(
            &Payload::Message(AnyMessage::ProgramChange(
                ProgramChange::new(7).unwrap().with_channel(1).unwrap(),
            )),
            Some(1),
        )
        .unwrap();
    let old = cache.program_state(plain, 1).unwrap().unwrap();
    cache.feed(&cc(0, 5), Some(2)).unwrap();
    assert_eq!(old.bank_msb, None);
    assert_eq!(old.bank_lsb, Some(4));
    assert_eq!(
        cache.program_state(plain, 1).unwrap().unwrap().pending_msb,
        Some(5)
    );
}

#[test]
fn notes_tuning_and_timecode_use_existing_decoders() {
    let mut cache = Universe::default();
    let scope = Scope::new(None, 1).unwrap();
    cache.feed(&cc(88, 5), Some(0)).unwrap();
    cache
        .feed(
            &Payload::Message(AnyMessage::NoteOn(
                NoteOn::new(60)
                    .unwrap()
                    .with_velocity(10)
                    .unwrap()
                    .with_channel(1)
                    .unwrap(),
            )),
            Some(1),
        )
        .unwrap();
    let note = cache.note_state(scope, 1, 60).unwrap().unwrap();
    assert_eq!(note.velocity, Some(1285));
    assert_eq!(note.velocity_resolution, Some(14));
    let sysex = SysEx::new(vec![127, 1, 8, 2, 0, 1, 60, 60, 0, 0])
        .unwrap()
        .with_group(2)
        .unwrap();
    for packet in middy::ump::packetize_sysex7(&sysex).unwrap() {
        cache
            .feed(
                &Payload::Ump(middy::ump::PacketValue::SysEx7(packet)),
                Some(2),
            )
            .unwrap();
    }
    assert!(cache
        .tuning_state(Some(2), 1, None, 0)
        .unwrap()
        .unwrap()
        .notes
        .contains_key(&60));
    let native = Payload::Message(AnyMessage::ParameterController(
        ParameterController::new(2, 1, 1, 2, 0xffffffff).unwrap(),
    ));
    cache.feed(&native, Some(3)).unwrap();
    assert_eq!(
        cache
            .parameter_state(
                Scope::new(Some(2), 2).unwrap(),
                1,
                ParameterFamily::Rpn,
                130
            )
            .unwrap()
            .unwrap()
            .native,
        Some(0xffffffff)
    );
    let full = Payload::Message(AnyMessage::SysEx(
        SysEx::new(vec![127, 127, 1, 1, 1, 2, 3, 4]).unwrap(),
    ));
    cache.feed(&full, Some(4)).unwrap();
    assert_eq!(
        cache
            .system_state(scope)
            .unwrap()
            .unwrap()
            .timecode
            .unwrap()
            .encoded
            .hours(),
        1
    );
}

#[test]
fn observations_keep_universe_indexes_across_interleaved_groups() {
    let mut cache = Universe::default();
    let grouped = |controller, value| {
        Payload::Message(AnyMessage::ControlChange(
            ControlChange::new(controller, value)
                .unwrap()
                .with_channel(1)
                .unwrap()
                .with_group(2)
                .unwrap(),
        ))
    };
    cache.feed(&grouped(7, 10), Some(0)).unwrap();
    cache.feed(&cc(1, 1), Some(1)).unwrap();
    let out = cache.feed(&grouped(39, 5), Some(2)).unwrap();
    assert_eq!((out[0].first_index, out[0].last_index), (Some(0), Some(2)));
    let middy::events::ObservationValue::Control(middy::controls::Observation::Control(value)) =
        &out[0].value
    else {
        panic!("control observation required");
    };
    assert_eq!((value.first_index, value.last_index), (0, 2));
}
