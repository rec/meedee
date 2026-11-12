//! Private, versioned values for local worker pipes. No interpreter or serde dependency.
use std::io::{self, Read, Write};

pub(super) const MAGIC: &[u8; 8] = b"MDWORK\0\x01";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Value {
    Null,
    Bool(bool),
    Integer(i128),
    String(String),
    Bytes(Vec<u8>),
    Words(Vec<u32>),
    List(Vec<Value>),
    Tuple(Vec<Value>),
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(super) fn write(stream: &mut impl Write, value: &Value) -> io::Result<()> {
    match value {
        Value::Null => stream.write_all(&[0]),
        Value::Bool(value) => stream.write_all(&[1, u8::from(*value)]),
        Value::Integer(value) => {
            stream.write_all(&[2])?;
            stream.write_all(&value.to_be_bytes())
        }
        Value::String(value) => write_bytes(stream, 3, value.as_bytes()),
        Value::Bytes(value) => write_bytes(stream, 4, value),
        Value::Words(words) => {
            stream.write_all(&[7])?;
            write_len(stream, words.len())?;
            for word in words {
                stream.write_all(&word.to_be_bytes())?;
            }
            Ok(())
        }
        Value::List(values) | Value::Tuple(values) => {
            stream.write_all(&[if matches!(value, Value::List(_)) {
                5
            } else {
                6
            }])?;
            write_len(stream, values.len())?;
            for value in values {
                write(stream, value)?;
            }
            Ok(())
        }
    }
}

fn write_len(stream: &mut impl Write, length: usize) -> io::Result<()> {
    let length = u64::try_from(length).map_err(|_| invalid("worker value length exceeds u64"))?;
    stream.write_all(&length.to_be_bytes())
}
fn write_bytes(stream: &mut impl Write, tag: u8, bytes: &[u8]) -> io::Result<()> {
    stream.write_all(&[tag])?;
    write_len(stream, bytes.len())?;
    stream.write_all(bytes)
}
fn array<const N: usize>(stream: &mut impl Read) -> io::Result<[u8; N]> {
    let mut value = [0; N];
    stream.read_exact(&mut value)?;
    Ok(value)
}
fn length(stream: &mut impl Read) -> io::Result<usize> {
    usize::try_from(u64::from_be_bytes(array(stream)?))
        .map_err(|_| invalid("worker value exceeds platform size"))
}
fn bytes(stream: &mut impl Read) -> io::Result<Vec<u8>> {
    let size = length(stream)?;
    let mut value = Vec::new();
    value.try_reserve_exact(size).map_err(|_| {
        io::Error::new(io::ErrorKind::OutOfMemory, "worker value allocation failed")
    })?;
    value.resize(size, 0);
    stream.read_exact(&mut value)?;
    Ok(value)
}

pub(super) fn read(stream: &mut impl Read) -> io::Result<Value> {
    read_nested(stream, 0)
}
fn read_nested(stream: &mut impl Read, depth: usize) -> io::Result<Value> {
    if depth > 32 {
        return Err(invalid("worker value nesting exceeds protocol limit"));
    }
    match array::<1>(stream)?[0] {
        0 => Ok(Value::Null),
        1 => match array::<1>(stream)?[0] {
            0 => Ok(Value::Bool(false)),
            1 => Ok(Value::Bool(true)),
            _ => Err(invalid("invalid worker boolean")),
        },
        2 => Ok(Value::Integer(i128::from_be_bytes(array(stream)?))),
        3 => String::from_utf8(bytes(stream)?)
            .map(Value::String)
            .map_err(|_| invalid("worker string is not UTF-8")),
        4 => Ok(Value::Bytes(bytes(stream)?)),
        tag @ (5 | 6) => {
            let size = length(stream)?;
            let mut values = Vec::new();
            values.try_reserve_exact(size).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "worker sequence allocation failed",
                )
            })?;
            for _ in 0..size {
                values.push(read_nested(stream, depth + 1)?);
            }
            Ok(if tag == 5 {
                Value::List(values)
            } else {
                Value::Tuple(values)
            })
        }
        7 => {
            let size = length(stream)?;
            let mut words = Vec::new();
            words.try_reserve_exact(size).map_err(|_| {
                io::Error::new(io::ErrorKind::OutOfMemory, "worker words allocation failed")
            })?;
            for _ in 0..size {
                words.push(u32::from_be_bytes(array(stream)?));
            }
            Ok(Value::Words(words))
        }
        _ => Err(invalid("unknown worker value tag")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn values_round_trip_without_changing_packet_widths_or_signed_timestamps() {
        let value = Value::Tuple(vec![
            Value::Integer(-17),
            Value::Integer(u64::MAX.into()),
            Value::Bytes(vec![0, 255]),
            Value::Words(vec![0x40903c00, u32::MAX]),
            Value::String("ump".into()),
            Value::List(vec![Value::Bool(true), Value::Null]),
        ]);
        let mut bytes = Vec::new();
        write(&mut bytes, &value).unwrap();
        assert_eq!(read(&mut bytes.as_slice()).unwrap(), value);
        for cut in 0..bytes.len() {
            assert!(read(&mut &bytes[..cut]).is_err());
        }
    }
    #[test]
    fn malformed_values_fail_instead_of_becoming_requests() {
        for bytes in [vec![99], vec![1, 2], vec![3, 0, 0, 0, 0, 0, 0, 0, 1, 255]] {
            assert!(read(&mut bytes.as_slice()).is_err());
        }
        let mut nested = Value::Null;
        for _ in 0..34 {
            nested = Value::List(vec![nested]);
        }
        let mut bytes = Vec::new();
        write(&mut bytes, &nested).unwrap();
        assert!(read(&mut bytes.as_slice()).is_err());
    }
}
