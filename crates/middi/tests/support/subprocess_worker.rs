//! Device-free peer for the real Rust subprocess client, including controlled faults.
#[path = "../../src/transport/subprocess/wire.rs"]
mod wire;
use std::io::{self, Read, Write};
use wire::Value;
fn tuple(v: Value) -> Vec<Value> {
    match v {
        Value::Tuple(v) | Value::List(v) => v,
        _ => panic!("expected sequence"),
    }
}
fn n(v: &Value) -> i128 {
    match v {
        Value::Integer(v) => *v,
        _ => panic!("expected number"),
    }
}
fn s(v: &Value) -> &str {
    match v {
        Value::String(v) => v,
        _ => panic!("expected string"),
    }
}
fn i(v: impl Into<i128>) -> Value {
    Value::Integer(v.into())
}
fn text(v: impl Into<String>) -> Value {
    Value::String(v.into())
}
fn send(tag: &str, payload: Value) {
    let mut out = io::stdout().lock();
    wire::write(&mut out, &Value::Tuple(vec![text(tag), payload])).unwrap();
    out.flush().unwrap();
}
fn ok(tag: &str, value: Value, dropped: u64) {
    send(tag, Value::Tuple(vec![value, i(dropped)]));
}
fn error(kind: &str, message: &str, fatal: bool, dropped: u64) {
    send(
        "error",
        Value::Tuple(vec![
            Value::Tuple(vec![text(kind), text(message)]),
            Value::Bool(fatal),
            i(dropped),
        ]),
    );
}
fn hang() -> ! {
    loop {
        std::thread::park();
    }
}
fn main() {
    let mode = std::env::args()
        .nth(1)
        .filter(|v| v != "-m")
        .or_else(|| std::env::var("MIDDI_TEST_MODE").ok())
        .unwrap_or_else(|| "healthy".into());
    let mut input = false;
    let mut ump = false;
    let mut timestamps = true;
    let mut snapshot = 0;
    let mut dropped = 0;
    let mut source = io::stdin();
    let mut magic = [0; 8];
    source.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, wire::MAGIC);
    io::stdout().write_all(wire::MAGIC).unwrap();
    io::stdout().flush().unwrap();
    loop {
        let request = match wire::read(&mut source) {
            Ok(v) => tuple(v),
            Err(_) => return,
        };
        let operation = s(&request[0]);
        let args = tuple(request[1].clone());
        match operation {
            "init" => {
                if mode == "hang_init" {
                    hang();
                }
                input = args[0] == Value::Bool(true);
                ump = n(&args[1]) >= 0x1000;
                timestamps = args[2] == Value::Bool(true);
                ok("ok", Value::Null, 0);
            }
            "ports" => {
                snapshot += 1;
                if mode == "discovery_error_once" && snapshot == 1 {
                    error(
                        "native",
                        "endpoint disappeared during metadata read",
                        false,
                        0,
                    );
                    continue;
                }
                let name = if input { "input port" } else { "output port" };
                let count = if mode == "ambiguous" { 2 } else { 1 };
                ok(
                    "ok",
                    Value::List(
                        (0..count)
                            .map(|_| Value::Tuple(vec![text(name), i(0)]))
                            .collect(),
                    ),
                    dropped,
                );
            }
            "open" => {
                if mode == "failed_open" {
                    error("native", "device vanished during connect", true, 0);
                    return;
                }
                let generation = n(&args[2]);
                if mode == "pressure" {
                    for (sequence, data, timestamp) in [
                        (0, vec![0xf8], Some(17)),
                        (1, vec![0x90, 60], Some(18)),
                        (2, vec![0xf8], None),
                    ] {
                        send(
                            "message",
                            Value::Tuple(vec![
                                i(generation),
                                i(sequence),
                                timestamp.map_or(Value::Null, i),
                                Value::Bytes(data),
                                i(7),
                            ]),
                        );
                    }
                    dropped = 7;
                }
                if matches!(
                    mode.as_str(),
                    "early" | "early_negative" | "early_absent" | "twice" | "burst"
                ) {
                    let data = if ump {
                        Value::Words(vec![0x40903c00, 0xc0000000])
                    } else {
                        Value::Bytes(vec![0x90, 60, 127])
                    };
                    send(
                        "message",
                        Value::Tuple(vec![
                            i(generation),
                            i(0),
                            if !timestamps || mode == "early_absent" {
                                Value::Null
                            } else if mode == "early_negative" {
                                i(i64::MIN)
                            } else {
                                i(17)
                            },
                            data,
                            i(0),
                        ]),
                    );
                }
                if mode == "twice" {
                    send(
                        "message",
                        Value::Tuple(vec![
                            i(generation),
                            i(1),
                            i(18),
                            Value::Bytes(vec![0xf8]),
                            i(0),
                        ]),
                    );
                }
                if mode == "burst" {
                    for index in 1..2049 {
                        send(
                            "message",
                            Value::Tuple(vec![
                                i(generation),
                                i(index),
                                i(index),
                                Value::Bytes(vec![0xf8]),
                                i(0),
                            ]),
                        );
                    }
                }
                ok("ok", Value::Null, dropped);
                if mode == "native_drop" || mode == "native_drop_idle" {
                    dropped = 7;
                    send("dropped", Value::Tuple(vec![i(generation), i(dropped)]));
                    if mode == "native_drop" {
                        send(
                            "message",
                            Value::Tuple(vec![
                                i(generation),
                                i(7),
                                i(17),
                                Value::Bytes(vec![0xf8]),
                                i(dropped),
                            ]),
                        );
                    }
                }
            }
            "send" | "send_command" => {
                if mode == "hang_send" {
                    hang();
                }
                if mode == "crash_send" {
                    std::process::exit(19);
                }
                if mode == "native_error" {
                    error("native", "device disconnected", true, dropped);
                    return;
                }
                let packets = tuple(args[0].clone());
                if mode.starts_with("partial_command_") {
                    let completed = mode.rsplit('_').next().unwrap().parse::<u64>().unwrap();
                    let cause = Value::Tuple(vec![text("native"), text("device disconnected")]);
                    let error = Value::Tuple(vec![
                        text("compound"),
                        Value::Tuple(vec![i(completed), i(packets.len() as u64), cause]),
                    ]);
                    send(
                        "error",
                        Value::Tuple(vec![error, Value::Bool(true), i(dropped)]),
                    );
                    return;
                }
                if mode == "check_bytes"
                    && packets != vec![Value::Bytes(vec![0xf0, 0x7d, 0, 0xff, 0xf7])]
                {
                    error("native", "bytes changed in transport", true, dropped);
                    return;
                }
                if mode == "check_ump" && packets != vec![Value::Words(vec![0x40903c00, u32::MAX])]
                {
                    error("native", "UMP words changed in transport", true, dropped);
                    return;
                }
                let expected: Option<&[u8]> = match mode.as_str() {
                    "check_command" => Some(&[
                        0xb0, 0x63, 9, 0xb0, 0x62, 0x52, 0xb0, 6, 0x4e, 0xb0, 0x26, 0x10, 0xb0,
                        0x65, 0x7f, 0xb0, 0x64, 0x7f,
                    ]),
                    "check_bank" => Some(&[0xbf, 0, 0x79, 0xbf, 0x20, 3, 0xcf, 0x7f]),
                    "check_mpe" => Some(&[
                        0xbf, 0x65, 0, 0xbf, 0x64, 6, 0xbf, 6, 15, 0xbf, 0x65, 0x7f, 0xbf, 0x64,
                        0x7f,
                    ]),
                    "check_note" if operation == "send_command" => {
                        Some(&[0xb0, 7, 1, 0xb0, 0x27, 0])
                    }
                    _ => None,
                };
                if let Some(expected) = expected {
                    let bytes =
                        packets
                            .iter()
                            .try_fold(Vec::new(), |mut result, packet| match packet {
                                Value::Bytes(bytes) => {
                                    result.extend_from_slice(bytes);
                                    Some(result)
                                }
                                _ => None,
                            });
                    if bytes.as_deref() != Some(expected) {
                        error("native", "command changed in transport", true, dropped);
                        return;
                    }
                }
                if mode == "check_note"
                    && operation == "send"
                    && !matches!(packets.as_slice(),[Value::Bytes(v)] if v==&[0x90,60,100]||v==&[0x80,60,0])
                {
                    error("native", "note bytes changed in transport", true, dropped);
                    return;
                }
                ok("ok", Value::Null, dropped);
            }
            "close" | "shutdown" => {
                if mode == "hang_close" {
                    hang();
                }
                ok(
                    if operation == "shutdown" {
                        "shutdown"
                    } else {
                        "ok"
                    },
                    Value::Null,
                    dropped,
                );
                if operation == "shutdown" {
                    return;
                }
            }
            _ => {
                error("invalid", "unknown fixture operation", false, dropped);
            }
        }
    }
}
