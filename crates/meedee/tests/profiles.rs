use meedee::messages::{AnyMessage, Command, PrimitiveMessage, SysEx};
use meedee::{codecs, controls, gm, mpe, timecode, universal};
use std::collections::HashSet;

#[test]
fn independent_vectors_preserve_expansion_order_and_primitive_wire_layouts() {
    for line in include_str!("fixtures/profiles.txt")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
    {
        let (fields, wire) = line.split_once('|').unwrap();
        let mut fields = fields.split_whitespace();
        let name = fields.next().unwrap();
        let v: Vec<i64> = fields.map(|v| v.parse().unwrap()).collect();
        let expected: Vec<u8> = wire
            .split_whitespace()
            .map(|v| u8::from_str_radix(v, 16).unwrap())
            .collect();
        let actual = match name {
            "ProgramChange" => codecs::expand(
                &meedee::messages::ProgramChange::new(v[3])
                    .and_then(|m| m.with_bank(0, 0))
                    .and_then(|m| m.with_bank(v[1], v[2]))
                    .and_then(|m| m.with_channel(v[0]))
                    .unwrap(),
            )
            .unwrap()
            .iter()
            .flat_map(|m| codecs::encode(m).unwrap())
            .collect(),
            "ZoneConfiguration" => {
                codecs::expand(&mpe::ZoneConfiguration::new(v[0], v[1]).unwrap())
                    .unwrap()
                    .iter()
                    .flat_map(|m| codecs::encode(m).unwrap())
                    .collect()
            }
            "Gm1On" => gm::GmMode::new(gm::Mode::Gm1, v[0])
                .unwrap()
                .encode()
                .unwrap(),
            "GmOff" => gm::GmMode::new(gm::Mode::Off, v[0])
                .unwrap()
                .encode()
                .unwrap(),
            "Gm2On" => gm::GmMode::new(gm::Mode::Gm2, v[0])
                .unwrap()
                .encode()
                .unwrap(),
            "IdentityRequest" => universal::IdentityRequest::new(v[0])
                .unwrap()
                .encode()
                .unwrap(),
            "MasterVolume" => universal::MasterVolume::new(v[0], v[1])
                .unwrap()
                .encode()
                .unwrap(),
            "MasterFineTuning" => universal::MasterFineTuning::new(v[0], v[1])
                .unwrap()
                .encode()
                .unwrap(),
            "MasterCoarseTuning" => universal::MasterCoarseTuning::new(v[0], v[1])
                .unwrap()
                .encode()
                .unwrap(),
            "FullFrame" => {
                timecode::FullFrame::new(v[0], timecode::FrameRate::Fps30, v[2], v[3], v[4], v[5])
                    .unwrap()
                    .encode()
                    .unwrap()
            }
            _ => panic!("unknown fixture"),
        };
        assert_eq!(actual, expected, "{name}");
        if expected[0] == 240 {
            let raw = SysEx::new(expected[1..expected.len() - 1].to_vec()).unwrap();
            assert!(
                gm::decode(&raw, 1024).unwrap().is_some()
                    || universal::decode(&raw).unwrap().is_some()
                    || timecode::decode(&raw).unwrap().is_some()
            );
            for size in 3..raw.data().len() {
                let truncated = SysEx::new(raw.data()[..size].to_vec()).unwrap();
                assert!(
                    gm::decode(&truncated, 1024).is_err()
                        || universal::decode(&truncated).is_err()
                        || timecode::decode(&truncated).is_err()
                );
            }
        }
    }
    let command = meedee::messages::ProgramChange::new(1)
        .and_then(|m| m.with_bank(0, 0))
        .and_then(|m| m.with_channel(1))
        .unwrap();
    assert!(matches!(
        command.expand().unwrap().last(),
        Some(AnyMessage::ProgramChange(_))
    ));
}
#[test]
fn gm_tables_cover_exact_addresses_without_fabricating_fallbacks() {
    let gm1 = gm::programs(1).unwrap();
    assert_eq!(gm1.len(), 128);
    assert_eq!(gm1[0].name(), "Acoustic Grand Piano");
    assert_eq!(gm1[0].bank_msb(), None);
    assert_eq!(gm1[0].bank_lsb(), None);
    assert_eq!(gm1[127].program(), 128);
    assert_eq!(gm1[127].name(), "Gunshot");
    assert_eq!(gm::find_programs("  acoustic GRAND piano ").len(), 2);
    assert_eq!(gm::find_programs("Piano").len(), 0);
    let gm2 = gm::programs(2).unwrap();
    assert_eq!(gm2.len(), 256);
    assert_eq!(gm2[0].bank_msb(), Some(121));
    assert_eq!(gm2[0].bank_lsb(), Some(0));
    assert!(gm::programs(0).is_err());
    assert!(gm::programs(3).is_err());
    assert!(gm::program(1, Some(128)).is_err());
    let addresses: HashSet<_> = gm2
        .iter()
        .map(|entry| (entry.bank_msb(), entry.bank_lsb(), entry.program()))
        .collect();
    assert_eq!(addresses.len(), 256);
    for program in 1..=128 {
        assert!(gm::program(program, Some(0)).unwrap().is_some());
    }
    assert_eq!(gm::program(6, Some(4)).unwrap().unwrap().name(), "EP Phase");
    assert_eq!(
        gm::program(126, Some(9)).unwrap().unwrap().name(),
        "Burst Noise"
    );
    assert!(gm::program(1, Some(127)).unwrap().is_none());
    assert_eq!(gm::drum_kits().len(), 9);
    assert_eq!(gm::find_drum_kits("  room SET ")[0].program(), 9);
    assert_eq!(
        (35..=81)
            .filter(|&note| gm::percussion(note, None).unwrap().is_some())
            .count(),
        47
    );
    assert!(gm::percussion(34, None).unwrap().is_none());
    assert_eq!(
        gm::percussion(36, Some(17)).unwrap().unwrap().name(),
        "Power Kick Drum"
    );
    assert_eq!(
        gm::percussion(75, Some(26)).unwrap().unwrap().name(),
        "Analog Claves"
    );
    assert_eq!(
        gm::percussion(41, Some(49)).unwrap().unwrap().name(),
        "Timpani F"
    );
    assert!(gm::percussion(27, Some(57)).unwrap().is_none());
    assert!(gm::percussion(36, Some(2)).unwrap().is_none());
    for kit in gm::drum_kits() {
        for note in 0..128 {
            let _ = gm::percussion(note, Some(kit.program().into())).unwrap();
        }
    }
    for bad in [0, 129] {
        assert!(gm::program(bad, None).is_err());
    }
}
#[test]
fn identity_and_universal_decoders_preserve_exact_fields_and_reject_corruption() {
    for manufacturer in [vec![65], vec![0, 32, 51]] {
        let value =
            universal::IdentityReply::new(3, manufacturer, 257, 16383, vec![0, 1, 2, 127]).unwrap();
        let wire = value.encode().unwrap();
        let raw = SysEx::new(wire[1..wire.len() - 1].to_vec()).unwrap();
        assert_eq!(
            universal::decode(&raw).unwrap(),
            Some(universal::UniversalMessage::IdentityReply(value))
        );
        for size in 4..raw.data().len() {
            assert!(universal::decode(&SysEx::new(raw.data()[..size].to_vec()).unwrap()).is_err());
        }
    }
    for manufacturer in [vec![], vec![0], vec![1, 2, 3], vec![0, 128, 0]] {
        assert!(universal::IdentityReply::new(0, manufacturer, 0, 0, vec![0; 4]).is_err());
    }
    assert!(universal::decode(&SysEx::new(vec![127, 1, 4, 4, 1, 64]).unwrap()).is_err());
    for data in [
        vec![126, 1, 9, 127],
        vec![127, 1, 4, 127],
        vec![127, 1, 1, 2],
    ] {
        let raw = SysEx::new(data).unwrap();
        assert!(gm::decode(&raw, 1024).unwrap().is_none());
        assert!(universal::decode(&raw).unwrap().is_none());
        assert!(timecode::decode(&raw).unwrap().is_none());
    }
}
#[test]
fn named_rpns_modes_and_mpe_keep_their_distinct_units_and_ranges() {
    assert_eq!(
        controls::pitch_bend_sensitivity(1, 2, 50).unwrap().value(),
        306
    );
    assert_eq!(
        controls::pitch_bend_sensitivity(1, 2, 127).unwrap().value(),
        383
    );
    assert!(controls::pitch_bend_sensitivity(1, 2, 128).is_err());
    assert_eq!(controls::modulation_depth_range(1, 64).unwrap().value(), 64);
    assert_eq!(controls::channel_fine_tuning(1, 0).unwrap().value(), 8192);
    assert_eq!(controls::channel_coarse_tuning(1, -64).unwrap().value(), 0);
    assert_eq!(controls::tuning_program(1, 127).unwrap().parameter(), 3);
    assert_eq!(controls::tuning_bank(1, 0).unwrap().parameter(), 4);
    assert_eq!(
        controls::local_control(16, true).unwrap().encode().unwrap(),
        vec![191, 122, 127]
    );
    assert_eq!(
        controls::all_notes_off(10).unwrap().encode().unwrap(),
        vec![185, 123, 0]
    );
    assert!(controls::mono_mode(1, 17).is_err());
    for (manager, members) in [(0, 1), (2, 1), (15, 1), (17, 1), (1, -1), (16, 16)] {
        assert!(mpe::ZoneConfiguration::new(manager, members).is_err());
    }
    for manager in [1, 16] {
        for members in 0..=15 {
            assert_eq!(
                mpe::ZoneConfiguration::new(manager, members)
                    .unwrap()
                    .expand()
                    .unwrap()
                    .len(),
                5
            );
        }
    }
    for rate in [
        timecode::FrameRate::Fps24,
        timecode::FrameRate::Fps25,
        timecode::FrameRate::DropFrame30,
        timecode::FrameRate::Fps30,
    ] {
        let max = match rate {
            timecode::FrameRate::Fps24 => 23,
            timecode::FrameRate::Fps25 => 24,
            _ => 29,
        };
        let value = timecode::FullFrame::new(127, rate, 23, 59, 59, max).unwrap();
        let wire = value.encode().unwrap();
        assert_eq!(
            timecode::decode(&SysEx::new(wire[1..wire.len() - 1].to_vec()).unwrap()).unwrap(),
            Some(value)
        );
        assert!(timecode::FullFrame::new(127, rate, 0, 0, 0, max + 1).is_err());
    }
    assert!(timecode::FullFrame::new(1, timecode::FrameRate::Fps30, 0, 0, 0, 0).is_err());
    let mut interpreter = controls::ControlInterpreter::new(None).unwrap();
    let cc = |controller, value| {
        meedee::events::Payload::Message(AnyMessage::ControlChange(
            meedee::messages::ControlChange::new(controller, value)
                .unwrap()
                .with_channel(1)
                .unwrap(),
        ))
    };
    interpreter.feed(&cc(7, 1)).unwrap();
    let wire = gm::GmMode::new(gm::Mode::Gm2, 127)
        .unwrap()
        .encode()
        .unwrap();
    let mode = meedee::events::Payload::Message(AnyMessage::SysEx(
        SysEx::new(wire[1..wire.len() - 1].to_vec()).unwrap(),
    ));
    interpreter.feed(&mode).unwrap();
    let result = interpreter.feed(&cc(39, 2)).unwrap();
    assert!(
        matches!(&result[0].value, meedee::events::ObservationValue::Control(controls::Observation::Diagnostic(v)) if v.kind==controls::DiagnosticKind::MissingCoarse)
    );
}
