//! No MIDI endpoints: the real subprocess client talks to a controlled native peer.
use meedee::{midi, transport, Input, NoteOn, Output, SubprocessOptions};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
static SERIAL: Mutex<()> = Mutex::new(());
fn fixture() -> PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let directory = project.join("../../target/subprocess-tests");
            std::fs::create_dir_all(&directory).unwrap();
            let path = directory.join(format!(
                "worker-{}{}",
                std::process::id(),
                std::env::consts::EXE_SUFFIX
            ));
            assert!(std::process::Command::new("rustc")
                .args(["--edition=2021", "-O"])
                .arg(project.join("tests/support/subprocess_worker.rs"))
                .arg("-o")
                .arg(&path)
                .status()
                .unwrap()
                .success());
            path
        })
        .clone()
}
fn options(mode: &str) -> SubprocessOptions {
    SubprocessOptions {
        program: fixture(),
        args: vec![mode.into()],
        timeout: Duration::from_secs(1),
    }
}
#[test]
fn high_level_input_and_output_use_the_shared_worker_without_python() {
    let _serial = SERIAL.lock().unwrap();
    let mut output = Output::new()
        .with_run_in_subprocess(true)
        .unwrap()
        .with_subprocess_options(options("healthy"))
        .unwrap();
    assert_eq!(output.send(Vec::<NoteOn>::new()).unwrap().sent_events, 0);
    let ports = output.ports().unwrap();
    assert_eq!(ports[0].name(), "output port");
    let mut output = output.connect(&ports[0]).unwrap();
    output.send(NoteOn::new(60).unwrap()).unwrap();
    output.close().unwrap();
    assert!(output.closed());
    let mut input = Input::new()
        .with_run_in_subprocess(true)
        .unwrap()
        .with_subprocess_options(options("early"))
        .unwrap();
    let ports = input.ports().unwrap();
    let mut input = input.connect(&ports[0]).unwrap();
    let message = input.receive(Some(Duration::from_secs(1)), None).unwrap();
    assert!(message.is_some());
    input.close().unwrap();
    assert!(input.closed());
}
#[test]
fn initialization_send_close_and_child_crash_have_bounded_failure() {
    let _serial = SERIAL.lock().unwrap();
    let start = Instant::now();
    let mut output = Output::new()
        .with_run_in_subprocess(true)
        .unwrap()
        .with_subprocess_options(options("hang_init"))
        .unwrap();
    assert!(matches!(
        output.ports(),
        Err(midi::Error::Init(transport::Error::Timeout(_)))
    ));
    assert!(start.elapsed() < Duration::from_secs(4));
    for mode in ["hang_send", "crash_send", "native_error", "hang_close"] {
        let client = transport::Output::new_subprocess(Default::default(), options(mode)).unwrap();
        let ports = client.ports().unwrap();
        let connection = client.connect(&ports[0]).unwrap();
        let start = Instant::now();
        if mode == "hang_close" {
            assert!(matches!(
                connection.close(),
                Err(transport::Error::Timeout(_))
            ));
        } else {
            let error = connection
                .send(&transport::Packet::Bytes(vec![0xf8]))
                .unwrap_err();
            assert!(matches!(
                error,
                transport::Error::Timeout(_)
                    | transport::Error::Worker(_)
                    | transport::Error::Native(_)
            ));
            assert!(connection.worker_failure().is_some());
        }
        assert!(start.elapsed() < Duration::from_secs(4));
    }
}
#[test]
fn failures_reap_workers_without_retaining_process_capacity() {
    let _serial = SERIAL.lock().unwrap();
    let mut failures = Vec::new();
    for _ in 0..40 {
        let client =
            transport::Output::new_subprocess(Default::default(), options("crash_send")).unwrap();
        let ports = client.ports().unwrap();
        let connection = client.connect(&ports[0]).unwrap();
        assert!(connection
            .send(&transport::Packet::Bytes(vec![0xf8]))
            .is_err());
        failures.push(connection);
    }
}
#[test]
fn worker_limit_and_spawn_failure_keep_distinct_resource_and_worker_categories() {
    let _serial = SERIAL.lock().unwrap();
    for _ in 0..35 {
        let mut missing = options("healthy");
        missing.program = fixture().with_extension("missing");
        assert!(matches!(
            transport::Output::new_subprocess(Default::default(), missing),
            Err(transport::Error::Worker(_))
        ));
    }
    let mut clients = Vec::new();
    for _ in 0..32 {
        clients.push(
            transport::Output::new_subprocess(Default::default(), options("healthy")).unwrap(),
        );
    }
    assert!(matches!(
        transport::Output::new_subprocess(Default::default(), options("healthy")),
        Err(transport::Error::Resource(_))
    ));
    drop(clients);
}

#[path = "../src/transport/subprocess/wire.rs"]
mod wire;
#[test]
fn standalone_worker_can_start_and_exit_without_creating_a_native_client() {
    use std::io::{Read, Write};
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_meedee-worker"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(wire::MAGIC).unwrap();
    wire::write(
        &mut input,
        &wire::Value::Tuple(vec![
            wire::Value::String("shutdown".into()),
            wire::Value::List(vec![]),
        ]),
    )
    .unwrap();
    input.flush().unwrap();
    let mut output = child.stdout.take().unwrap();
    let mut magic = [0; 8];
    output.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, wire::MAGIC);
    assert_eq!(
        wire::read(&mut output).unwrap(),
        wire::Value::Tuple(vec![
            wire::Value::String("shutdown".into()),
            wire::Value::Tuple(vec![wire::Value::Null, wire::Value::Integer(0)])
        ])
    );
    drop(input);
    assert!(child.wait().unwrap().success());
}

#[test]
fn bounded_forwarding_retains_the_prefix_and_combines_loss_during_silence() {
    let _serial = SERIAL.lock().unwrap();
    let (packets, received) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let client = transport::Input::new_subprocess(
        transport::InputOptions {
            receive_byte_limit: 2,
            ..Default::default()
        },
        options("pressure"),
    )
    .unwrap();
    let ports = client.ports().unwrap();
    let connection = client
        .connect(&ports[0], move |packet| {
            let first = packet.sequence == 0;
            packets.send(packet).unwrap();
            if first {
                wait.recv_timeout(Duration::from_secs(2)).unwrap();
            }
        })
        .unwrap();
    let first = received.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!((first.sequence, first.timestamp_ns), (0, Some(17)));
    assert_eq!(connection.dropped_packets(), 8);
    release.send(()).unwrap();
    let last = received.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!((last.sequence, last.timestamp_ns), (2, None));
    let diagnostics = connection.diagnostics();
    connection.close().unwrap();
    assert_eq!(diagnostics.dropped_packets(), 8);
}
