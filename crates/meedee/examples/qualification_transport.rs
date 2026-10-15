//! Opt-in native loopback. Run through the supervised qualification test.
use meedee::transport::{Input, InputOptions, Output, OutputOptions, Packet};
use std::sync::mpsc::sync_channel;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let name = format!(
        "meedee-qualification-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    let (sender, receiver) = sync_channel(128);
    let start = Instant::now();
    let input = Input::new(InputOptions::default())?.create_virtual(&name, move |value| {
        // Qualification callbacks never block the native thread.
        let _ = sender.try_send(value);
    })?;
    let client = Output::new(OutputOptions::default())?;
    let matches: Vec<_> = client
        .ports()?
        .into_iter()
        .filter(|p| p.name() == name)
        .collect();
    if matches.len() != 1 {
        return Err("test-owned endpoint was not discovered uniquely".into());
    }
    let output = client.connect(&matches[0])?;
    let opened = start.elapsed().as_nanos();
    let mut samples = Vec::with_capacity(256);
    for n in 0..256 {
        let value = Packet::Bytes(vec![0xb0, 7, (n % 128) as u8]);
        let start = Instant::now();
        output.send(&value)?;
        let received = receiver.recv_timeout(Duration::from_secs(2))?;
        assert_eq!(received.packet, value);
        assert_eq!(received.sequence, n);
        samples.push(start.elapsed().as_nanos());
    }
    drop(output);
    drop(input);
    println!("{{\"measurement\":\"rust_native_roundtrip\",\"open_wall_ns\":{opened},\"count\":256,\"samples_ns\":{samples:?},\"timing\":\"host send entry to receive return\"}}");
    Ok(())
}
