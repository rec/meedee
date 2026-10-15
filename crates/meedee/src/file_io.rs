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
