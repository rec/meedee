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
            "BankProgramSelect" => {
                codecs::expand(&gm::BankProgramSelect::new(v[0], v[1], v[2], v[3]).unwrap())
                    .iter()
                    .flat_map(codecs::encode)
                    .collect()
            }
            "ZoneConfiguration" => {
                codecs::expand(&mpe::ZoneConfiguration::new(v[0], v[1]).unwrap())
                    .iter()
                    .flat_map(codecs::encode)
                    .collect()
            }
            "Gm1On" => gm::Gm1On::new(v[0]).unwrap().encode(),
            "GmOff" => gm::GmOff::new(v[0]).unwrap().encode(),
            "Gm2On" => gm::Gm2On::new(v[0]).unwrap().encode(),
            "IdentityRequest" => universal::IdentityRequest::new(v[0]).unwrap().encode(),
            "MasterVolume" => universal::MasterVolume::new(v[0], v[1]).unwrap().encode(),
            "MasterFineTuning" => universal::MasterFineTuning::new(v[0], v[1])
                .unwrap()
                .encode(),
            "MasterCoarseTuning" => universal::MasterCoarseTuning::new(v[0], v[1])
                .unwrap()
                .encode(),
            "FullFrame" => {
                timecode::FullFrame::new(v[0], timecode::FrameRate::Fps30, v[2], v[3], v[4], v[5])
                    .unwrap()
                    .encode()
            }
            _ => panic!("unknown fixture"),
        };
        assert_eq!(actual, expected, "{name}");
        if expected[0] == 240 {
            let raw = SysEx::new(expected[1..expected.len() - 1].to_vec()).unwrap();
            assert!(
                gm::decode(&raw).unwrap().is_some()
                    || universal::decode(&raw).unwrap().is_some()
                    || timecode::decode(&raw).unwrap().is_some()
            );
            for size in 3..raw.data().len() {
                let truncated = SysEx::new(raw.data()[..size].to_vec()).unwrap();
                assert!(
                    gm::decode(&truncated).is_err()
                        || universal::decode(&truncated).is_err()
                        || timecode::decode(&truncated).is_err()
                );
            }
        }
    }
    let command = gm::BankProgramSelect::new(1, 0, 0, 1).unwrap();
    assert!(matches!(
        command.expand().last(),
        Some(AnyMessage::ProgramChange(_))
    ));
}
#[test]
fn gm_tables_cover_exact_addresses_without_fabricating_fallbacks() {
    let gm1 = gm::programs();
    assert_eq!(gm1.len(), 128);
    assert_eq!(gm1[0].name(), "Acoustic Grand Piano");
    assert_eq!(gm1[127].program(), 128);
    assert_eq!(gm1[127].name(), "Gunshot");
    assert_eq!(gm::find_programs("  acoustic GRAND piano ").len(), 2);
    assert_eq!(gm::find_programs("Piano").len(), 0);
    let gm2 = gm::gm2_programs();
    assert_eq!(gm2.len(), 256);
    let addresses: HashSet<_> = gm2
        .iter()
        .map(|entry| (entry.bank_msb(), entry.bank_lsb(), entry.program()))
        .collect();
    assert_eq!(addresses.len(), 256);
    for program in 1..=128 {
        assert!(gm::gm2_program(0, program).unwrap().is_some());
    }
    assert_eq!(gm::gm2_program(4, 6).unwrap().unwrap().name(), "EP Phase");
    assert_eq!(
        gm::gm2_program(9, 126).unwrap().unwrap().name(),
        "Burst Noise"
    );
    assert!(gm::gm2_program(127, 1).unwrap().is_none());
    assert_eq!(gm::drum_kits().len(), 9);
    assert_eq!(gm::find_drum_kits("  room SET ")[0].program(), 9);
    assert_eq!(
        (35..=81)
            .filter(|&note| gm::percussion(note).unwrap().is_some())
            .count(),
        47
    );
    assert!(gm::percussion(34).unwrap().is_none());
    assert_eq!(
        gm::gm2_percussion(17, 36).unwrap().unwrap().name(),
        "Power Kick Drum"
    );
    assert_eq!(
        gm::gm2_percussion(26, 75).unwrap().unwrap().name(),
        "Analog Claves"
    );
    assert_eq!(
        gm::gm2_percussion(49, 41).unwrap().unwrap().name(),
        "Timpani F"
    );
    assert!(gm::gm2_percussion(57, 27).unwrap().is_none());
    assert!(gm::gm2_percussion(2, 36).unwrap().is_none());
    for kit in gm::drum_kits() {
        for note in 0..128 {
            let _ = gm::gm2_percussion(kit.program().into(), note).unwrap();
        }
    }
    for bad in [0, 129] {
        assert!(gm::program(bad).is_err());
    }
}
#[test]
fn identity_and_universal_decoders_preserve_exact_fields_and_reject_corruption() {
    for manufacturer in [vec![65], vec![0, 32, 51]] {
        let value =
            universal::IdentityReply::new(3, manufacturer, 257, 16383, vec![0, 1, 2, 127]).unwrap();
        let wire = value.encode();
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
        assert!(gm::decode(&raw).unwrap().is_none());
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
        controls::local_control(16, true).unwrap().encode(),
        vec![191, 122, 127]
    );
    assert_eq!(
        controls::all_notes_off(10).unwrap().encode(),
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
        let wire = value.encode();
        assert_eq!(
            timecode::decode(&SysEx::new(wire[1..wire.len() - 1].to_vec()).unwrap()).unwrap(),
            Some(value)
        );
        assert!(timecode::FullFrame::new(127, rate, 0, 0, 0, max + 1).is_err());
    }
    assert!(timecode::FullFrame::new(1, timecode::FrameRate::Fps30, 0, 0, 0, 0).is_err());
    let mut interpreter = controls::Interpreter::new();
    interpreter.feed(&AnyMessage::ControlChange(
        meedee::messages::ControlChange::new(1, 7, 1).unwrap(),
    ));
    interpreter.feed(&gm::Gm2On::new(127).unwrap());
    let result = interpreter.feed(&AnyMessage::ControlChange(
        meedee::messages::ControlChange::new(1, 39, 2).unwrap(),
    ));
    assert!(
        matches!(&result[0],controls::Observation::Diagnostic(value) if value.kind==controls::DiagnosticKind::MissingCoarse)
    );
}
