# Practical Rust MIDI examples

These examples use the `meedee` Rust crate. Each block is an independent function
body; put it inside a function returning `Result<(), Box<dyn std::error::Error>>`
and finish with `Ok(())`. The `?` operator propagates a checked error.

Both Rust and Python construct Input/Output lazily; first discovery or I/O
initializes the resource. Empty playback opens no ports.

Live examples select the first available MIDI port. Connect a keyboard or synth
before running them. MIDI carries messages, not audio. Channels are 1..16, notes
and velocities are 0..127, and program numbers are 1..128.

## 1. Play one note

```rust
use meedee::messages::Note;
use meedee::midi::Output;
use std::time::Duration;

Output::new().send(Note::new(Duration::from_millis(500), 60)?)?;
```

This plays middle C for half a second, with velocity 90 on channel 1. `Note`
sends the attack and release. The temporary output closes when the statement ends.

## 2. Play a tune in beats

```rust
use meedee::messages::{Beats, Note, Rest};
use meedee::midi::Output;

let mut out = Output::new().with_bpm(120)?;
out.send_all([
    Note::new(Beats::from(1), 60)?.into(),
    Note::new(Beats::new(1, 2)?, 62)?.with_velocity(70)?.into(),
    Note::new(Beats::new(1, 2)?, 64)?.with_velocity(100)?.into(),
    Rest::new(Beats::from(1)).into(),
    Note::new(Beats::from(2), 64)?.into(),
])?;
out.close()?;
```

One beat is a quarter note; `Beats::new(1, 2)` is half a beat. Each note or rest
advances the sequence. `Duration` always means elapsed time, even with a tempo;
`Beats` requires a tempo. Clocks and scheduling are automatic. `.into()` lets
notes, rests, messages and delays share one sequence.

`Output::new().with_port("FLOW 8")?` selects a unique name substring; shell patterns
support `*`, `?`, `[abc]`, `[a-z]` and `[!abc]`, just as in Python. Empty patterns
are rejected. To choose from discovery, use:

```rust
use meedee::midi::Output;

let mut output = Output::new();
let ports = output.ports()?;
println!("{ports:?}");
let mut out = output.connect(&ports[0])?;
out.close()?;
```

This discovery example assumes there is at least one output. `Input` supports
the same selection methods. The low-level `transport` module remains available
for virtual ports, callbacks and backend-specific settings. Closing is terminal;
create a new object to reconnect. Python `connect()` returns the same object; Rust
`connect()` consumes and returns the resource to express ownership.

## 3. Print incoming messages

```rust
use meedee::midi::Input;

for message in Input::new() {
    println!("{message:?}");
}
```

Input yields messages, never full `Note` values. Malformed traffic and loss are
reported on stderr; decoding continues. Terminal connection errors stop the
iterator. `with_channel(1)?` or `with_channels([1, 3])?` filters channel messages;
system messages and diagnostics still pass through.

## 4. Transpose a keyboard

```rust
use meedee::midi::{Input, Output};
use meedee::events::Payload;
use meedee::messages::AnyMessage;

let mut out = Output::new();
for message in Input::new() {
    match message {
        Payload::Midi1(AnyMessage::NoteOn(note)) => out.send(note.transpose(12))?,
        Payload::Midi1(AnyMessage::NoteOff(note)) => out.send(note.transpose(12))?,
        other => out.send(other)?,
    }
}
out.close()?;
```

`transpose` clamps to MIDI's note range and preserves timing/source metadata.
`Payload` distinguishes MIDI 1, UMP and compound commands; `AnyMessage` identifies
the MIDI 1 message kind. These are enums holding the messages themselves, not
separate timing wrappers.

## 5. Split a keyboard between two sounds

```rust
use meedee::midi::{Input, Output};
use meedee::events::Payload;
use meedee::messages::{AnyMessage, ProgramChange};

let mut low = Output::new().with_channel(2)?;
let mut high = Output::new().with_channel(3)?;
low.send(ProgramChange::new(34)?)?;
high.send(ProgramChange::new(81)?)?;

for message in Input::new().with_channel(1)? {
    let key = match &message {
        Payload::Midi1(AnyMessage::NoteOn(note)) => Some(note.note()),
        Payload::Midi1(AnyMessage::NoteOff(note)) => Some(note.note()),
        _ => None,
    };
    match key {
        Some(note) if note < 60 => low.send(message)?,
        Some(_) => high.send(message)?,
        None if matches!(&message, Payload::Midi1(
            AnyMessage::ControlChange(_) | AnyMessage::PolyPressure(_) |
            AnyMessage::ChannelPressure(_) | AnyMessage::ProgramChange(_) |
            AnyMessage::PitchBend(_)
        )) => {
            low.send(message.clone())?;
            high.send(message)?;
        }
        None => low.send(message)?,
    }
}
low.close()?;
high.close()?;
```

System messages pass through the input filter and are forwarded once.
An explicitly configured output channel overrides a MIDI 1 message's channel.
Without that setting, an output preserves assigned channels and supplies channel
1 only for unassigned messages. To send several channels through one output,
leave its channel unset and use message builders such as `.with_channel(2)?`.

## 6. Record and replay

```rust
use meedee::midi::{Input, Output};
use std::time::Duration;

Output::new().play(Input::new().record(Duration::from_secs(10))?)?;
```

Recording closes the input and returns a `Recording` containing declarations,
messages and any receive problems. Replay infers its origin from the first message.
To retain the result, bind it to a variable and print the whole recording.

To save and read the capture:

```rust
use meedee::midi::{Input, Output};
use meedee::archive::Reader;
use std::time::Duration;

Input::new().record_to(Duration::from_secs(10), "rec.meedee", true)?;
let report = Output::new().play(Reader::open("rec.meedee")?)?;
println!("{report:?}");
```

The final `true` allows overwriting; use `false` to preserve existing files.
Recording retains the capture in memory before writing, so memory use grows with
traffic. A disk-write failure can leave a partial file, rejected by the strict
reader. Receive errors remain visible as gap/discontinuity records; a structurally
complete file does not imply a loss-free performance.

Replay stops at a gap, discontinuity, malformed record, timestamp regression or
unmapped clock. Its error includes confirmed progress. Explicit clock mappings,
rounding permission, cancellation and metrics are configured with `Output::with_playback`.

## 7. Handle input diagnostics explicitly

```rust
use meedee::midi::Input;
use meedee::events::InputItem;

let mut input = Input::new();
for item in input.with_errors() {
    match item? {
        InputItem::Message(message) => println!("{message:?}"),
        InputItem::Diagnostic(diagnostic) => eprintln!("{diagnostic:?}"),
    }
}
```

The native callback feeds a bounded raw-packet queue. Decoding happens on the
calling thread and resets partial state after loss. Queue overflow is reported
after the accepted prefix, even if the sender goes silent. Closing also reports
remaining native loss and unfinished messages. `with_receive_limits` configures
queue and SysEx limits; `with_transport` configures backend admission separately.

## 8. Overlap notes and send a compound command

```rust
use meedee::midi::Output;
use meedee::messages::{NoteOn, RpnSet};
use std::time::Duration;

let mut out = Output::new();
out.send(RpnSet::new(0, 2 << 7)?)?; // Pitch-bend sensitivity: two semitones.
out.send_all([
    NoteOn::new(60)?.into(),
    Duration::from_millis(300).into(),
    NoteOn::new(64)?.into(),
    Duration::from_millis(300).into(),
    NoteOn::new(67)?.into(),
    Duration::from_secs(1).into(),
])?;
out.close()?;
```

These `NoteOn` messages overlap until close releases them. Compound commands
expand internally. The complete sequence is validated before its first send.

Close releases tracked MIDI 1 note counts and resets changed pitch bend,
modulation, sustain, sostenuto, soft pedal and channel/polyphonic pressure.
Cleanup sends note-offs first, sorted by channel and note, then controller resets
in the same deterministic order as Python.
Volume, pan, expression, programs, tuning and other parameters are left alone.
The pitch-bend sensitivity set above therefore remains in effect. `Drop` performs
best-effort cleanup; explicit `close()` returns cleanup errors. Use channels owned
by this connection: cleanup can affect another writer sharing a channel, and a
failed transport can prevent cleanup.

## 9. Preserve provenance while editing a message

```rust
use meedee::midi::{Input, Output};
use meedee::events::Payload;
use meedee::messages::{AnyMessage, Message};

let mut out = Output::new();
for message in Input::new() {
    if let Payload::Midi1(AnyMessage::NoteOn(note)) = message {
        let edited = note.with_velocity(70)?.transpose(12);
        println!("{:?}", edited.metadata());
        out.send(edited)?;
        break;
    }
}
out.close()?;
```

The `Message` trait exposes optional timestamp, source and packet-span metadata.
Builders and transposition preserve it. Equality and hashing compare musical
content, ignoring provenance; archive records compare recorded occurrences.
Encoding a channel message directly requires an assigned channel. `Output`
resolves an unassigned channel for you.

## 10. Send native MIDI 2 messages

```rust
use meedee::midi::Output;
use meedee::transport::{OutputOptions, Protocol};
use meedee::ump::{Midi2NoteOff, Midi2NoteOn};
use std::time::Duration;

let mut out = Output::new().with_transport(OutputOptions {
    protocol: Protocol::Ump,
    ..OutputOptions::default()
})?;
out.send_all([
    Midi2NoteOn::new(1, 1, 60, 40_000, 0, 0)?.into(),
    Duration::from_millis(500).into(),
    Midi2NoteOff::new(1, 1, 60, 0, 0, 0)?.into(),
])?;
out.close()?;
```

This needs a native UMP backend and endpoint. Group and channel are explicit;
MIDI 2 velocity uses its native 16-bit range. There is no automatic protocol
translation, full-note scheduling or cleanup for UMP. Raw packets likewise retain
their own addressing. The `ump` module provides explicit translation when needed.

## 11. Inspect and compare saved captures

These operations open files, not MIDI ports. Reports keep capture quality separate
from structural completeness.

```rust
use meedee::inspection::{self, ComparisonOptions, Format, Options};

println!("{:?}", inspection::inspect_file("rec.meedee", Format::Archive, &Options::default())?);
println!("{:?}", inspection::compare_files(
    "rec.meedee", "other.meedee", Format::Archive, &ComparisonOptions::default(),
)?);
```

For playback diagnostics, configure the output once. The options apply to each
subsequent `play()` call; ordinary playback needs no options.

```rust
use meedee::archive::Reader;
use meedee::midi::{Output, PlaybackOptions};

let mut out = Output::new().with_playback(PlaybackOptions {
    metrics: true,
    ..Default::default()
});
println!("{:?}", out.play(Reader::open("rec.meedee")?)?);
out.close()?;
```

`PlaybackOptions` also accepts an origin timestamp, clock mappings, rounding
permission and a shared `events::Cancellation`. Inspection and metrics snapshots
expose read-only getters with the same names as Python properties.

## 12. Capture and restore a SysEx dump

Initiate a one-message dump on the instrument within ten seconds. The capture
retains complete messages, then creates a new backup file only after success.

```rust
use meedee::{midi::Input, transfer, transfer_input};
use std::fs::OpenOptions;

let mut input = Input::new();
let mut capture = transfer::Capture::new(
    input.stream()?.into(), 1, 10_000_000_000, 8_388_608, 1_048_576,
)?;
let report = transfer_input::run(&mut capture, &mut input, None)?;
input.close()?;
println!("{report:?}");
if report.status == "completed" {
    capture.write(&mut OpenOptions::new().write(true).create_new(true).open("backup.syx")?)?;
}
```

Run this separately to restore the backup. The first output must be the intended
instrument, ready to receive it. Sending a dump can replace sounds or settings;
choose the gap from the device's documentation.

```rust
use meedee::{midi::Output, transfer, transfer_output};
use std::time::Duration;

let plan = transfer::read_file("backup.syx", Duration::from_millis(100), &Default::default())?;
let mut session = transfer::Session::new(plan);
let mut output = Output::new();
println!("{:?}", transfer_output::run(&mut session, &mut output, None)?);
output.close()?;
```

## 13. Query a tuning dump

This asks device 0 for tuning program 2 through the first input/output pair.
The final report includes a matching reply, timeout or failure with known progress.

```rust
use meedee::{midi::{Input, Output}, tuning, tuning_live};

let mut input = Input::new();
let mut output = Output::new();
let mut query = tuning::Query::new(
    tuning::BulkDumpRequest::new(0, 2)?.into(), input.stream()?.into(), 5_000_000_000,
)?;
println!("{:?}", tuning_live::run(&mut query, &mut input, &mut output, None)?);
input.close()?;
output.close()?;
```

Live drivers require fresh sessions and own their monotonic clocks. They borrow
connections; the caller owns closing them. Failures never trigger automatic retries.
High-level I/O errors use `midi::Error`; native categories and error causes remain
available for inspection instead of being flattened into strings.

## 14. Observe endpoint changes

Each call performs one complete discovery scan. A failed or oversized scan stops
the poller and retains its last complete snapshot.

```rust
use meedee::{discovery::Poller, midi::Protocol};

let mut poller = Poller::new(Protocol::Midi1)?;
println!("{:?}", poller.poll()?);
// Call poll() again when the application wants another observation.
poller.close();
```

Python additionally provides process isolation, CLI commands, unit strings and
uFor integration. Rust uses typed durations, builders, enums and `Result` values;
its `transport` and `file_io` modules expose lower-level native and streaming
primitives. Corresponding workflows otherwise use the same public module names.

## 15. Discover MIDI-CI devices

This uses the experimental manufacturer identity 0x7D and a caller-selected local
MUID. Choose a fresh unused MUID for a real scan. Python's example generates one
with `secrets`; Rust leaves that choice to the application's random-number source.

```rust
use meedee::{ci, ci_live, midi::{Input, Output}};

let mut input = Input::new();
let mut output = Output::new();
let identity = ci::Identity::new(vec![0x7d, 0, 0], 1, 1, vec![0; 4])?;
let route = ci::Route::new(input.stream()?.into(), "device".into(), None)?;
let session = ci::Session::new(12345, identity, route, 0, None, 1024, 128, 128, 8_388_608)?;
let mut browser = ci::Browser::new(session, 3_000_000_000, 1_000_000_000, 30_000_000_000, 128)?;
println!("{:?}", ci_live::run(&mut browser, &mut input, &mut output, &ci_live::Options::default())?);
input.close()?;
output.close()?;
```

`ci_live::Options` sets cancellation and SysEx assembly limits, matching Python's
keyword options. Set both transports to UMP and give the route an explicit group
for SysEx7. This scan sends Discovery and endpoint inquiries only; it performs no
Profiles, Property Exchange or instrument configuration writes.

## 16. Export a just-intonation tuning

Create `just-three.syx` with keys 69, 70 and 71 tuned to 440, 660 and 880 Hz.
Unselected keys are preserved. This uses the same `bulk_dump` builder as Python;
Rust supplies frequencies directly instead of using Python's optional uFor adapter.

```rust
use meedee::{messages::PrimitiveMessage, tuning};
use std::{fs::OpenOptions, io::Write};

let pitches = [(69, 440.0), (70, 660.0), (71, 880.0)]
    .into_iter()
    .map(|(note, hz)| {
        tuning::NoteTuning::new(
            note, tuning::TuningValue::Pitch(tuning::Pitch::from_frequency(hz)?.pitch),
        )
    })
    .collect::<Result<Vec<_>, _>>()?;
let dump = tuning::bulk_dump(&pitches, 0, 2, "Just three".into(), None, true)?;
OpenOptions::new().write(true).create_new(true).open("just-three.syx")?
    .write_all(&dump.encode()?)?;
println!("{dump:?}");
```

For a transfer plan with a 20 ms gap, use
`tuning::bulk_plan(&pitches, 0, 2, "Just three".into(), 20_000_000, None, true)?`.
For individual real-time changes, use
`tuning::note_changes(&pitches, 0, 2, true, None, false)?`.
These builders do not open ports or send messages. `None` selects the unbanked
format; `Some(bank)` selects the banked format. The final boolean permits
preserving unmapped keys and explicit `NoChange` entries.

## Live routing with progress reports

The same managed route forwards ten seconds of channel messages, remapped to channel 2:

```rust
use meedee::{midi::{Input, Output}, routing, transport::Protocol};
use std::{collections::BTreeMap, time::Duration};

let mut input = Input::new();
let route = routing::Route::new(
    "keys".into(), input.stream()?.into(), "synth".into(), Protocol::Midi1,
    None, None, Some(2), None, false,
)?;
let mut router = routing::Router::new(vec![route], 128, 8_388_608)?;
let mut outputs = BTreeMap::from([("synth".into(), Output::new())]);
println!("{:?}", routing::run(
    &mut router, &mut input, &mut outputs, Some(Duration::from_secs(10)), None,
)?);
input.close()?;
for output in outputs.values_mut() { output.close()?; }
```

Known loss, overload or send failure stops all routes. On error,
`router.snapshot()` retains confirmed sends and any uncertain delivery. The driver
leaves connections open and generates no cleanup messages; closing the outputs
applies their normal cleanup policy. The route's final boolean opts into non-channel
traffic when `true`.
