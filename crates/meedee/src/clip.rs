//! M2-116-U 1.0 MIDI Clip framing. All UMPs, including clock/configuration, are retained.
use crate::file_io::{invalid, Source};
use crate::ump::{self, UmpMessage, UmpValue};
use std::io::{self, Read, Write};
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Entry {
    pub tick: Option<u64>,
    pub sequence: bool,
    pub message: UmpMessage,
}
#[derive(Default, Clone)]
struct State {
    tick: u64,
    dcs: bool,
    tpq: bool,
    started: bool,
    ended: bool,
    last_dcs: bool,
    header_events: usize,
    tempo: bool,
    signature: bool,
    previous_tempo: bool,
}
impl State {
    fn accept(&mut self, v: UmpMessage, offset: u64) -> io::Result<Entry> {
        if self.ended {
            return Err(invalid(offset, "data after EndOfClip"));
        }
        let is_dcs = matches!(v, UmpMessage::DeltaClockstamp(_));
        if let UmpMessage::DeltaClockstamp(d) = &v {
            if !self.tpq && (self.dcs || d.ticks() != 0) {
                return Err(invalid(offset, "first DCS must be zero before DCTPQ"));
            }
            if self.last_dcs {
                return Err(invalid(
                    offset,
                    "DCS needs an intervening event; use Noop for long gaps",
                ));
            }
            self.tick = self
                .tick
                .checked_add(u64::from(d.ticks()))
                .ok_or_else(|| invalid(offset, "clip tick overflow"))?;
            self.dcs = true;
        } else if let UmpMessage::DeltaClockstampTicksPerQuarter(_) = &v {
            if self.tpq || !self.last_dcs || self.tick != 0 {
                return Err(invalid(offset, "DCTPQ must follow initial zero DCS"));
            }
            self.tpq = true;
        } else if !self.tpq {
            // Optional profile-tag SysEx packets are retained but never negotiated/executed.
            if !matches!(v, UmpMessage::SysEx7(_)) || self.dcs {
                return Err(invalid(
                    offset,
                    "expected optional profile packets or DCS/DCTPQ",
                ));
            }
        } else {
            if !self.dcs {
                return Err(invalid(offset, "message lacks DCS"));
            }
            if let UmpMessage::Stream(s) = &v {
                match s.status() {
                    32 => {
                        if self.started || !self.last_dcs {
                            return Err(invalid(offset, "StartOfClip must follow DCS once"));
                        }
                        self.started = true;
                    }
                    33 => {
                        if !self.started || !self.last_dcs {
                            return Err(invalid(offset, "EndOfClip must follow DCS in sequence"));
                        }
                        self.ended = true;
                    }
                    _ => {}
                }
            }
            if !self.started {
                let is_tempo =
                    matches!(&v,UmpMessage::FlexData(f) if f.status_bank()==0&&f.status()==0);
                let is_sig =
                    matches!(&v,UmpMessage::FlexData(f) if f.status_bank()==0&&f.status()==1);
                if is_tempo {
                    if self.tempo || self.header_events != 0 || self.tick != 0 {
                        return Err(invalid(offset, "configuration tempo must be first at zero"));
                    }
                    self.tempo = true;
                }
                if is_sig {
                    if self.signature || !self.previous_tempo || self.tick != 0 {
                        return Err(invalid(
                            offset,
                            "configuration signature must follow tempo at zero",
                        ));
                    }
                    self.signature = true;
                }
                self.previous_tempo = is_tempo;
                self.header_events += 1;
            }
        }
        self.last_dcs = is_dcs;
        Ok(Entry {
            tick: self.dcs.then_some(self.tick),
            sequence: self.started,
            message: v,
        })
    }
    fn finish(&self, offset: u64) -> io::Result<()> {
        if !self.ended {
            return Err(invalid(offset, "missing EndOfClip"));
        }
        Ok(())
    }
}
pub struct Reader<R> {
    source: Source<R>,
    state: State,
    preserve_unknown: bool,
    failed: bool,
}
impl<R: Read> Reader<R> {
    pub fn new(inner: R, preserve_unknown: bool) -> io::Result<Self> {
        let mut source = Source::new(inner, 16)?;
        if source.array::<8>()? != *b"SMF2CLIP" {
            return Err(invalid(0, "expected SMF2CLIP"));
        }
        Ok(Self {
            source,
            state: State::default(),
            preserve_unknown,
            failed: false,
        })
    }
    fn read_entry(&mut self) -> io::Result<Option<Entry>> {
        let Some(a) = self.source.optional_byte()? else {
            self.state.finish(self.source.offset)?;
            return Ok(None);
        };
        let b = self.source.array::<3>()?;
        let first = u32::from_be_bytes([a, b[0], b[1], b[2]]);
        let mut words = vec![first];
        for _ in 1..ump::word_count(first) {
            words.push(u32::from_be_bytes(self.source.array()?));
        }
        let v = ump::decode(&words, self.preserve_unknown)
            .map_err(|e| invalid(self.source.offset, e))?;
        self.state.accept(v, self.source.offset).map(Some)
    }
}
impl<R: Read> Iterator for Reader<R> {
    type Item = io::Result<Entry>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.read_entry() {
            Ok(v) => v.map(Ok),
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}
pub struct Writer<W> {
    inner: W,
    state: State,
    offset: u64,
    failed: bool,
}
impl<W: Write> Writer<W> {
    pub fn new(mut inner: W) -> io::Result<Self> {
        inner.write_all(b"SMF2CLIP")?;
        Ok(Self {
            inner,
            state: State::default(),
            offset: 8,
            failed: false,
        })
    }
    pub fn write(&mut self, message: &UmpMessage) -> io::Result<()> {
        if self.failed {
            return Err(invalid(self.offset, "writer failed"));
        }
        let result = (|| {
            let mut state = self.state.clone();
            state.accept(message.clone(), self.offset)?;
            let bytes = ump::words_to_bytes(&message.encode_words(), false);
            self.inner.write_all(&bytes)?;
            self.offset += bytes.len() as u64;
            self.state = state;
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    pub fn finish(&mut self) -> io::Result<()> {
        if self.failed {
            return Err(invalid(self.offset, "writer failed"));
        }
        self.state.finish(self.offset)?;
        self.inner.flush()
    }
}
