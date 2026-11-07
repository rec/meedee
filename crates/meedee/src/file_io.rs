//! Bounded file I/O with byte locations. No native MIDI initialization.
use std::io::{self, Read, Write};

pub const DEFAULT_LIMIT: usize = 1_048_576;
pub fn invalid(offset: u64, reason: impl std::fmt::Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("byte {offset}: {reason}"),
    )
}
pub struct Source<R> {
    pub inner: R,
    pub offset: u64,
    pub limit: usize,
}
impl<R: Read> Source<R> {
    pub fn new(inner: R, limit: usize) -> io::Result<Self> {
        if limit == 0 {
            return Err(invalid(0, "payload limit must be positive"));
        }
        Ok(Self {
            inner,
            offset: 0,
            limit,
        })
    }
    pub fn byte(&mut self) -> io::Result<u8> {
        Ok(self.array::<1>()?[0])
    }
    pub fn optional_byte(&mut self) -> io::Result<Option<u8>> {
        let mut b = [0];
        loop {
            match self.inner.read(&mut b) {
                Ok(0) => return Ok(None),
                Ok(_) => {
                    self.offset += 1;
                    return Ok(Some(b[0]));
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    return Err(io::Error::new(
                        e.kind(),
                        format!("byte {}: {e}", self.offset),
                    ))
                }
            }
        }
    }
    pub fn array<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let mut b = [0; N];
        self.read_exact(&mut b)?;
        Ok(b)
    }
    pub fn read_exact(&mut self, mut b: &mut [u8]) -> io::Result<()> {
        while !b.is_empty() {
            match self.inner.read(b) {
                Ok(0) => return Err(invalid(self.offset, "truncated input")),
                Ok(n) => {
                    self.offset += n as u64;
                    b = &mut b[n..];
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    return Err(io::Error::new(
                        e.kind(),
                        format!("byte {}: {e}", self.offset),
                    ))
                }
            }
        }
        Ok(())
    }
    pub fn payload(&mut self, n: usize) -> io::Result<Vec<u8>> {
        if n > self.limit {
            return Err(invalid(self.offset, "payload exceeds configured limit"));
        }
        let mut b = vec![0; n];
        self.read_exact(&mut b)?;
        Ok(b)
    }
}
pub fn write_all(w: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    w.write_all(bytes)
}

/// A lossless file record. MIDI files begin with their header, followed by events.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Record {
    Archive(crate::archive::Record),
    Header(crate::smf::Header),
    Event(crate::smf::Event),
}
impl Record {
    pub fn channel(&self) -> Option<u8> {
        use crate::{archive, events::Payload, messages::AnyCommand};
        match self {
            Self::Event(event) => event.channel(),
            Self::Archive(archive::Record::Event(message)) => match message.as_ref() {
                Payload::Midi1(message) => crate::smf::message_channel(message),
                Payload::Ump(message) => {
                    let words = crate::ump::encode(message).ok()?;
                    matches!(words[0] >> 28, 2 | 4).then_some(((words[0] >> 16) as u8 & 15) + 1)
                }
                Payload::Command(command) => match command {
                    AnyCommand::ControlChange14(v) => v.channel(),
                    AnyCommand::RpnSet(v) => v.channel(),
                    AnyCommand::NrpnSet(v) => v.channel(),
                    AnyCommand::ParameterStep(v) => v.channel(),
                    AnyCommand::BankProgramSelect(v) => v.channel(),
                    AnyCommand::ZoneConfiguration(v) => Some(v.manager_channel()),
                },
            },
            _ => None,
        }
    }
    pub fn transpose(self, semitones: i64) -> Self {
        use crate::messages::Message;
        match self {
            Self::Event(event) => Self::Event(event.transpose(semitones)),
            Self::Archive(crate::archive::Record::Event(message)) => Self::Archive(
                crate::archive::Record::Event(Box::new(message.transpose(semitones))),
            ),
            other => other,
        }
    }
}

type Prefixed<R> = std::io::Chain<std::io::Cursor<[u8; 4]>, R>;
enum Input<R> {
    Archive(crate::archive::Reader<Prefixed<R>>),
    Smf(crate::smf::Reader<Prefixed<R>>),
}

/// Detect MIDI files and meedee archives from their contents, regardless of name.
/// Use format-specific readers for other formats and detailed codec control.
pub struct Reader<R = std::io::BufReader<std::fs::File>> {
    input: Input<R>,
    header: Option<crate::smf::Header>,
    complete: bool,
    problem: Option<String>,
}
impl Reader {
    pub fn open(path: impl AsRef<std::path::Path>) -> io::Result<Self> {
        Self::new(
            std::io::BufReader::new(std::fs::File::open(path)?),
            DEFAULT_LIMIT,
            false,
            false,
        )
    }
}
impl<R: Read> Reader<R> {
    pub fn new(
        mut inner: R,
        limit: usize,
        recover: bool,
        preserve_unknown: bool,
    ) -> io::Result<Self> {
        let mut prefix = [0; 4];
        inner.read_exact(&mut prefix)?;
        let smf = prefix == *b"MThd";
        let inner = std::io::Cursor::new(prefix).chain(inner);
        let (input, header) = if smf {
            if recover || preserve_unknown {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "archive recovery options do not apply to MIDI files",
                ));
            }
            let reader = crate::smf::Reader::new(inner, limit)?;
            let header = Some(reader.header.clone());
            (Input::Smf(reader), header)
        } else {
            (
                Input::Archive(crate::archive::Reader::new(
                    inner,
                    limit,
                    recover,
                    preserve_unknown,
                )?),
                None,
            )
        };
        Ok(Self {
            input,
            header,
            complete: false,
            problem: None,
        })
    }
    pub fn complete(&self) -> bool {
        self.complete
    }
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }
}
impl<R: Read> Iterator for Reader<R> {
    type Item = io::Result<Record>;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(header) = self.header.take() {
            return Some(Ok(Record::Header(header)));
        }
        match &mut self.input {
            Input::Archive(reader) => {
                let result = reader.next().map(|v| v.map(Record::Archive));
                self.complete = reader.complete;
                self.problem = reader.problem.clone();
                result
            }
            Input::Smf(reader) => {
                let result = reader.next().map(|v| v.map(Record::Event));
                if let Some(Err(error)) = &result {
                    self.problem = Some(error.to_string());
                }
                if result.is_none() && self.problem.is_none() {
                    self.complete = true;
                }
                result
            }
        }
    }
}

enum Output<W> {
    Archive(crate::archive::Writer<W>),
    AwaitingHeader(W),
    Smf(crate::smf::Writer<W>),
}
/// Choose the output format from .mid/.midi or .meedee; creation is exclusive.
/// Call finish() to validate completion and report final write errors.
pub struct Writer<W = std::fs::File> {
    output: Option<Output<W>>,
    limit: usize,
}
impl Writer {
    pub fn create(path: impl AsRef<std::path::Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let extension = path.extension().and_then(|v| v.to_str()).unwrap_or("");
        validate_extension(extension)?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        Self::new(file, extension, DEFAULT_LIMIT)
    }
}
fn validate_extension(extension: &str) -> io::Result<()> {
    match extension.to_ascii_lowercase().as_str() {
        "mid" | "midi" | "meedee" => Ok(()),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected .mid, .midi or .meedee destination",
        )),
    }
}
impl<W: Write + std::io::Seek> Writer<W> {
    /// For streams, specify the same extension used by a filename destination.
    pub fn new(inner: W, extension: &str, limit: usize) -> io::Result<Self> {
        validate_extension(extension)?;
        if limit == 0 {
            return Err(invalid(0, "payload limit must be positive"));
        }
        let output = if extension.eq_ignore_ascii_case("meedee") {
            Output::Archive(crate::archive::Writer::new(inner, limit)?)
        } else {
            Output::AwaitingHeader(inner)
        };
        Ok(Self {
            output: Some(output),
            limit,
        })
    }
    pub fn write(&mut self, record: &Record) -> io::Result<()> {
        match (self.output.as_mut(), record) {
            (Some(Output::Archive(writer)), Record::Archive(record)) => writer.write(record),
            (Some(Output::Smf(writer)), Record::Event(event)) => writer.write(event),
            (Some(Output::AwaitingHeader(_)), Record::Header(header)) => {
                let Some(Output::AwaitingHeader(inner)) = self.output.take() else {
                    unreachable!()
                };
                self.output = Some(Output::Smf(crate::smf::Writer::new(
                    inner,
                    header.clone(),
                    self.limit,
                )?));
                Ok(())
            }
            (None, _) => Err(invalid(0, "writer closed or failed")),
            _ => Err(invalid(
                0,
                "record does not match destination format or MIDI header order",
            )),
        }
    }
    pub fn finish(&mut self) -> io::Result<()> {
        self.finish_with_status(true, "")
    }
    pub fn finish_with_status(&mut self, complete: bool, reason: &str) -> io::Result<()> {
        match self.output.as_mut() {
            Some(Output::Archive(writer)) => writer.finish(complete, reason)?,
            Some(Output::Smf(writer)) => {
                if !complete || !reason.is_empty() {
                    return Err(invalid(0, "completion status applies only to archives"));
                }
                writer.finish()?;
            }
            Some(Output::AwaitingHeader(_)) => {
                return Err(invalid(0, "MIDI header must be written first"))
            }
            None => return Err(invalid(0, "writer closed or failed")),
        }
        self.output = None;
        Ok(())
    }
}
