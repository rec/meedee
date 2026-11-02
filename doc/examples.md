# Practical Python MIDI examples

Start with one note, then work through timing, live input, files and instrument
management. Each code block is a separate script. Use Python 3.13 and install
with `uv sync --locked`; the tuning example also needs `--extra ufor`.
See [build requirements](https://github.com/rec/meedee/blob/main/README.md#build-and-qualification).

The live examples use the **first available port** and hard-coded settings.
Connect a suitable keyboard or synth before running them. MIDI carries messages,
not audio; configure the synth's sound and audio output separately. Channels are
1..16, notes and velocities are 0..127, and program numbers are 1..128.

Outputs run in-process by default. Use `isolated=True` for worker failure
containment; neither mode requires a main guard. Input clients still default to
isolation. Native MIDI 1 output tracks notes and selected controls for cleanup
when explicitly closed or when leaving its context.

## 1. Play one note

Play middle C for half a second on the first output.

```python
from meedee.messages import Note
from meedee.midi import Output

with Output() as out:
    out(Note("500 ms", 60))
```

`Note` sends both the attack and release. Velocity defaults to 90; the channel
comes from the output, which defaults to 1. Use `Output(channel=2)` to change it.

## 2. Play a tune in beats

Set tempo once. Each note or rest advances the tune by its duration.

```python
from meedee.messages import Note, Rest
from meedee.midi import Output

with Output(bpm=120) as out:  # Numeric durations are beats.
    out(
        Note(duration=1, note=60, velocity=90),
        Note(0.5, 62, 70),
        Note(0.5, 64, 100),
        Rest("1 beat"),
        Note(2, 64, 100),
    )
```

One beat is a quarter note. Without `bpm`, numeric durations mean seconds.
Explicit time units keep their meaning at any tempo; explicit beats require
`bpm`. The clock and scheduling are automatic, with no `Event` objects or sleeps.

`Output(port="FLOW 8")` selects a unique name substring; `*` and `?` support
wildcard matching. Two-stage `client.connect(client.ports()[0])` also works.
For overlapping attacks, use `NoteOn` with delays and explicit `NoteOff` releases;
`Note` instead releases before the next item starts. Numeric delays follow the
same tempo rule as notes and rests.

Context exit releases outstanding tracked MIDI 1 notes and resets changed pitch
bend, modulation, sustain, sostenuto, soft pedal and channel/polyphonic pressure.
Volume, pan, expression, programs and tuning are left alone. Reserve the channels
for these examples; transport failure can prevent cleanup. UMP requires explicit
addressing and does not support these full-note values or automatic cleanup.

## 3. See what your keyboard sends

Print messages from the first input. Stop with Ctrl-C.

```python
from meedee.midi import Input

for message in Input():
    print(message)
```

Iteration yields individual MIDI messages, never `Note` values. Recoverable input
errors are printed to stderr and reception continues; a terminal input failure
is reported and ends iteration. Setup, close and output failures still raise.
Use `receive()` when you need timestamped events, or `Input(raw=True)` for raw
transport packets.

## 4. Transpose a keyboard up an octave

Use separate input and output devices to avoid a feedback loop.

```python
from meedee.messages import NoteOff, NoteOn
from meedee.midi import Input, Output

with Input() as input, Output() as output:
    for m in input:
        if isinstance(m, (NoteOn, NoteOff)):
            output(m.transpose(12))
        else:
            output(m)
```

`transpose()` returns a new message and clamps the note to 0..127. To handle
input diagnostics yourself, use `with_errors()`:

```python
import sys

from meedee.messages import Message, NoteOff, NoteOn
from meedee.midi import Input, Output

with Input() as input, Output() as output:
    for m in input.with_errors():
        if isinstance(m, (NoteOn, NoteOff)):
            output(m.transpose(12))
        elif isinstance(m, Message):
            output(m)
        else:
            print("ERROR", m, file=sys.stderr)
```

## 5. Split a keyboard into bass and lead

Receive channel 1 and split at middle C. Other channel messages, such as sustain
and pitch bend, go to both parts.

```python
from meedee.messages import NoteOff, NoteOn, ProgramChange
from meedee.midi import Input, Output

with Input(channel=1) as input, Output() as out:
    out(ProgramChange(34, channel=2))
    out(ProgramChange(81, channel=3))

    for m in input:
        if isinstance(m, (NoteOn, NoteOff)):
            if m.note < 60:
                out(m.replace(channel=2))
            else:
                out(m.replace(channel=3))
        else:
            out(m.replace(channel=2))
            out(m.replace(channel=3))
```

`Input(channel=1)` accepts only channel-1 messages. A container such as
`Input(channel=[1, 4])` selects several channels. Channel-less traffic is excluded;
diagnostics are always retained. `replace()` returns a new message.

Alternatively, give each output a channel. An explicitly configured output
channel overrides the channel on every MIDI 1 channel message sent through it:

```python
from meedee.messages import NoteOff, NoteOn, ProgramChange
from meedee.midi import Input, Output

with Input() as input, Output(channel=2) as out2, Output(channel=3) as out3:
    out2(ProgramChange(34))
    out3(ProgramChange(81))

    for m in input:
        if isinstance(m, (NoteOn, NoteOff)):
            if m.note < 60:
                out2(m)
            else:
                out3(m)
        else:
            out2(m)
            out3(m)
```

Use `Output()` for multiple channels: it preserves assigned message channels
and uses channel 1 for unassigned messages. `Output(channel=1)` explicitly forces
channel 1. The override applies to MIDI 1 message objects, full notes and compound
commands; encoded packets and timestamped `play()` retain their explicit addressing.

## 6. Record and replay a performance

Record ten seconds in memory, then replay it through the first output.

```python
from meedee.midi import Input, Output

Output().play(Input().record(10))
```

Or save the recording and play it later:

```python
from meedee.archive import Reader
from meedee.midi import Input, Output

filename = "rec.meedee"
Input().record(10, output=filename, overwrite=True)

# Later:
Output().play(Reader(filename))
```

`record()` returns the captured records, including any diagnostics. File output
writes the same capture; existing files are protected unless `overwrite=True`.
Playback preserves timing between messages and trims initial silence. Both calls
close their connections automatically, and `play()` closes an archive reader.

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
from meedee import midi, transfer, transfer_input

client = midi.Input(isolated=False)
with client.connect(client.ports()[0]) as receiver:
    capture = transfer.Capture(stream=receiver.stream, expected_messages=1, timeout=30)
    report = transfer_input.run(capture, receiver)
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
from meedee import midi, transfer, transfer_output

plan = transfer.read_file("backup.syx", gap=0.1)
print(plan)
client = midi.Output()
with client.connect(client.ports()[0]) as output:
    print(transfer_output.run(transfer.Session(plan), output))
```

## 9. Export a just-intonation tuning

Create `just-three.syx` with keys 69, 70 and 71 tuned to 440, 660 and 880 Hz.
Install the `ufor` extra first. The dump targets MTS device ID 0, tuning program 2;
unselected keys use NoChange. Send it with the restore example when the receiving
synth supports these features and is configured for that device ID.

```python
from pathlib import Path

from ufor.tuning import RatioTable, Tuning

from meedee import codecs, messages, syx, ufor_tuning

source = Tuning(source=RatioTable(values=["1", "3/2", "2"]), root_note=69)
preview = ufor_tuning.preview(source, notes=[69, 70, 71])
print(preview)
dump = ufor_tuning.bulk_dump(
    preview,
    device_id=0,
    program=2,
    name="Just three",
    preserve_unmapped=True,
)
with Path("just-three.syx").open("xb") as stream:
    syx.write(stream, [messages.SysEx(data=codecs.encode(dump)[1:-1])])
```

After the instrument has loaded it, select the tuning with
`output(controls.tuning_program(channel=1, program=2))`, importing `controls`
from `meedee`. Then use the beat-based tune example with notes 69, 70 and 71.
MTS program numbers are 0..127, unlike ordinary program changes.

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
    events.Event(
        stream="mpe", timestamp=events.Timestamp(ticks=b, clock=clock), message=m
    )
    for b, m in [
        (0, root.note_on),
        (0, moving.note_on),
        (1, messages.PitchBend(channel=moving.channel, value=round(8192 * 2 / 48))),
        *((2, m) for m in ending),
    ]
]
client = midi.Output()
with client.connect(client.ports()[0]) as output:
    output(zone)
    for voice in [root, moving]:
        output(
            controls.pitch_bend_sensitivity(
                channel=voice.channel, semitones=48, cents=0
            )
        )
        output(messages.PitchBend(channel=voice.channel, value=0))
    print(output.play(sequence, origin=origin))
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

from meedee import ci, ci_live, midi

identity = ci.Identity(
    manufacturer=b"\x7d\x00\x00", family=1, model=1, revision=b"\x00" * 4
)
inputs = midi.Input(isolated=False)
outputs = midi.Output()
with (
    inputs.connect(inputs.ports()[0]) as receiver,
    outputs.connect(outputs.ports()[0]) as output,
):
    browser = ci.Browser(
        local_muid=randbelow(ci.MAX_MUID + 1),
        identity=identity,
        route=ci.Route(input=receiver.stream, output="device"),
        discovery_timeout=3,
        endpoint_timeout=1,
        timeout=30,
    )
    print(ci_live.run(browser, receiver, output))
```

The browser handles management replies but performs no Profiles, Property
Exchange or instrument configuration writes. An empty inventory does not prove
the device is offline. For native UMP, set both clients' protocol to
`midi.Protocol.Ump` and set `group=1` on the route; backend support is required.

See the [API contract](https://github.com/rec/meedee/blob/main/plan/api.md) and
[MIDI-CI contract](https://github.com/rec/meedee/blob/main/plan/ci.md) for ownership,
bounds and failure behavior.
