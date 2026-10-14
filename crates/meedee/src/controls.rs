//! Optional observations of one source's primitive stream. Never replaces raw
//! messages or infers physical units, lost messages, or unknown absolute values.

use crate::messages::{AnyMessage, ControlChange};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParameterFamily {
    Rpn,
    Nrpn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StepDirection {
    Increment,
    Decrement,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ControlObservation {
    pub channel: u8,
    pub controller: u8,
    pub value: u16,
    pub resolution: u8,
    pub first_index: usize,
    pub last_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ParameterObservation {
    pub channel: u8,
    pub family: ParameterFamily,
    pub parameter: u16,
    pub value: u16,
    pub resolution: u8,
    pub first_index: usize,
    pub last_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ParameterStep {
    pub channel: u8,
    pub family: ParameterFamily,
    pub parameter: u16,
    pub direction: StepDirection,
    pub first_index: usize,
    pub last_index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticKind {
    MissingCoarse,
    MissingSelection,
    IncompleteSelection,
    SelectionInvalidated,
    StateReset,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub channel: Option<u8>,
    pub controller: Option<u8>,
    pub first_index: usize,
    pub last_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Observation {
    Control(ControlObservation),
    Parameter(ParameterObservation),
    Step(ParameterStep),
    Diagnostic(Diagnostic),
}

/// Minimal primitive-stream input. Other advances the index without copying an
/// unrelated payload, including a potentially large SysEx. From<&AnyMessage>
/// preserves the normal Rust stream interface.
pub enum InputMessage {
    ControlChange(ControlChange),
    SystemReset,
    Other,
}

impl From<&AnyMessage> for InputMessage {
    fn from(message: &AnyMessage) -> Self {
        match message {
            AnyMessage::ControlChange(value) => Self::ControlChange(value.clone()),
            AnyMessage::SystemReset(_) => Self::SystemReset,
            _ => Self::Other,
        }
    }
}

/// Fixed bounded state for 16 channels of one source. No timers, source registry
/// or parameter-value cache. Input indexes saturate at usize::MAX.
#[derive(Default)]
pub struct Interpreter {
    channels: [Channel; 16],
    next_index: usize,
}

impl Interpreter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, message: impl Into<InputMessage>) -> Vec<Observation> {
        let index = self.next_index;
        self.next_index = self.next_index.saturating_add(1);
        match message.into() {
            InputMessage::Other => Vec::new(),
            InputMessage::SystemReset => self.clear(None, index),
            InputMessage::ControlChange(message) => {
                let channel = message.channel();
                if message.controller() == 121 {
                    return self.clear(Some(channel), index);
                }
                self.channels[usize::from(channel - 1)].feed(&message, index)
            }
        }
    }

    /// Known loss/reconnect invalidates all channels. A boundary diagnostic uses
    /// the next input index, matching codec finish's end-position convention.
    pub fn reset(&mut self) -> Vec<Observation> {
        self.clear(None, self.next_index)
    }

    fn clear(&mut self, channel: Option<u8>, index: usize) -> Vec<Observation> {
        if let Some(channel) = channel {
            self.channels[usize::from(channel - 1)] = Channel::default();
        } else {
            self.channels = Default::default();
        }
        vec![Observation::Diagnostic(Diagnostic {
            kind: DiagnosticKind::StateReset,
            channel,
            controller: None,
            first_index: index,
            last_index: index,
        })]
    }
}

#[derive(Clone, Copy)]
struct IndexedByte {
    value: u8,
    index: usize,
}

#[derive(Default)]
struct Channel {
    coarse: [Option<IndexedByte>; 32],
    family: Option<ParameterFamily>,
    selector_msb: Option<IndexedByte>,
    selector_lsb: Option<IndexedByte>,
    data: Option<IndexedByte>,
}

impl Channel {
    fn feed(&mut self, message: &ControlChange, index: usize) -> Vec<Observation> {
        let channel = message.channel();
        let controller = message.controller();
        let value = message.value();
        let byte = IndexedByte { value, index };
        match controller {
            98..=101 => {
                let family = if controller >= 100 {
                    ParameterFamily::Rpn
                } else {
                    ParameterFamily::Nrpn
                };
                let mut result = Vec::new();
                // A completed selection never donates a half to its successor.
                if self.family.is_some()
                    && (self.family != Some(family) || self.selected().is_some())
                {
                    let first = self.selection_start(index);
                    self.selector_msb = None;
                    self.selector_lsb = None;
                    result.push(diagnostic(
                        DiagnosticKind::SelectionInvalidated,
                        channel,
                        controller,
                        first,
                        index,
                    ));
                }
                self.family = Some(family);
                self.data = None;
                if controller == 99 || controller == 101 {
                    self.selector_msb = Some(byte);
                } else {
                    self.selector_lsb = Some(byte);
                }
                if self.selected() == Some((ParameterFamily::Rpn, 16383)) {
                    self.family = None;
                    self.selector_msb = None;
                    self.selector_lsb = None;
                }
                result
            }
            6 | 38 | 96 | 97 => {
                let Some((family, parameter)) = self.selected() else {
                    let kind = if self.family.is_some() {
                        DiagnosticKind::IncompleteSelection
                    } else {
                        DiagnosticKind::MissingSelection
                    };
                    return vec![diagnostic(
                        kind,
                        channel,
                        controller,
                        self.selection_start(index),
                        index,
                    )];
                };
                let first_index = self.selection_start(index);
                if controller == 96 || controller == 97 {
                    // Relative changes can cross a coarse boundary. A later LSB
                    // must not combine with the stale pre-step coarse value.
                    self.data = None;
                    return vec![Observation::Step(ParameterStep {
                        channel,
                        family,
                        parameter,
                        direction: if controller == 96 {
                            StepDirection::Increment
                        } else {
                            StepDirection::Decrement
                        },
                        first_index,
                        last_index: index,
                    })];
                }
                let (value, resolution) = if controller == 6 {
                    self.data = Some(byte);
                    (u16::from(value), 7)
                } else if let Some(coarse) = self.data {
                    ((u16::from(coarse.value) << 7) | u16::from(value), 14)
                } else {
                    return vec![diagnostic(
                        DiagnosticKind::MissingCoarse,
                        channel,
                        controller,
                        first_index,
                        index,
                    )];
                };
                vec![Observation::Parameter(ParameterObservation {
                    channel,
                    family,
                    parameter,
                    value,
                    resolution,
                    first_index,
                    last_index: index,
                })]
            }
            // Bank Select is not a generic knob. Data Entry is handled above.
            1..=31 => {
                self.coarse[usize::from(controller)] = Some(byte);
                vec![Observation::Control(ControlObservation {
                    channel,
                    controller,
                    value: value.into(),
                    resolution: 7,
                    first_index: index,
                    last_index: index,
                })]
            }
            33..=63 => {
                let base = controller - 32;
                let Some(coarse) = self.coarse[usize::from(base)] else {
                    return vec![diagnostic(
                        DiagnosticKind::MissingCoarse,
                        channel,
                        controller,
                        index,
                        index,
                    )];
                };
                vec![Observation::Control(ControlObservation {
                    channel,
                    controller: base,
                    value: (u16::from(coarse.value) << 7) | u16::from(value),
                    resolution: 14,
                    first_index: coarse.index,
                    last_index: index,
                })]
            }
            _ => Vec::new(),
        }
    }

    fn selected(&self) -> Option<(ParameterFamily, u16)> {
        Some((
            self.family?,
            (u16::from(self.selector_msb?.value) << 7) | u16::from(self.selector_lsb?.value),
        ))
    }

    fn selection_start(&self, fallback: usize) -> usize {
        [self.selector_msb, self.selector_lsb]
            .into_iter()
            .flatten()
            .map(|b| b.index)
            .min()
            .unwrap_or(fallback)
    }
}

fn diagnostic(
    kind: DiagnosticKind,
    channel: u8,
    controller: u8,
    first_index: usize,
    last_index: usize,
) -> Observation {
    Observation::Diagnostic(Diagnostic {
        kind,
        channel: Some(channel),
        controller: Some(controller),
        first_index,
        last_index,
    })
}
