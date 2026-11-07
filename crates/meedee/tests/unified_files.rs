use meedee::{archive, file_io, smf, AnyMessage, NoteOn, Reader, Writer};
use std::io::{self, Cursor};

fn fixture() -> (smf::Header, Vec<smf::Event>, Vec<u8>) {
    let header = smf::Header::new(1, 2, 480, vec![1, 2]).unwrap();
    let records = vec![
        smf::Event {
            track: 0,
            tick: 0,
            record: smf::Record::Chunk {
                id: *b"JUNK",
                data: vec![1, 2],
            },
        },
        smf::Event {
            track: 0,
            tick: 0,
            record: smf::Record::Meta {
                kind: 81,
                data: vec![7, 161, 32],
            },
        },
        smf::Event {
            track: 0,
            tick: 0,
            record: smf::Record::Message(AnyMessage::NoteOn(
                NoteOn::new(120).unwrap().with_channel(1).unwrap(),
            )),
        },
        smf::Event {
            track: 0,
            tick: 480,
            record: smf::Record::Meta {
                kind: 47,
                data: vec![],
            },
        },
        smf::Event {
            track: 1,
            tick: 0,
            record: smf::Record::Message(AnyMessage::NoteOn(
                NoteOn::new(60).unwrap().with_channel(10).unwrap(),
            )),
        },
        smf::Event {
            track: 1,
            tick: 1,
            record: smf::Record::SysEx {
                status: 240,
                data: vec![125, 1],
            },
        },
        smf::Event {
            track: 1,
            tick: 2,
            record: smf::Record::SysEx {
                status: 247,
                data: vec![2, 247],
            },
        },
        smf::Event {
            track: 1,
            tick: 480,
            record: smf::Record::Meta {
                kind: 47,
                data: vec![],
            },
        },
    ];
    let mut bytes = Cursor::new(Vec::new());
    let mut writer = smf::Writer::new(&mut bytes, header.clone(), 1024).unwrap();
    for event in &records {
        writer.write(event).unwrap();
    }
    writer.finish().unwrap();
    (header, records, bytes.into_inner())
}

#[test]
fn unified_loop_preserves_file_structure_and_transposes_only_selected_channels() {
    let (header, mut expected, bytes) = fixture();
    let mut source = Reader::new(bytes.as_slice(), 1024, false, false).unwrap();
    let mut output = Cursor::new(Vec::new());
    {
        let mut writer = Writer::new(&mut output, "mid", 1024).unwrap();
        for record in &mut source {
            let mut record = record.unwrap();
            if record.channel().unwrap_or(10) != 10 {
                record = record.transpose(12);
            }
            writer.write(&record).unwrap();
        }
        writer.finish().unwrap();
    }
    assert!(source.complete());
    assert_eq!(source.problem(), None);
    expected[2] = expected[2].clone().transpose(12);
    let reader = smf::Reader::new(Cursor::new(output.into_inner()), 1024).unwrap();
    assert_eq!(reader.header, header);
    assert_eq!(reader.collect::<io::Result<Vec<_>>>().unwrap(), expected);
    assert!(
        matches!(&expected[2].record, smf::Record::Message(AnyMessage::NoteOn(v)) if v.note() == 127)
    );
}

#[test]
fn unified_archive_records_keep_completion_and_support_output_sequences() {
    let mut bytes = Cursor::new(Vec::new());
    let records = [file_io::Record::Archive(archive::Record::Metadata {
        key: "title".into(),
        data: vec![1],
        stream: None,
        timestamp: None,
    })];
    let mut writer = Writer::new(&mut bytes, "meedee", 1024).unwrap();
    writer.write(&records[0]).unwrap();
    writer.finish().unwrap();
    let mut reader = Reader::new(bytes.get_ref().as_slice(), 1024, false, false).unwrap();
    assert_eq!(
        reader.by_ref().collect::<io::Result<Vec<_>>>().unwrap(),
        records
    );
    assert!(reader.complete());
    let reader = Reader::new(bytes.get_ref().as_slice(), 1024, false, false).unwrap();
    let mut output = meedee::Output::new();
    assert_eq!(output.send(reader).unwrap().sent_events, 0);
}

#[test]
fn unified_writer_requires_header_order_and_complete_tracks() {
    let (header, records, _) = fixture();
    let mut writer = Writer::new(Cursor::new(Vec::new()), "mid", 1024).unwrap();
    assert!(writer
        .write(&file_io::Record::Event(records[1].clone()))
        .is_err());
    writer.write(&file_io::Record::Header(header)).unwrap();
    assert!(writer.finish().is_err());
    assert!(Writer::new(Cursor::new(Vec::new()), "txt", 1024).is_err());
    let (_, _, bytes) = fixture();
    let mut reader = Reader::new(&bytes[..bytes.len() - 1], 1024, false, false).unwrap();
    assert!(reader.by_ref().collect::<io::Result<Vec<_>>>().is_err());
    assert!(!reader.complete());
    assert!(reader.problem().is_some());
}
