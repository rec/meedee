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
use meedee::messages::{NoteOff, NoteOn};
use std::time::Duration;

let mut out = Output::new().with_transport(OutputOptions {
    protocol: Protocol::Ump,
    ..OutputOptions::default()
})?;
out.send_all([
    NoteOn::new_midi2(60)?.with_velocity(40_000)?.into(),
    Duration::from_millis(500).into(),
    NoteOff::new_midi2(60)?.into(),
])?;
out.close()?;
```

This needs a native UMP backend and endpoint. Group defaults to 1; output
resolves the channel.
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
use meedee::{midi::{Input, Output, Protocol}, routing};
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
Report.routes maps route names to RouteReport snapshots containing admitted,
confirmed and capacity_rejections counts. Python exposes the same fields on
read-only routing.RouteReport values. Snapshots retain their counts as routing
continues or fails.

## Monitor with typed observations

This is the same ten-second passive monitor, with native typed entries:

```rust
use meedee::{inspection, midi::Input};
use std::time::Duration;

let mut input = Input::new();
let source = input.stream()?.to_owned();
let clock = input.clock()?.clone();
let mut monitor = inspection::Monitor::new(source, clock, Default::default())?;
let report = inspection::run(
    &mut monitor, &mut input,
    |entry| { println!("{entry:?}"); Ok(()) },
    Some(Duration::from_secs(10)), None, true,
)?;
println!("{report:?}");
input.close()?;
```

`MonitorOptions` selects message type names, channel, protocol and UMP group.
`MonitorEntry` contains a message with semantic observations and note-state
snapshots, a diagnostic, or a loss-counter total. Filtering still updates state;
loss notifications are never filtered. Known loss resets interpretation and
continues; terminal failures return an error with progress retained in
`monitor.snapshot()`. The driver requires a fresh monitor and leaves the input
open. Use `|_| Ok(())` when only the report is needed. The final boolean enables
host delivery metrics. The library retains no payload history or formatted text.

## Step an RPN or NRPN parameter

Use the same single-step command with typed family and direction builders:

```rust
use meedee::{controls::{ParameterFamily, StepDirection}, messages::ParameterStep, midi::Output};

let mut out = Output::new();
out.send(ParameterStep::new(7)?)?;
out.send(ParameterStep::new(7)?
    .with_family(ParameterFamily::Nrpn)?
    .with_direction(StepDirection::Decrement))?;
out.close()?;
```

Choose parameter numbers supported by your instrument. The defaults are RPN,
increment, and an unassigned channel. To use NRPN parameter 16383, select the
family before that parameter: `ParameterStep::new(0)?.with_family(ParameterFamily::Nrpn)?.with_parameter(16383)?`.
Each builder preserves a valid command. `controls::ParameterStep` remains the
separate incoming observation type.

UMP encoding returns `Result`, just like MIDI 1 encoding. Propagate encoding
errors instead of assuming every value is ready to send:

```rust
use meedee::ump;

let packet = ump::Noop::new()?;
let words = ump::encode(&packet)?;
println!("{words:?}");
```

Implementations of `ump::UmpValue` likewise return `Result<Vec<u32>, ValueError>`
from `encode_words()`. Python's `ump.encode()` continues to return a tuple of
words and raises an exception on failure.

## Interpret controller gestures

The same interpreter accepts plain `events::Payload` messages and messages with
source/timing metadata. Keep one instance per source.

```rust
use meedee::controls::ControlInterpreter;
use meedee::events::InputItem;
use meedee::midi::Input;

let mut interpreter = ControlInterpreter::new(None)?;
let mut input = Input::new();
for item in input.with_errors() {
    let observations = match item? {
        InputItem::Message(message) => interpreter.feed(&message)?,
        InputItem::Diagnostic(diagnostic) => interpreter.feed_diagnostic(&diagnostic)?,
    };
    println!("{observations:?}");
}
```

`VelocityInterpreter` uses the same interface for MIDI 1 velocity prefixes.
Both return observation vectors with optional source and contributor timestamps;
`value` contains the interpreted result or diagnostic. Known loss clears pending
fragments and retains the original diagnostic. Clock changes report `StateReset`.
Call `reset()` before changing source identity, including between anonymous and
named messages. Use `new(Some(group))` for MIDI 1 inside UMP. These interpreters
do not interpret native MIDI 2 channel messages.

## Track, assemble and decode with optional provenance

The public owners are `mpe::Tracker`, `ump::Assembler`, `timecode::Assembler` and
`tuning::Decoder`. Each accepts plain `events::Payload` messages as well as
messages carrying metadata. `feed` returns observation vectors; source changes
require `reset()`. Clock changes report `StateReset`, and `feed_diagnostic` clears
pending state while retaining the original loss report.

```rust
use meedee::{events::Payload, messages::AnyMessage, mpe, tuning, ump};

let zone = mpe::ZoneConfiguration::new(1, 2)?;
let mut tracker = mpe::Tracker::new(zone.clone(), None)?;
println!("{:?}", tracker.snapshot(2)?);
tracker.configure(zone); // Clears notes, source binding and timing context.

let mut assembler = ump::Assembler::new(1_048_576, 8_388_608, 64)?;
let packets = ump::packetize_sysex7(2, &(0..20).collect::<Vec<u8>>())?;
assembler.feed(&Payload::Ump(ump::UmpMessage::SysEx7(packets[0].clone())))?;
println!("{:?}", assembler.finish());

let sysex = meedee::messages::SysEx::new(vec![127, 1, 8, 2, 0, 1, 60, 60, 0, 0])?;
println!("{:?}", tuning::decode(&sysex)?);
let mut decoder = tuning::Decoder::new(None)?;
println!("{:?}", decoder.feed(&Payload::Midi1(AnyMessage::SysEx(sysex)))?);
```

UMP `finish()` reports each truncated assembly's group and exact contributor range,
clears pending data and permits further input. `retained_bytes()` and
`active_streams()` expose current resource use. Tuning `feed_assembled(&observation)`
decodes completed SysEx7 with its original provenance; `tuning::decode()` remains
the stateless single-message operation. Both decoding paths return malformed-data errors.

For timecode, construct `timecode::Assembler::new(timeout_ns, group)` and call
`feed(&message, now_ns)` or `poll(now_ns)` with nondecreasing elapsed host time.
`locked()` exposes sequence lock. Reset clears assembly and source binding while
preserving the monotonic host-clock requirement. Complete FullFrame SysEx relocates
the observed position. Select a group explicitly for MIDI 1 inside UMP.

## Incremental file timelines and codec helpers

Python now exposes the existing Rust `smf::Timeline`, `clip::Timeline`,
`ump::word_count` and `smf::validate_meta` operations directly.

```rust
use meedee::{smf, ump};

let header = smf::Header::new(0, 1, 480, vec![])?;
let mut timeline = smf::Timeline::new(header, "performance".into(), None, 1_048_576)?;
println!("{:?}", timeline.origin());
println!("{:?}", timeline.finish());
println!("{}", ump::word_count(0x40903c00));
smf::validate_meta(81, &[0x07, 0xa1, 0x20], 0)?;
```

Feed merged `smf::Event` records through `timeline.feed(&event)`. For Clip entries,
construct `clip::Timeline::new(stream, include_configuration)` and use its
`feed(&entry)`. Its `clock()` and `origin()` are optional until established.

## Create a virtual MIDI port

Create a software source and receive its messages using the ordinary MIDI types.
The backend must support virtual ports.

```rust
use meedee::messages::{NoteOff, NoteOn};
use meedee::midi::{Input, Output};
use std::time::Duration;

let mut output = Output::new().create_virtual("meedee-example")?;
let mut input = Input::new();
let port = input.ports()?.into_iter().find(|p| p.name() == "meedee-example").unwrap();
let mut input = input.connect(&port)?;
output.send(NoteOn::new(60)?)?;
println!("{:?}", input.receive(Some(Duration::from_secs(1)), None)?);
output.send(NoteOff::new(60)?)?;
input.close()?;
output.close()?;
```

`Input::new().create_virtual(name)` creates a software destination that other
applications can send to. Both methods open immediately and return the same
high-level type as `connect()`. Input retains decoding, channel filters, receive
limits, timestamps and recording. Output retains sending, playback and cleanup.
Closing is terminal and Drop performs the ordinary scoped cleanup. Unsupported
backends return the transport capability error through `midi::Error::Connect`.

## Receive raw packets

Raw input preserves packet boundaries, sequence numbers, native clocks and optional
nanosecond timestamps. MIDI 1 packets may contain fragments or multiple messages.
Rust infers `Input<ReceivedPacket>` from `raw()`.

```rust
use meedee::midi::Input;

for packet in Input::new().raw()?.take(10) {
    println!("{packet:?}");
}
```

`receive(timeout, cancel)` and `with_errors()` return typed `InputItem<ReceivedPacket>`
values, with `Message` containing the received packet and `Diagnostic` containing
stream diagnostics. Default iteration reports diagnostics to stderr and yields
packets. Decoded input keeps its existing `Input<Payload>` behavior. Raw input
has no channel-filter or decoded-recording methods. Selecting raw mode rejects
an existing channel filter and requires an unopened input without a callback.

## Receive through a callback

Create a software destination and wait up to ten seconds for its first delivery.
Use `connect(&port)` for a physical input. The same builder works after `raw()`.

```rust
use meedee::midi::Input;
use std::sync::mpsc;
use std::time::Duration;

let (sender, receiver) = mpsc::channel();
let mut input = Input::new()
    .with_callback(move |item| { let _ = sender.send(item); })?
    .create_virtual("meedee-callback")?;
println!("{:?}", receiver.recv_timeout(Duration::from_secs(10)));
input.close()?;
```

Callbacks receive `midi::Result<InputItem<T>>`, including loss diagnostics and
terminal errors. They run serially on a dedicated dispatch thread through the
same bounded receive queue as iteration. Closures must be `Send + 'static` and
return promptly. Blocking MIDI operations are rejected inside callbacks. Use a
channel to ask the controlling thread to perform them.

Receive, iteration and timed record/record_to are unavailable in callback mode.
Attached background recording is available through start_recording.
Callback panic stops further delivery and is observable through `check_health()`.
Close stops native delivery, drains accepted input to the callback and joins the
dispatch thread. It waits for an active callback to return. Closing is terminal.
Dropped-packet counts remain readable after close. Callback health and loss
counter updates from the dispatch thread can lag by its polling interval.

## Inspect diagnostics after close

Input and Output expose `closed()`. It becomes true at terminal closure, including
when cleanup fails. Opened input retains its stream, clock, dropped-packet count
and first terminal failure for inspection after close.

```rust
use meedee::midi::Input;

let mut input = Input::new().create_virtual("meedee-diagnostics")?;
input.close()?;
println!("closed = {}", input.closed());
println!("dropped packets = {}", input.dropped_packets());
println!("{:?}", input.clock()?);
println!("{:?}", input.failure());
```

`failure()` returns an optional shared, read-only `Arc<midi::Error>`, preserving
its category and cause. Retained errors delivered by receive, callback, close or
health checking use `midi::Error::Shared(error)`. Match `error.as_ref()` to inspect
the original variant, or use the standard error `source()` chain. A normal close
leaves failure empty, while `check_health()` reports that input is closed.

Reading retained diagnostics after close performs no native calls and does not
reopen resources. Closing an unopened input leaves dropped_packets at zero and
failure empty. Its stream and
clock getters return a state error because no identity or clock was established.
An unopened object starts with `closed() == false`. A normally closed input can
still drain its accepted messages, using the existing iteration contract.

## Recognize GM messages in a saved dump

Read a saved dump without opening a MIDI port. One decoder recognizes GM mode
changes, controller destinations, key controls and global parameters:

```rust
use meedee::{gm, syx};
use std::fs::File;

for message in syx::Reader::new(File::open("backup.syx")?, 1_048_576)? {
    if let Some(decoded) = gm::decode(&message?, 1024)? {
        println!("{decoded:?}");
    }
}
```

The result is GmMessage, with one variant for each concrete Python GM message
class. Unknown messages return None; malformed supported messages return Err.
The explicit positive entry limit corresponds to Python's max_entries keyword,
whose default is 1024.

## Read exact recorded times

Timestamp::seconds() returns an exact Rational value, corresponding to Python's
Timestamp.seconds property. Recorded tick units are preserved until conversion:

```rust
use meedee::{archive, messages::Message};

for record in archive::Reader::open("rec.meedee")? {
    if let archive::Record::Event(message) = record? {
        if let Some(timestamp) = message.timestamp() {
            println!("{:?}", timestamp.seconds()?);
        }
    }
}
```

Rust keeps Read-based functions such as inspection::inspect alongside the path
operation inspection::inspect_file. Python's inspect_file accepts either a path
or a binary reader. Both use the same parser and limits. Rust option fields
correspond to Python keywords, including ComparisonOptions.source_map and
clock_map. with_playback settings persist across subsequent play calls; Python's
play keywords apply to one call. Clocks are automatic in ordinary playback.
play leaves a retained output open for subsequent playback or sending, with
normal close/Drop cleanup. Rust compound-send errors retain their typed cause;
Python exposes that cause through __cause__ and uses WorkerError for worker
failures in both real and simulated transports.


## Record while processing input

Attach the existing archive recorder while retaining normal input access.
Recording runs independently of receive calls and also supports callback inputs.

```rust
use meedee::midi::{Input, RecordingOptions};
use std::time::Duration;

let mut input = Input::new();
input.start_recording("rec.meedee", &RecordingOptions::default())?;
println!("{:?}", input.receive(Some(Duration::from_secs(10)), None)?);
println!("{:?}", input.stop_recording(Duration::from_secs(5))?);
input.close()?;
```

The returned Arc<archive::Recorder> gives access to the same recorder, including
opt-in pressure metrics. RecordingOptions matches Python's label, best_effort,
payload_limit, record_limit, byte_limit and metrics keywords. File handling matches
Python's existing recorder: starting a recording creates or truncates the path.

Recording has independent admission. Receive-queue overflow does not by itself
make a successfully retained recording incomplete. Known loss, unfinished messages,
callback panic and writer failure remain visible in the final report. Raw input
cannot attach decoded archive recording.

A stop timeout retains the stopping recorder. Retry stop_recording later, including
after close, to obtain the final report. No replacement recording can start until
finalization succeeds. recording_report() retains the completed snapshot after
close; an unfinished stop does not invent a report.

## Cancel a receive without closing the input

Share Cancellation with another thread to interrupt a receive:

```rust
use meedee::{events::Cancellation, midi::Input};
use std::time::Duration;

let cancel = Cancellation::default();
let mut input = Input::new();
println!("{:?}", input.receive(Some(Duration::from_secs(10)), Some(&cancel))?);
input.close()?;
```

cancel.cancel() makes receive return Error::Cancelled without consuming queued
input or closing the connection. Pass None as the next cancellation argument to
receive again. Ok(None) means timeout only. After accepted input drains, normal
closure returns Error::Closed; a terminal failure returns the retained error.
Ordinary iteration stops quietly on normal closure; with_errors returns a terminal
failure once.

last_message_time() returns Option<Instant> at raw packet ingress, before channel
filtering, independently of consuming messages. Python exposes monotonic seconds
for the same host observation. Both retain the value after close and return None
before any packet. This time is independent of the MIDI packet's optional timestamp.

A callback panic stops further calls, retains its failure and closes the transport
outside callback scope. Attached recording becomes incomplete. Python follows
the same policy for callback exceptions, preserving the original exception in
failure and reporting it to stderr.
