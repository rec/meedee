# Ten practical Python MIDI projects

Each example is a complete program with an observable result: sound from an
instrument, useful live output, a recorded performance or a new file. They build
from playing a melody to managing tuning, expression and device discovery.

Use Python 3.13. In this checkout, install with `uv sync --locked`, save a code
block as a `.py` file, and run it with `uv run your_example.py`. Example 8 also
needs `uv sync --locked --extra ufor`. Source-build requirements are in
[README.md](https://github.com/rec/meedee/blob/main/README.md#build-and-qualification).

The live examples list ports and ask for exact names. An input is a source your
program receives from; an output is a destination it sends to. Names must match
one port in that client's current discovery snapshot. Connect your hardware or
start your software instrument before running a script. A MIDI connection carries
messages, not audio: the instrument still needs its own audio output configured.

Keep the `if __name__ == "__main__":` guards. The default Python transport uses
process isolation on macOS, Windows and Linux. These programs use MIDI 1, which
works with ordinary keyboards and synths; native backend availability depends
on the platform and build.

## 1. Play a melody on a synthesizer

**Result:** the selected instrument plays an ascending C-major scale.

**You need:** a hardware synth or software instrument listening on MIDI channel
1. Select a playable sound and reserve channel 1 for this example. Save as
`melody.py`.

```python
from meedee import events, messages, midi


def main() -> None:
    client = midi.Output()
    ports = client.ports()
    for port in ports:
        print(port.name)
    name = input("Synth input port: ")
    (port,) = [p for p in ports if p.name == name]

    # One tick is one millisecond; every event has a deadline from the same zero.
    clock = events.Clock(identity="melody", tick_numerator=1, tick_denominator=1000)
    origin = events.Timestamp(ticks=0, clock=clock)
    notes = [60, 62, 64, 65, 67, 69, 71, 72]
    sequence: list[events.Event] = []
    for index, note in enumerate(notes):
        start = index * 350
        sequence.extend(
            [
                events.Event(
                    stream="melody",
                    timestamp=events.Timestamp(ticks=start, clock=clock),
                    message=messages.NoteOn(channel=1, note=note, velocity=90),
                ),
                events.Event(
                    stream="melody",
                    timestamp=events.Timestamp(ticks=start + 300, clock=clock),
                    message=messages.NoteOff(channel=1, note=note, velocity=0),
                ),
            ]
        )

    with client.connect(port) as output:
        finished = False
        try:
            print("Playing the scale with 300 ms notes and 50 ms gaps.")
            report = output.play(sequence, origin=origin, metrics=True)
            finished = not report.cancelled
            print(
                f"Sent {report.sent_events} events; "
                f"max lateness={report.max_lateness_ns} ns"
            )
        finally:
            if not finished:
                for note in notes:
                    output.send(messages.NoteOff(channel=1, note=note, velocity=0))
    print("Melody finished.")


if __name__ == "__main__":
    main()
```

Message constructors use channels 1..16 and MIDI 1 notes/velocities 0..127.
`output.play()` schedules the timestamped sequence against one monotonic origin,
so send overhead does not accumulate into later note deadlines. The timestamps
are exact musical intentions; operating-system timing and output handoff can
still be late. Metrics report host lateness, not physical audio latency.

The sequence includes its NoteOffs. If playback is interrupted, the `finally`
block attempts releases for the phrase's keys on the reserved channel. A failed
send still leaves device state uncertain; closing a port does not silence an
instrument. No bank or program changes are needed for this first example.

## 2. See what your keyboard is sending

**Result:** playing the keyboard prints note names, channels, velocities and
press/release actions for 30 seconds. Use this to check a keyboard connection or
identify the channel it transmits on. Save as `keyboard_monitor.py`.

**You need:** a MIDI keyboard. No output instrument is required.

```python
from time import monotonic

from meedee import events, messages, midi


def note_name(note: int) -> str:
    names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]
    return f"{names[note % 12]}{note // 12 - 1}"


def main() -> None:
    client = midi.TypedInput()
    ports = client.ports()
    for port in ports:
        print(port.name)
    name = input("Keyboard output port: ")
    (port,) = [p for p in ports if p.name == name]

    with client.connect(port) as receiver:
        deadline = monotonic() + 30
        print("Play some notes. Middle C is labelled C4 here.")
        while monotonic() < deadline:
            if (failure := receiver.failure) is not None:
                raise failure
            if receiver.dropped_packets:
                raise RuntimeError("Input lost packets; restart the monitor.")
            item = receiver.receive(timeout=0.05)
            if isinstance(item, events.StreamDiagnostic):
                raise RuntimeError(item.reason)
            if not isinstance(item, events.Event):
                continue
            message = item.message
            if isinstance(message, (messages.NoteOn, messages.NoteOff)):
                pressed = isinstance(message, messages.NoteOn) and message.velocity > 0
                action = "press" if pressed else "release"
                print(
                    f"{note_name(message.note):3}  channel={message.channel:2}  "
                    f"{action:7}  velocity={message.velocity:3}"
                )
    print("Monitoring finished.")


if __name__ == "__main__":
    main()
```

Typed input decodes packets into messages and retains their timestamp and source
span in `events.Event`. A zero-velocity NoteOn is displayed as a release.
Diagnostics and the transport drop counter are separate loss signals; neither
should be silently filtered out. Slow terminal output can itself overload input.

## 3. Turn your keyboard into an octave transposer

**Result:** notes played on channel 1 reach a second instrument one octave higher.
Note releases and polyphonic pressure are transposed consistently. Controllers,
channel pressure and pitch bend on channel 1 pass through unchanged.

**You need:** separate keyboard input and synth output ports. Avoid selecting a
loopback that feeds the program's output back into its input. Use a synth on
channel 1. Save as `transpose_keyboard.py`.

```python
from time import monotonic

from meedee import events, messages, midi


def main() -> None:
    inputs = midi.TypedInput()
    outputs = midi.Output()
    sources = inputs.ports()
    destinations = outputs.ports()
    print("Inputs:", [p.name for p in sources])
    print("Outputs:", [p.name for p in destinations])
    source_name = input("Keyboard output port: ")
    destination_name = input("Synth input port: ")
    (source,) = [p for p in sources if p.name == source_name]
    (destination,) = [p for p in destinations if p.name == destination_name]

    with inputs.connect(source) as receiver, outputs.connect(destination) as output:
        print("Transposing +12 for 60 seconds. Release keys and pedals before it ends.")
        deadline = monotonic() + 60
        while monotonic() < deadline:
            if (failure := receiver.failure) is not None:
                raise failure
            if receiver.dropped_packets:
                raise RuntimeError("Input loss; forwarding stopped.")
            item = receiver.receive(timeout=0.05)
            if isinstance(item, events.StreamDiagnostic):
                raise RuntimeError(item.reason)
            if not isinstance(item, events.Event):
                continue
            message = item.message
            if isinstance(
                message, (messages.NoteOn, messages.NoteOff, messages.PolyPressure)
            ):
                if message.channel == 1 and message.note + 12 <= 127:
                    output.send(message.replace(note=message.note + 12))
            elif isinstance(message, messages.ControlChange):
                if message.channel == 1 and message.controller < 120:
                    output.send(message)
            elif isinstance(message, (messages.ChannelPressure, messages.PitchBend)):
                if message.channel == 1:
                    output.send(message)
    print("Transposer stopped.")


if __name__ == "__main__":
    main()
```

The highest octave is omitted because transposing it would exceed note 127.
The same rule applies to releases. System messages, SysEx, program changes and
channel-mode controls are excluded. This forwarder does not claim ownership of
the synth and sends no reset on exit. If forwarding is interrupted while notes
or pedals are active, use the instrument's own panic/reset control.

## 4. Split a keyboard into bass and lead parts

**Result:** keys below middle C play a bass sound on channel 2; middle C and above
play a lead sound on channel 3. Modulation, volume, expression, sustain, pitch
bend and channel pressure affect both parts.

**You need:** a multitimbral GM-compatible instrument with channels 2 and 3
available, and a keyboard transmitting on channel 1. The program explicitly sets
GM program 34 (fingered electric bass) and 81 (square lead), without sending a GM
reset. Save as `keyboard_split.py`.

```python
from time import monotonic

from meedee import events, messages, midi


def main() -> None:
    inputs = midi.TypedInput()
    outputs = midi.Output()
    sources = inputs.ports()
    destinations = outputs.ports()
    print("Inputs:", [p.name for p in sources])
    print("Outputs:", [p.name for p in destinations])
    source_name = input("Keyboard output port: ")
    destination_name = input("Multitimbral synth input port: ")
    (source,) = [p for p in sources if p.name == source_name]
    (destination,) = [p for p in destinations if p.name == destination_name]

    with inputs.connect(source) as receiver, outputs.connect(destination) as output:
        output.send(messages.ProgramChange(channel=2, program=34))
        output.send(messages.ProgramChange(channel=3, program=81))
        print("Split at C4 for 60 seconds. Release keys and pedals before it ends.")
        deadline = monotonic() + 60
        while monotonic() < deadline:
            if (failure := receiver.failure) is not None:
                raise failure
            if receiver.dropped_packets:
                raise RuntimeError("Input loss; split forwarding stopped.")
            item = receiver.receive(timeout=0.05)
            if isinstance(item, events.StreamDiagnostic):
                raise RuntimeError(item.reason)
            if not isinstance(item, events.Event):
                continue
            message = item.message
            if isinstance(
                message, (messages.NoteOn, messages.NoteOff, messages.PolyPressure)
            ):
                if message.channel == 1:
                    channel = 2 if message.note < 60 else 3
                    output.send(message.replace(channel=channel))
            elif isinstance(message, messages.ControlChange):
                if message.channel == 1 and message.controller in (1, 7, 11, 64):
                    for channel in [2, 3]:
                        output.send(message.replace(channel=channel))
            elif isinstance(message, (messages.PitchBend, messages.ChannelPressure)):
                if message.channel == 1:
                    for channel in [2, 3]:
                        output.send(message.replace(channel=channel))
    print("Keyboard split stopped.")


if __name__ == "__main__":
    main()
```

Unlike static channel/group routing, this split needs application logic to inspect
the note number. The split point stays fixed, so a release always goes to the
same part as its NoteOn. As in example 3, an interrupted route sends no automatic
cleanup; release keys/pedals before the observation window ends.

## 5. Record a performance, then hear it back

**Result:** records ten seconds of keyboard input into a uniquely named archive,
reports capture quality, and plays the take through a selected instrument.
The archive remains on disk for later inspection or playback.

**You need:** a keyboard and a listening synth. The synth must accept the
keyboard's channels. Recording is passive; there is no live thru in this example.
Save as `record_and_replay.py`.

```python
from pathlib import Path
from time import monotonic
from uuid import uuid4

from meedee import archive, events, inspection, midi


def main() -> None:
    path = Path(f"take-{uuid4().hex}.meedee")
    inputs = midi.TypedInput()
    ports = inputs.ports()
    print("Inputs:", [p.name for p in ports])
    name = input("Keyboard output port: ")
    (port,) = [p for p in ports if p.name == name]
    with inputs.connect(port) as receiver:
        receiver.start_recording(path, label="Keyboard performance", metrics=True)
        try:
            print("Recording for 10 seconds. Release keys and pedals before the end.")
            deadline = monotonic() + 10
            while monotonic() < deadline:
                if (failure := receiver.failure) is not None:
                    raise failure
                receiver.receive(timeout=0.05)
        finally:
            recorded = receiver.stop_recording()
    print(f"Saved {path}: {recorded.records} records, pressure={recorded.pressure}")
    if not recorded.complete:
        raise RuntimeError(f"Incomplete recording: {recorded.failure}")
    quality = inspection.inspect_file(path, format="archive")
    if not quality.complete or quality.gap_records or quality.timestamp_regressions:
        raise RuntimeError(f"Take failed quality checks: {quality.problem}")

    reader = archive.Reader(path)
    origin: events.Timestamp | None = None
    try:
        for record in reader:
            if isinstance(record, events.Event) and record.timestamp is not None:
                origin = record.timestamp
                break
    finally:
        reader.close()
    if origin is None:
        raise RuntimeError("No timestamped events were recorded.")

    outputs = midi.Output()
    ports = outputs.ports()
    print("Outputs:", [p.name for p in ports])
    name = input("Synth input port for replay: ")
    (port,) = [p for p in ports if p.name == name]
    reader = archive.Reader(path)
    try:
        with outputs.connect(port) as output:
            played = output.play(reader, origin=origin, metrics=True)
            print(f"Replayed {played.sent_events} events.")
            print(f"Maximum host lateness: {played.max_lateness_ns} ns")
    finally:
        reader.close()


if __name__ == "__main__":
    main()
```

Recording is bounded and reports loss/write failures. The unique destination
avoids intentionally overwriting an earlier take; the recorder itself would
truncate an existing path. The application event queue is drained while the
background recorder writes. Full inspection happens before output starts.

Replay starts at the first timestamped event, intentionally trimming initial
silence. Capture timestamps retain relative timing, not a promise of physical
latency. Playback can include controllers and SysEx received from the keyboard.
Stopping playback or reaching the end of a take does not generate missing
NoteOffs. Archive v1 is currently unpublished; see
[the format contract](https://github.com/rec/meedee/blob/main/plan/archive-format.md).

## 6. Export a transposed MIDI file

**Result:** reads a `.mid` file, prints its format and message counts, then writes
a new file one octave higher. The output can be imported into a DAW or notation
program. No MIDI ports are opened. Save as `transpose_file.py`.

**You need:** an existing Standard MIDI File. Channel 10 is left unchanged under
the usual GM drum convention. The program changes notes and polyphonic pressure,
while retaining tracks, ticks, tempo, other metadata and file records.

```python
from pathlib import Path

from meedee import inspection, messages, smf


def main() -> None:
    source = Path(input("Source .mid file: "))
    destination = Path(input("New transposed .mid file: "))
    report = inspection.inspect_file(source, format="smf")
    print("Header:", report.smf_header)
    print("Messages:", report.message_counts)
    if not report.complete:
        raise RuntimeError(report.problem)

    # Check the range before creating any output file.
    reader = smf.Reader(source)
    try:
        for event in reader:
            message = event.record
            if isinstance(
                message, (messages.NoteOn, messages.NoteOff, messages.PolyPressure)
            ):
                if message.channel != 10 and message.note > 115:
                    raise ValueError("Transposition would exceed MIDI note 127.")
    finally:
        reader.close()

    reader = smf.Reader(source)
    count = 0
    try:
        with destination.open("xb") as stream:
            writer = smf.Writer(stream, header=reader.header)
            for event in reader:
                message = event.record
                if isinstance(
                    message, (messages.NoteOn, messages.NoteOff, messages.PolyPressure)
                ):
                    if message.channel != 10:
                        event = event.replace(
                            record=message.replace(note=message.note + 12)
                        )
                        count += 1
                writer.write(event)
            writer.finish()
    finally:
        reader.close()
    print(f"Wrote {destination}; transposed {count} note/pressure records.")


if __name__ == "__main__":
    main()
```

The destination is exclusively created, so existing files are preserved. A disk
or later read failure can leave a partial new file. This preserves decoded file
records; running-status byte encoding need not be identical. For direct playback
of a file, `smf.TimedReader` supplies tempo-aware performance events to
`output.play(score, origin=score.origin)`.

## 7. Back up and restore a synthesizer dump

**Result:** either captures an instrument-initiated SysEx dump into a new `.syx`
file, or sends an existing backup back to the selected instrument with explicit
pacing. Save as `sysex_backup.py` and run it once per operation.

**You need:** an instrument with a documented MIDI 1 SysEx dump function. Find
its expected message count and minimum inter-message gap in its documentation.
Restoring a dump can replace sounds or settings on the instrument. Use a backup
for the intended device and select its port deliberately.

```python
from pathlib import Path

from meedee import midi, transfer, transfer_input, transfer_output


def backup(path: Path) -> None:
    count = int(input("Expected SysEx message count: "))
    timeout = float(input("Overall capture timeout in seconds: "))
    client = midi.TypedInput()
    ports = client.ports()
    print("Inputs:", [p.name for p in ports])
    name = input("Instrument output port: ")
    (port,) = [p for p in ports if p.name == name]
    with client.connect(port) as receiver:
        capture = transfer.Capture(
            stream=receiver.stream,
            expected_messages=count,
            timeout_ns=int(timeout * 1_000_000_000),
        )
        print("Input is open. Start the dump on the instrument now.")
        report = transfer_input.run(capture, receiver)
    if report.status != "completed":
        raise RuntimeError(f"Capture {report.status}: {report.reason}")
    capture.write(path)
    print(
        f"Saved {report.received_messages} messages, "
        f"{report.wire_bytes} bytes to {path}."
    )


def restore(path: Path) -> None:
    gap_ms = float(input("Minimum gap after each message, in milliseconds: "))
    plan = transfer.read_file(path, gap_ns=int(gap_ms * 1_000_000))
    print(f"Ready to send {plan.message_count} messages, {plan.wire_bytes} bytes.")
    client = midi.Output()
    ports = client.ports()
    print("Outputs:", [p.name for p in ports])
    name = input("Instrument input port to restore: ")
    (port,) = [p for p in ports if p.name == name]
    input("Prepare the instrument to receive its dump, then press Enter to send: ")
    with client.connect(port) as output:
        report = transfer_output.run(transfer.Session(plan), output)
    print(
        f"{report.status}: {report.confirmed_messages}/"
        f"{report.total_messages} handed off."
    )
    if report.status != "completed":
        raise RuntimeError(
            f"Transfer stopped: {report.reason}; "
            f"uncertain message={report.uncertain_message}"
        )


def main() -> None:
    operation = input("Operation (backup or restore): ")
    path = Path(input(".syx file: "))
    if operation == "backup":
        backup(path)
    elif operation == "restore":
        restore(path)
    else:
        raise ValueError("Choose backup or restore.")


if __name__ == "__main__":
    main()
```

Backup creates no file after receive loss, timeout or decoding failure. Its path
writer refuses overwrite. Reaching the supplied count does not prove the
instrument's entire dump was captured. Final disk errors can leave a partial file.
Restore sends whole messages, with no splitting, retries or inferred device
acknowledgements. Successful output handoff does not establish device acceptance.

## 8. Load a just-intonation tuning and audition it

**Result:** saves a three-pitch MTS dump, loads it into tuning program 2, selects
that tuning for channel 1 and plays the three keys. The intended pitches are
440 Hz, 660 Hz and 880 Hz rather than adjacent equal-tempered semitones.

**You need:** `meedee[ufor]` and a synth supporting ordinary MTS bulk dumps,
NoChange entries and tuning-program selection by RPN. Choose the instrument's
specific device ID, 0..126. This replaces three entries of tuning program 2;
back up that program first if needed. Save as `audition_tuning.py`.

```python
from pathlib import Path
from uuid import uuid4

from ufor.tuning import RatioTable, Tuning

from meedee import codecs, controls, events, messages, midi, syx, ufor_tuning


def main() -> None:
    device_id = int(input("Instrument MTS device ID (0..126): "))
    if not 0 <= device_id <= 126:
        raise ValueError("Use a specific device ID from 0 through 126.")
    source = Tuning(source=RatioTable(values=["1", "3/2", "2"]), root_note=69)
    preview = ufor_tuning.preview(source, notes=[69, 70, 71])
    for key in preview.entries:
        print(f"Key {key.note}: {key.frequency} Hz; error={key.error_cents} cents")
    dump = ufor_tuning.bulk_dump(
        preview,
        device_id=device_id,
        program=2,
        name="Just three",
        preserve_unmapped=True,
    )
    raw = messages.SysEx(data=codecs.encode(dump)[1:-1])
    path = Path(f"just-three-{uuid4().hex}.syx")
    with path.open("xb") as stream:
        syx.write(stream, [raw])
    print(f"Saved tuning dump to {path}.")

    clock = events.Clock(identity="audition", tick_numerator=1, tick_denominator=1000)
    origin = events.Timestamp(ticks=0, clock=clock)
    sequence: list[events.Event] = []
    for index, note in enumerate([69, 70, 71, 70, 69]):
        start = index * 600
        sequence.extend(
            [
                events.Event(
                    stream="audition",
                    timestamp=events.Timestamp(ticks=start, clock=clock),
                    message=messages.NoteOn(channel=1, note=note, velocity=90),
                ),
                events.Event(
                    stream="audition",
                    timestamp=events.Timestamp(ticks=start + 600, clock=clock),
                    message=messages.NoteOff(channel=1, note=note, velocity=0),
                ),
            ]
        )

    client = midi.Output()
    ports = client.ports()
    print("Outputs:", [p.name for p in ports])
    name = input("MTS synth input port: ")
    (port,) = [p for p in ports if p.name == name]
    with client.connect(port) as output:
        output.send(raw)
        input("Wait for the synth to finish loading, then press Enter to audition: ")
        output.send(controls.tuning_program(channel=1, program=2))
        finished = False
        try:
            print("Auditioning keys 69, 70, 71, 70, 69 with 600 ms per note.")
            report = output.play(sequence, origin=origin, metrics=True)
            finished = not report.cancelled
            print(
                f"Sent {report.sent_events} events; "
                f"max lateness={report.max_lateness_ns} ns"
            )
        finally:
            if not finished:
                for note in [69, 70, 71]:
                    output.send(messages.NoteOff(channel=1, note=note, velocity=0))
    print("Audition finished; tuning program 2 remains loaded and selected.")


if __name__ == "__main__":
    main()
```

Unselected keys are explicitly preserved with NoChange. MTS tuning programs are
0..127, unlike ordinary `ProgramChange.program`, which is 1..128. The library can
construct and send the tuning but cannot establish that an instrument applied it.
The wait is for the operator to confirm readiness, not an inferred acknowledgement.
Reserve channel 1 for the audition. As in example 1, playback schedules both
attacks and releases, with explicit release attempts if the sequence is interrupted.

## 9. Bend one note in a chord with MPE

**Result:** holds C and E while bending only E up about two semitones, increasing
its pressure and moving its timbre controller. C remains at its original pitch.

**You need:** an MPE-capable instrument with an expressive preset. This configures
a lower zone with two members (channels 2 and 3), sets their bend range to ±48
semitones and leaves that configuration in place. Use these channels exclusively
for the example. Save as `mpe_expression.py`.

```python
from meedee import controls, events, messages, midi, mpe


def main() -> None:
    client = midi.Output()
    ports = client.ports()
    print("Outputs:", [p.name for p in ports])
    name = input("MPE synth input port: ")
    (port,) = [p for p in ports if p.name == name]
    zone = mpe.ZoneConfiguration(manager_channel=1, members=2)
    allocator = mpe.Allocator(zone=zone)
    root = allocator.allocate(note=60, velocity=90)
    moving = allocator.allocate(note=64, velocity=90)

    clock = events.Clock(identity="mpe-phrase", tick_numerator=1, tick_denominator=1000)
    origin = events.Timestamp(ticks=0, clock=clock)
    sequence: list[events.Event] = []
    for voice in [root, moving]:
        initial = [
            messages.PitchBend(channel=voice.channel, value=0),
            messages.ChannelPressure(channel=voice.channel, pressure=0),
            messages.ControlChange(channel=voice.channel, controller=74, value=64),
            voice.note_on,
        ]
        sequence.extend(
            events.Event(
                stream="mpe",
                timestamp=origin,
                message=m,
            )
            for m in initial
        )
    for step in range(41):
        changes = [
            messages.PitchBend(
                channel=moving.channel, value=round(8192 * 2 / 48 * step / 40)
            ),
            messages.ChannelPressure(
                channel=moving.channel, pressure=round(127 * step / 40)
            ),
            messages.ControlChange(
                channel=moving.channel, controller=74, value=64 + round(63 * step / 40)
            ),
        ]
        sequence.extend(
            events.Event(
                stream="mpe",
                timestamp=events.Timestamp(ticks=step * 50, clock=clock),
                message=m,
            )
            for m in changes
        )
    # Build releases from the allocator's voice tokens before playing the phrase.
    releases = [allocator.release(voice_id=v.id, velocity=0) for v in [root, moving]]
    ending: list[messages.Message] = [*releases]
    ending.extend(
        messages.PitchBend(channel=v.channel, value=0) for v in [root, moving]
    )
    sequence.extend(
        events.Event(
            stream="mpe",
            timestamp=events.Timestamp(ticks=2500, clock=clock),
            message=m,
        )
        for m in ending
    )

    with client.connect(port) as output:
        output.send(zone)
        for voice in [root, moving]:
            output.send(
                controls.pitch_bend_sensitivity(
                    channel=voice.channel, semitones=48, cents=0
                )
            )
        finished = False
        try:
            print(
                "Holding C while E bends upward with independent pressure and timbre."
            )
            report = output.play(sequence, origin=origin, metrics=True)
            finished = not report.cancelled
            print(
                f"Sent {report.sent_events} events; "
                f"max lateness={report.max_lateness_ns} ns"
            )
        finally:
            if not finished:
                for message in ending:
                    output.send(message)
    print("MPE phrase finished.")


if __name__ == "__main__":
    main()
```

The allocator assigns separate member channels and returns voice tokens used to
construct the correct releases. The sequence schedules expression changes every
50 ms through 2 seconds, then releases the chord at 2.5 seconds. Events sharing a
timestamp retain list order. The receiving preset decides how pressure and CC74
sound. A successful send does not prove it accepted the MPE configuration or
bend range. NoteOffs and bend resets are scheduled in the phrase; interrupted
playback attempts that same ending immediately. Cleanup affects this program's
reserved member channels, without issuing a global All Notes Off or reset; a
transport failure can still prevent release.

## 10. Inventory MIDI-CI devices on a connection

**Result:** sends MIDI-CI Discovery, requests Product Instance IDs from discovered
format-2 peers, and prints a device inventory with identities and capabilities.
Save as `ci_inventory.py`.

**You need:** a MIDI-CI-capable device connected in both directions. Supply the
identity of your local participant, not the identity of the device you hope to
find. Use a deliberately chosen experimental identity for experimental work;
do not imply ownership of an assigned manufacturer identity.

```python
from secrets import randbelow

from meedee import ci, ci_live, midi


def main() -> None:
    manufacturer = bytes.fromhex(input("Local manufacturer identity (3 bytes hex): "))
    family = int(input("Local family number: "))
    model = int(input("Local model number: "))
    revision = bytes.fromhex(input("Local revision (4 bytes hex): "))
    identity = ci.Identity(
        manufacturer=manufacturer, family=family, model=model, revision=revision
    )
    inputs = midi.TypedInput()
    outputs = midi.Output()
    sources = inputs.ports()
    destinations = outputs.ports()
    print("Inputs:", [p.name for p in sources])
    print("Outputs:", [p.name for p in destinations])
    source_name = input("Device output port: ")
    destination_name = input("Device input port: ")
    (source,) = [p for p in sources if p.name == source_name]
    (destination,) = [p for p in destinations if p.name == destination_name]

    with inputs.connect(source) as receiver, outputs.connect(destination) as output:
        browser = ci.Browser(
            local_muid=randbelow(ci.MAX_MUID + 1),
            identity=identity,
            route=ci.Route(input=receiver.stream, output="device"),
            discovery_ns=3_000_000_000,
            endpoint_ns=1_000_000_000,
            timeout_ns=30_000_000_000,
        )
        print("Scanning MIDI-CI peers...")
        report = ci_live.run(browser, receiver, output)
    print(
        f"Scan {report.status}; "
        f"{report.confirmed_actions} management actions handed off."
    )
    if report.reason:
        print("Reason:", report.reason)
    for peer in report.peers:
        product = peer.info.identity
        print(f"MUID {peer.muid:07x}, CI format {peer.version}")
        print(f"  Manufacturer: {product.manufacturer.hex()}")
        print(f"  Family/model: {product.family}/{product.model}")
        print(f"  Revision: {product.revision.hex()}")
        print(f"  Product Instance ID: {peer.product_instance_id}")
        print(f"  Declared capabilities: {peer.info.capabilities:#x}")
    if not report.peers:
        print("No peers observed during this scan.")
    for diagnostic in report.diagnostics:
        print(f"Diagnostic: {diagnostic.kind}: {diagnostic.reason}")


if __name__ == "__main__":
    main()
```

The browser also sends required management replies and stops on input loss or
transport failure. It performs no Profiles, Property Exchange or instrument
configuration writes. An empty inventory does not prove the device is offline;
identities and capability bits are peer declarations.

For a native UMP connection, construct both typed clients with
`protocol=midi.Protocol.Ump` and add the intended `group=1` (or another 1..16
group) to `ci.Route`. The same browser adapter handles SysEx7 packetization and
assembly. Native UMP availability depends on the selected backend.

For ownership, bounds and failure details, consult
[the API contract](https://github.com/rec/meedee/blob/main/plan/api.md) and
[the MIDI-CI contract](https://github.com/rec/meedee/blob/main/plan/ci.md).
