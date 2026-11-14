# Practical Rust MIDI examples

## Inspect one input universe

```rust
use middy::controls::ParameterFamily;
use middy::events::Payload;
use middy::messages::{AnyMessage, ControlChange};
use middy::universe::{CacheOptions, Category, Retention, Scope, Universe};

let mut options = CacheOptions {
    default: Retention::Minimum,
    universe: Some(Retention::Full),
    ..Default::default()
};
options.categories.insert(Category::Notes, Retention::Minimum);
let mut cache = Universe::new(options);
for (controller, value) in [(99, 1), (98, 2), (6, 10), (38, 5)] {
    let event = Payload::Message(AnyMessage::ControlChange(
        ControlChange::new(controller, value)?.with_channel(1)?
    ));
    cache.feed(&event, None)?;
}
let state = cache.parameter_state(Scope::new(None, 1)?, 1, ParameterFamily::Nrpn, 130)?
    .expect("observed parameter");
let components = state.components.expect("MIDI 1 components");
assert_eq!((components.coarse, components.fine), (Some(10), Some(5)));
let snapshot = cache.snapshot();
cache.reset();
assert_eq!(snapshot.parameters.len(), 1);
```

Use one instance per source. `feed(&event, None)` and `poll(None)` use the local
monotonic clock. Supply `Some(now_ns)` consistently for deterministic replay.
`set_timecode_timeout` accepts a positive `Duration` before feeding events.
`configure_mpe` accepts an explicit zone configuration.

These examples use the `middy` Rust crate. Put a complete example inside a
function returning `Result<(), Box<dyn std::error::Error>>` and finish with
`Ok(())`. Later fragments sometimes reuse connections from the preceding block.
The `?` operator propagates a checked error.

Both Rust and Python construct Input/Output lazily; first discovery or I/O
initializes the resource. Empty playback opens no ports.

Live examples select the first available MIDI port. Connect a keyboard or synth
before running them. MIDI carries messages, not audio. Channels are 1..16, notes
and velocities are 0..127, and program numbers are 1..128.

## 1. Play one note

```rust
use middy::messages::Note;
use middy::midi::Output;
use std::time::Duration;

Output::new().send(Note::new(Duration::from_millis(500), 60)?)?;
```

This plays middle C for half a second, with velocity 90 on channel 1. `Note`
sends the attack and release. The temporary output closes when the statement ends.

## 2. Play a tune in beats

```rust
use middy::messages::{Beats, Note, Rest};
use middy::midi::Output;

let mut out = Output::new().with_bpm(120)?;
out.send([
    middy::midi::OutputValue::from(Note::new(Beats::from(1), 60)?),
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
use middy::midi::Output;

let mut output = Output::new();
let ports = output.ports()?;
println!("{ports:?}");
let mut out = output.connect(&ports[0])?;
out.close()?;
```

This discovery example assumes there is at least one output. `Input` supports
the same selection methods. The low-level `transport` module exposes native
driver primitives; high-level Input/Output also support virtual ports and callbacks.
Closing is terminal;
create a new object to reconnect. Python `connect()` returns the same object; Rust
`connect()` consumes and returns the resource to express ownership.

## 3. Print incoming messages

```rust
use middy::midi::Input;

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
use middy::midi::{Input, Output};
use middy::events::Payload;
use middy::messages::AnyMessage;

let mut out = Output::new();
for message in Input::new() {
    match message {
        Payload::Message(AnyMessage::NoteOn(note)) => out.send(note.transpose(12))?,
        Payload::Message(AnyMessage::NoteOff(note)) => out.send(note.transpose(12))?,
        other => out.send(other)?,
    };
}
out.close()?;
```

`transpose` clamps to MIDI's note range and preserves timing/source metadata.
`Payload` distinguishes semantic messages, UMP packet values and compound commands; `AnyMessage` identifies
the MIDI 1 message kind. These are enums holding the messages themselves, not
separate timing wrappers.

## 5. Split a keyboard between two sounds

```rust
use middy::midi::{Input, Output};
use middy::events::Payload;
use middy::messages::{AnyMessage, ProgramChange};

let mut low = Output::new().with_channel(2)?;
let mut high = Output::new().with_channel(3)?;
low.send(ProgramChange::new(34)?)?;
high.send(ProgramChange::new(81)?)?;

for message in Input::new().with_channel(1)? {
    let key = match &message {
        Payload::Message(AnyMessage::NoteOn(note)) => Some(note.note()),
        Payload::Message(AnyMessage::NoteOff(note)) => Some(note.note()),
        Payload::Message(AnyMessage::PolyPressure(note)) => Some(note.note()),
        _ => None,
    };
    match key {
        Some(note) if note < 60 => low.send(message)?,
        Some(_) => high.send(message)?,
        None if matches!(&message, Payload::Message(
            AnyMessage::ControlChange(_) |
            AnyMessage::ChannelPressure(_) | AnyMessage::ProgramChange(_) |
            AnyMessage::PitchBend(_)
        )) => {
            low.send(message.clone())?;
            high.send(message)?
        }
        None => low.send(message)?,
    };
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
use middy::midi::{Input, Output};
use std::time::Duration;

Output::new().send(Input::new().record(Duration::from_secs(10))?)?;
```

Recording closes the input and returns a `Recording` containing declarations,
messages and any receive problems. Replay infers its origin from the first message.
To retain the result, bind it to a variable and print the whole recording.

To save and read the capture:

```rust
use middy::midi::{Input, Output};
use middy::Reader;
use std::time::Duration;

Input::new().record_to(Duration::from_secs(10), "rec.middy", true)?;
let report = Output::new().send(Reader::open("rec.middy")?)?;
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

## Transpose a MIDI file

```rust
use middy::{Reader, Writer};

let mut writer = Writer::create("song-up.mid")?;
for record in Reader::open("song.mid")? {
    let mut m = record?;
    if m.channel().unwrap_or(10) != 10 {
        m = m.transpose(12);
    }
    writer.write(&m)?;
}
writer.finish()?;
```

This preserves the header, tracks, ticks, tempo, SysEx fragments and other file
records. Notes on channel 10 are unchanged; other note numbers are transposed
and clamped to 0..127. The reader detects MIDI files, SysEx streams and middy archives from
contents, yielding a `file_io::Record` for each value. MIDI files yield their
header first. SysEx messages use `Record::Archive(archive::Record::Event(...))`.
The writer selects `.mid`, `.midi`, `.syx` or `.middy` from the destination
name and refuses overwrite. Call `finish()` to validate and finalize the file;
an earlier error can leave a partial output without changing the source.
Format-specific readers and writers remain available for detailed control.
The SysEx codec is internal; use the generic file API for `.syx` files.
For streams use `Writer::new(stream, "syx", limit)`; empty input is an empty
SysEx stream. `Writer::write` accepts `WriteRecord` values, including file
records and typed primitive messages such as a tuning `BulkDump`.

## 7. Handle input diagnostics explicitly

```rust
use middy::midi::Input;
use middy::events::InputItem;

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
use middy::midi::Output;
use middy::messages::{NoteOn, ParameterSet};
use std::time::Duration;

let mut out = Output::new();
out.send(ParameterSet::new(0, 2 << 7)?)?; // Pitch-bend sensitivity: two semitones.
out.send([
    middy::midi::OutputValue::from(NoteOn::new(60)?),
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
use middy::midi::{Input, Output};
use middy::events::Payload;
use middy::messages::{AnyMessage, Message};

let mut out = Output::new();
for message in Input::new() {
    if let Payload::Message(AnyMessage::NoteOn(note)) = message {
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
use middy::midi::Output;
use middy::transport::{OutputOptions, Encoding};
use middy::messages::{ControllerFamily, NoteOff, NoteOn, ParameterController};
use std::time::Duration;

let mut out = Output::new().with_transport(OutputOptions {
    encoding: Encoding::Ump,
    ..OutputOptions::default()
})?;
out.send([
    middy::midi::OutputValue::from(NoteOn::new_midi2(60)?.with_velocity(40_000)?),
    Duration::from_millis(500).into(),
    NoteOff::new_midi2(60)?.into(),
])?;
out.send(ParameterController::new_with_family(
    1, 1, 1, 2, 1 << 31, ControllerFamily::Assignable,
)?)?;
out.close()?;
```

This needs a native UMP backend and endpoint. Group defaults to 1; output
resolves the channel.
MIDI 2 velocity uses its native 16-bit range. There is no automatic protocol
translation, full-note scheduling or cleanup for UMP. Raw packets likewise retain
their own addressing. The `translation` module provides explicit conversion when needed.

MIDI 1 UMP uses the same message types, with a group and native MIDI 1 values:

```rust
use middy::{messages::{AnyMessage, NoteOn, PerNotePitchBend, SysEx}, events::Payload, translation, ump};

let legacy = NoteOn::new(60)?.with_velocity(90)?.with_channel(2)?.with_group(3)?;
assert_eq!(ump::encode(&legacy)?, [0x22913c5a]);
assert_eq!(ump::decode(&[0x22913c5a], false)?, Payload::from(legacy.clone()));
let wide = translation::to_midi2(&AnyMessage::NoteOn(legacy), 3)?;
assert_eq!(wide.messages[0].version(), 2);
let per_note = PerNotePitchBend::new(3, 2, 60, 1234)?;
let dump = SysEx::new((0..20).collect())?.with_group(3)?;
```

Send `per_note` and `dump` through an existing UMP output. Its configured channel
overrides every channel-addressed semantic message. Complete SysEx and grouped
MIDI 1 commands package into bounded UMP streams, with one successful handoff per
logical value. Incoming data fragments remain separate until explicitly assembled.
Remove a MIDI 1 value's group with `without_group()?` before byte encoding.

## 11. Inspect and compare saved captures

These operations open files, not MIDI ports. Reports keep capture quality separate
from structural completeness.

```rust
use middy::inspection::{self, ComparisonOptions, Format, Options};

println!("{:?}", inspection::inspect_file("rec.middy", Format::Archive, &Options::default())?);
println!("{:?}", inspection::compare_files(
    "rec.middy", "other.middy", Format::Archive, &ComparisonOptions::default(),
)?);
```

For playback diagnostics, configure the output once. The options apply to each
subsequent `send()` call; ordinary playback needs no options.

```rust
use middy::archive::Reader;
use middy::midi::{Output, PlaybackOptions};

let mut out = Output::new().with_playback(PlaybackOptions {
    metrics: true,
    ..Default::default()
});
println!("{:?}", out.send(Reader::open("rec.middy")?)?);
out.close()?;
```

`PlaybackOptions` also accepts an origin timestamp, clock mappings, rounding
permission and a shared `events::Cancellation`. Inspection and metrics snapshots
expose read-only getters with the same names as Python properties.

## 12. Capture and restore a SysEx dump

Initiate a dump on the instrument during a ten-second recording. Save its SysEx
messages separately; `recording.records` also retains timing and diagnostics.

```rust
use middy::{midi::Input, file_io, archive::Record, events::Payload, messages::AnyMessage};
use std::time::Duration;

let recording = Input::new().record(Duration::from_secs(10))?;
println!("{:?}", recording.records);
let mut writer = file_io::Writer::create("backup.syx")?;
for record in &recording.records {
    if let Record::Event(message) = record {
        if let Payload::Message(AnyMessage::SysEx(message)) = message.as_ref() {
            writer.write(message)?;
        }
    }
}
writer.finish()?;
```

Run this separately to restore the backup. Choose the wait duration from the
instrument's documentation. Sending a dump can replace sounds or settings.

```rust
use middy::{events, file_io, midi::Output};
use std::time::Duration;

let mut output = Output::new();
println!("{:?}", output.send(events::with_gap(file_io::Reader::open("backup.syx")?, Duration::from_millis(100)))?);
output.close()?;
```

### Request or capture an opaque instrument dump

```rust
use middy::{file_io, midi::{Input, Output}, sysex};

let mut receiver = Input::new().with_port("VL70")?;
let mut output = Output::new().with_port("VL70")?;
let result = sysex::dump(&mut receiver, Some(&mut output), None, &Default::default())?;
println!("{}: {:?}", result.status, result.reason);
let mut writer = file_io::Writer::create("vl70.syx")?;
for message in &result.messages {
    writer.write(message)?;
}
writer.finish()?;
```

The call sends an identity request and requires exactly one supported reply
before requesting the dump. Pass `Some("yamaha/vl70-m")` for the model to skip
detection. For a dump started on the instrument, pass `None` for the output and
an explicit model such as `Some("moog/subsequent-37")`. The call leaves connections
open. Manual completion remains unverified. `DumpOptions`
controls scope, device ID, deadlines, retention limits and cancellation. See
[model coverage and scope limits](sysex-dumps.md).

To inquire about identities independently of dump support, use existing
connections and collect every raw reply through the bounded window:

```rust
use middy::universal;

let identities = universal::identify(&mut receiver, &mut output, &Default::default())?;
println!("{}: {:?}", identities.status, identities.reason);
for reply in &identities.messages {
    println!("{:02x?}", reply.data());
}
```

`IdentityOptions` controls device ID (default 127, broadcast), timeout (one
second), event/byte/payload limits and cancellation. `IdentityReport` retains
duplicates and nonstandard reply layouts; it never chooses a model. Use
`universal::decode` for standard typed replies. `completed` means the collection
window ended cleanly, and `timed_out` means no reply; cancellation, failure and
limit reports retain any admitted reply prefix.

To restore the in-memory capture with its observed relative message timing,
run this separately after checking the capture report and deciding to restore:

```rust
println!("{:?}", output.send(result.messages)?);
output.close()?;
```

This uses timestamps retained from the input, whose default timestamp mode is
`TimestampMode::Absolute`. It preserves observed message timing; it does not infer
the synth's required receive gap. In particular, separately requested replies
include request turnaround time. Saving to `.syx` discards timing. For an untimed
`.syx` file, use the `events::with_gap` example above, choosing the gap from the
instrument's documentation. It inserts a wait after every item, including the
last. Output uses ordinary sequence timing, not completion-relative pacing.
The generic helper yields `events::GapItem<T>` values (`Item(T)` or
`Gap(Duration)`); it does not sleep or send.

## 13. Query a tuning dump

This asks device 0 for tuning program 2 through the first input/output pair.
The final report includes a matching reply, timeout or failure with known progress.

```rust
use middy::{midi::{Input, Output}, tuning};

let mut input = Input::new();
let mut output = Output::new();
let mut query = tuning::Query::new(
    tuning::BulkDumpRequest::new(0, 2)?.into(), input.stream()?.into(), 5_000_000_000,
)?;
println!("{:?}", tuning::query(&mut query, &mut input, &mut output, None)?);
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
use middy::{discovery::Poller, midi::Encoding};

let mut poller = Poller::new(Encoding::Bytes)?;
println!("{:?}", poller.poll()?);
// Call poll() again when the application wants another observation.
poller.close();
```

Python additionally provides CLI commands, unit strings and
uFor integration. Rust uses typed durations, builders, enums and `Result` values;
its `transport` and `file_io` modules expose lower-level native and streaming
primitives. Corresponding workflows otherwise use the same public module names.

## 15. Discover MIDI-CI devices

This uses the experimental manufacturer identity 0x7D and a caller-selected local
MUID. Choose a fresh unused MUID for a real scan. Python's example generates one
with `secrets`; Rust leaves that choice to the application's random-number source.

```rust
use middy::{ci, midi::{Input, Output}};

let mut input = Input::new();
let mut output = Output::new();
let identity = ci::Identity::new(vec![0x7d, 0, 0], 1, 1, vec![0; 4])?;
let route = ci::Route::new(input.stream()?.into(), "device".into(), None)?;
let session = ci::Session::new(12345, identity, route, 0, None, 1024, 128, 128, 8_388_608)?;
let mut browser = ci::Browser::new(session, 3_000_000_000, 1_000_000_000, 30_000_000_000, 128)?;
println!("{:?}", ci::browse(&mut browser, &mut input, &mut output, &ci::Options::default())?);
input.close()?;
output.close()?;
```

`ci::Options` sets cancellation and SysEx assembly limits, matching Python's
keyword options. Set both transports to UMP and give the route an explicit group
for SysEx7. This scan sends Discovery and endpoint inquiries only; it performs no
Profiles, Property Exchange or instrument configuration writes.

## 16. Export a just-intonation tuning

Create `just-three.syx` with keys 69, 70 and 71 tuned to 440, 660 and 880 Hz.
Unselected keys are preserved. Rust's `bulk_dump` accepts `NoteTuning` values;
Python's `bulk_dump` instead accepts a uFor tuning and selected MIDI keys.

```rust
use middy::{file_io, tuning};

let pitches = [(69, 440.0), (70, 660.0), (71, 880.0)]
    .into_iter()
    .map(|(note, hz)| {
        tuning::NoteTuning::new(
            note, tuning::TuningValue::Pitch(tuning::Pitch::from_frequency(hz)?.pitch),
        )
    })
    .collect::<Result<Vec<_>, _>>()?;
let dump = tuning::bulk_dump(&pitches, 0, 0, "Just three".into(), None)?;
let mut writer = file_io::Writer::create("just-three.syx")?;
writer.write(&dump)?;
writer.finish()?;
println!("{dump:?}");
```

Use `output.send(events::with_gap(messages, gap))` to insert explicit waits
between complete SysEx messages and after the final message.
For individual real-time changes, use
`tuning::note_changes(&pitches, 0, 0, true, None)?`.
These builders do not open ports or send messages. `None` selects the unbanked
format; `Some(bank)` selects the banked format. Omitted keys are preserved
automatically, and both builders accept explicit `NoChange` entries.
Python defaults bulk dumps and tuning-program selection to program 0; Rust passes
the program explicitly because Rust functions do not support default arguments.

## Live routing with progress reports

The same managed route forwards ten seconds of channel messages, remapped to channel 2:

```rust
use middy::{midi::{Input, Output, Encoding}, routing};
use std::{collections::BTreeMap, time::Duration};

let mut input = Input::new();
let route = routing::Route::new(
    "keys".into(), input.stream()?.into(), "synth".into(), Encoding::Bytes,
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
use middy::{inspection, midi::Input};
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

`MonitorOptions` selects message type names, channel, encoding and UMP group.
`MonitorEntry` contains a message with semantic observations and note-state
snapshots, a diagnostic, or a loss-counter total. Filtering still updates state;
loss notifications are never filtered. Known loss resets interpretation and
continues; terminal failures return an error with progress retained in
`monitor.snapshot()`. The driver requires a fresh monitor and leaves the input
open. Use `|_| Ok(())` when only the report is needed. The final boolean enables
host delivery metrics. The library retains no payload history or formatted text.

## Select a program and bank

```rust
use middy::{GmMode, Output, ProgramChange, gm};
use std::time::Duration;

println!("{:?}", gm::program(34, None)?);
println!("{:?}", gm::program(6, Some(4))?);
println!("{:?}", gm::percussion(36, Some(17))?);

let mut out = Output::new();
out.send(GmMode::new(gm::Mode::Gm2, 1)?)?;
out.send(Duration::from_millis(100))?;
out.send(ProgramChange::new(34)?)?;
out.close()?;
```

```rust
use middy::{Output, ProgramChange};

let mut out = Output::new();
out.send(ProgramChange::new(34)?)?;
out.send(ProgramChange::new(34)?.with_bank(0, 8)?)?;
out.close()?;
```

## Change a tuning note

```rust
use middy::{Output, tuning::{NoteChange, NoteTuning, Pitch, TuningValue}};

let pitch = NoteTuning::new(69, TuningValue::Pitch(Pitch::new(69, 0)?))?;
let mut out = Output::new();
out.send(NoteChange::new(1, 2, vec![pitch])?.with_bank(3)?)?;
out.close()?;
```

## Send controller values

```rust
use middy::{ControlChange, Output};

let mut out = Output::new();
out.send(ControlChange::new(1, 90)?)?;
out.send(ControlChange::new_with_resolution(1, 9000, 14)?)?;
out.close()?;
```

## Step an RPN or NRPN parameter

```rust
use middy::{Output, ParameterFamily, ParameterSet};

let mut out = Output::new();
out.send(ParameterSet::new(0, 2 << 7)?)?; // Pitch-bend range: 2 semitones.
out.send(ParameterSet::new(1234, 10000)?.with_family(ParameterFamily::Nrpn)?)?;
out.close()?;
```

Use the same single-step command with typed family and direction builders:

```rust
use middy::{controls::{ParameterFamily, StepDirection}, messages::ParameterStep, midi::Output};

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
use middy::ump;

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
For example, CC 99=1 and CC 98=2 select NRPN 130. A later CC 98=3
selects NRPN 131 using the retained MSB. Each channel caches both Data Entry components
separately for each RPN/NRPN parameter, so returning to NRPN 130 permits a CC 38
update using its earlier observed CC 6. An unobserved coarse value still produces
MissingCoarse. Each family needs both selector halves initially. Stepping a
parameter invalidates only its cached components. RPN-null disables updates
until a later selector activates a family, preserving cached values. Reset/loss
clears the applicable cache; the sparse cache otherwise grows with observed parameters.

Inspect the cache without changing the selection or consuming an input:

```rust
use middy::controls::{ControlInterpreter, ParameterFamily, ParameterState};
use middy::events::Payload;
use middy::messages::{AnyMessage, ControlChange};

let mut view = ControlInterpreter::new(None)?;
for (controller, value) in [(99, 1), (98, 2), (38, 5)] {
    view.feed(&Payload::Message(AnyMessage::ControlChange(
        ControlChange::new(controller, value)?.with_channel(1)?,
    )))?;
}
assert_eq!(view.parameter_state(1, ParameterFamily::Nrpn, 130)?,
    Some(ParameterState { coarse: None, fine: Some(5) }));
view.feed(&Payload::Message(AnyMessage::ControlChange(
    ControlChange::new(6, 10)?.with_channel(1)?,
)))?;
assert_eq!(view.parameter_state(1, ParameterFamily::Nrpn, 130)?,
    Some(ParameterState { coarse: Some(10), fine: None }));
view.feed(&Payload::Message(AnyMessage::ControlChange(
    ControlChange::new(38, 6)?.with_channel(1)?,
)))?;
assert_eq!(view.parameter_state(1, ParameterFamily::Nrpn, 130)?,
    Some(ParameterState { coarse: Some(10), fine: Some(6) }));
```

The lookup returns an owned snapshot, or `None` for no cached data. Missing
components use `None` independently. A new coarse input clears the previous fine.

```rust
use middy::controls::ControlInterpreter;
use middy::events::InputItem;
use middy::midi::Input;

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
use middy::{events::Payload, messages::AnyMessage, mpe, tuning, ump};

let zone = mpe::ZoneConfiguration::new(1, 2)?;
let mut tracker = mpe::Tracker::new(zone.clone(), None)?;
println!("{:?}", tracker.snapshot(2)?);
tracker.configure(zone); // Clears notes, source binding and timing context.

let mut assembler = ump::Assembler::new(1_048_576, 8_388_608, 64)?;
let packets = ump::packetize_sysex7(&middy::messages::SysEx::new((0..20).collect())?.with_group(2)?)?;
assembler.feed(&Payload::Ump(ump::PacketValue::SysEx7(packets[0].clone())))?;
println!("{:?}", assembler.finish());

let sysex = middy::messages::SysEx::new(vec![127, 1, 8, 2, 0, 1, 60, 60, 0, 0])?;
println!("{:?}", tuning::decode(&sysex)?);
let mut decoder = tuning::Decoder::new(None)?;
println!("{:?}", decoder.feed(&Payload::Message(AnyMessage::SysEx(sysex)))?);
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
use middy::{smf, ump};

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
use middy::messages::{NoteOff, NoteOn};
use middy::midi::{Input, Output};
use std::time::Duration;

let mut output = Output::new().create_virtual("middy-example")?;
let mut input = Input::new();
let port = input.ports()?.into_iter().find(|p| p.name() == "middy-example").unwrap();
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
use middy::midi::Input;

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
use middy::midi::Input;
use std::sync::mpsc;
use std::time::Duration;

let (sender, receiver) = mpsc::channel();
let mut input = Input::new()
    .with_callback(move |item| { let _ = sender.send(item); })?
    .create_virtual("middy-callback")?;
println!("{:?}", receiver.recv_timeout(Duration::from_secs(10)));
input.close()?;
```

Callbacks receive `midi::Result<InputItem<T>>`, including loss diagnostics and
terminal errors. They run serially on a dedicated dispatch thread through the
same bounded receive queue as iteration. Closures must be `Send + 'static` and
return promptly. Blocking MIDI operations are rejected inside callbacks. Use a
channel to ask the controlling thread to perform them.

Receive, iteration and timed record/record_to are unavailable in callback mode.
Attached background recording is available through start.
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
use middy::midi::Input;

let mut input = Input::new().create_virtual("middy-diagnostics")?;
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
use middy::{archive, events::Payload, file_io, gm, messages::AnyMessage};

for record in file_io::Reader::open("backup.syx")? {
    let file_io::Record::Archive(archive::Record::Event(message)) = record? else { continue; };
    let Payload::Message(AnyMessage::SysEx(message)) = message.as_ref() else { continue; };
    if let Some(decoded) = gm::decode(message, 1024)? {
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
use middy::{archive, messages::Message};

for record in archive::Reader::open("rec.middy")? {
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
clock_map. with_playback settings persist across subsequent send calls; Python's
send keywords apply to one call. Clocks are automatic in ordinary playback.
send leaves a retained output open for subsequent playback or sending, with
normal close/Drop cleanup. Rust compound-send errors retain their typed cause;
Python exposes that cause through __cause__ and uses WorkerError for worker
failures in both real and simulated transports.


## Record while processing input

Attach the existing archive recorder while retaining normal input access.
Recording runs independently of receive calls and also supports callback inputs.

```rust
use middy::midi::{Input, RecordingOptions};
use std::time::Duration;

let mut input = Input::new();
input.start(Some("rec.middy"), &RecordingOptions::default())?;
println!("{:?}", input.receive(Some(Duration::from_secs(10)), None)?);
println!("{:?}", input.stop(Duration::from_secs(5))?);
input.close()?;
```

The returned Arc<archive::Recorder> gives access to the same recorder, including
opt-in pressure metrics. RecordingOptions matches Python's label, best_effort,
payload_limit, record_limit, byte_limit and metrics keywords. File handling matches
Python's existing recorder: starting a recording creates or truncates the path.

For unbounded memory capture, pass no path and read the records after stopping:

```rust
use middy::midi::{Input, RecordingOptions};
use std::path::Path;
use std::time::Duration;

let mut input = Input::new();
let recorder = input.start(None::<&Path>, &RecordingOptions::default())?;
println!("{:?}", input.receive(Some(Duration::from_secs(10)), None)?);
let report = input.stop(Duration::from_secs(5))?;
let records = recorder.records().unwrap();
input.close()?;
```

Memory capture retains diagnostics and continues after them, marking the final
report incomplete. `Input::record(duration)` uses the same attached memory
recorder, closes the input and returns the captured records.

Recording has independent admission. Receive-queue overflow does not by itself
make a successfully retained recording incomplete. Known loss, unfinished messages,
callback panic and writer failure remain visible in the final report. Raw input
cannot attach decoded archive recording.

A stop timeout retains the stopping recorder. Retry stop later, including
after close, to obtain the final report. No replacement recording can start until
finalization succeeds. recording_report() retains the completed snapshot after
close; an unfinished stop does not invent a report.

## Cancel a receive without closing the input

Share Cancellation with another thread to interrupt a receive:

```rust
use middy::{events::Cancellation, midi::Input};
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


## One send operation

send accepts a single value, an iterable of values or archive records, a Recording,
or a Reader. OutputSource adapts these forms to the same operation. Every call
returns a playback Report, including immediate sends and empty sequences.

A tuple concatenates independent sources without collecting a reader in memory:

```rust
use middy::{archive::Reader, messages::Note, midi::Output};
use std::time::Duration;

let report = Output::new().send((
    Note::new(Duration::from_millis(500), 60)?,
    Reader::open("rec.middy")?,
    Note::new(Duration::from_millis(500), 67)?,
))?;
println!("{report:?}");
```

A direct message sends immediately regardless of its optional timestamp. Within
an iterable, all timed messages preserve relative timestamps, starting when that
source is reached. Untimed collections use note/rest/delay timing. Mixing timed
and untimed values within one source is rejected. Streamed failures retain the
confirmed prefix in Failure.report and the underlying cause in Failure.cause.
Python preserves its original typed exception, attaching report and completed_events
after execution begins; it does not wrap the exception in PlaybackError.
with_playback configures origin, clock mappings, cancellation, rounding and metrics
for send. Cancellation returns a report and leaves the output open; empty sources
open no ports. A cancelled note may remain active until release or normal cleanup.


## Run MIDI in a subprocess

Install `middy-worker` on PATH, for example with
`cargo install --path crates/middy --bin middy-worker` from this checkout.
Rust applications use the worker without Python:

```rust
use middy::{Note, Output};
use std::time::Duration;

let mut out = Output::new().with_run_in_subprocess(true)?;
out.send(Note::new(Duration::from_millis(500), 60)?)?;
out.close()?;
```

To select another executable, arguments or native-operation deadline, configure
`SubprocessOptions` before discovery or connection:

```rust
use middy::{Output, SubprocessOptions};
use std::time::Duration;

let mut out = Output::new()
    .with_run_in_subprocess(true)?
    .with_subprocess_options(SubprocessOptions {
        timeout: Duration::from_secs(2),
        ..SubprocessOptions::default()
    })?;
println!("{:?}", out.ports()?);
out.close()?;
```

`Input` supports the same builders, including raw input, callbacks and recording.
Worker startup remains lazy. Each client owns one child; discovery snapshots are
validated against their originating client and generation, without rediscovery on
connect. Callbacks stay in the parent. Native-operation timeouts, child exits and
terminal errors invalidate the worker; operations are never retried automatically.
The default deadline is five seconds. Parent callbacks must return on their own.
Python's interpreter bootstrap and direct Rust launcher use this same engine.

Python's `Input`, `Output` and discovery poller use frozen Pydantic configuration
models and report invalid settings with `ValidationError`. Rust continues to use
the builders and options shown above, with typed validation errors. Units,
defaults, lazy opening, terminal closure and resource ownership are unchanged.
Python copying/pickling is rejected, and Rust input/output owners remain
non-`Clone`; a new resource is constructed from configuration.

## Exact clock mapping

```rust
use middy::events::{Clock, ClockMapping, Rational};

let source = Clock::new("source".into(), 1, 1_000_000_000, "host".into(), None)?;
let target = Clock::new("target".into(), 1, 1_000_000_000, "host".into(), None)?;
let mapping = ClockMapping::new(
    source, target, Rational::new(1001, 1000)?,
    Rational::new(1, 4)?, Some(Rational::new(1, 1_000_000)?),
)?;
println!("{mapping:?}");
```
