//! Named factories for setup and endpoint packets; no discovery transaction/I/O.
use super::*;
pub fn tempo(group: i64, ten_nanoseconds_per_quarter: i64) -> Result<FlexData, ValueError> {
    let value = bounded(
        ten_nanoseconds_per_quarter,
        0,
        4294967295,
        "ten_nanoseconds_per_quarter",
    )? as u32;
    FlexData::new(group, 0, None, 0, 0, vec![value, 0, 0])
}
pub fn time_signature(
    group: i64,
    numerator: i64,
    denominator_power: i64,
    thirtyseconds_per_quarter: i64,
) -> Result<FlexData, ValueError> {
    let n = bounded(numerator, 1, 256, "numerator")? as u32 & 255;
    let d = bounded(denominator_power, 0, 255, "denominator_power")? as u32;
    let b = bounded(
        thirtyseconds_per_quarter,
        0,
        255,
        "thirtyseconds_per_quarter",
    )? as u32;
    FlexData::new(group, 0, None, 0, 1, vec![n << 24 | d << 16 | b << 8, 0, 0])
}
#[allow(clippy::too_many_arguments)]
pub fn metronome(
    group: i64,
    clocks_per_primary: i64,
    accent1: i64,
    accent2: i64,
    accent3: i64,
    subdivision1: i64,
    subdivision2: i64,
) -> Result<FlexData, ValueError> {
    let mut b = [0u8; 12];
    for (i, v) in [
        clocks_per_primary,
        accent1,
        accent2,
        accent3,
        subdivision1,
        subdivision2,
    ]
    .into_iter()
    .enumerate()
    {
        b[i] = bounded(v, 0, 255, "metronome byte")? as u8;
    }
    FlexData::new(group, 0, None, 0, 2, bytes_words(&b))
}
pub fn key_signature(
    group: i64,
    channel: Option<i64>,
    sharps_flats: i64,
    tonic: i64,
) -> Result<FlexData, ValueError> {
    let sf = bounded(sharps_flats, -8, 7, "sharps_flats")? as u32 & 15;
    let tonic = bounded(tonic, 0, 7, "tonic")? as u32;
    FlexData::new(group, 0, channel, 0, 5, vec![(sf << 4 | tonic) << 24, 0, 0])
}
pub fn endpoint_discovery(
    major: i64,
    minor: i64,
    filter: i64,
) -> Result<StreamMessage, ValueError> {
    let mut d = vec![0; 14];
    d[0] = bounded(major, 0, 255, "major")? as u8;
    d[1] = bounded(minor, 0, 255, "minor")? as u8;
    d[5] = bounded(filter, 0, 31, "filter")? as u8;
    StreamMessage::new(0, 0, d)
}
#[allow(clippy::too_many_arguments)]
pub fn endpoint_info(
    major: i64,
    minor: i64,
    static_blocks: bool,
    function_blocks: i64,
    midi1: bool,
    midi2: bool,
    rx_jr: bool,
    tx_jr: bool,
) -> Result<StreamMessage, ValueError> {
    let mut d = vec![0; 14];
    d[0] = bounded(major, 0, 255, "major")? as u8;
    d[1] = bounded(minor, 0, 255, "minor")? as u8;
    d[2] = u8::from(static_blocks) << 7 | bounded(function_blocks, 0, 32, "function_blocks")? as u8;
    d[4] = u8::from(midi2) << 1 | u8::from(midi1);
    d[5] = u8::from(rx_jr) << 1 | u8::from(tx_jr);
    StreamMessage::new(0, 1, d)
}
pub fn device_identity(
    manufacturer: Vec<u8>,
    family: i64,
    model: i64,
    revision: Vec<u8>,
) -> Result<StreamMessage, ValueError> {
    if manufacturer.len() != 3
        || revision.len() != 4
        || manufacturer.iter().chain(&revision).any(|&v| v > 127)
        || manufacturer[0] != 0 && (manufacturer[1] != 0 || manufacturer[2] != 0)
    {
        return Err(ValueError("identity requires three manufacturer bytes (single-byte ID followed by zeros) and four revision bytes".into()));
    }
    let family = bounded(family, 0, 16383, "family")? as u16;
    let model = bounded(model, 0, 16383, "model")? as u16;
    let mut d = vec![0; 14];
    d[3..6].copy_from_slice(&manufacturer);
    d[6..10].copy_from_slice(&[
        (family & 127) as u8,
        (family >> 7) as u8,
        (model & 127) as u8,
        (model >> 7) as u8,
    ]);
    d[10..].copy_from_slice(&revision);
    StreamMessage::new(0, 2, d)
}
pub fn stream_configuration(
    request: bool,
    protocol: i64,
    rx_jr: bool,
    tx_jr: bool,
) -> Result<StreamMessage, ValueError> {
    let mut d = vec![0; 14];
    d[0] = bounded(protocol, 1, 2, "protocol")? as u8;
    d[1] = u8::from(rx_jr) << 1 | u8::from(tx_jr);
    StreamMessage::new(0, if request { 5 } else { 6 }, d)
}
pub fn function_block_discovery(block: i64, filter: i64) -> Result<StreamMessage, ValueError> {
    let mut d = vec![0; 14];
    d[0] = bounded(block, 0, 255, "block")? as u8;
    d[1] = bounded(filter, 0, 3, "filter")? as u8;
    StreamMessage::new(0, 16, d)
}
#[allow(clippy::too_many_arguments)]
pub fn function_block_info(
    active: bool,
    block: i64,
    ui_hint: i64,
    midi1_port: i64,
    direction: i64,
    first_group: i64,
    groups: i64,
    midi_ci_version: i64,
    sysex8_streams: i64,
) -> Result<StreamMessage, ValueError> {
    let mut d = vec![0; 14];
    d[0] = u8::from(active) << 7 | bounded(block, 0, 31, "block")? as u8;
    d[1] = (bounded(ui_hint, 0, 3, "ui_hint")? as u8) << 4
        | (bounded(midi1_port, 0, 2, "midi1_port")? as u8) << 2
        | bounded(direction, 1, 3, "direction")? as u8;
    d[2] = bounded(first_group, 1, 16, "first_group")? as u8 - 1;
    d[3] = bounded(groups, 1, 16, "groups")? as u8;
    d[4] = bounded(midi_ci_version, 0, 255, "midi_ci_version")? as u8;
    d[5] = bounded(sysex8_streams, 0, 255, "sysex8_streams")? as u8;
    StreamMessage::new(0, 17, d)
}
pub fn start_of_clip() -> StreamMessage {
    StreamMessage::new(0, 32, vec![0; 14]).expect("constant clip marker")
}
pub fn end_of_clip() -> StreamMessage {
    StreamMessage::new(0, 33, vec![0; 14]).expect("constant clip marker")
}
