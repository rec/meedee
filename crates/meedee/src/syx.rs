//! Streaming concatenated MIDI 1 SysEx dumps. Realtime/other bytes are rejected.
use crate::file_io::{invalid, Source};
use crate::messages::SysEx;
use std::io::{self, Read, Write};
pub struct Reader<R> {
    source: Source<R>,
    failed: bool,
}
impl<R: Read> Reader<R> {
    pub fn new(inner: R, limit: usize) -> io::Result<Self> {
        Ok(Self {
            source: Source::new(inner, limit)?,
            failed: false,
        })
    }
    fn read_message(&mut self) -> io::Result<Option<SysEx>> {
        let Some(b) = self.source.optional_byte()? else {
            return Ok(None);
        };
        if b != 240 {
            return Err(invalid(self.source.offset - 1, "SYX expects F0"));
        }
        let mut data = Vec::new();
        loop {
            let b = self.source.byte()?;
            if b == 247 {
                break;
            }
            if b > 127 {
                return Err(invalid(self.source.offset - 1, "non-payload status in SYX"));
            }
            if data.len() == self.source.limit {
                return Err(invalid(self.source.offset - 1, "SYX payload limit"));
            }
            data.push(b);
        }
        Ok(Some(SysEx::new(data).expect("seven-bit payload")))
    }
}
impl<R: Read> Iterator for Reader<R> {
    type Item = io::Result<SysEx>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.read_message() {
            Ok(v) => v.map(Ok),
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}
pub fn write(writer: &mut impl Write, message: &SysEx, limit: usize) -> io::Result<()> {
    if limit == 0 || message.data().len() > limit {
        return Err(invalid(0, "SYX payload limit"));
    }
    writer.write_all(&[240])?;
    writer.write_all(message.data())?;
    writer.write_all(&[247])
}
