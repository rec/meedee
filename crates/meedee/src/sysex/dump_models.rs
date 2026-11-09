//! Transfer envelopes only. Sources and limitations: doc/sysex-dumps.md.
use crate::drivers;
use crate::messages::SysEx;
use crate::midi;
use crate::universal::{self, UniversalMessage};

pub(super) struct Recipe {
    pub steps: Vec<Step>,
    pub verified: bool,
    model: &'static str,
    device: u8,
}
pub(super) struct Step {
    pub request: SysEx,
    prefix: Vec<u8>,
    address: Vec<u8>,
    length: Option<usize>,
    checksum: Checksum,
    rejection_prefix: Option<Vec<u8>>,
}
#[derive(Clone, Copy)]
enum Checksum {
    None,
    Yamaha,
    Roland,
}

pub(super) fn is_identity_reply(data: &[u8]) -> bool {
    data.first() == Some(&0x7e) && data.get(2..4) == Some(&[6, 2])
}

/// Exact identity signatures and address relationships from the model manuals.
/// Unknown identities never select a recipe by manufacturer alone.
pub(super) fn identify(
    message: &SysEx,
) -> Result<Option<(&'static str, Option<u8>)>, &'static str> {
    let data = message.data();
    // Rev2 Appendix E also documents a single revision byte. Recognize that
    // exact layout locally; keep the general universal decoder strict.
    if data.len() == 10 && is_identity_reply(data) && data.get(4..9) == Some(&[1, 0x2f, 1, 0, 0]) {
        return Ok(Some(("sequential/prophet-rev2", None)));
    }
    let Some(UniversalMessage::IdentityReply(reply)) = universal::decode(message)
        .map_err(|_| "malformed identity reply; specify model explicitly")?
    else {
        return Ok(None);
    };
    let id = reply.device_id();
    match (reply.manufacturer(), reply.family(), reply.model()) {
        ([0x43], 0x2080, 0x010c) if id <= 15 => Ok(Some(("yamaha/vl70-m", Some(id)))),
        ([0x41], 0x01d1, 0) if id == 0x10 => Ok(Some(("roland/vt-4", Some(id)))),
        ([0x42], 0x00d1, 0) if id <= 15 => Ok(Some(("korg/minilogue-xd", Some(id)))),
        ([1], 0x00af, 0) => Ok(Some(("sequential/prophet-rev2", None))),
        _ => Ok(None),
    }
}

pub(super) fn recipe(
    model: &str,
    scope: &str,
    device: Option<u8>,
    remote: bool,
) -> midi::Result<Recipe> {
    let model = MODELS
        .iter()
        .copied()
        .find(|m| *m == model)
        .ok_or_else(|| drivers::invalid("unknown SysEx dump model; see sysex.models()"))?;
    let default = if model == "roland/vt-4" { 0x10 } else { 0 };
    let device = device.unwrap_or(default);
    let valid_device = match model {
        "roland/vt-4" => device == 0x10,
        "yamaha/vl70-m" | "korg/minilogue-xd" | "behringer/deepmind-12" => device <= 15,
        _ => device == 0,
    };
    if !valid_device {
        return Err(drivers::invalid(
            "device_id is outside this model's documented wire addresses",
        ));
    }
    let mut value = Recipe {
        steps: vec![],
        verified: true,
        model,
        device,
    };
    if !remote {
        if scope != "all" {
            return Err(drivers::invalid(
                "manual capture uses scope=all; select the dump on the instrument",
            ));
        }
        return Ok(value);
    }
    if !matches!(scope, "all" | "current" | "system") {
        return Err(drivers::invalid(
            "dump scope must be all, current or system",
        ));
    }
    match model {
        "yamaha/vl70-m" => {
            if scope != "current" {
                value.steps.push(yamaha(device, [0, 0, 0], 0x18));
            }
            if scope == "current" {
                value.steps.push(yamaha(device, [0x10, 0, 0], 0x0f));
                value.steps.push(yamaha(device, [0x20, 0, 0], 0x56b));
            }
            if scope == "all" {
                for n in 0..6 {
                    value.steps.push(yamaha(device, [0x30, 0, n], 0x56b));
                }
                for n in 0..64 {
                    value.steps.push(yamaha(device, [0x40, 0, n], 0xa3));
                }
            }
        }
        "roland/vt-4" => {
            if scope != "current" {
                value.steps.push(roland([0, 0, 0, 0], 0x10));
            }
            if scope == "current" || scope == "all" {
                for (base, size, count) in [
                    (0x10, 0x20, 8),
                    (0x20, 0x10, 4),
                    (0x30, 0x20, 4),
                    (0x40, 0x10, 4),
                    (0x50, 0x10, 4),
                    (0x60, 0x10, 4),
                ] {
                    value.steps.push(roland([base, 0, 0, 0], size));
                    if scope == "all" {
                        for n in 0..count {
                            value.steps.push(roland([base + 1, n, 0, 0], size));
                        }
                    }
                }
            }
        }
        "korg/minilogue-xd" => {
            // The published byte counts disagree with its parameter tables. Do
            // not guess a payload size or claim verified completion.
            value.verified = false;
            if scope == "all" {
                return Err(drivers::invalid("minilogue xd automatic scope=all is unavailable: published program-address widths conflict; use current/system or manual capture"));
            }
            let (request, reply) = if scope == "system" {
                (0x0e, 0x51)
            } else {
                (0x10, 0x40)
            };
            value.steps.push(korg(device, request, reply));
        }
        "sequential/prophet-rev2" => {
            let (request, reply, size) = match scope {
                "current" => (6,3,2339), "system" => (0x0e,0x0f,50),
                _ => return Err(drivers::invalid("Rev2 automatic scope=all is unavailable: published bank/program ranges conflict; use current/system or manual capture")),
            };
            value.steps.push(step(
                vec![1, 0x2f, request],
                vec![1, 0x2f, reply],
                vec![],
                Some(size),
                Checksum::None,
            ));
        }
        "behringer/deepmind-12" => {
            let prefix = vec![0, 0x20, 0x32, 0x20, device];
            if scope != "current" {
                value.steps.push(deepmind(&prefix, 5, 6, &[], 56));
            }
            if scope == "current" {
                value.steps.push(deepmind(&prefix, 3, 4, &[], 278));
            }
            if scope == "all" {
                for b in 0..8 {
                    for p in 0..128 {
                        value.steps.push(deepmind(&prefix, 1, 2, &[b, p], 278));
                    }
                }
            }
        }
        _ => {
            return Err(drivers::invalid(
                "this model currently supports manual dump capture only; omit output",
            ))
        }
    }
    Ok(value)
}
impl Recipe {
    pub fn manual_matches(&self, data: &[u8]) -> bool {
        match self.model {
            "yamaha/vl70-m" => {
                data.len() >= 3
                    && data[0] == 0x43
                    && data[1] == self.device
                    && matches!(data[2], 0x57 | 0x4c)
            }
            "roland/vt-4" => data.starts_with(&[0x41, self.device, 0, 0, 0, 0x51, 0x12]),
            "korg/minilogue-xd" => {
                data.len() >= 6
                    && data.starts_with(&[0x42, 0x30 | self.device, 0, 1, 0x51])
                    && matches!(data[5], 0x40 | 0x4c | 0x51 | 0x44 | 0x45)
            }
            "sequential/prophet-rev2" => {
                data.len() >= 3 && data.starts_with(&[1, 0x2f]) && matches!(data[2], 2 | 3 | 0x0f)
            }
            "behringer/deepmind-12" => {
                data.len() >= 6
                    && data.starts_with(&[0, 0x20, 0x32, 0x20, self.device])
                    && matches!(data[5], 2 | 4 | 6)
            }
            "novation/peak" => data.starts_with(&[0, 0x20, 0x29]),
            "elektron/digitakt" => data.starts_with(&[0, 0x20, 0x3c]),
            "moog/subsequent-37" => data.starts_with(&[4]),
            _ => false,
        }
    }
}
impl Step {
    pub fn matches(&self, data: &[u8]) -> bool {
        if self
            .rejection_prefix
            .as_ref()
            .is_some_and(|p| data.starts_with(p))
        {
            return true;
        }
        let offset = self.prefix.len()
            + if matches!(self.checksum, Checksum::Yamaha) {
                2
            } else {
                0
            };
        data.starts_with(&self.prefix)
            && data.get(offset..offset + self.address.len()) == Some(self.address.as_slice())
    }
    pub fn validate(&self, data: &[u8]) -> Result<(), &'static str> {
        if self
            .rejection_prefix
            .as_ref()
            .is_some_and(|p| data.starts_with(p))
        {
            return Err("instrument rejected the dump request");
        }
        if self.length.is_some_and(|n| data.len() != n)
            || data.len() <= self.prefix.len() + self.address.len()
        {
            return Err("dump reply length does not match the documented envelope");
        }
        let start = match self.checksum {
            Checksum::None => return Ok(()),
            Checksum::Yamaha => 3,
            Checksum::Roland => 7,
        };
        if matches!(self.checksum, Checksum::Yamaha)
            && (usize::from(data[3]) * 128 + usize::from(data[4]) != data.len() - 9)
        {
            return Err("Yamaha dump byte count does not match its payload");
        }
        if data[start..].iter().fold(0u8, |a, b| a.wrapping_add(*b)) & 127 != 0 {
            return Err("dump reply checksum mismatch");
        }
        Ok(())
    }
}
fn step(
    request: Vec<u8>,
    prefix: Vec<u8>,
    address: Vec<u8>,
    size: Option<usize>,
    checksum: Checksum,
) -> Step {
    let length = size.map(|n| {
        prefix.len()
            + address.len()
            + n
            + match checksum {
                Checksum::None => 0,
                Checksum::Yamaha => 3,
                Checksum::Roland => 1,
            }
    });
    Step {
        request: SysEx::new(request).expect("documented seven-bit request"),
        prefix,
        address,
        length,
        checksum,
        rejection_prefix: None,
    }
}
fn yamaha(device: u8, address: [u8; 3], size: usize) -> Step {
    let mut request = vec![0x43, 0x20 | device, 0x57];
    request.extend(address);
    step(
        request,
        vec![0x43, device, 0x57],
        address.to_vec(),
        Some(size),
        Checksum::Yamaha,
    )
}
fn roland(address: [u8; 4], size: u8) -> Step {
    let mut request = vec![0x41, 0x10, 0, 0, 0, 0x51, 0x11];
    request.extend(address);
    request.extend([0, 0, 0, size]);
    let sum = request[7..].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    request.push(0u8.wrapping_sub(sum) & 127);
    step(
        request,
        vec![0x41, 0x10, 0, 0, 0, 0x51, 0x12],
        address.to_vec(),
        Some(size as usize),
        Checksum::Roland,
    )
}
fn korg(device: u8, command: u8, reply: u8) -> Step {
    let request = vec![0x42, 0x30 | device, 0, 1, 0x51, command];
    let mut value = step(
        request,
        vec![0x42, 0x30 | device, 0, 1, 0x51, reply],
        vec![],
        None,
        Checksum::None,
    );
    value.rejection_prefix = Some(vec![0x42, 0x30 | device, 0, 1, 0x51, 0x24]);
    value
}
fn deepmind(prefix: &[u8], command: u8, reply: u8, program: &[u8], size: usize) -> Step {
    let mut request = prefix.to_vec();
    request.push(command);
    request.extend(program);
    let mut response = prefix.to_vec();
    response.extend([reply, 6]);
    step(
        request,
        response,
        program.to_vec(),
        Some(size),
        Checksum::None,
    )
}

pub(super) const MODELS: &[&str] = &[
    "yamaha/vl70-m",
    "roland/vt-4",
    "korg/minilogue-xd",
    "sequential/prophet-rev2",
    "novation/peak",
    "elektron/digitakt",
    "moog/subsequent-37",
    "behringer/deepmind-12",
];
