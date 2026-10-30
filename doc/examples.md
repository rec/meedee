# Python examples, from messages to managed sessions

These examples use Python 3.13 and the public `meedee` modules. Start with the
first examples, which need no MIDI devices. Later examples introduce files,
live connections and stateful workflows. The library targets macOS, Windows and
Linux; available native backends depend on the build and operating system.

In this checkout, install the Python bindings with `uv sync --locked` and use
`uv run` to run your example scripts. Only example 15 needs the optional `ufor`
extra: `uv sync --locked --extra ufor`. The Python library examples do not require
the optional CLI dependencies. Source builds need the native toolchain described
in [README.md](../README.md#build-and-qualification).

Examples are independent unless they explicitly name an earlier example. File
examples use memory streams or exclusive creation (`"xb"`), except live recording
in example 18, whose path must be new to avoid overwriting. Native examples must
be saved as scripts and run under their shown
`if __name__ == "__main__":` guard: Python transport uses process isolation by
default. Replace example port names with exact names from your own system.

## 1. Construct and modify a note

**Device-free.** Message values are immutable and validated at construction.
`replace()` creates another checked value.

```python
from meedee.messages import NoteOn

note = NoteOn(channel=1, note=60, velocity=100)
softer = note.replace(velocity=64)

assert note.velocity == 100
assert softer.velocity == 64
assert softer.note == 60
```

Channels are 1..16, note numbers and MIDI 1 velocities are 0..127, and ordinary
`ProgramChange.program` is 1..128. `PitchBend.value` is signed, -8192..8191,
with zero at the center. A zero-velocity `NoteOn` remains a `NoteOn` value;
interpretation layers can treat it as a release.

## 2. Encode messages and decode a fragmented byte stream

**Device-free.** `codecs.encode()` produces MIDI 1 bytes. Keep one incremental
decoder per source so messages split across packets and running status work.

```python
from meedee import codecs, messages

note = messages.NoteOn(channel=1, note=60, velocity=100)
wire = codecs.encode(note)
assert wire == bytes.fromhex("90 3c 64")

decoder = codecs.Decoder(sysex_limit=4096)
assert decoder.feed(wire[:2]) == []
assert decoder.feed(wire[2:]) == [note]

# Running status reuses 0x90; a real-time clock may occur between data bytes.
items = decoder.feed(bytes.fromhex("3e f8 50"))
assert items == [
    messages.TimingClock(),
    messages.NoteOn(channel=1, note=62, velocity=80),
]
assert decoder.finish() == []
```

`feed()` can also return `codecs.Diagnostic` values. Inspect them instead of
assuming every item is a message. Call `finish()` at the end of a finite stream
to detect truncation. After known input loss, call `reset()` before feeding more
bytes; partial decoder state no longer describes a reliable stream.

## 3. Build compound controls and interpret their primitive messages

**Device-free.** A 14-bit controller or RPN command expands into several ordinary
control changes. The interpreter observes those messages without replacing them.

```python
from meedee import codecs, controls, gm, messages

volume = messages.ControlChange14(channel=1, controller=7, value=12000)
primitives = codecs.expand(volume)
assert len(primitives) == 2

interpreter = controls.Interpreter()
for message in primitives:
    for observation in interpreter.feed(message):
        print(observation)

bend_range = controls.pitch_bend_sensitivity(channel=1, semitones=12, cents=0)
for message in codecs.expand(bend_range):
    print(codecs.encode(message).hex())

print(gm.program(program=1).name)  # Acoustic Grand Piano
bank = gm.BankProgramSelect(channel=1, bank_msb=0, bank_lsb=0, program=1)
print(codecs.expand(bank))
```

Keep interpreters source-local and reset them after loss. For live output, use
`connection.send_command(volume)`, `send_command(bend_range)` or
`send_command(bank)`. `send_message()` is for primitive messages. A compound
send can fail after a prefix; `midi.CompoundSendError` exposes
`completed_messages` (possibly unknown) and `total_messages`. Retrying the whole
command can repeat already-sent changes.

## 4. Preserve timestamps and packet provenance

**Device-free.** Packet decoding adds stream identity, exact time and packet
spans to messages. Here, one note arrives across two packets.

```python
from meedee import events, messages, midi

clock = events.Clock(identity="keyboard-clock", origin="example")
decoder = events.PacketDecoder(stream="keyboard", protocol=midi.Protocol.Midi1)

first = events.ReceivedPacket(
    clock=clock, timestamp_ns=10, sequence=0, packet=bytes.fromhex("90 3c")
)
last = events.ReceivedPacket(
    clock=clock, timestamp_ns=20, sequence=1, packet=bytes.fromhex("64")
)
assert decoder.feed(first) == []
items = decoder.feed(last)
assert len(items) == 1
note = items[0]
assert isinstance(note, events.Event)
assert note.message == messages.NoteOn(channel=1, note=60, velocity=100)
assert note.timestamp == events.Timestamp(ticks=20, clock=clock)
assert note.span is not None
assert (note.span.first_packet, note.span.last_packet) == (0, 1)
assert decoder.finish() == []
```

Default clock ticks are nanoseconds. The event uses the completion packet's
time; its span retains the contributing packet range. A clock descriptor does
not synchronize devices or establish timestamp accuracy.

## 5. Exercise the same decoder with an in-memory transport

**Device-free.** Test connections provide bounded queues and production packet
decoding without workers, threads or MIDI clients.

```python
from meedee import events, messages, testing

clock = events.Clock(identity="test-clock", origin="manual")
time = testing.ManualClock(now_ns=0)
with testing.Input(stream="keys", protocol="midi1") as receiver:
    with testing.Output(protocol="midi1") as output:
        assert receiver.inject(b"\x90\x3c\x64", clock=clock, timestamp_ns=time.now_ns)
        item = receiver.receive()
        assert isinstance(item, events.Event)
        output.send_message(item.message)
        assert output.take_packet() == b"\x90\x3c\x64"
        assert item.message == messages.NoteOn(channel=1, note=60, velocity=100)
        time.advance(1_000_000)
        assert receiver.receive(timeout=1.0) is None
        assert time.now_ns == 1_000_000
```

An empty receive returns immediately, regardless of the timeout, and never
advances the manual clock. Input overflow drops newest packets and exposes loss
counters. Use `lose(count)`, `fail(reason)` and `fail_next_send(accepted=True)`
to exercise failures. An accepted simulated send failure deliberately represents
uncertain delivery: the sink can retain a packet even though sending raised.

## 6. Observe held notes and sustain

**Device-free.** A note tracker describes observed traffic. It supports MIDI 1,
MIDI 1 inside UMP and native MIDI 2, keeping UMP groups separate.

```python
from meedee import events, messages, notes

tracker = notes.Tracker(stream="keys", detail_limit=32)
traffic = [
    messages.NoteOn(channel=1, note=60, velocity=100),
    messages.ControlChange(channel=1, controller=64, value=127),
    messages.NoteOff(channel=1, note=60, velocity=0),
]
for message in traffic:
    tracker.feed(events.Event(stream="keys", message=message))

report = tracker.snapshot()
for channel in report.channels:
    for note in channel.notes:
        print(channel.channel, note.note, note.held, note.released)

tracker.invalidate()  # An observation boundary after known loss.
```

Repeated NoteOns use held/released counts per key. Released counts describe
matched releases since the last boundary; pedal values are observed separately,
without inferring pedal-held voices. This is an observation of traffic, not proof
of an instrument's sounding voices; the tracker does not generate cleanup messages.

## 7. Encode native MIDI 2 and packetize SysEx for UMP

**Device-free.** UMP uses words and explicit groups, rather than MIDI 1 bytes.
MIDI 2 note velocity is 16-bit.

```python
from meedee import messages, ump

note = ump.Midi2NoteOn(
    group=2,
    channel=1,
    note=60,
    velocity=50000,
    attribute_type=0,
    attribute_data=0,
)
words = ump.encode(note)
assert ump.decode(words) == note

wrapped = ump.Midi1Packet(
    group=2, message=messages.NoteOn(channel=1, note=60, velocity=100)
)
assert ump.decode(ump.encode(wrapped)) == wrapped

# SysEx7 packetization takes payload only, without F0 and F7 framing.
packets = ump.packetize_sysex7(group=2, data=b"\x7d\x01\x02\x03\x04\x05\x06")
for packet in packets:
    print(ump.encode(packet))

translation = ump.to_midi1(note, allow_loss=True)
print(translation.status, translation.reason)
```

Groups, like channels, are 1..16. Wrapping MIDI 1 in UMP preserves MIDI 1
resolution. Converting MIDI 2 to MIDI 1 may lose information; inspect the
translation result before using its messages. Encoding UMP does not establish
that a native backend or connected device supports it.

## 8. Write and read SysEx dumps in memory

**Device-free.** `messages.SysEx.data` excludes the F0/F7 framing bytes.

```python
from io import BytesIO

from meedee import messages, syx

buffer = BytesIO()
syx.write(buffer, [messages.SysEx(data=b"\x7d\x01"), messages.SysEx(data=b"\x7d\x02")])
assert buffer.getvalue() == bytes.fromhex("f0 7d 01 f7 f0 7d 02 f7")

buffer.seek(0)
reader = syx.Reader(buffer)
try:
    for message in reader:
        print(message.data.hex())
finally:
    reader.close()
assert not buffer.closed  # The reader does not own a borrowed stream.
```

The experimental manufacturer byte `0x7d` here is only sample data. A `.syx`
file stores whole SysEx messages without timestamps or device-specific pacing.
Writing a file does not send it.

## 9. Create a Standard MIDI File with explicit tempo

**Device-free.** SMF events have absolute file ticks and zero-based track indexes.
This example writes one quarter-note at 120 BPM to memory.

```python
from io import BytesIO

from meedee import messages, smf

buffer = BytesIO()
writer = smf.Writer(buffer, header=smf.Header(format=0, tracks=1, division=480))
writer.write(
    smf.Event(track=0, tick=0, record=smf.Meta(kind=0x51, data=b"\x07\xa1\x20"))
)
writer.write(
    smf.Event(track=0, tick=0, record=messages.NoteOn(channel=1, note=60, velocity=100))
)
writer.write(
    smf.Event(
        track=0, tick=480, record=messages.NoteOff(channel=1, note=60, velocity=0)
    )
)
writer.write(smf.Event(track=0, tick=480, record=smf.Meta(kind=0x2F, data=b"")))
writer.finish()

buffer.seek(0)
reader = smf.TimedReader(buffer, stream="score")
try:
    origin = reader.origin  # Explicit zero keeps any leading silence.
    for item in reader:
        print(item)
finally:
    reader.close()
```

`smf.Reader` preserves metadata and file records; `smf.TimedReader` produces
performance events with exact tempo conversion. Use `MergedReader` for a
bounded merge of tracks. Type 2 files need an explicit `sequence`. File SysEx
fragments are not automatically whole live SysEx messages.

## 10. Store exact events in an archive

**Device-free; creates `example.meedee`.** Archives retain clock descriptors,
streams, typed events and explicit gaps. Declare clocks and streams before
using them. Unlike an SMF PPQN clock, this clock directly expresses seconds:
480 ticks are half a second.

```python
from meedee import archive, events, messages

clock = events.Clock(identity="score", tick_numerator=1, tick_denominator=960)
on = events.Event(
    stream="piano",
    timestamp=events.Timestamp(ticks=0, clock=clock),
    message=messages.NoteOn(channel=1, note=60, velocity=100),
)
off = on.replace(
    timestamp=events.Timestamp(ticks=480, clock=clock),
    message=messages.NoteOff(channel=1, note=60, velocity=0),
)
with open("example.meedee", "xb") as destination:
    writer = archive.Writer(destination)
    writer.write(clock)
    writer.write(
        archive.Stream(id="piano", label="Piano", clock_identity=clock.identity)
    )
    writer.write(on)
    writer.write(off)
    writer.finish()

reader = archive.Reader("example.meedee")
try:
    for record in reader:
        print(record)
    assert reader.complete
finally:
    reader.close()
```

Writers need `finish()`; normal archive reading checks per-record CRCs and the
completion trailer. Recovery is explicit with `recover=True`: a verified prefix
is not a complete capture. The archive v1 format is still unpublished; see
[the format contract](../plan/archive-format.md).

## 11. Inspect capture quality and compare records

**Device-free.** This uses the archive created in example 10.

```python
from meedee import inspection

report = inspection.inspect_file("example.meedee", format="archive", detail_limit=16)
print(report.complete, report.problem)
print(report.message_counts)
print(report.gap_records, report.lost_events, report.timestamp_regressions)
for segment in report.timing_segments:
    print(segment.source, segment.first, segment.last, segment.regressions)

same = inspection.compare_files(
    "example.meedee", "example.meedee", format="archive", detail_limit=16
)
assert same.equal and same.left_complete and same.right_complete
```

Choose `archive`, `smf`, `syx` or `clip` explicitly. Structural completeness is
separate from timing and loss diagnostics. Comparison checks records in the
same format, including order and metadata. It does not align events after
insertions/deletions. For different captures, `tick_tolerance` permits a stated
tolerance in existing ticks; `source_map` and `clock_map` explicitly match archive
identities. Matching identities does not synchronize clocks.

## 12. Write a MIDI Clip containing UMP

**Device-free.** A Clip has its own configuration, sequence markers and delta
clockstamps. Preserve those records when rewriting one.

```python
from io import BytesIO

from meedee import clip, messages, ump

buffer = BytesIO()
writer = clip.Writer(buffer)
packets = [
    ump.DeltaClockstamp(ticks=0),
    ump.DeltaClockstampTicksPerQuarter(ticks=480),
    ump.DeltaClockstamp(ticks=0),
    ump.start_of_clip(),
    ump.DeltaClockstamp(ticks=0),
    ump.Midi1Packet(group=1, message=messages.NoteOn(channel=1, note=60, velocity=100)),
    ump.DeltaClockstamp(ticks=480),
    ump.Midi1Packet(group=1, message=messages.NoteOff(channel=1, note=60, velocity=0)),
    ump.DeltaClockstamp(ticks=0),
    ump.end_of_clip(),
]
for packet in packets:
    writer.write(packet)
writer.finish()

buffer.seek(0)
reader = clip.Reader(buffer)
try:
    for entry in reader:
        print(entry.tick, entry.sequence, entry.message)
finally:
    reader.close()
```

Clip ticks are file units. Live host timestamps require a separate clock model;
serializing packets alone does not establish a playback schedule.

## 13. Allocate MPE member channels

**Device-free.** An allocator generates messages for voices that your application
owns. A tracker observes traffic from an existing MPE source.

```python
from meedee import codecs, mpe

zone = mpe.ZoneConfiguration(manager_channel=1, members=4)
allocator = mpe.Allocator(zone=zone)
voice = allocator.allocate(note=60, velocity=100)
on = voice.note_on
off = allocator.release(voice_id=voice.id, velocity=0)
assert on.channel == off.channel

tracker = mpe.Tracker(zone=zone)
for message in [on, off]:
    print(tracker.feed(message))
print(codecs.expand(zone))  # Configuration messages; nothing has been sent.
```

To configure a live output, explicitly send the zone with `send_command(zone)`
before sending the allocator's note messages. The allocator does not detect
other writers or configure the instrument. Observation and allocation are
separate operations.

## 14. Build and inspect a MIDI Tuning Standard bulk dump

**Device-free.** MTS programs are 0..127, unlike ordinary ProgramChange values.
This creates an equal-tempered table with an explicit target device and program.

```python
from io import BytesIO

from meedee import codecs, messages, syx, tuning

conversion = tuning.Pitch.from_frequency(frequency_hz=442.0)
print(conversion.pitch, conversion.error_cents)

table = tuning.TuningTable(
    values=[tuning.Pitch(semitone=n, fraction=0) for n in range(128)]
)
dump = tuning.BulkDump(device_id=1, program=2, name="Equal temper", table=table)
wire = codecs.encode(dump)
raw = messages.SysEx(data=wire[1:-1])
decoded = tuning.decode(raw)
assert decoded is not None and decoded.message == dump
print(decoded.checksum)

buffer = BytesIO()
syx.write(buffer, [raw])
```

`Pitch.from_frequency()` reports quantization error rather than concealing it.
A `NoChange` entry explicitly preserves a key. The table has 128 entries; the
receiver must support the selected MTS format. This example only produces bytes.

## 15. Resolve a uFor tuning before producing MTS

**Device-free; requires `meedee[ufor]`.** Preview keeps each key's status and
quantization error so you can examine partial coverage before exporting.

```python
from ufor.tuning import RatioTable, Tuning

from meedee import codecs, messages, syx, ufor_tuning

source = Tuning(source=RatioTable(values=["1", "3/2", "2"]), root_note=69)
preview = ufor_tuning.preview(source, notes=[69, 70, 71])
for key in preview.entries:
    print(key.note, key.status, key.frequency, key.error_cents)

dump = ufor_tuning.bulk_dump(
    preview, device_id=1, program=2, name="Three pitches", preserve_unmapped=True
)
wire = codecs.encode(dump)
with open("three-pitches.syx", "xb") as destination:
    syx.write(destination, [messages.SysEx(data=wire[1:-1])])
```

Unselected keys become `NoChange` only because preservation was explicitly
requested. Out-of-range pitches and evaluation failures must be resolved before
building a dump. uFor also accepts Scala-derived tuning models; see
[the tuning workflow](../plan/ufor-tuning.md#offline-file-workflow).

## 16. Discover ports and send a note

**Live: enumerates ports and sends a note pair.** First list available ports by
running only the discovery part of `main()`. Then replace the placeholder name.
Use this selection helper in all subsequent live examples, keeping it at module
scope in the same script as the example.

```python
from time import sleep

from meedee import messages, midi


def select_port(ports: list[midi.Port], name: str) -> midi.Port:
    matches = [p for p in ports if p.name == name]
    if len(matches) != 1:
        raise ValueError(f"Expected one port named {name!r}; found {len(matches)}")
    return matches[0]


def main() -> None:
    print(midi.compiled_backends())
    client = midi.TypedOutput()
    ports = client.ports()
    for port in ports:
        print(port.name, port.backend, port.backend_id)
    port = select_port(ports, "Exact synth input name")
    with client.connect(port) as output:
        note = messages.NoteOn(channel=1, note=60, velocity=100)
        output.send_message(note)
        try:
            sleep(0.25)
        finally:
            output.send_message(messages.NoteOff(channel=1, note=60, velocity=0))


if __name__ == "__main__":
    main()
```

Discover and connect with the same client and current snapshot. Duplicate names
are rejected by this helper. You can choose a compiled backend explicitly, for
example `midi.TypedOutput(backend=midi.Backend.CoreMidi)` on macOS. Default raw
and typed connections use MIDI 1; UMP requires `protocol=midi.Protocol.Ump` and
a suitable backend. An input port is a source received by your application; an
output port is a destination sent to by your application.

The `finally` block attempts this example's NoteOff. A send error still leaves
the actual device state uncertain; closing a connection sends no automatic
cleanup. For raw MIDI 1 output, use `midi.Output()` and
`connection.send(codecs.encode(note))`. Raw UMP output accepts
`midi.UmpPacket(words=ump.encode(message))`.

## 17. Receive typed events and observe notes for a bounded period

**Live: opens one input and sends nothing.** Include `select_port` from example
16. Keep observations source-local and check both diagnostics and transport
counters, including during silence.

```python
from time import monotonic

from meedee import events, midi, notes


def main() -> None:
    client = midi.TypedInput(event_limit=256)
    port = select_port(client.ports(), "Exact keyboard output name")
    with client.connect(port) as receiver:
        tracker = notes.Tracker(stream=receiver.stream)
        deadline = monotonic() + 10
        observed_drops = 0
        while monotonic() < deadline:
            if receiver.failure is not None:
                raise receiver.failure
            drops = receiver.dropped_packets
            if drops != observed_drops:
                print("Transport drops:", drops)
                tracker.invalidate()
                observed_drops = drops
            item = receiver.receive(timeout=0.05)
            if item is None:
                continue
            tracker.feed(item)
            if isinstance(item, events.StreamDiagnostic):
                print("Diagnostic:", item.kind, item.reason)
            else:
                print(item.timestamp, item.message)
        print(tracker.snapshot().channels)
    # Close may make additional asynchronously reported loss/failure visible.
    print("Final transport state:", receiver.dropped_packets, receiver.failure)


if __name__ == "__main__":
    main()
```

Counters are asynchronous observations, not proof of loss-free delivery.
Sequence-derived loss and transport counters can overlap; do not add them.
This example continues after known loss with invalidated interpretation state.
Typed queue or worker failure is terminal. Slow printing can overload input.

## 18. Record a live input and examine recording pressure

**Live: records one input for five seconds; creates `take.meedee`.** Include
`select_port` from example 16. Use a new path: `start_recording()` accepts a path
rather than your own exclusive stream and overwrites an existing file.

```python
from time import monotonic

from meedee import inspection, midi


def main() -> None:
    client = midi.TypedInput()
    port = select_port(client.ports(), "Exact keyboard output name")
    with client.connect(port) as receiver:
        recorder = receiver.start_recording(
            "take.meedee",
            label="Keyboard",
            record_limit=256,
            byte_limit=8_388_608,
            metrics=True,
        )
        try:
            deadline = monotonic() + 5
            while monotonic() < deadline:
                if receiver.failure is not None:
                    raise receiver.failure
                receiver.receive(timeout=0.05)  # Drain the application event queue.
            print("Current pressure:", recorder.pressure)
        finally:
            report = receiver.stop_recording()
    print(report.complete, report.gap_records, report.failure, report.pressure)
    if report.complete:
        quality = inspection.inspect_file("take.meedee", format="archive")
        print(quality.complete, quality.gap_records, quality.timestamp_regressions)


if __name__ == "__main__":
    main()
```

The recorder is a bounded background writer. Its budget includes queued and
in-flight writes. Default recording becomes incomplete after observed input
loss or admission/write failure; `best_effort=True` permits explicit gap records.
Always inspect the final report. Metrics are opt-in and describe retained
pressure, not physical latency.

## 19. Plan exact timing, then play a file

**Device-free timing calculation:** use explicit units and mappings rather than
comparing unrelated clock ticks.

```python
from meedee import events, messages

source = events.Clock(identity="source", tick_numerator=1, tick_denominator=1000)
target = events.Clock(identity="target")
mapping = events.ClockMapping(source=source, target=target)
mapped = mapping.map_time(events.Timestamp(ticks=500, clock=source))
assert (mapped.numerator, mapped.denominator) == (1, 2)  # Target time in seconds.

origin = events.Timestamp(ticks=0, clock=source)
plan = events.PlaybackPlan(origin=origin)
event = events.Event(
    stream="score",
    timestamp=events.Timestamp(ticks=500, clock=source),
    message=messages.NoteOff(channel=1, note=60, velocity=0),
)
assert plan.deadline(event).delay_ns == 500_000_000
```

The mapping above declares an assumed shared zero with default rate/offset; it
is not measured synchronization. Real sources require an application-established
mapping and uncertainty policy.

**Live playback: reads `song.mid` and sends its performance events.** Include
`select_port` from example 16. Supply a trusted MIDI 1 performance file.

```python
from meedee import midi, smf


def main() -> None:
    client = midi.TypedOutput()
    port = select_port(client.ports(), "Exact synth input name")
    score = smf.TimedReader("song.mid", stream="score")
    try:
        with client.connect(port) as output:
            report = output.play(score, origin=score.origin, metrics=True)
            print(report.sent_events, report.max_lateness_ns, report.lateness)
    finally:
        score.close()


if __name__ == "__main__":
    main()
```

Playback runs on the caller's thread and stops on failure without retry. Pass an
`events.Cancellation` as `cancel=` when another part of your application should
stop playback. Cancellation sends no cleanup. Lateness measures host handoff,
not device acceptance. Streaming playback can send a prefix before discovering
later malformed content; fully validate first when that is unacceptable.

## 20. Route and remap events with explicit delivery confirmation

**Device-free.** A router filters/remaps and queues deliveries; your host sends
them. Here the sink is an in-memory transport.

```python
from meedee import events, messages, midi, routing, testing

router = routing.Router(
    routes=[
        routing.Route(
            name="keys-to-synth",
            source="keys",
            destination="synth",
            protocol="midi1",
            channel=1,
            remap_channel=2,
        )
    ]
)
router.feed(
    events.Event(
        stream="keys", message=messages.NoteOn(channel=1, note=60, velocity=100)
    )
)
with testing.Output(protocol="midi1") as output:
    while (delivery := router.peek()) is not None:
        try:
            output.send_message(delivery.event.message)
        except midi.SendError as error:
            router.fail(delivery.id, str(error))
            break
        router.confirm(delivery.id)
    assert output.take_packet() == bytes.fromhex("91 3c 64")
print(router.snapshot().confirmed)
```

Routes forward channel voice by default. Opt into non-channel traffic with
`include_nonchannel=True`. Merge order is caller arrival order; timestamps remain
provenance, not scheduling instructions. Input loss, capacity failure or a failed
send stops the router; a failed send is uncertain and must not be confirmed or
silently retried. For multiple destinations, send through the connection named
by `delivery.destination`. `routing.preview_file()` applies routes to an archive
without opening ports. See [the routing contract](../plan/api.md).

## 21. Pace a SysEx transfer or capture a passive dump

**Live send: transmits `backup.syx`.** Include `select_port` from example 16.
Inspect the plan before opening an output. The example gap is 100 ms; choose the
minimum gap documented for your instrument.

```python
from meedee import midi, transfer, transfer_output


def main() -> None:
    plan = transfer.read_file("backup.syx", gap_ns=100_000_000)
    print(plan.message_count, plan.wire_bytes, plan.gap_ns)
    client = midi.TypedOutput()
    port = select_port(client.ports(), "Exact synth input name")
    with client.connect(port) as output:
        session = transfer.Session(plan)
        report = transfer_output.run(session, output)
    print(
        report.status,
        report.confirmed_messages,
        report.uncertain_message,
        report.reason,
    )


if __name__ == "__main__":
    main()
```

Whole messages are sent without splitting, automatic retries or inferred
acknowledgements. The gap starts after each successful handoff. Confirmed
progress means output handoff, not confirmed instrument acceptance.

**Live receive: sends nothing; creates `backup.syx` only after successful
capture.** Run this alternative in its own script, including `select_port`.
Initiate the dump on the instrument after opening input.

```python
from meedee import midi, transfer, transfer_input


def main() -> None:
    client = midi.TypedInput()
    port = select_port(client.ports(), "Exact synth output name")
    with client.connect(port) as receiver:
        capture = transfer.Capture(
            stream=receiver.stream, expected_messages=8, timeout_ns=30_000_000_000
        )
        report = transfer_input.run(capture, receiver)
    print(report.status, report.received_messages, report.reason)
    if report.status == "completed":
        with open("backup.syx", "xb") as destination:
            capture.write(destination)


if __name__ == "__main__":
    main()
```

Message count is caller-supplied; reaching it does not prove device-specific dump
completeness. Receive loss, timeout or decoding failure prevents successful
capture. A final disk-write failure can still leave a partial file.

## 22. Request a matched MTS tuning dump

**Live: sends one MTS request and receives its reply.** Include `select_port`
from example 16. Device ID 1 and program 2 are examples; choose the instrument's
actual settings. `BankedDumpRequest` additionally selects a bank.

```python
from meedee import midi, tuning, tuning_live


def main() -> None:
    inputs = midi.TypedInput()
    outputs = midi.TypedOutput()
    source = select_port(inputs.ports(), "Exact synth output name")
    destination = select_port(outputs.ports(), "Exact synth input name")
    with inputs.connect(source) as receiver, outputs.connect(destination) as output:
        query = tuning.Query(
            request=tuning.BulkDumpRequest(device_id=1, program=2),
            stream=receiver.stream,
            timeout_ns=3_000_000_000,
        )
        report = tuning_live.run(query, receiver, output)
    print(report.status, report.uncertain_send, report.reason)
    if report.status == "completed":
        assert report.decoded is not None
        print("Checksum:", report.decoded.checksum)
        with open("queried-tuning.syx", "xb") as destination_file:
            query.write(destination_file)


if __name__ == "__main__":
    main()
```

The raw matched reply is retained. Legacy bulk-dump checksum mismatches are
flagged rather than rewritten, so inspect the checksum result before trusting
the capture. MTS has no transaction ID; an identical already-queued dump can
match. Timeouts stop the query without automatic retries.

## 23. Observe topology changes by polling

**Live discovery: enumerates inputs and outputs, sends no MIDI.** Polling is
caller-driven and can miss a device that appears and disappears between scans.

```python
from time import sleep

from meedee.discovery import Poller


def main() -> None:
    with Poller(protocol="midi1", history_limit=8) as poller:
        for index in range(5):
            report = poller.poll()
            if report.stopped:
                print("Discovery stopped:", report.reason)
                break
            print(report.generation, report.port_count)
            if report.last is not None:
                for change in report.last.changes:
                    print(change.kind, change.endpoint.name, change.count)
            if index < 4:
                sleep(1)


if __name__ == "__main__":
    main()
```

Complete descriptors and duplicate counts are compared. A rename is observed as
removal/addition, without asserting physical identity. Failed or over-limit scans
stop and preserve the last complete snapshot instead of inventing removals.

## 24. Browse MIDI-CI peers and endpoint identities

**Live: sends Discovery and subsequent endpoint inquiries, plus required
management replies.** Include `select_port` from example 16. Use the caller's
actual local participant identity, not the instrument's identity. This function
requires those fields explicitly and generates a fresh local MUID for the scan.

```python
from secrets import randbelow

from meedee import ci, ci_live, midi


def browse(identity: ci.Identity) -> ci.BrowseReport:
    inputs = midi.TypedInput()
    outputs = midi.TypedOutput()
    source = select_port(inputs.ports(), "Exact synth output name")
    destination = select_port(outputs.ports(), "Exact synth input name")
    with inputs.connect(source) as receiver, outputs.connect(destination) as output:
        browser = ci.Browser(
            local_muid=randbelow(0x0FFFFF00),
            identity=identity,
            route=ci.Route(input=receiver.stream, output="synth"),
            discovery_ns=3_000_000_000,
            endpoint_ns=1_000_000_000,
            timeout_ns=30_000_000_000,
        )
        return ci_live.run(browser, receiver, output)


def main() -> None:
    manufacturer = bytes.fromhex(input("Local manufacturer identity (3 bytes hex): "))
    family = int(input("Local family number: "))
    model = int(input("Local model number: "))
    revision = bytes.fromhex(input("Local revision (4 bytes hex): "))
    report = browse(
        ci.Identity(
            manufacturer=manufacturer, family=family, model=model, revision=revision
        )
    )
    print(report.status, report.reason, report.confirmed_actions)
    for peer in report.peers:
        print(peer.muid, peer.info.identity, peer.product_instance_id)


if __name__ == "__main__":
    main()
```

For UMP SysEx7, construct both clients with `protocol=midi.Protocol.Ump` and add
an explicit `group=1` (or the intended 1..16 group) to `ci.Route`. The host adapter
handles packetization and assembly. The browser stops on input loss or transport
failure. An empty scan does not prove a device is offline; peer capabilities are
declarations. Profiles and Property Exchange are deferred. See
[the CI contract](../plan/ci.md#one-scan-management-browser).

## Where to go next

[The API contract](../plan/api.md) describes ownership, bounds, file fidelity,
loss and failure semantics. [README.md](../README.md) covers installation,
backends and the optional CLI. Use `events.Cancellation` for caller-requested
stops, bounded limits appropriate to your workload, and final reports to decide
what a session actually established. Pure codecs and state machines can be used
without initializing native transport.
