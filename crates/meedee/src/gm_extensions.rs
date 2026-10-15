//! Universal GM2-related SysEx. Unknown valid identifiers remain raw numbers.
use crate::messages::{Message, PrimitiveMessage, SysEx, ValueError};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ControllerDestination {
    pub(crate) device_id: u8,
    pub(crate) channel: u8,
    pub(crate) source: u8,
    pub(crate) controller: Option<u8>,
    pub(crate) entries: Vec<(u8, u8)>,
}
impl ControllerDestination {
    pub fn new(
        device_id: i64,
        channel: i64,
        source: i64,
        controller: Option<i64>,
        entries: Vec<(i64, i64)>,
        max_entries: usize,
    ) -> Result<Self, ValueError> {
        let source = bounded(source, 1, 3)?;
        let controller =
            match (source, controller) {
                (3, Some(c)) if (1..=31).contains(&c) || (64..=95).contains(&c) => Some(c as u8),
                (1 | 2, None) => None,
                _ => return Err(ValueError(
                    "CC source requires controller 1..31 or 64..95; pressure sources require None"
                        .into(),
                )),
            };
        Ok(Self {
            device_id: bounded(device_id, 0, 127)?,
            channel: bounded(channel, 1, 16)?,
            source,
            controller,
            entries: pairs(entries, max_entries)?,
        })
    }
    pub fn device_id(&self) -> u8 {
        self.device_id
    }
    pub fn channel(&self) -> u8 {
        self.channel
    }
    pub fn source(&self) -> u8 {
        self.source
    }
    pub fn controller(&self) -> Option<u8> {
        self.controller
    }
    pub fn entries(&self) -> &[(u8, u8)] {
        &self.entries
    }
}
impl Message for ControllerDestination {}
impl PrimitiveMessage for ControllerDestination {
    fn encode(&self) -> Vec<u8> {
        let mut b = vec![240, 127, self.device_id, 9, self.source, self.channel - 1];
        b.extend(self.controller);
        append_pairs(&mut b, &self.entries);
        b.push(247);
        b
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyBasedControl {
    device_id: u8,
    channel: u8,
    note: u8,
    entries: Vec<(u8, u8)>,
}
impl KeyBasedControl {
    pub fn new(
        device_id: i64,
        channel: i64,
        note: i64,
        entries: Vec<(i64, i64)>,
        max_entries: usize,
    ) -> Result<Self, ValueError> {
        let entries = pairs(entries, max_entries)?;
        if entries
            .iter()
            .any(|&(c, _)| matches!(c,0|6|32|38|96..=101|122..=127))
        {
            return Err(ValueError(
                "controller is excluded by key-based instrument control".into(),
            ));
        }
        Ok(Self {
            device_id: bounded(device_id, 0, 127)?,
            channel: bounded(channel, 1, 16)?,
            note: bounded(note, 0, 127)?,
            entries,
        })
    }
    pub fn device_id(&self) -> u8 {
        self.device_id
    }
    pub fn channel(&self) -> u8 {
        self.channel
    }
    pub fn note(&self) -> u8 {
        self.note
    }
    pub fn entries(&self) -> &[(u8, u8)] {
        &self.entries
    }
}
impl Message for KeyBasedControl {}
impl PrimitiveMessage for KeyBasedControl {
    fn encode(&self) -> Vec<u8> {
        let mut b = vec![240, 127, self.device_id, 10, 1, self.channel - 1, self.note];
        append_pairs(&mut b, &self.entries);
        b.push(247);
        b
    }
}
/// IDs are big endian base-128; values are little endian base-128. Raw tuples
/// support all protocol widths without overflowing machine-sized integers.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GlobalParameterControl {
    device_id: u8,
    slot_path: Vec<u16>,
    parameter_width: u8,
    value_width: u8,
    entries: Vec<(Vec<u8>, Vec<u8>)>,
}
impl GlobalParameterControl {
    pub fn new(
        device_id: i64,
        slot_path: Vec<i64>,
        parameter_width: i64,
        value_width: i64,
        entries: Vec<(Vec<u8>, Vec<u8>)>,
        max_entries: usize,
    ) -> Result<Self, ValueError> {
        if max_entries == 0 || entries.len() > max_entries || slot_path.len() > 127 {
            return Err(ValueError(
                "global parameter entry/path limit exceeded".into(),
            ));
        }
        let pw = bounded(parameter_width, 1, 127)?;
        let vw = bounded(value_width, 1, 127)?;
        if entries.iter().any(|(p, v)| {
            p.len() != pw as usize || v.len() != vw as usize || p.iter().chain(v).any(|&b| b > 127)
        }) {
            return Err(ValueError(
                "parameter/value bytes must match their declared widths and be seven-bit".into(),
            ));
        }
        let slot_path = slot_path
            .into_iter()
            .map(|v| {
                if (0..=16383).contains(&v) {
                    Ok(v as u16)
                } else {
                    Err(ValueError("slot must be 0..16383".into()))
                }
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            device_id: bounded(device_id, 0, 127)?,
            slot_path,
            parameter_width: pw,
            value_width: vw,
            entries,
        })
    }
    pub fn device_id(&self) -> u8 {
        self.device_id
    }
    pub fn slot_path(&self) -> &[u16] {
        &self.slot_path
    }
    pub fn parameter_width(&self) -> u8 {
        self.parameter_width
    }
    pub fn value_width(&self) -> u8 {
        self.value_width
    }
    pub fn entries(&self) -> &[(Vec<u8>, Vec<u8>)] {
        &self.entries
    }
}
impl Message for GlobalParameterControl {}
impl PrimitiveMessage for GlobalParameterControl {
    fn encode(&self) -> Vec<u8> {
        let mut b = vec![
            240,
            127,
            self.device_id,
            4,
            5,
            self.slot_path.len() as u8,
            self.parameter_width,
            self.value_width,
        ];
        for &s in &self.slot_path {
            b.extend([(s >> 7) as u8, (s & 127) as u8]);
        }
        for (p, v) in &self.entries {
            b.extend(p);
            b.extend(v);
        }
        b.push(247);
        b
    }
}
/// GM2 effect slots: 257=reverb, 258=chorus. Unknown IDs/values stay generic.
pub fn reverb_parameter(
    device_id: i64,
    parameter: i64,
    value: i64,
) -> Result<GlobalParameterControl, ValueError> {
    effect_parameter(device_id, 257, parameter, value, 1)
}
pub fn chorus_parameter(
    device_id: i64,
    parameter: i64,
    value: i64,
) -> Result<GlobalParameterControl, ValueError> {
    effect_parameter(device_id, 258, parameter, value, 4)
}
fn effect_parameter(
    device_id: i64,
    slot: i64,
    parameter: i64,
    value: i64,
    last: i64,
) -> Result<GlobalParameterControl, ValueError> {
    let p = bounded(parameter, 0, last)?;
    let v = bounded(value, 0, 127)?;
    GlobalParameterControl::new(device_id, vec![slot], 1, 1, vec![(vec![p], vec![v])], 1024)
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ExtendedMessage {
    ControllerDestination(ControllerDestination),
    KeyBasedControl(KeyBasedControl),
    GlobalParameterControl(GlobalParameterControl),
}
pub fn decode_extended(
    message: &SysEx,
    max_entries: usize,
) -> Result<Option<ExtendedMessage>, ValueError> {
    let d = message.data();
    if d.len() < 4 || d[0] != 127 {
        return Ok(None);
    }
    let id = i64::from(d[1]);
    match (d[2], d[3]) {
        (9, 1..=3) => {
            let start = if d[3] == 3 { 6 } else { 5 };
            require(d, start)?;
            let controller = if start == 6 {
                Some(i64::from(d[5]))
            } else {
                None
            };
            Ok(Some(ExtendedMessage::ControllerDestination(
                ControllerDestination::new(
                    id,
                    i64::from(d[4]) + 1,
                    d[3].into(),
                    controller,
                    read_pairs(&d[start..], max_entries)?,
                    max_entries,
                )?,
            )))
        }
        (10, 1) => {
            require(d, 6)?;
            Ok(Some(ExtendedMessage::KeyBasedControl(
                KeyBasedControl::new(
                    id,
                    i64::from(d[4]) + 1,
                    d[5].into(),
                    read_pairs(&d[6..], max_entries)?,
                    max_entries,
                )?,
            )))
        }
        (4, 5) => {
            require(d, 7)?;
            let slots = usize::from(d[4]);
            let pw = usize::from(d[5]);
            let vw = usize::from(d[6]);
            let start = 7 + slots * 2;
            require(d, start)?;
            if pw == 0
                || vw == 0
                || !d[start..].chunks_exact(pw + vw).remainder().is_empty()
                || max_entries == 0
                || (d.len() - start) / (pw + vw) > max_entries
            {
                return Err(ValueError(
                    "invalid global parameter widths, length or entry limit".into(),
                ));
            }
            let path = d[7..start]
                .chunks_exact(2)
                .map(|v| (i64::from(v[0]) << 7) | i64::from(v[1]))
                .collect();
            let entries = d[start..]
                .chunks_exact(pw + vw)
                .map(|v| (v[..pw].to_vec(), v[pw..].to_vec()))
                .collect();
            Ok(Some(ExtendedMessage::GlobalParameterControl(
                GlobalParameterControl::new(id, path, pw as i64, vw as i64, entries, max_entries)?,
            )))
        }
        _ => Ok(None),
    }
}
fn require(d: &[u8], n: usize) -> Result<(), ValueError> {
    if d.len() < n {
        Err(ValueError("truncated profile SysEx".into()))
    } else {
        Ok(())
    }
}
fn bounded(v: i64, lo: i64, hi: i64) -> Result<u8, ValueError> {
    if (lo..=hi).contains(&v) {
        Ok(v as u8)
    } else {
        Err(ValueError(format!("value must be {lo}..{hi}")))
    }
}
fn pairs(entries: Vec<(i64, i64)>, limit: usize) -> Result<Vec<(u8, u8)>, ValueError> {
    if limit == 0 || entries.len() > limit {
        return Err(ValueError("entry limit exceeded".into()));
    }
    entries
        .into_iter()
        .map(|(p, v)| Ok((bounded(p, 0, 127)?, bounded(v, 0, 127)?)))
        .collect()
}
fn read_pairs(d: &[u8], limit: usize) -> Result<Vec<(i64, i64)>, ValueError> {
    if !d.chunks_exact(2).remainder().is_empty() || limit == 0 || d.len() / 2 > limit {
        return Err(ValueError("invalid pair length or entry limit".into()));
    }
    Ok(d.chunks_exact(2)
        .map(|v| (v[0].into(), v[1].into()))
        .collect())
}
fn append_pairs(b: &mut Vec<u8>, entries: &[(u8, u8)]) {
    for &(p, v) in entries {
        b.extend([p, v]);
    }
}
