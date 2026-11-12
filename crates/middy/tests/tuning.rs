//! Vectors are stated independently from the implementation, from MTS pp. 1-8.
use middy::messages::{PrimitiveMessage, SysEx};
use middy::tuning::*;

fn table() -> TuningTable {
    TuningTable::new(vec![TuningValue::NoChange; 128]).unwrap()
}
fn scale(resolution: i64) -> ScaleOffsets {
    ScaleOffsets::new(resolution, vec![0; 12]).unwrap()
}
fn changes() -> Vec<NoteTuning> {
    vec![NoteTuning::new(69, TuningValue::Pitch(Pitch::new(69, 0).unwrap())).unwrap()]
}

#[test]
fn bulk_builders_preserve_unmapped_keys_and_exact_transfer_bytes() {
    let request = BulkDumpRequest::new(1, 2).unwrap();
    assert_eq!(request.bank(), None);
    let banked = request.clone().with_bank(0).unwrap();
    assert_ne!(request, banked);
    assert_eq!(banked.without_bank(), request);
    let change = NoteChange::new(1, 2, changes()).unwrap();
    assert!(change.realtime() && change.bank().is_none());
    assert!(change.clone().with_realtime(false).is_err());
    let quiet = change.with_bank(0).unwrap().with_realtime(false).unwrap();
    assert!(quiet.clone().without_bank().is_err());
    assert!(quiet.with_realtime(true).unwrap().without_bank().is_ok());
    let notes = changes();
    assert!(bulk_dump(&notes, 1, 2, "partial".into(), None, false).is_err());
    for bank in [None, Some(3)] {
        let dump = bulk_dump(&notes, 127, 2, "partial".into(), bank, true).unwrap();
        let table = match &dump {
            TuningMessage::BulkDump(d) => {
                assert_eq!(d.bank().map(i64::from), bank);
                d.table()
            }
            _ => panic!("expected bulk dump"),
        };
        for (note, pitch) in table.values().iter().enumerate() {
            assert_eq!(
                *pitch,
                if note == 69 {
                    notes[0].pitch()
                } else {
                    TuningValue::NoChange
                }
            );
        }
        assert!(dump.encode().unwrap().starts_with(&[0xf0, 0x7e, 127]));
    }
}

#[test]
fn full_keyboard_builders_preserve_order_and_split_at_127_keys() {
    let notes: Vec<_> = (0..128)
        .rev()
        .map(|n| NoteTuning::new(n, TuningValue::Pitch(Pitch::new(n, 0).unwrap())).unwrap())
        .collect();
    for bank in [None, Some(0)] {
        assert!(bulk_dump(&notes, 1, 2, "equal".into(), bank, false).is_ok());
        for realtime in [true, false] {
            if bank.is_none() && !realtime {
                continue;
            }
            let messages = note_changes(&notes, 1, 2, realtime, bank, false).unwrap();
            let batches: Vec<_> = messages
                .iter()
                .map(|m| match m {
                    TuningMessage::NoteChange(m) => {
                        assert_eq!(m.bank().map(i64::from), bank);
                        assert_eq!(m.realtime(), realtime);
                        m.changes()
                    }
                    _ => panic!("expected note changes"),
                })
                .collect();
            assert_eq!(
                batches.iter().map(|b| b.len()).collect::<Vec<_>>(),
                [127, 1]
            );
            assert_eq!(batches.concat(), notes);
        }
    }
}

#[test]
fn builders_reject_invalid_selections_and_require_explicit_preservation() {
    let notes = changes();
    for invalid in [
        vec![],
        vec![notes[0].clone(); 2],
        vec![notes[0].clone(); 129],
    ] {
        assert!(bulk_dump(&invalid, 1, 0, "test".into(), None, true).is_err());
        assert!(note_changes(&invalid, 1, 0, true, None, true).is_err());
    }
    let unchanged = [NoteTuning::new(69, TuningValue::NoChange).unwrap()];
    assert!(bulk_dump(&unchanged, 1, 0, "test".into(), None, false).is_err());
    assert!(note_changes(&unchanged, 1, 0, true, None, false).is_err());
    assert!(note_changes(&unchanged, 1, 0, true, None, true).is_ok());
    assert!(note_changes(&notes, 1, 0, false, None, false).is_err());
    assert!(note_changes(&notes, 128, 0, true, None, false).is_err());
    assert!(bulk_dump(&notes, 1, 128, "test".into(), None, true).is_err());
    assert!(bulk_dump(&notes, 1, 0, "test".into(), Some(128), true).is_err());
    assert!(bulk_dump(&notes, 1, 0, "x".repeat(17), None, true).is_err());
}
fn message(name: &str) -> TuningMessage {
    match name {
        "BulkDumpRequest" => BulkDumpRequest::new(1, 2).unwrap().into(),
        "BulkDumpRequestBanked" => BulkDumpRequest::new(1, 2)
            .and_then(|v| v.with_bank(3))
            .unwrap()
            .into(),
        "BulkDump" => BulkDump::new(1, 2, "".into(), table()).unwrap().into(),
        "BulkDumpBanked" => BulkDump::new(1, 2, "".into(), table())
            .and_then(|v| v.with_bank(3))
            .unwrap()
            .into(),
        "ScaleOctaveDump7" | "ScaleOctaveDump14" => ScaleOctaveDump::new(
            1,
            3,
            2,
            "".into(),
            scale(if name.ends_with("14") { 14 } else { 7 }),
        )
        .unwrap()
        .into(),
        "NoteChange" => NoteChange::new(1, 2, changes()).unwrap().into(),
        "NoteChangeBankedTrue" | "NoteChangeBankedFalse" => NoteChange::new(1, 2, changes())
            .and_then(|v| v.with_bank(3))
            .and_then(|v| v.with_realtime(name.ends_with("True")))
            .unwrap()
            .into(),
        _ => ScaleOctaveChange::new(
            1,
            name.ends_with("True"),
            vec![1, 8, 16],
            scale(if name.contains("14") { 14 } else { 7 }),
        )
        .unwrap()
        .into(),
    }
}
fn vectors() -> Vec<(TuningMessage, Vec<u8>)> {
    include_str!("fixtures/tuning.txt")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let (name, wire) = line.split_once('|').unwrap();
            let mut data = Vec::new();
            for token in wire.split_whitespace() {
                match token {
                    "NAME" => data.extend([32; 16]),
                    "TABLE" => data.extend([127; 384]),
                    "Z12" => data.extend([64; 12]),
                    "Z24" => data.extend([64, 0].repeat(12)),
                    _ => data.push(u8::from_str_radix(token, 16).unwrap()),
                }
            }
            (message(name), data)
        })
        .collect()
}
#[test]
fn normative_wire_vectors_cover_every_family_and_realtime_variant() {
    for (message, wire) in vectors() {
        assert_eq!(message.encode().unwrap(), wire, "{message:?}");
        let decoded = decode(&SysEx::new(wire[1..wire.len() - 1].to_vec()).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(decoded.message, message);
        assert_ne!(decoded.checksum, ChecksumStatus::LegacyMismatch);
    }
}
#[test]
fn recognized_payloads_require_exact_lengths_and_modern_checksums() {
    for (message, wire) in vectors() {
        let data = &wire[1..wire.len() - 1];
        for size in 4..data.len() {
            assert!(
                decode(&SysEx::new(data[..size].to_vec()).unwrap()).is_err(),
                "{message:?}, prefix {size}"
            );
        }
        let mut extra = data.to_vec();
        extra.push(0);
        assert!(decode(&SysEx::new(extra).unwrap()).is_err());
        if matches!(
            message,
            TuningMessage::BulkDump(_) | TuningMessage::ScaleOctaveDump(_)
        ) {
            let mut bad = data.to_vec();
            *bad.last_mut().unwrap() ^= 1;
            let result = decode(&SysEx::new(bad).unwrap());
            if matches!(&message, TuningMessage::BulkDump(v) if v.bank().is_none()) {
                assert_eq!(
                    result.unwrap().unwrap().checksum,
                    ChecksumStatus::LegacyMismatch
                );
            } else {
                assert!(result.is_err());
            }
        }
    }
    for data in [vec![1, 2, 3], vec![126, 1, 8, 127], vec![127, 1, 8, 1]] {
        assert!(decode(&SysEx::new(data).unwrap()).unwrap().is_none());
    }
    assert!(decode(&SysEx::new(vec![126, 1, 8]).unwrap()).is_err());
    let mut channels = ScaleOctaveChange::new(1, true, vec![1], scale(7))
        .unwrap()
        .encode()
        .unwrap();
    channels[5] |= 4;
    assert!(decode(&SysEx::new(channels[1..channels.len() - 1].to_vec()).unwrap()).is_err());
}
#[test]
fn exact_pitch_sentinel_and_quantization_boundaries() {
    assert!(Pitch::new(127, 16383).is_err());
    assert_eq!(Pitch::new(69, 0).unwrap().frequency_hz(), 440.0);
    assert_eq!(
        Pitch::from_frequency(440.0).unwrap().pitch,
        Pitch::new(69, 0).unwrap()
    );
    for semitone in 0..128 {
        for fraction in [0, 1, 8192, 16382] {
            let pitch = Pitch::new(semitone, fraction).unwrap();
            let conversion =
                Pitch::from_cents(semitone, fraction as f64 * 100.0 / 16384.0).unwrap();
            assert_eq!(conversion.pitch, pitch);
            assert_eq!(conversion.error_cents, 0.0);
            assert_eq!(
                Pitch::from_frequency(pitch.frequency_hz()).unwrap().pitch,
                pitch
            );
        }
    }
    let half = 100.0 / 32768.0;
    assert_eq!(
        Pitch::from_cents(0, half).unwrap().pitch,
        Pitch::new(0, 0).unwrap()
    );
    assert_eq!(
        Pitch::from_cents(0, 3.0 * half).unwrap().pitch,
        Pitch::new(0, 2).unwrap()
    );
    assert_eq!(Pitch::from_cents(0, half).unwrap().error_cents, -half);
    assert!(Pitch::from_cents(0, -0.000001).is_err());
    assert!(Pitch::from_cents(127, 100.0).is_err());
    let minimum = Pitch::new(0, 0).unwrap().frequency_hz();
    let maximum = Pitch::new(127, 16382).unwrap().frequency_hz();
    assert!(Pitch::from_frequency(f64::from_bits(minimum.to_bits() - 1)).is_err());
    assert!(Pitch::from_frequency(f64::from_bits(maximum.to_bits() + 1)).is_err());
    for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(Pitch::from_frequency(value).is_err());
    }
    let notes = vec![
        NoteTuning::new(0, TuningValue::NoChange).unwrap(),
        NoteTuning::new(127, TuningValue::Pitch(Pitch::new(127, 16382).unwrap())).unwrap(),
    ];
    let message: TuningMessage = NoteChange::new(127, 127, notes).unwrap().into();
    let wire = message.encode().unwrap();
    assert_eq!(&wire[8..11], &[127, 127, 127]);
    assert_eq!(&wire[12..15], &[127, 127, 126]);
    assert_eq!(
        decode(&SysEx::new(wire[1..wire.len() - 1].to_vec()).unwrap())
            .unwrap()
            .unwrap()
            .message,
        message
    );
}
#[test]
fn scale_offsets_use_signed_exact_units_and_report_error() {
    assert_eq!(
        ScaleOffsets::from_cents(7, vec![0.5; 12])
            .unwrap()
            .offsets
            .values(),
        &[0; 12]
    );
    assert_eq!(
        ScaleOffsets::from_cents(7, vec![1.5; 12])
            .unwrap()
            .offsets
            .values(),
        &[2; 12]
    );
    for (resolution, low, high) in [(7, -64, 63), (14, -8192, 8191)] {
        let scale = ScaleOffsets::new(resolution, vec![low; 12]).unwrap();
        assert_eq!(scale.values(), &[low as i16; 12]);
        assert!(ScaleOffsets::new(resolution, vec![high + 1; 12]).is_err());
        assert!(ScaleOffsets::new(resolution, vec![low - 1; 12]).is_err());
        for endpoint in [low, high] {
            let value: TuningMessage = ScaleOctaveDump::new(
                1,
                0,
                0,
                "limits".into(),
                ScaleOffsets::new(resolution, vec![endpoint; 12]).unwrap(),
            )
            .unwrap()
            .into();
            let wire = value.encode().unwrap();
            assert_eq!(
                decode(&SysEx::new(wire[1..wire.len() - 1].to_vec()).unwrap())
                    .unwrap()
                    .unwrap()
                    .message,
                value
            );
        }
    }
    assert!(ScaleOffsets::from_cents(14, vec![100.0; 12]).is_err());
    assert!(ScaleOffsets::from_cents(7, vec![f64::NAN; 12]).is_err());
    assert!(ScaleOffsets::new(8, vec![0; 12]).is_err());
    assert!(TuningTable::new(vec![TuningValue::NoChange; 127]).is_err());
    assert!(BulkDump::new(1, 0, "é".into(), table()).is_err());
    assert!(BulkDump::new(1, 0, "abcdefghijklmnopq".into(), table()).is_err());
    assert!(NoteChange::new(0, 0, vec![changes()[0].clone(); 128]).is_err());
    assert!(ScaleOctaveChange::new(0, false, vec![1, 1], scale(7)).is_err());
}
#[test]
fn request_matching_is_bounded_unicast_and_terminal() {
    let request = message("BulkDumpRequestBanked");
    let mut tracker = ReplyTracker::new(&request, 100, 10).unwrap();
    assert!(!tracker.observe(&message("BulkDump"), 101).unwrap());
    assert!(!tracker
        .observe(
            &BulkDump::new(2, 2, "".into(), table())
                .and_then(|v| v.with_bank(3))
                .unwrap()
                .into(),
            102
        )
        .unwrap());
    assert!(!tracker
        .observe(
            &BulkDump::new(1, 2, "".into(), table())
                .and_then(|v| v.with_bank(4))
                .unwrap()
                .into(),
            103
        )
        .unwrap());
    assert!(!tracker
        .observe(
            &BulkDump::new(1, 3, "".into(), table())
                .and_then(|v| v.with_bank(3))
                .unwrap()
                .into(),
            104
        )
        .unwrap());
    assert!(tracker.observe(&message("ScaleOctaveDump7"), 109).unwrap());
    assert_eq!(tracker.status(), RequestStatus::Completed);
    assert!(!tracker.observe(&message("BulkDumpBanked"), 110).unwrap());
    assert_eq!(tracker.cancel(), RequestStatus::Completed);
    let mut tracker = ReplyTracker::new(&request, 100, 10).unwrap();
    assert!(!tracker.observe(&message("BulkDumpBanked"), 110).unwrap());
    assert_eq!(tracker.status(), RequestStatus::TimedOut);
    assert!(tracker.poll(109).is_err());
    let mut tracker = ReplyTracker::new(&message("BulkDumpRequest"), 0, 10).unwrap();
    assert_eq!(tracker.cancel(), RequestStatus::Cancelled);
    assert!(!tracker.observe(&message("BulkDump"), 1).unwrap());
    assert!(ReplyTracker::new(&BulkDumpRequest::new(127, 0).unwrap().into(), 0, 10).is_err());
    assert!(ReplyTracker::new(&request, u64::MAX, 1).is_err());
    assert!(ReplyTracker::new(&request, 0, 0).is_err());
    assert!(ReplyTracker::new(&message("BulkDump"), 0, 1).is_err());
}
