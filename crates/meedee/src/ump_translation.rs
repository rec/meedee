//! Explicit stateless translation. Compound MIDI 1 input needs caller context;
//! no bank, selector, profile, source or routing state is guessed.
use super::*;
use crate::messages as m;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TranslationStatus {
    Exact,
    Quantized,
    Unsupported,
    ProfileDependent,
    ContextRequired,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Translation<T> {
    pub status: TranslationStatus,
    pub reason: &'static str,
    pub group: Option<u8>,
    pub messages: Vec<T>,
}
impl<T> Translation<T> {
    pub fn converted(&self) -> bool {
        !self.messages.is_empty()
    }
}
fn report<T>(
    status: TranslationStatus,
    reason: &'static str,
    group: Option<u8>,
    messages: Vec<T>,
    allow_loss: bool,
) -> Translation<T> {
    Translation {
        status,
        reason,
        group,
        messages: if status == TranslationStatus::Quantized && !allow_loss {
            vec![]
        } else {
            messages
        },
    }
}
/// Min-Center-Max scaling, M2-115-U 1.0.2. No floating-point approximation.
pub fn scale_up(value: u32, source_bits: u8, destination_bits: u8) -> Result<u32, ValueError> {
    if !(2..=31).contains(&source_bits)
        || destination_bits <= source_bits
        || destination_bits > 32
        || u64::from(value) >= (1u64 << source_bits)
    {
        return Err(ValueError("invalid source value/bit widths".into()));
    }
    let shift = destination_bits - source_bits;
    let mut scaled = value << shift;
    if value > (1u32 << (source_bits - 1)) {
        let repeat = source_bits - 1;
        let mask = (1u32 << repeat) - 1;
        let v = value & mask;
        let mut tail = if shift > repeat {
            v << (shift - repeat)
        } else {
            v >> (repeat - shift)
        };
        while tail != 0 {
            scaled |= tail;
            tail >>= repeat;
        }
    }
    Ok(scaled)
}
fn up(value: u32, src: u8, dst: u8) -> u32 {
    scale_up(value, src, dst).expect("validated field and constant bit widths")
}
pub fn to_midi2(message: &AnyMessage, group: i64) -> Result<Translation<UmpMessage>, ValueError> {
    bounded(group, 1, 16, "group")?;
    let g = Some(group as u8);
    let value =
        match message {
            AnyMessage::NoteOn(v) if v.velocity() == 0 => UmpMessage::Midi2NoteOff(
                Midi2NoteOff::new(group, v.channel().into(), v.note().into(), 32768, 0, 0)?,
            ),
            AnyMessage::NoteOn(v) => UmpMessage::Midi2NoteOn(Midi2NoteOn::new(
                group,
                v.channel().into(),
                v.note().into(),
                up(v.velocity().into(), 7, 16).into(),
                0,
                0,
            )?),
            AnyMessage::NoteOff(v) => UmpMessage::Midi2NoteOff(Midi2NoteOff::new(
                group,
                v.channel().into(),
                v.note().into(),
                up(v.velocity().into(), 7, 16).into(),
                0,
                0,
            )?),
            AnyMessage::PolyPressure(v) => UmpMessage::Midi2PolyPressure(Midi2PolyPressure::new(
                group,
                v.channel().into(),
                v.note().into(),
                up(v.pressure().into(), 7, 32).into(),
            )?),
            AnyMessage::ChannelPressure(v) => {
                UmpMessage::Midi2ChannelPressure(Midi2ChannelPressure::new(
                    group,
                    v.channel().into(),
                    up(v.pressure().into(), 7, 32).into(),
                )?)
            }
            AnyMessage::PitchBend(v) => UmpMessage::Midi2PitchBend(Midi2PitchBend::new(
                group,
                v.channel().into(),
                i64::from(up((i32::from(v.value()) + 8192) as u32, 14, 32)) - 2147483648,
            )?),
            AnyMessage::ProgramChange(v) => UmpMessage::Midi2ProgramChange(
                Midi2ProgramChange::new(group, v.channel().into(), v.program().into(), 0, 0, 0)?,
            ),
            AnyMessage::ControlChange(v) => {
                if matches!(v.controller(), 0 | 6 | 32 | 38 | 88 | 96..=101) {
                    return Ok(report(
                    TranslationStatus::ContextRequired,
                    "bank, parameter or velocity-prefix sequence requires explicit source context",
                    g,
                    vec![],
                    false,
                ));
                }
                let value = if matches!(v.controller(), 84 | 126) {
                    u32::from(v.value()) << 25
                } else {
                    up(v.value().into(), 7, 32)
                };
                UmpMessage::Midi2ControlChange(Midi2ControlChange::new(
                    group,
                    v.channel().into(),
                    v.controller().into(),
                    value.into(),
                )?)
            }
            AnyMessage::SysEx(v) => {
                return Ok(report(
                    TranslationStatus::Exact,
                    "SysEx7 packetization preserves all payload bytes",
                    g,
                    packetize_sysex7(group, v.data())?
                        .into_iter()
                        .map(UmpMessage::SysEx7)
                        .collect(),
                    false,
                ))
            }
            _ => UmpMessage::Midi1(Midi1Packet::new(group, message.clone())?),
        };
    Ok(report(
        TranslationStatus::Exact,
        if matches!(message, AnyMessage::ProgramChange(_)) {
            "program-only translation retains the receiver's current bank"
        } else if matches!(message,AnyMessage::NoteOn(v) if v.velocity()==0) {
            "zero-velocity NoteOn becomes NoteOff with default release velocity"
        } else {
            "native MIDI 1 value is preserved under standard scaling"
        },
        g,
        vec![value],
        false,
    ))
}
pub fn to_midi1(
    message: &UmpMessage,
    allow_loss: bool,
) -> Result<Translation<AnyMessage>, ValueError> {
    use UmpMessage::*;
    let words = message.encode_words();
    let group = if matches!(words[0] >> 28, 1..=5 | 13) {
        Some(((words[0] >> 24) & 15) as u8 + 1)
    } else {
        None
    };
    let mut lost = false;
    let mut scale = |v: u32, src: u8, dst: u8| -> i64 {
        let coarse = v >> (src - dst);
        if up(coarse, dst, src) != v {
            lost = true;
        }
        i64::from(coarse)
    };
    let output = match message {
        Midi1(v) => vec![v.message.clone()],
        Midi2NoteOn(v) => {
            if v.attribute_type != 0 {
                return Ok(report(
                    TranslationStatus::ProfileDependent,
                    "note attributes require explicit profile/manufacturer interpretation",
                    group,
                    vec![],
                    allow_loss,
                ));
            }
            let velocity = scale(v.velocity.into(), 16, 7);
            let velocity = if velocity == 0 {
                lost = true;
                1
            } else {
                velocity
            };
            vec![AnyMessage::NoteOn(m::NoteOn::new(
                v.channel.into(),
                v.note.into(),
                velocity,
            )?)]
        }
        Midi2NoteOff(v) => {
            if v.attribute_type != 0 {
                return Ok(report(
                    TranslationStatus::ProfileDependent,
                    "note attributes require explicit profile/manufacturer interpretation",
                    group,
                    vec![],
                    allow_loss,
                ));
            }
            vec![AnyMessage::NoteOff(m::NoteOff::new(
                v.channel.into(),
                v.note.into(),
                scale(v.velocity.into(), 16, 7),
            )?)]
        }
        Midi2PolyPressure(v) => vec![AnyMessage::PolyPressure(m::PolyPressure::new(
            v.channel.into(),
            v.note.into(),
            scale(v.pressure, 32, 7),
        )?)],
        Midi2ChannelPressure(v) => vec![AnyMessage::ChannelPressure(m::ChannelPressure::new(
            v.channel.into(),
            scale(v.pressure, 32, 7),
        )?)],
        Midi2PitchBend(v) => vec![AnyMessage::PitchBend(m::PitchBend::new(
            v.channel.into(),
            scale((i64::from(v.value) + 2147483648) as u32, 32, 14) - 8192,
        )?)],
        Midi2ControlChange(v) => {
            if matches!(v.controller, 0 | 6 | 32 | 38 | 88 | 96..=101) {
                return Ok(report(
                    TranslationStatus::ContextRequired,
                    "this MIDI 2 CC is ignored or has different MIDI 1 sequence semantics",
                    group,
                    vec![],
                    allow_loss,
                ));
            }
            let value = if matches!(v.controller, 84 | 126) {
                i64::from(v.value >> 25)
            } else {
                scale(v.value, 32, 7)
            };
            vec![AnyMessage::ControlChange(m::ControlChange::new(
                v.channel.into(),
                v.controller.into(),
                value,
            )?)]
        }
        Midi2ProgramChange(v) => {
            let mut output = vec![];
            if v.options == 1 {
                output.push(AnyMessage::ControlChange(m::ControlChange::new(
                    v.channel.into(),
                    0,
                    v.bank_msb.into(),
                )?));
                output.push(AnyMessage::ControlChange(m::ControlChange::new(
                    v.channel.into(),
                    32,
                    v.bank_lsb.into(),
                )?));
            }
            output.push(AnyMessage::ProgramChange(m::ProgramChange::new(
                v.channel.into(),
                v.program.into(),
            )?));
            output
        }
        Midi2RegisteredController(v) if v.bank == 127 && v.index == 127 => {
            return Ok(report(
                TranslationStatus::Unsupported,
                "RPN null has no MIDI 1 Data Entry target",
                group,
                vec![],
                allow_loss,
            ))
        }
        Midi2RegisteredController(v) => {
            parameter_to_midi1(v.channel, v.bank, v.index, v.value, false, &mut lost)?
        }
        Midi2AssignableController(v) => {
            parameter_to_midi1(v.channel, v.bank, v.index, v.value, true, &mut lost)?
        }
        SysEx7(v) if v.format == 0 => vec![AnyMessage::SysEx(SysEx::new(v.data.clone())?)],
        SysEx7(_) => {
            return Ok(report(
                TranslationStatus::ContextRequired,
                "assemble SysEx7 before translating its payload",
                group,
                vec![],
                allow_loss,
            ))
        }
        Midi2RegisteredPerNoteController(_)
        | Midi2AssignablePerNoteController(_)
        | Midi2PerNoteManagement(_)
        | Midi2PerNotePitchBend(_) => return Ok(report(
            TranslationStatus::ProfileDependent,
            "no default MIDI 1 per-note equivalent; explicit alternate profile mapping required",
            group,
            vec![],
            allow_loss,
        )),
        _ => {
            return Ok(report(
                TranslationStatus::Unsupported,
                "no stateless MIDI 1 equivalent for this packet family",
                group,
                vec![],
                allow_loss,
            ))
        }
    };
    Ok(report(
        if lost {
            TranslationStatus::Quantized
        } else {
            TranslationStatus::Exact
        },
        if lost {
            "precision loss requires allow_loss=True; downscaling truncates low bits"
        } else {
            "values have an exact MIDI 1 representation"
        },
        group,
        output,
        allow_loss,
    ))
}
fn parameter_to_midi1(
    channel: u8,
    bank: u8,
    index: u8,
    value: u32,
    nrpn: bool,
    lost: &mut bool,
) -> Result<Vec<AnyMessage>, ValueError> {
    let data = if !nrpn && bank == 0 && matches!(index, 2 | 3 | 4 | 6) {
        (value >> 25) << 7
    } else {
        value >> 18
    };
    let restored = if !nrpn && bank == 0 && matches!(index, 0 | 2 | 3 | 4 | 6) {
        data << 18
    } else {
        up(data, 14, 32)
    };
    *lost |= restored != value;
    let (msb, lsb) = if nrpn { (99, 98) } else { (101, 100) };
    [
        (msb, bank),
        (lsb, index),
        (6, (data >> 7) as u8),
        (38, (data & 127) as u8),
    ]
    .into_iter()
    .map(|(c, v)| {
        Ok(AnyMessage::ControlChange(m::ControlChange::new(
            channel.into(),
            c,
            v.into(),
        )?))
    })
    .collect()
}
