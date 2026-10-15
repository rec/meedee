use meedee::messages::{AnyMessage, NoteOn};
use meedee::{clip, smf, syx, ump};
use std::io::{self, Cursor, Write};
const SMF:&[u8]=b"MThd\0\0\0\x06\0\0\0\x01\x01\xe0MTrk\0\0\0\x13\0\xff\x51\x03\x07\xa1\x20\0\x90\x3c\x64\x83\x60\x3c\0\0\xff\x2f\0";
#[test]
fn smf_streaming_preserves_running_status_ticks_and_unknown_records() {
    let mut r = smf::Reader::new(Cursor::new(SMF), 1024).unwrap();
    let h = r.header.clone();
    let events = r.by_ref().collect::<io::Result<Vec<_>>>().unwrap();
    assert_eq!(events.len(), 4);
    assert_eq!(events[2].tick, 480);
    assert!(
        matches!(&events[1].record,smf::Record::Message(AnyMessage::NoteOn(v)) if v.velocity()==100)
    );
    let mut out = Cursor::new(Vec::new());
    {
        let mut w = smf::Writer::new(&mut out, h.clone(), 1024).unwrap();
        for e in &events {
            w.write(e).unwrap();
        }
        w.finish().unwrap();
    }
    assert_eq!(
        smf::Reader::new(Cursor::new(out.into_inner()), 1024)
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .unwrap(),
        events
    );
    for cut in 0..SMF.len() {
        let result = smf::Reader::new(Cursor::new(&SMF[..cut]), 1024)
            .and_then(|r| r.collect::<io::Result<Vec<_>>>());
        assert!(result.is_err(), "cut {cut}");
    }
    assert!(smf::Header::new(0, 2, 480, vec![]).is_err());
    assert!(smf::Header::new(1, 2, 0xe328, vec![]).is_ok());
    assert!(smf::Header::new(1, 2, 0x8001, vec![]).is_err());
    let mut unknown = SMF[..14].to_vec();
    unknown.extend(b"JUNK\0\0\0\x02xy");
    unknown.extend(&SMF[14..]);
    let got = smf::Reader::new(Cursor::new(unknown), 1024)
        .unwrap()
        .collect::<io::Result<Vec<_>>>()
        .unwrap();
    assert!(matches!(&got[0].record,smf::Record::Chunk{id,data}if id==b"JUNK"&&data==b"xy"));
    let mut bad = SMF.to_vec();
    bad[21] = 18;
    assert!(smf::Reader::new(Cursor::new(bad), 1024)
        .unwrap()
        .collect::<io::Result<Vec<_>>>()
        .is_err());
    assert!(smf::Reader::new(Cursor::new(SMF), 2).is_err());
}
#[test]
fn smf_fragments_meta_and_independent_tracks_roundtrip() {
    let h = smf::Header::new(2, 2, 0xe328, vec![1, 2]).unwrap();
    let e = vec![
        smf::Event {
            track: 0,
            tick: 0,
            record: smf::Record::SysEx {
                status: 240,
                data: vec![125, 1],
            },
        },
        smf::Event {
            track: 0,
            tick: 3,
            record: smf::Record::SysEx {
                status: 247,
                data: vec![2, 247],
            },
        },
        smf::Event {
            track: 0,
            tick: 3,
            record: smf::Record::Meta {
                kind: 47,
                data: vec![],
            },
        },
        smf::Event {
            track: 1,
            tick: 0,
            record: smf::Record::Meta {
                kind: 119,
                data: vec![255, 0],
            },
        },
        smf::Event {
            track: 1,
            tick: 0,
            record: smf::Record::Meta {
                kind: 47,
                data: vec![],
            },
        },
    ];
    let mut out = Cursor::new(vec![]);
    {
        let mut w = smf::Writer::new(&mut out, h.clone(), 10).unwrap();
        for v in &e {
            w.write(v).unwrap();
        }
        w.finish().unwrap();
    }
    let r = smf::Reader::new(Cursor::new(out.into_inner()), 10).unwrap();
    assert_eq!(r.header, h);
    assert_eq!(r.collect::<io::Result<Vec<_>>>().unwrap(), e);
}
#[test]
fn syx_limits_and_interruption_never_return_partial_dump() {
    let v = syx::Reader::new(Cursor::new(b"\xf0\x7d\x01\xf7\xf0\xf7"), 10)
        .unwrap()
        .collect::<io::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(v.len(), 2);
    assert_eq!(v[0].data(), [125, 1]);
    for b in [&b"\xf0\x7d"[..], &b"\xf0\x90\xf7"[..], &b"\x90"[..]] {
        assert!(syx::Reader::new(Cursor::new(b), 10)
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .is_err());
    }
    assert!(syx::Reader::new(Cursor::new(b"\xf0\x01\x02\xf7"), 1)
        .unwrap()
        .collect::<io::Result<Vec<_>>>()
        .is_err());
}
#[test]
fn clip_requires_timing_start_end_and_exact_big_endian_packets() {
    let b=b"SMF2CLIP\x00\x40\x00\x00\x00\x30\x01\xe0\x00\x40\x00\x00\xf0\x20\x00\x00\0\0\0\0\0\0\0\0\0\0\0\0\x00\x40\x00\x01\x20\x90\x3c\x64\x00\x40\x00\x00\xf0\x21\x00\x00\0\0\0\0\0\0\0\0\0\0\0\0";
    let e = clip::Reader::new(Cursor::new(b), false)
        .unwrap()
        .collect::<io::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(e.len(), 8);
    assert_eq!(e[5].tick, Some(1));
    let mut out = vec![];
    {
        let mut w = clip::Writer::new(&mut out).unwrap();
        for v in &e {
            w.write(&v.message).unwrap();
        }
        w.finish().unwrap();
    }
    assert_eq!(out, b);
    for cut in 0..b.len() {
        assert!(clip::Reader::new(Cursor::new(&b[..cut]), false)
            .and_then(|r| r.collect::<io::Result<Vec<_>>>())
            .is_err());
    }
    let mut w = clip::Writer::new(vec![]).unwrap();
    assert!(w
        .write(&ump::UmpMessage::Midi1(
            ump::Midi1Packet::new(1, AnyMessage::NoteOn(NoteOn::new(1, 60, 100).unwrap())).unwrap()
        ))
        .is_err());
    assert!(w.finish().is_err());
}
struct FullDisk;
impl Write for FullDisk {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::from_raw_os_error(28))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn disk_errors_propagate() {
    assert!(clip::Writer::new(FullDisk).is_err());
    let v = meedee::messages::SysEx::new(vec![125]).unwrap();
    assert!(syx::write(&mut FullDisk, &v, 10).is_err());
}
