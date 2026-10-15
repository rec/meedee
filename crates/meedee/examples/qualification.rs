//! Device-free, fixed-size release-build measurements. No timing assertions.
use meedee::codecs::{self, DecodeItem, Decoder};
use meedee::messages::{Command, ControlChange, NrpnSet, PrimitiveMessage, SysEx};
use std::hint::black_box;
use std::time::Instant;

fn measure(name: &str, operations: usize, mut action: impl FnMut()) {
    let samples: Vec<_> = (0..7)
        .map(|_| {
            let start = Instant::now();
            action();
            start.elapsed().as_nanos()
        })
        .collect();
    let total: u128 = samples.iter().sum();
    println!("{{\"measurement\":\"{name}\",\"operations_per_batch\":{operations},\"batch_wall_ns\":{samples:?},\"operations_per_second\":{}}}", operations as f64 * 7e9 / total as f64);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let message = ControlChange::new(1, 7, 127)?;
    let wire = message.encode().repeat(2048);
    measure("rust_cc_decode", 2048, || {
        let values = Decoder::default().feed(black_box(&wire));
        assert_eq!(values.len(), 2048);
        assert!(values.iter().all(|v| matches!(v, DecodeItem::Message(_))));
        black_box(values);
    });
    let command = NrpnSet::new(1, 1234, 12345, 14)?;
    measure("rust_nrpn_expand_encode", 2048, || {
        for _ in 0..2048 {
            let parts = black_box(&command).expand();
            assert_eq!(parts.len(), 6);
            assert_eq!(
                parts
                    .iter()
                    .map(|p| black_box(p.encode()).len())
                    .sum::<usize>(),
                18
            );
        }
    });
    for size in [1024, 65536, 1048576] {
        let sysex = SysEx::new(vec![0x7d; size])?;
        let encoded = codecs::encode(&sysex);
        measure(&format!("rust_sysex_{size}_bytes"), size, || {
            let mut decoder = Decoder::default();
            let mut count = 0;
            for chunk in black_box(&encoded).chunks(1024) {
                for item in decoder.feed(chunk) {
                    assert!(matches!(item, DecodeItem::Message(_)));
                    count += 1;
                    black_box(item);
                }
            }
            assert_eq!(count, 1);
            assert!(decoder.finish().is_empty());
        });
    }
    Ok(())
}
