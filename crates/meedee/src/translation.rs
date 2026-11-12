//! Explicit stateless translation. Compound MIDI 1 input needs caller context;
//! no bank, selector, profile, source or routing state is guessed.
use crate::events::Payload;
use crate::messages as m;
use crate::messages::{AnyMessage, ValueError};
use crate::ump::PacketValue;
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
pub fn to_midi2(message: &AnyMessage, group: i64) -> Result<Translation<AnyMessage>, ValueError> {
    if message.version() != 1 {
        return Err(ValueError("to_midi2 requires MIDI 1 input".into()));
    }
    crate::messages::checked_group(group)?;
    let g = Some(group as u8);
    let value = match message {
        AnyMessage::NoteOn(v) if v.velocity() == 0 => AnyMessage::NoteOff(
            crate::messages::NoteOff::new_midi2(v.note().into())
                .and_then(|m| m.with_group(group))
                .and_then(|m| m.with_channel(v.resolved_channel()?.into()))
                .and_then(|m| m.with_velocity(32768))
                .and_then(|m| m.with_attribute_type(0))
                .and_then(|m| m.with_attribute_data(0))?,
        ),
        AnyMessage::NoteOn(v) => AnyMessage::NoteOn(
            crate::messages::NoteOn::new_midi2(v.note().into())
                .and_then(|m| m.with_group(group))
                .and_then(|m| m.with_channel(v.resolved_channel()?.into()))
                .and_then(|m| m.with_velocity(up(v.velocity().into(), 7, 16).into()))
                .and_then(|m| m.with_attribute_type(0))
                .and_then(|m| m.with_attribute_data(0))?,
        ),
        AnyMessage::NoteOff(v) => AnyMessage::NoteOff(
            crate::messages::NoteOff::new_midi2(v.note().into())
                .and_then(|m| m.with_group(group))
                .and_then(|m| m.with_channel(v.resolved_channel()?.into()))
                .and_then(|m| m.with_velocity(up(v.velocity().into(), 7, 16).into()))
                .and_then(|m| m.with_attribute_type(0))
                .and_then(|m| m.with_attribute_data(0))?,
        ),
        AnyMessage::PolyPressure(v) => AnyMessage::PolyPressure(
            crate::messages::PolyPressure::new_midi2(v.note().into())
                .and_then(|m| m.with_group(group))
                .and_then(|m| m.with_channel(v.resolved_channel()?.into()))
                .and_then(|m| m.with_pressure(up(v.pressure(), 7, 32).into()))?,
        ),
        AnyMessage::ChannelPressure(v) => AnyMessage::ChannelPressure(
            crate::messages::ChannelPressure::new_midi2()
                .and_then(|m| m.with_group(group))
                .and_then(|m| m.with_channel(v.resolved_channel()?.into()))
                .and_then(|m| m.with_pressure(up(v.pressure(), 7, 32).into()))?,
        ),
        AnyMessage::PitchBend(v) => AnyMessage::PitchBend(
            crate::messages::PitchBend::new_midi2()
                .and_then(|m| m.with_group(group))
                .and_then(|m| m.with_channel(v.resolved_channel()?.into()))
                .and_then(|m| {
                    m.with_value(i64::from(up((v.value() + 8192) as u32, 14, 32)) - 2147483648)
                })?,
        ),
        AnyMessage::ProgramChange(v) => {
            let mut value = crate::messages::ProgramChange::new_midi2(v.program().into())
                .and_then(|m| m.with_group(group))
                .and_then(|m| m.with_channel(v.resolved_channel()?.into()))?;
            if let (Some(msb), Some(lsb)) = (v.bank_msb(), v.bank_lsb()) {
                value = value.with_bank(msb.into(), lsb.into())?;
            }
            AnyMessage::ProgramChange(value)
        }
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
                v.value() << 25
            } else {
                up(v.value(), v.resolution(), 32)
            };
            AnyMessage::ControlChange(
                crate::messages::ControlChange::new_midi2(v.controller().into(), value.into())
                    .and_then(|m| m.with_group(group))
                    .and_then(|m| m.with_channel(v.resolved_channel()?.into()))?,
            )
        }
        AnyMessage::SysEx(v) => {
            return Ok(report(
                TranslationStatus::Exact,
                "SysEx preserves all payload bytes",
                g,
                vec![AnyMessage::SysEx(v.clone().with_group(group)?)],
                false,
            ))
        }
        _ => message.clone().with_group(group)?,
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
        crate::messages::inherit_metadata(message, vec![value])?,
        false,
    ))
}
pub fn to_midi1(
    message: &Payload,
    allow_loss: bool,
) -> Result<Translation<AnyMessage>, ValueError> {
    use AnyMessage::*;
    let (message, group) = match message {
        Payload::Message(v) if v.version() == 1 => {
            return Ok(report(
                TranslationStatus::Exact,
                "native MIDI 1 values retained",
                v.group(),
                vec![v.clone().without_group()?],
                false,
            ));
        }
        Payload::Message(v) => (v, v.group()),
        Payload::Ump(PacketValue::SysEx7(v)) if v.format() == 0 => {
            return Ok(report(
                TranslationStatus::Exact,
                "complete SysEx7 payload retained",
                Some(v.group()),
                crate::messages::inherit_metadata(
                    v,
                    vec![AnyMessage::SysEx(m::SysEx::new(v.data().to_vec())?)],
                )?,
                false,
            ));
        }
        Payload::Ump(PacketValue::SysEx7(v)) => {
            return Ok(report(
                TranslationStatus::ContextRequired,
                "assemble SysEx7 before translating its payload",
                Some(v.group()),
                vec![],
                allow_loss,
            ));
        }
        _ => {
            return Ok(report(
                TranslationStatus::Unsupported,
                "no stateless MIDI 1 equivalent for this packet family",
                None,
                vec![],
                allow_loss,
            ))
        }
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
        NoteOn(v) => {
            if v.attribute_type().unwrap() != 0 {
                return Ok(report(
                    TranslationStatus::ProfileDependent,
                    "note attributes require explicit profile/manufacturer interpretation",
                    group,
                    vec![],
                    allow_loss,
                ));
            }
            let velocity = scale(v.velocity().into(), 16, 7);
            let velocity = if velocity == 0 {
                lost = true;
                1
            } else {
                velocity
            };
            vec![AnyMessage::NoteOn(
                m::NoteOn::new(v.note().into())
                    .and_then(|m| m.with_velocity(velocity))
                    .and_then(|m| m.with_channel(v.resolved_channel()?.into()))?,
            )]
        }
        NoteOff(v) => {
            if v.attribute_type().unwrap() != 0 {
                return Ok(report(
                    TranslationStatus::ProfileDependent,
                    "note attributes require explicit profile/manufacturer interpretation",
                    group,
                    vec![],
                    allow_loss,
                ));
            }
            vec![AnyMessage::NoteOff(
                m::NoteOff::new(v.note().into())
                    .and_then(|m| m.with_velocity(scale(v.velocity().into(), 16, 7)))
                    .and_then(|m| m.with_channel(v.resolved_channel()?.into()))?,
            )]
        }
        PolyPressure(v) => vec![AnyMessage::PolyPressure(
            m::PolyPressure::new(v.note().into())
                .and_then(|m| m.with_pressure(scale(v.pressure(), 32, 7)))
                .and_then(|m| m.with_channel(v.resolved_channel()?.into()))?,
        )],
        ChannelPressure(v) => vec![AnyMessage::ChannelPressure(
            m::ChannelPressure::new()
                .and_then(|m| m.with_pressure(scale(v.pressure(), 32, 7)))
                .and_then(|m| m.with_channel(v.resolved_channel()?.into()))?,
        )],
        PitchBend(v) => vec![AnyMessage::PitchBend(
            m::PitchBend::new()
                .and_then(|m| {
                    m.with_value(scale((i64::from(v.value()) + 2147483648) as u32, 32, 14) - 8192)
                })
                .and_then(|m| m.with_channel(v.resolved_channel()?.into()))?,
        )],
        ControlChange(v) => {
            if matches!(v.controller(), 0 | 6 | 32 | 38 | 88 | 96..=101) {
                return Ok(report(
                    TranslationStatus::ContextRequired,
                    "this MIDI 2 CC is ignored or has different MIDI 1 sequence semantics",
                    group,
                    vec![],
                    allow_loss,
                ));
            }
            let value = if matches!(v.controller(), 84 | 126) {
                i64::from(v.value() >> 25)
            } else {
                scale(v.value(), 32, 7)
            };
            vec![AnyMessage::ControlChange(
                m::ControlChange::new(v.controller().into(), value)
                    .and_then(|m| m.with_channel(v.resolved_channel()?.into()))?,
            )]
        }
        ProgramChange(v) => {
            let mut output = vec![];
            if v.bank_msb().is_some() {
                output.push(AnyMessage::ControlChange(
                    m::ControlChange::new(0, v.bank_msb().unwrap().into())
                        .and_then(|m| m.with_channel(v.resolved_channel()?.into()))?,
                ));
                output.push(AnyMessage::ControlChange(
                    m::ControlChange::new(32, v.bank_lsb().unwrap().into())
                        .and_then(|m| m.with_channel(v.resolved_channel()?.into()))?,
                ));
            }
            output.push(AnyMessage::ProgramChange(
                m::ProgramChange::new(v.program().into())
                    .and_then(|m| m.with_channel(v.resolved_channel()?.into()))?,
            ));
            output
        }
        ParameterController(v)
            if v.family() == m::ControllerFamily::Registered
                && v.bank() == 127
                && v.index() == 127 =>
        {
            return Ok(report(
                TranslationStatus::Unsupported,
                "RPN null has no MIDI 1 Data Entry target",
                group,
                vec![],
                allow_loss,
            ))
        }
        ParameterController(v) => parameter_to_midi1(
            v.channel(),
            v.bank(),
            v.index(),
            v.value(),
            v.family() == m::ControllerFamily::Assignable,
            &mut lost,
        )?,
        PerNoteController(_) | PerNoteManagement(_) | PerNotePitchBend(_) => return Ok(report(
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
    let output = crate::messages::inherit_metadata(message, output)?;
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
        Ok(AnyMessage::ControlChange(
            m::ControlChange::new(c, v.into()).and_then(|m| m.with_channel(channel.into()))?,
        ))
    })
    .collect()
}
