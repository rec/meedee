# Practical Python MIDI examples

A graded series of examples.

For the Rust API, see the [practical Rust examples](rust-examples.md).

## 1. Play middle C for half a second

```python
from meedee.messages import Note
from meedee.midi import Output

Output().send(Note("500 ms", 60))
```

## 2. Play a tune in 120 bpm

```python
from meedee.messages import Note, Rest
from meedee.midi import Output

Output(bpm=120).send(
    Note(duration=1, note=60, velocity=90),  # 1 beat
    Note(0.5, 62, 70),
    Note(0.5, 64, 100),
    Rest("1 beat"),
    Note(2, 64, 100),
)
```

## 3. Listen to an input

```python
from meedee.midi import Input

for message in Input():
    print(message)
```

## 4. Transpose a keyboard up an octave

```python
from meedee.messages import NoteOff, NoteOn
from meedee.midi import Input, Output

with Input() as input, Output() as output:
    for m in input:
        output.send(m.transpose(12))
```

Errors are printed on the console. To handle them yourself, use `Input.with_errors()`:

```python
import sys

from meedee.messages import Message, NoteOff, NoteOn
from meedee.midi import Input, Output

with Input() as input, Output() as output:
    for m in input.with_errors():
        if isinstance(m, Message):
            output.send(m.transpose(12))
        else:
            print("ERROR", m, file=sys.stderr)
```

## 5. Split a keyboard into bass and lead

Receive channel 1.

Notes below middle C are sent to channel 2; above that, to channel 3.

Other channel messages, such as sustain and pitch bend, go to both channels.

MIDI messages without channels, like sysexes, are passed through

```python
from meedee.messages import NoteOff, NoteOn, ProgramChange
from meedee.midi import Input, Output

with Input(channel=1) as input, Output() as out:
    out.send(ProgramChange(34, channel=2))
    out.send(ProgramChange(81, channel=3))

    for m in input:
        if hasattr(m, 'note'):
            channel = 2 if m.note < 60 else 3
            out.send(m.replace(channel=channel))
        elif hasattr(m, 'channel'):
            out.send(m.replace(channel=2))
            out.send(m.replace(channel=3))
        else:
            out.send(m)
```

Another way.

```python
from meedee.messages import NoteOff, NoteOn, ProgramChange
from meedee.midi import Input, Output

with Input() as input, Output(channel=2) as o2, Output(channel=3) as o3:
    o2.send(ProgramChange(34))
    o3.send(ProgramChange(81))

    for m in input:
        if hasattr(m, 'note'):
            out = o2 if m.note < 60 else o3
            out.send(m)
        elif hasattr(m, "channel"):
            o2.send(m)
            o3.send(m)
        else:
            o2.send(m)
```

## 6. Record and replay a performance

Record ten seconds in memory, then replay it.

```python
from meedee.midi import Input, Output

Output().send(Input().record(10))
```

Or save the recording and play it later:

```python
from meedee.archive import Reader
from meedee.midi import Input, Output

filename = "rec.meedee"
Input().record(10, output=filename, overwrite=True)

# Later:
Output().send(Reader(filename))
```

## 7. Transpose a MIDI file

Read `song.mid` and write `song-up.mid`, preserving tracks, timing and metadata.
Channel 10 is left unchanged under the GM drum convention. This opens no ports.

```python
from pathlib import Path

from meedee import inspection, messages, smf

print(inspection.inspect_file("song.mid", format="smf"))
reader = smf.Reader("song.mid")
try:
    transformed: list[smf.Event] = []
    for event in reader:
        message = event.record
        if isinstance(
            message, (messages.NoteOn, messages.NoteOff, messages.PolyPressure)
        ):
            if message.channel != 10:
                event = event.replace(record=message.replace(note=message.note + 12))
        transformed.append(event)
    with Path("song-up.mid").open("xb") as stream:
        writer = smf.Writer(stream, header=reader.header)
        for event in transformed:
            writer.write(event)
        writer.finish()
finally:
    reader.close()
print(inspection.inspect_file("song-up.mid", format="smf"))
```

The example holds the file in memory and rejects out-of-range notes before
creating the destination. Exclusive creation preserves an existing destination.

## 8. Back up and restore SysEx

Start a dump on the instrument after launching this capture script. It expects
one complete SysEx message within 30 seconds and creates `backup.syx` only after
successful capture, refusing overwrite. Adjust the count for your instrument.

```python
from meedee import midi, transfer

client = midi.Input(isolated=False)
with client.connect(client.ports()[0]) as receiver:
    capture = transfer.Capture(stream=receiver.stream, expected_messages=1, timeout=30)
    report = transfer.capture(capture, receiver)
print(report)
if report.status != "completed":
    raise RuntimeError(report.reason)
capture.write("backup.syx")
```

Run this separate script to restore it. The first output must be the intended
instrument, ready to receive this dump: restoration can replace sounds/settings.
The 100 ms gap is transfer pacing, not musical timing; use the device's documented
requirement. Successful handoff does not prove the device accepted the dump.

```python
from meedee import midi, transfer

plan = transfer.read_file("backup.syx", gap=0.1)
print(plan)
client = midi.Output()
with client.connect(client.ports()[0]) as output:
    print(transfer.send(transfer.Session(plan), output))
```

## 9. Export a just-intonation tuning

Create `just-three.syx` with keys 69, 70 and 71 tuned to 440, 660 and 880 Hz.
Install the `ufor` extra first. The dump targets MTS device ID 0, tuning program 2;
unselected keys use NoChange. Send it with the restore example when the receiving
synth supports these features and is configured for that device ID.

```python
from pathlib import Path

from ufor.tuning import RatioTable, Tuning

from meedee import codecs, messages, syx, tuning

source = Tuning(source=RatioTable(values=["1", "3/2", "2"]), root_note=69)
pitches = [tuning.convert(source, n) for n in [69, 70, 71]]
print(pitches)
dump = tuning.bulk_dump(
    pitches,
    device_id=0,
    program=2,
    name="Just three",
    preserve_unmapped=True,
)
with Path("just-three.syx").open("xb") as stream:
    syx.write(stream, [messages.SysEx(data=codecs.encode(dump)[1:-1])])
```

After the instrument has loaded it, select the tuning with
`output.send(controls.tuning_program(channel=1, program=2))`, importing `controls`
from `meedee`. Then use the beat-based tune example with notes 69, 70 and 71.
MTS program numbers are 0..127, unlike ordinary program changes.

Only `convert()` needs uFor. The shared Rust builders `bulk_dump()`, `bulk_plan()`
and `note_changes()` also accept `NoteTuning` values constructed directly.
`bulk_plan()` prepares a dump for paced transfer; `note_changes()` produces
ordered messages of at most 127 keys each.

## 10. Bend one note of an MPE chord

Hold C and E for two beats, bending only E up two semitones after the first beat.
This configures an MPE lower zone on channels 2 and 3 with a 48-semitone bend
range. Use an MPE synth and reserve these channels; the configuration remains.

```python
from meedee import controls, events, messages, midi, mpe

zone = mpe.ZoneConfiguration(manager_channel=1, members=2)
allocator = mpe.Allocator(zone=zone)
root = allocator.allocate(note=60, velocity=90)
moving = allocator.allocate(note=64, velocity=75)
bpm = 120
clock = events.Clock(identity="mpe", tick_numerator=60, tick_denominator=bpm)
origin = events.Timestamp(ticks=0, clock=clock)
releases = [allocator.release(voice_id=v.id, velocity=0) for v in [root, moving]]
ending = [*releases, messages.PitchBend(channel=moving.channel, value=0)]
sequence = [
    m.replace(stream="mpe", timestamp=events.Timestamp(ticks=b, clock=clock))
    for b, m in [
        (0, root.note_on),
        (0, moving.note_on),
        (1, messages.PitchBend(channel=moving.channel, value=round(8192 * 2 / 48))),
        *((2, m) for m in ending),
    ]
]
client = midi.Output()
with client.connect(client.ports()[0]) as output:
    output.send(zone)
    for voice in [root, moving]:
        output.send(
            controls.pitch_bend_sensitivity(
                channel=voice.channel, semitones=48, cents=0
            )
        )
        output.send(messages.PitchBend(channel=voice.channel, value=0))
    print(output.send(sequence, origin=origin))
```

The allocator supplies separate member channels and the corresponding releases.
The explicit beat positions here describe expression within a held chord.

## 11. Discover MIDI-CI devices

Connect a MIDI-CI device in both directions. This sends Discovery and Product
Instance ID inquiries, then prints the complete inventory report. The local
identity below is explicitly experimental, using manufacturer ID 0x7D; it does
not identify a commercial product. A fresh local MUID is generated for each scan.

```python
from secrets import randbelow

from meedee import ci, midi

identity = ci.Identity(
    manufacturer=b"\x7d\x00\x00", family=1, model=1, revision=b"\x00" * 4
)
with midi.Input() as receiver, midi.Output() as output:
    browser = ci.Browser(
        local_muid=randbelow(ci.MAX_MUID + 1),
        identity=identity,
        route=ci.Route(input=receiver.stream, output="device"),
        discovery_timeout=3,
        endpoint_timeout=1,
        timeout=30,
    )
    print(ci.browse(browser, receiver, output))
```

The browser handles management replies but performs no Profiles, Property
Exchange or instrument configuration writes. An empty inventory does not prove
the device is offline. For native UMP, set both clients' protocol to
`midi.Protocol.Ump` and set `group=1` on the route; backend support is required.

See the [API contract](https://github.com/rec/meedee/blob/main/plan/api.md) and
[MIDI-CI contract](https://github.com/rec/meedee/blob/main/plan/ci.md) for ownership,
bounds and failure behavior.

## Live routing with progress reports

For a managed route, forward ten seconds from the first input to the first output,
remapping channel messages to channel 2:

```python
from meedee import midi, routing

with midi.Input() as source, midi.Output() as destination:
    router = routing.Router(routes=[routing.Route(
        name="keys", source=source.stream, destination="synth",
        protocol="midi1", remap_channel=2,
    )])
    print(routing.run(router, source, {"synth": destination}, duration=10))
```

The driver stops all routes on known loss, overload or send failure. After an
exception, `router.snapshot()` retains confirmed sends and any uncertain delivery.
It leaves connections open; the surrounding contexts close them using their normal
cleanup policy. Non-channel traffic requires `include_nonchannel=True` on the route.
Report.routes maps route names to read-only RouteReport snapshots, each with
admitted, confirmed and capacity_rejections counts. A snapshot keeps its counts
when routing continues or fails.

## Monitor with typed observations

Inspect ten seconds of input without forwarding it. The callback receives typed
entries; this example prints each message, diagnostic or loss-counter total.

```python
from meedee import inspection, midi


def show(entry: inspection.MonitorEntry) -> None:
    print(entry.message or entry.diagnostic or entry.transport_drop_total)


with midi.Input() as source:
    monitor = inspection.Monitor(stream=source.stream, clock=source.clock)
    print(inspection.run(monitor, source, duration=10, callback=show, metrics=True))
```

Entries also provide `observations` and `note_state`. Set `types=["NoteOn"]`,
`channel=1`, or an explicit UMP `group` on the monitor to select displayed traffic.
Filtering still updates interpretation; diagnostics and loss notifications always
reach the callback. Known loss resets interpretation and monitoring continues.
Terminal input failures raise, with progress available from `monitor.snapshot()`.

The driver requires a fresh monitor and leaves the input open. Omit `callback`
for a summary alone. Callbacks must not reenter the same monitor. No payload
history is retained by the library; the CLI owns bounded previews and display
history. Metrics use host delivery times, independently of source timestamps.

## Step an RPN or NRPN parameter

Increment registered parameter 7 once, then decrement non-registered parameter 7
once. Choose parameter numbers supported by your instrument.

```python
from meedee import messages, midi

with midi.Output() as out:
    out.send(messages.ParameterStep(7))
    out.send(messages.ParameterStep(7, -1, family="nrpn"))
```

`direction` is `1` or `-1`; it is not a step count. Family defaults to `"rpn"`.
The output supplies the channel when it is unassigned. `controls.ParameterStep`
is a separate observation returned when interpreting incoming controller messages.

## Send native MIDI 2 messages

The same channel-message classes support MIDI 2 with `version=2`. Group defaults
to 1, and output resolves the channel. Values use their native MIDI 2 widths:

```python
from meedee.messages import NoteOn, NoteOff
from meedee.midi import Output, Protocol

with Output(protocol=Protocol.Ump) as out:
    out.send(NoteOn(60, 40_000, version=2), "500 ms", NoteOff(60, version=2))
```

This requires a native UMP endpoint. Protocol conversion is always explicit.

Omitted NoteOn velocity uses the scaled equivalent of MIDI 1 velocity 90.
An explicitly supplied velocity is a raw 16-bit value. The same version selection
applies to NoteOff, PolyPressure, ControlChange, ProgramChange, ChannelPressure
and PitchBend. Use `ump.to_midi1()` or `ump.to_midi2()` for conversion;
`replace(version=...)` cannot change the protocol.

## Interpret controller gestures

Move a high-resolution controller or send RPN/NRPN changes from the first input.
The interpreter combines the contributing MIDI 1 messages and reports what was
observed. It also accepts plain messages without source or timing metadata.

```python
from meedee.controls import ControlInterpreter
from meedee.midi import Input

interpreter = ControlInterpreter()
with Input() as source:
    for message in source.with_errors():
        if isinstance(message, Exception):
            raise message
        for observation in interpreter.feed(message):
            print(observation)
```

Use `VelocityInterpreter` instead to interpret MIDI 1 high-resolution velocity
prefixes. Both return a list of observations; `observation.value` holds the
interpreted result or diagnostic. Source and contributor timestamps are optional.
Known-loss diagnostics clear pending fragments and are retained in the result.
Clock changes report `StateReset`. Call `reset()` before switching sources,
including switching between anonymous and named messages. `reset()` returns nothing.
For MIDI 1 inside UMP, select an explicit `group`; native MIDI 2 interpretation
is not performed by these interpreters.

## Track MPE notes and expression

Track the first input using a lower MPE zone. The tracker reports observations
with optional source/timing metadata; `snapshot()` returns current channel state.

```python
from meedee import midi, mpe

tracker = mpe.Tracker(zone=mpe.ZoneConfiguration(manager_channel=1, members=2))
for message in midi.Input():
    for observation in tracker.feed(message):
        print(observation)
```

`configure(zone=...)` clears tracked notes, source binding and timing context.

## Assemble UMP and report an unfinished dump

This example receives only the beginning of a synthetic SysEx7 dump. `finish()`
reports truncation, including group and contributor indexes/timestamps when known.
It clears pending fragments and leaves the assembler reusable.

```python
from meedee import ump

assembler = ump.Assembler()
packets = ump.packetize_sysex7(group=2, data=bytes(range(20)))
assembler.feed(packets[0])
print(assembler.finish())
```

## Decode tuning messages

Use `tuning.decode()` for one complete SysEx message. Use `tuning.Decoder` for
observation lists with source/timing metadata, including `feed_assembled()` for
an observation produced by the UMP assembler.

```python
from meedee import syx, tuning

message = next(syx.Reader("tuning.syx"))
print(tuning.decode(message))
print(tuning.Decoder().feed(message))
```

Both report malformed tuning through the same decoding errors.

## Follow MIDI timecode

Timecode assembly uses an explicit monotonic host clock for its timeout; received
message timestamps remain provenance. `poll(now_ns=...)` checks for timeout during
silence, and `locked` reports whether a continuous running sequence was observed.

```python
from time import monotonic_ns
from meedee import midi, timecode

assembler = timecode.Assembler(timeout="100 ms")
for message in midi.Input():
    for observation in assembler.feed(message, now_ns=monotonic_ns()):
        print(observation)
```

All four classes accept anonymous messages, retain metadata when present and
require `reset()` before changing source identity. Clock changes report
`StateReset`; known-loss diagnostics reset pending state and retain their details.
Use an explicit `group` for MIDI 1 interpretation inside UMP. Timecode reset
preserves the requirement that host time never moves backwards.

## Convert already-parsed file records into timed messages

Use a timeline when records come from your own parsing or editing workflow.
`smf.Timeline` accepts events in merged tick order, applying tempo changes exactly.
For ordinary playback from a file, `smf.TimedReader` performs both steps for you.

```python
from meedee import smf

reader = smf.MergedReader("performance.mid")
timeline = smf.Timeline(reader.header, stream="performance")
for event in reader:
    for message in timeline.feed(event):
        print(message)
print(timeline.finish())
```

For a UMP Clip file, use the corresponding entry converter:

```python
from meedee import clip

timeline = clip.Timeline(stream="performance")
for entry in clip.Reader("performance.clip"):
    for message in timeline.feed(entry):
        print(message)
print(timeline.finish())
```

Both expose `clock` and `origin`. Clip returns `None` until the corresponding
clock/origin is established. `finish()` reports unfinished message fragments.
Select `sequence` explicitly for a format-2 SMF; Clip can include configuration
messages with `include_configuration=True`.

## Inspect packet framing and validate MIDI-file metadata

```python
from meedee import smf, ump

print(ump.word_count(0x40903C00))  # Two 32-bit words in this UMP packet.
smf.validate_meta(81, b"\x07\xa1\x20")  # Valid tempo: 500,000 microseconds/beat.
```

`word_count()` checks the input is an unsigned 32-bit integer and reports framing
length, including reserved message types. It does not validate a packet's contents.
`validate_meta()` returns `None` on success and raises a file-validation error on
failure. Supply `offset=...` to identify a byte position in that error. The SMF
reader and writer already perform this validation during normal file operations.

## Create a virtual MIDI port

On a backend with virtual-port support, create a software source and receive
its messages through the ordinary input API.

```python
from meedee.messages import NoteOn, NoteOff
from meedee.midi import Input, Output

with Output().create_virtual("meedee-example") as out:
    with Input(port="meedee-example") as source:
        out.send(NoteOn(60))
        print(source.receive(timeout=1))
        out.send(NoteOff(60))
```

`Input().create_virtual(name)` creates a software destination that other
applications can send to. Both operations return the connected Input or Output.
Closing is terminal. Rust offers the corresponding `create_virtual()` methods
on `midi::Input` and `midi::Output`.

## Receive raw packets

Print ten received packets with their boundaries, sequence numbers and clocks.
Use `with_errors()` to handle diagnostics explicitly.

```python
from itertools import islice
from meedee.midi import Input

for packet in islice(Input(raw=True), 10):
    print(packet)
```

Rust selects the same mode with `Input::new().raw()?`, which produces a typed
`Input<ReceivedPacket>`. Channel filtering and decoded recording require decoded input.

## Receive through a callback

Create a software destination and wait up to ten seconds for its first delivery.

```python
from queue import Queue
from meedee.midi import Input

received = Queue()
with Input().create_virtual("meedee-callback", callback=received.put):
    print(received.get(timeout=10))
```

Rust configures its callback through `with_callback(...)` before `connect()` or
`create_virtual()`. Its callback receives a typed result containing data, a stream
diagnostic or a terminal error. Both APIs serialize callback delivery and require
callbacks to return promptly. Perform blocking MIDI operations on the controlling
thread. Receive and iteration are unavailable while callbacks own delivery.

## Inspect diagnostics after close

Opened inputs retain their stream, clock, dropped-packet count and terminal
failure after close. This works even when close raises a cleanup error.

```python
from meedee.midi import Input

with Input().create_virtual("meedee-diagnostics") as source:
    print(source.clock)
print(source.closed)
print(source.dropped_packets)
print(source.clock)
print(source.failure)
```

Reading closed, dropped_packets or failure never opens an input. An unopened
input has zero drops and no failure, including after close. Stream and clock
remain available once established. Closing an unopened input leaves those two
properties unavailable. Output also exposes closed, and both kinds of object
become terminally closed even when cleanup fails. Rust provides the corresponding
`closed()`, `stream()`, `clock()`, `dropped_packets()` and `failure()` methods.

## Recognize GM messages in a saved dump

Inspect a saved SysEx dump without opening a MIDI port. The same decoder handles
GM mode changes, controller destinations, key controls and global parameters:

```python
from meedee import gm, syx

with syx.Reader("backup.syx") as dump:
    for message in dump:
        if (decoded := gm.decode(message)) is not None:
            print(decoded)
```

Unknown messages return None; malformed supported messages raise ValueError.
The default max_entries is 1024; pass a different positive limit when needed.
Rust returns the corresponding GmMessage enum from the same decoding operation.

## Read exact recorded times

Timestamp.seconds is an exact Rational value in seconds, independent of the
clock's original tick units. This prints the times attached to a saved performance:

```python
from meedee import archive, messages

with archive.Reader("rec.meedee") as capture:
    for message in capture:
        if isinstance(message, messages.Message) and message.timestamp is not None:
            print(message.timestamp.seconds)
```

Python file functions accept paths or binary readers. Rust keeps separate path
and Read-based entry points, such as inspect_file and inspect. Rust options
structs correspond to Python keyword arguments; they do not require extra Python
objects. Rust with_playback settings persist across play calls, while Python's
play keywords apply to the current call. Both create their playback clock
automatically.
send() leaves a retained output open for subsequent playback or sending; context
exit, close() or destruction performs its usual cleanup. CompoundSendError keeps
the underlying typed failure in __cause__, alongside its confirmed progress.


## Record while processing input

This starts background recording, receives one message or waits up to ten seconds,
then finalizes the file. Recording also works with callbacks and does not depend
on calls to receive.

```python
from meedee import midi

with midi.Input() as source:
    source.start_recording("rec.meedee")
    print(source.receive(timeout=10))
    print(source.stop_recording())
```

The recorder has its own bounded admission budget. A receive-queue overflow does
not by itself discard data admitted to the recorder. Inspect the final report:
loss, unfinished messages, callback failure or writer failure can make it incomplete.
Starting creates or truncates the destination file. Raw inputs cannot attach a
decoded archive recorder.

If stop_recording times out, it stops admitting records but retains the recorder.
Call stop_recording again to obtain the final report, even after closing the input.
A new recording cannot start until finalization succeeds. recording_report keeps
the last finalized report after close.

## Cancel a receive without closing the input

A cancellation token can be shared with another thread to interrupt a wait:

```python
from meedee import events, midi

cancel = events.Cancellation()
with midi.Input() as source:
    print(source.receive(timeout=10, cancel=cancel))
```

Calling cancel.cancel() makes receive raise InterruptedError without consuming a
queued message or closing the input. Omit cancel on the next call to receive again.
None means timeout; after accepted input drains, normal closure raises StateError
and terminal failure raises the retained error. Iteration ends quietly on normal
closure and with_errors yields a terminal input failure once.

last_message_time is local host monotonic seconds at the latest observed raw
packet, before channel filtering. It is None until a packet arrives, updates
independently of consuming messages and remains readable after close. Rust exposes
the same observation as Option<Instant>. Neither is a MIDI timestamp.

A user callback exception stops further delivery and starts safe cleanup. failure
retains that exact exception, while sys.unraisablehook reports it to stderr.
An attached recording is marked incomplete. Do blocking MIDI work on the
controlling thread; receive and iteration remain unavailable in callback mode.


## One send operation

Use send for individual messages, notes, sequences and recordings. Output objects
are not callable. Every successful call returns a PlaybackReport; simple examples
can ignore it.

```python
from meedee.archive import Reader
from meedee.messages import Note
from meedee.midi import Output

with Output() as out:
    print(out.send(Note("500 ms", 60), Reader("rec.meedee"), Note("500 ms", 67)))
```

Each argument follows the previous argument. A direct message sends immediately,
including one carrying receive metadata. Notes, rests and delays supply sequence
timing. An iterable of timestamped messages uses relative recorded timing,
starting when that argument is reached. An untimed iterable uses note/rest/delay
timing. Mixing timed and untimed values inside one iterable raises an error;
no missing timing is inferred. For a streamed source, a later error can follow
an already-sent prefix. The error's report retains that confirmed progress.
Execution failures preserve their original exception type, such as SendError or
CompoundSendError, with report and completed_events attached. Validation before
execution can raise without those progress attributes.

send accepts the former playback options: origin, mappings, cancel,
allow_rounding and metrics. A cancelled call returns a report with cancelled=True
and leaves the output open. Empty input opens no ports. Cancellation can leave a
note active until explicit release or normal output cleanup.
