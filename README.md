# meedee

A Rust MIDI toolkit with matching Python bindings, using vendored
[libremidi](https://github.com/celtera/libremidi) for native transport. Python
imports are `meedee.midi`, `meedee.events`, `meedee.messages`, `meedee.codecs`,
`meedee.controls`, `meedee.tuning`, `meedee.gm`, `meedee.universal`, `meedee.mpe`,
`meedee.timecode`, `meedee.ump`, `meedee.smf`, `meedee.syx`, `meedee.clip` and
`meedee.archive` and `meedee.ci`; the package root is empty. There are no Python
runtime dependencies.

The toolkit is under development. Raw MIDI 1.0 bytes and native UMP transport
are implemented but native platform qualification remains incomplete. Immutable
MIDI 1.0 messages and a bounded incremental byte
codec, fine controls, RPN/NRPN commands and MIDI Tuning Standard values/codecs
are available, along with GM sound maps and selected profile codecs/helpers.
UMP/MIDI 2.0 values, codecs and explicit translation are available. Streaming
SMF/SYX/MIDI Clip files, exact event archives, typed I/O, bounded recording and
explicit clock-aware playback are implemented. Remaining qualification work is
tracked in [the plan](plan/meedee.md).
Encode primitive messages for raw transport; use `send_command` for compound
control, bank/program and MPE configuration commands.

## Files, typed I/O and exact time

```python
from meedee import archive, events, messages, smf

clock = events.Clock(identity="score", tick_numerator=1, tick_denominator=480)
note = events.Event(
    stream="piano",
    timestamp=events.Timestamp(ticks=480, clock=clock),
    message=messages.NoteOn(channel=1, note=60, velocity=100),
)
writer = archive.Writer("take.meedee")
writer.write(clock)
writer.write(archive.Stream(id="piano", label="Piano", clock_identity=clock.identity))
writer.write(note)
writer.finish()
reader = archive.Reader("take.meedee")
records = list(reader)  # Completion and every record CRC are checked.
assert reader.complete
reader.close()

score = smf.TimedReader("song.mid", stream="score")
origin = score.origin  # Explicit zero preserves any leading silence.
# With an opened TypedOutputConnection:
# output.play(score, origin=origin)
score.close()
```

SMF supports types 0/1/2, PPQN and SMPTE timing. `smf.Reader` retains metadata,
unknown chunks and F0/F7 file records. `smf.MergedReader` merges tracks with a
bounded lookahead and deterministic tick/track order; Type 2 requires an explicit
`sequence`. `smf.TimedReader` is a performance view with exact tempo conversion,
not a metadata-preserving replacement for `Reader`. `smf.export_event` reports
unsupported content and quantization; loss and UMP group routing require explicit
decisions. `syx` handles concatenated SysEx dumps; `clip` handles MIDI Clip 1.0.
File writers require `finish()`. Readers have `close()`; borrowed streams remain
open. No codec starts MIDI workers. See [the API contract](plan/api.md).

The [archive v1 layout and local review](plan/archive-format.md) document exact
clock/record encoding and verified-prefix recovery. Unknown content requires
explicit preservation; known Event headers are validated even for future message
representations. Version 1 remains unpublished, with format freeze and future
compatibility guarantees still owner decisions.

`midi.TypedInput` and `midi.TypedOutput` use the existing raw transport, including
its default Python process isolation. Input connections provide either a callback
or `receive(timeout=..., cancel=...)`, returning an immutable `events.Event` or
`events.StreamDiagnostic`. Events preserve primitive messages, the completion
packet's timestamp and their contributing packet span. Optional `EventInterpreter`,
`EventVelocityInterpreter`, `EventTracker`, `EventDecoder` and `EventAssembler`
views live in their respective controls/MPE/tuning/timecode/UMP modules. Views
are source-scoped; UMP-to-MIDI-1 interpretation requires an explicit group.

`TypedInputConnection.dropped_packets` exposes the observed raw receive-drop
count, including updates during silence. Poll it alongside `failure` when loss
must stop a live session: typed queue overflow is reported through `failure`.
Isolated updates are asynchronous, so neither property proves loss-free delivery
or device health.

`input_connection.start_recording(path)` attaches a bounded background recorder;
`stop_recording()` returns a report whose `complete`, `gap_records` and `failure`
fields must be checked. By default any observed input loss or write/admission
failure makes recording incomplete. `best_effort=True` permits explicit gap
records, not silent loss. Capture defaults to 128 in-flight records and 8 MiB of
payload. General file records default to a configurable 1 MiB payload limit.

`output.play(source, origin=..., mappings=..., cancel=...)` runs on the caller's
thread. It requires timestamped events and explicit mappings between unrelated
clocks, reports lateness and stops on failure without retry. Cancellation sends
no automatic NoteOff/reset or other cleanup. Archive records are checked while
streaming: reading and verifying an entire capture before playback is necessary
if no output may precede discovering a damaged or incomplete trailer. CRCs detect
accidental corruption and do not authenticate a file.

Pass `metrics=True` to `start_recording` or `archive.Recorder` for live/final
`.pressure` reports: admitted records, capacity rejections and peak reserved
records/payload bytes, including in-flight writes. Pass it to `output.play` for
`.lateness`, a bounded histogram with exact count/minimum/maximum/sum covering
successful handoffs. Defaults leave these fields `None`. These observations do
not establish device acceptance or physical latency.

`meedee monitor --metrics` adds host delivery rate and interval statistics to its
final summary, before display filters. `meedee.metrics` also exposes reusable
Rust-backed `Histogram` and caller-timed `Delivery` accumulators, with independent
frozen snapshots and no native client initialization. See
[timing and load diagnostics](plan/api.md#timing-and-load-diagnostics) for units,
histogram boundaries and measurement windows.

## Passive SysEx capture

To receive a dump initiated on an instrument, use the optional CLI:

```bash
meedee sysex-receive backup.syx --port "Synth MIDI Out" --expected-messages 8 --timeout-ms 30000
```

This passively collects MIDI 1 SysEx, closes the input, then creates a new file.
It refuses overwrite and writes nothing after receive loss, timeout or decoding
failure. Final disk errors can leave a partial file. The count is caller-supplied;
success does not establish instrument-specific dump completeness. Shared
`transfer.Capture` and `transfer_input.run` provide the same workflow to library
users. See [capture semantics](plan/api.md#passive-midi-1-sysex-capture).

## Offline uFor tuning

Python 3.13+ users with `meedee[cli,ufor]` can run `meedee ufor-preview` and
`meedee ufor-export` on uFor Tuning JSON or Scala `.scl`. Scala requires explicit
root note/frequency. Export creates an ordinary or banked bulk `.syx` file without
overwriting; sending remains a separate `transfer-send` action. See the
[offline tuning workflow](plan/ufor-tuning.md#offline-file-workflow).

## Request an MTS tuning dump

```bash
meedee tuning-preview --device-id 1 --program 2 --timeout-ms 3000
meedee tuning-query tuning.syx --input-port "Synth Out" --output-port "Synth In" --device-id 1 --program 2 --timeout-ms 3000
```

Add `--bank` for a banked request. Preview opens no ports; query sends one MIDI 1
dump request and saves only a matched reply, after closing both connections.
Existing files are preserved. Raw checksum bytes remain unchanged; a legacy
checksum mismatch is saved with a flag and exit status 1. Other failures do not
retry. Library users can drive `tuning.Query` with `tuning_live.run`. See
[query semantics](plan/api.md#one-shot-midi-1-mts-dump-queries).

## File inspection and capture validation

```python
from meedee import inspection

report = inspection.inspect_file("take.meedee", format="archive")
print(report.complete, report.problem)
print(report.message_counts)
print(report.gap_records, report.lost_events, report.discontinuities)
for source in report.streams:
    print(source.id, source.label, source.clock_identity)
```

Choose `format="archive"`, `"smf"`, `"syx"` or `"clip"` explicitly. Paths and
binary streams are accepted; borrowed streams stay open and are read from their
current position. Inspection starts no MIDI workers and sends nothing. Rust
applications use `meedee::inspection::inspect` with the corresponding `Format`.

The frozen report counts successfully decoded records and messages without
retaining the event payloads. It retains archive clock/stream descriptors and,
for SMF, the header and per-track record counts. Collection properties return
copies. Malformed input, CRC failure, missing completion and configured payload
limits stop inspection at the verified prefix, with `complete=False` and the
reader's location/reason in `problem`. File access and read failures raise
`OSError`. Unknown archive/UMP content is rejected unless
`preserve_unknown=True`; preserved unknown/rejected UMP layouts and unknown
archive records are counted as opaque. SMF unknown chunks are always counted as
opaque, following that reader's preservation policy.

`complete` means the entire file passed the selected reader's completion checks.
A structurally complete capture may still have gaps or discontinuities. Inspect
those counts separately. UMP counts include individual clock/configuration
packets, and SMF SysEx fragments remain fragments, without implicit assembly.
`untimed_events` counts archive events with no timestamp, all SYX messages and
Clip packets before a known file tick; SMF events always have file ticks.
File ticks are not host timestamps, and clock descriptors do not synchronize
independent sources. This report does not certify device capabilities or safe
playback. Existing readers remain the way to inspect individual raw records.

Working storage follows the existing readers' payload/descriptor limits plus
bounded timing detail (`detail_limit=128` by default). `source_counts` counts
archive events per stream, SMF records per `track:N`, Clip packets and SYX dumps.
`timing_segments` retain exact first/last/minimum/maximum ticks, missing times
and regressions. Archive clocks remain independent; SMF/Clip ticks are file
units. Gaps, discontinuities and clock changes start new source-local segments.
`timestamp_regressions` remains a diagnostic and does not change `complete`.
Omitted segment/diagnostic totals make exhausted detail limits visible.

Other working storage follows the existing readers' payload/descriptor limits plus
fixed message-family counts and one counter per declared SMF track. It does not
grow with event count. Inspection reads to EOF without a CPU/time deadline or
filesystem cancellation guarantee. No file conversion or rewriting is performed.

`inspection.compare_files(left, right, format=...)` compares decoded records in
the same format, preserving metadata, configuration, source order and opaque
content. It is not a byte-for-byte comparison of running-status encoding.
Archives use explicit left-to-right `source_map` and `clock_map` dictionaries;
unmapped identities must match exactly. Clock units and other descriptors still
must match. An identity mapping declares a comparison role, not synchronization.

Timestamps are exact by default. `tick_tolerance=N` permits a nonnegative
difference in the unchanged clock/file tick unit; alternatively
`ignore_timestamps=True` ignores timestamp values and presence. These choices
cannot be combined. Span packet indexes and all non-timing content remain
significant. Clip DCS values are compared through cumulative ticks under those
timing policies; DCTPQ/configuration remain exact. No automatic time alignment,
protocol conversion, stream merging or matching past insertions/deletions occurs.

The comparison report contains equality, both completion/problem states, record
counts, total differences and at most `detail_limit` indexed examples. None as
an example's record index identifies an SMF header difference. Both files are
read through completion checks; identical invalid prefixes are never equal.
Examples contain reasons/indexes rather than copies of potentially large payloads.
Equal files can both contain capture loss; use `validate` to check capture quality.
Missing/inaccessible files and read failures raise errors. Comparison does not
retain the full files or change either source.

### Optional command-line tools

Install `meedee[cli]` (in this checkout: `uv sync --extra cli`). The optional
Tyro/Pydantic dependency set does not change ordinary library installations.

```sh
meedee inspect take.meedee --format archive
meedee validate take.meedee --format archive --output json
meedee compare before.meedee after.meedee --format archive --tick-tolerance 1000
```

`inspect` prints full summaries and timing detail; `validate` prints a compact
completeness/quality verdict. `compare` preserves the library's record semantics.
Archive identity mappings use `--source-map LEFT RIGHT` and `--clock-map LEFT RIGHT`
with additional key/value pairs as needed. All commands accept `--output json`.
Exit statuses are 0 for a clean/equal result, 1 for differences or reported
capture-quality problems, and 2 for malformed/incomplete data, access errors or
invalid arguments. Missing times alone are informational, since untimed captures
and SYX files are legitimate. Opaque content, gaps, discontinuities and timestamp
regressions are quality findings. No offline command opens a MIDI client.

### Passive live monitoring

```sh
meedee monitor "Exact input port name" --protocol midi1 --duration 30
meedee monitor "Exact UMP input name" --protocol ump --group 16 --channel 1 --output json
```

The name must match exactly one input in the selected backend's current discovery
snapshot; no first-port fallback or automatic reconnect is performed. Protocol
selection is required; `--backend` can select a compiled backend by its API name
(for example `CoreMidi` or `CoreMidiUmp`). Isolation remains enabled by default.
This command opens only input and sends no MIDI. Stop with Ctrl-C or `--duration`
in seconds. Input closes before the final summary, including on terminal failure.

`--types NoteOn ControlChange`, `--channel` and UMP-only `--group` filter display,
not the underlying source interpretation. Existing Rust control and velocity
views receive the full source; MIDI 1 interpretation inside UMP requires a selected
group. Other MIDI 2 values remain typed packet observations, without translation.
Loss/failure diagnostics bypass filters. Known loss resets interpretation and
source timing, then monitoring continues. Terminal typed-queue or worker failures
stop with status 2. A completed session with diagnostics/loss/regressions returns
1; a clean session returns 0. A live summary is not file-completion evidence.

Retained display history defaults to 128 entries and SysEx previews to 64 bytes
(`--history-limit`, `--preview-bytes`). The typed input retains its existing count,
payload and SysEx bounds (`--event-limit`, `--receive-byte-limit`, `--sysex-limit`).
Source timing examples use `--detail-limit`. The final summary includes bounded
history and all observed counters, including filtered events. Output is emitted
on the controlling thread, never in a native callback. Slow output can still
overload input; it is reported rather than silently treated as complete capture.

`transport_drop_total` is an asynchronous observation and can overlap
sequence-derived `lost_events`; they are not additive. Counter changes reset views
without adding the same loss again, including updates during silence and close.
The ongoing pure Rust `inspection::Observer` / Python `inspection.Observer` uses
the same timing/message aggregation as files and exposes immutable snapshots.
It is tied to one source and initial connection clock; create another observer
for another connection. Its snapshots never claim file completion. Device health,
driver-loss detection, physical latency and hardware interoperability remain
outside these observations.

## MIDI-CI management

`meedee.ci` provides immutable MIDI-CI 1.2.1 Discovery/Reply, Endpoint Inquiry/Reply,
InvalidateMuid and ACK/NAK values, encoded with `codecs.encode`. Interpretation
is explicit through `ci.decode(SysEx)`; raw input still supplies primitive SysEx.
`ci.Session` is a caller-driven management state machine with explicit random
local MUIDs, routes, send-result reporting, integer-time deadlines and restart.
It sends nothing and starts no threads. Read [the CI contract](plan/ci.md) before
wiring actions to transport, including provisional replies and send-result reporting.

Profiles and Property Exchange are deferred pending full specification access.
Sessions advertise zero optional-category bits. These codecs are not proof of
native UMP availability, peer responsiveness or successful device configuration.

## Messages and byte codecs

```python
from meedee.codecs import Decoder, encode
from meedee.messages import NoteOn

note = NoteOn(channel=1, note=60, velocity=100)
wire = encode(note)  # b'\x90\x3c\x64'
decoder = Decoder()
assert decoder.feed(wire[:2]) == []
assert decoder.feed(wire[2:]) == [note]
assert decoder.finish() == []
```

The pure codec supports MIDI 1.0 note, pressure, CC, program, signed pitch bend,
system common, real-time and SysEx messages. Values are immutable, keyword-only,
hashable and support checked `replace`. Channels are 1–16, programs are 1–128,
CC identifiers are 0–127, and PitchBend.value is -8192–8191 with center zero.
Channel-mode messages remain primitive ControlChange values; zero-velocity
NoteOn is preserved.

`Decoder.feed()` returns messages and frozen diagnostics in completion/detection
order, handles running status and interleaved real-time bytes, and retains
partial messages between calls. Its configurable `sysex_limit` defaults to
1 MiB of payload. Oversized SysEx is discarded with one diagnostic; real-time
delivery and parsing subsequent messages continue. `finish()` reports truncated
input and clears state. `reset()` silently clears state after known loss.
As a conservative policy, SysEx ended by a new status instead of F7 is also
discarded with a diagnostic, even though MIDI permits implicit termination.
Use one decoder per source; diagnostic offsets count bytes actually fed.

These are live byte-stream rules, not Standard MIDI File or UMP parsing.
Encoding adds full status/framing bytes. No timestamps are invented or attached
to decoded values; raw transport callbacks remain raw; typed adapters add event timing explicitly. Input snapshots and returned
lists have memory costs beyond the retained SysEx limit. Details and remaining
protocol boundaries are in [the API contract](plan/api.md).

## MIDI Tuning Standard

```python
from meedee import codecs, messages, tuning

conversion = tuning.Pitch.from_frequency(frequency_hz=441.123456)
change = tuning.SingleNoteChange(
    device_id=1,
    program=0,
    changes=[tuning.NoteTuning(note=69, pitch=conversion.pitch)],
)
wire = codecs.encode(change)
decoded = tuning.decode(messages.SysEx(data=wire[1:-1]))
assert decoded is not None
assert decoded.message == change
# Inspect conversion.error_cents before explicitly sending wire to an instrument.
```

The pure tuning module supports original/banked dump requests, key-based tables,
single-note changes and both scale/octave resolutions. Values and messages are
frozen, with exact integer protocol storage, explicit NoChange, checked
replacement and quantization error reports. Hz/cents inputs are rounded only to
the chosen encoding's resolution. Channels are 1–16; tuning banks/programs are
0–127. Every message requires a device ID; 127 explicitly requests broadcast.

The byte decoder keeps returning raw SysEx; `tuning.decode` performs explicit
interpretation. Original-bulk checksum mismatches are reported and accepted for
legacy interoperability; later formats require a correct checksum. `ReplyTracker`
offers caller-driven unicast correlation, deadlines and cancellation without
opening ports or sending requests. MTS has no transaction ID, so identical late
or unsolicited dumps cannot be distinguished. A successful send does not prove
an instrument accepted or applied the tuning. See [the API contract](plan/api.md)
for units, names, checksum behavior and request tracking.

## Optional uFor tuning bridge

On Python 3.13+, install the `ufor` extra (`uv sync --extra ufor --python 3.13`).
`meedee.ufor_tuning.preview(source, notes=[...])` resolves an existing uFor
`Tuning`, retaining exact frequencies where available, encoded pitches and signed
MTS rounding errors. Finite-source gaps and evaluation/range failures stay
explicit. `bulk_dump()` and `note_changes()` construct messages with explicit
device/program/bank and real-time settings. They open no ports and send nothing.
Unmapped keys require deliberate `preserve_unmapped=True`; invalid pitches always
block generation. See the [bridge contract](plan/ufor-tuning.md).

Ordinary meedee installs retain Python 3.11 support and no runtime dependencies.
The optional module explains its Python requirement when imported on 3.11/3.12.

## General MIDI and profile helpers

```python
from meedee import codecs, controls, gm, mpe

matches = gm.find_programs("acoustic grand piano")  # All GM1/GM2 matches.
entry = gm.gm2_program(bank_lsb=4, program=6)
assert entry is not None and entry.name == "EP Phase"
selection = gm.BankProgramSelect(
    channel=1, bank_msb=entry.bank_msb, bank_lsb=entry.bank_lsb, program=entry.program,
)
packets = [codecs.encode(p) for p in codecs.expand(selection)]
volume = controls.Controller.ChannelVolume.message(channel=1, value=100)
zone = mpe.ZoneConfiguration(manager_channel=1, members=7)
# An open MIDI 1.0 output can explicitly send selection or zone with send_command().
```

Rust owns the GM1 instrument/family and percussion maps, all 256 GM2 melodic
entries and nine drum-kit note maps. Python returns frozen entries or list
copies. Program numbers are 1–128, notes and bank bytes retain their wire
numbers, and GM1 percussion uses channel 10. GM1 defines no banks: its entries
expose program/name/family only. GM2 entries provide their standardized banks.
Unknown map addresses return None; the library does not emulate a device's
fallback sounds. Name lookup ignores case and surrounding whitespace, preserves
the published names and returns all matches without nickname/fuzzy aliases.

BankProgramSelect sends both bank halves before ProgramChange, with the same
serialization and partial-progress rules as other commands. It never changes
mode automatically. Gm1On/Gm2On/GmOff are explicit SysEx values; GM2 On resets
musical state and requires settling time (the standard specifies up to 100 ms).
The library inserts no sleeps. GM1 initialization varies by device. GM2 bank
selection can change a channel's melodic/rhythm role, including channel 10.

`meedee.universal` provides identity request/reply, master volume and signed
master fine/coarse tuning. `meedee.timecode` provides full-frame MTC values with
an explicit rate code and broadcast device ID 127, plus explicit quarter-frame
assembly and SMPTE position arithmetic. Their `decode(SysEx)` functions interpret
complete messages explicitly; the byte decoder continues to return raw SysEx.
MPE support configures one zone at a time and offers explicit allocation and
overlap-aware tracking; it does not configure bend ranges or infer release tails.
Named RPN helpers keep pitch-bend
cents, fine-tuning units and GM2 modulation-depth units distinct. Channel-mode
helpers construct ordinary CC values. All Notes Off can leave sustained sound;
it has a different purpose from All Sound Off.

Feed a relevant typed GM mode message to the control interpreter, or reset it
explicitly, to invalidate cached context. Constructing/sending a value has no
automatic connection to that pure interpreter. Bounded GM2 controller-destination,
key-based instrument-control and global effect-parameter SysEx codecs are available.
These codecs and maps do not imply a synthesizer, complete GM2 message coverage,
device support or live qualification. See [the API contract](plan/api.md).

Slice 6.5 adds bounded GM2 routing/key/global-parameter SysEx, a pure MPE allocator
and overlap-aware tracker, one-shot high-resolution velocity interpretation, and
caller-driven quarter-frame time-code assembly. These operate without ports or
background clocks. See [the API policies](plan/api.md) for explicit source/reset,
allocation, sustain, entry limits and sequence-lock behavior.


## Fine controls and parameters

```python
from meedee.codecs import expand
from meedee.controls import Interpreter
from meedee.messages import NrpnSet

command = NrpnSet(channel=1, parameter=1234, value=10000, resolution=14)
primitives = expand(command)
interpreter = Interpreter()  # One per independent input source.
observations = [o for m in primitives for o in interpreter.feed(m)]
# An open MIDI 1.0 output can send the whole command:
# connection.send_command(command)
```

`ControlChange14` emits a coarse/fine CC pair. RPN/NRPN set commands require
explicit 7-/14-bit resolution. Increment/decrement commands represent one
directional operation, with no guessed amount or physical unit. Each parameter
command selects afresh and finishes with RPN-null, without reserving an NRPN
identifier. Raw `send()` and primitive `encode()` remain unchanged; use `expand`
to inspect commands and `send_command` to serialize their complete sequence.
UMP outputs reject these MIDI 1.0 commands rather than translating silently.

`send_command` prevents interleaving on its connection. `CompoundSendError`
reports the known completed-message prefix and total count. A failed message
may already have effects; worker loss or timeout reports unknown progress
(`completed_messages=None`). It never retries or sends more cleanup after an
error, so selection release is not guaranteed after failure. Other applications
can still interleave traffic on the device.

The optional interpreter returns frozen, indexed observations and diagnostics
alongside your retained raw stream. It never fabricates a missing fine value or
an absolute value from a relative step. A new coarse CC forgets old fine state;
repeated fine updates can use that known coarse value. A changed complete RPN/
NRPN selection requires both halves afresh, a deliberate conservative policy
that declines single-half selector edits. Bank Select and Data Entry are not
presented as generic knobs. Reset after known loss/reconnect; SystemReset and
Reset All Controllers invalidate the applicable local state automatically.
There are no timers, guessed device defaults, timestamps or source registry.
Rust exposes the same values and state machine; its common Message marker has
PrimitiveMessage and Command subtraits for encoding and expansion.

## UMP and MIDI 2.0

```python
from meedee import ump

note = ump.Midi2NoteOn(
    group=1, channel=1, note=60, velocity=50_000,
    attribute_type=0, attribute_data=0,
)
words = ump.encode(note)  # Immutable tuple of 32-bit words.
assert ump.decode(words) == note
report = ump.to_midi1(note)
assert report.status == ump.TranslationStatus.Quantized
assert not report.converted  # Explicit allow_loss=True is needed for output.
```

This pure module includes groupless utility/stream packets, grouped MIDI 1.0,
all MIDI 2.0 channel-voice families, SysEx7/8 packetization and bounded assembly,
Mixed Data Set packets, Flex Data and endpoint/function-block message codecs.
Groups/channels use 1–16 and programs 1–128; native-width values remain integers.
The incremental Decoder reports malformed packets and recovers at their declared
word boundaries. Optional OpaquePacket preservation retains complete rejected or
unknown layouts without claiming semantic validity or safe device transmission.
Byte conversion requires an explicit byte order. JR ticks are not host nanoseconds.

Assembler defaults to 1 MiB per message, 8 MiB total retained payload and 64
active streams. It reports interruption, abort, overflow and truncation without
returning partial SysEx/text as complete. Flex and endpoint text also obey their
smaller protocol limits. Retain raw packets, use one engine per independent
source and reset after known loss. Text assembly validates UTF-8 and padding;
it does not interpret lyrics, dates or language tags. MDS packets are preserved;
dataset transfer/assembly is not provided.

Translation is explicit and stateless. It reports exact, quantized, unsupported,
profile-dependent and context-required cases. Bank/RPN/velocity-prefix input
sequences require source context and are not guessed. Returned primitive lists
are not atomic live sends; typed output requires an explicit send operation.
Declared Capabilities can reject incompatible protocol/stream use; None means
those declarations do not establish support. No capability discovery or profile
negotiation runs automatically. Raw output still takes meedee.midi.UmpPacket;
construct it explicitly from the encoded words. CoreMIDI UMP SysEx7 discovery/reply
has passed between test-owned virtual peers. Other native backends and physical
MIDI 2.0 device compatibility remain unqualified.

## Python transport

```python
from meedee.midi import Output

output = Output()
ports = output.ports()
# Select an endpoint deliberately from this snapshot, for example ports[index].
# with output.connect(ports[index]) as connection:
#     connection.send(bytes([0x90, 60, 100]))
#     connection.send(bytes([0x80, 60, 0]))
```

`Input` and `Output` constructors accept keyword-only settings. Defaults are
`protocol=Protocol.Midi1`, `backend=None`, `isolated=True`, and `timeout=5.0`.
Input additionally accepts `timestamps=TimestampMode.Absolute` and
`receive_byte_limit=8_388_608`. Choose `Protocol.Ump` explicitly for UMP, and
send an immutable `UmpPacket(words=[...])`. There is no automatic protocol
selection, conversion or backend fallback. `compiled_backends()` reports build
support, not service, driver or device health. An unavailable backend raises
`CapabilityError`.

`ports()` returns immutable `Port` snapshots with `name`, `backend`, and
`backend_id`. IDs can duplicate, be zero, or change. A snapshot belongs to one
client and discovery generation; refreshing discovery invalidates older
snapshots. `connect(port, callback)` on input or `connect(port)` on output uses
that selection without looking it up again by ID. Virtual endpoints use
`create_virtual(name, callback)` or `create_virtual(name)`. Native client and
real-connection names are not configurable through the pinned C API. CoreMIDI
virtual names still use the fork's existing ASCII input conversion.

Input callbacks accept one immutable `ReceivedPacket` from `meedee.events`.
Its `packet` is owned `bytes` or an immutable `UmpPacket` whose `words` are a
tuple of unsigned 32-bit integers. Its `timestamp_ns` is an exact signed integer
or `None` when capture is disabled. Zero is a valid captured timestamp. `clock`
records backend, mode, unique connection identity and the exact tick unit
1/1,000,000,000 seconds; resolution is unknown. Reopening and distinct isolated
children use distinct clock identities. Backend Absolute timestamps may have a
host-clock fallback, are not UTC and are not assumed comparable across clocks.
Nanosecond storage does not promise nanosecond accuracy.

Receive packets are raw transport chunks, including partial SysEx or multiple
UMPs. The binding does not assemble unfinished SysEx or apply `Ignore` filters.
Use the pure byte codec explicitly for framing; typed transport/timestamps and
receive filters remain later work. `send()` accepts unsigned-byte buffers,
including bytearray and memoryview, or `UmpPacket`; mutable and strided buffers
are snapshotted. Send
validation checks protocol, size and UMP framing, not full MIDI semantics.
Success means backend acceptance, not physical delivery. UMP batch failures
report an accepted prefix; the failed packet may also have had effects. There
is no automatic resend, reconnection, tuning reset or all-notes-off on close.

## Ownership, isolation and failures

Retain the returned connection. It keeps its client alive and owns its single
open resource. The client is unavailable for discovery/opening while connected.
Successful explicit `close()` restores the original client and returns `None`;
context managers close too. Body and close failures are preserved together in a
`BaseExceptionGroup`. `closed` is a momentary ownership snapshot, not device
health or a reservation against another thread closing it.

Every failed open that enters the Rust transport invalidates the client,
including stale/foreign snapshots. Ordinary Python argument validation happens
first. Native discovery/send/close errors can also require a fresh client.
An interrupted isolated request invalidates its worker because native ownership
or send effects may be uncertain. Failure to allocate the returned connection's
clock after opening also discards that worker.
Input silence does not identify unplugging, rebooting, unresponsiveness or an
idle device. A failed send can already have changed physical state.

By default each client has a separate Python child process owning its native
transport. Input callbacks run in the parent. Native operation deadlines
terminate a failed child; native errors, crashes and timeouts invalidate that
client without automatically reopening it. `worker_failure` reports a known
worker failure, not endpoint health. `isolated=False` uses the same Rust
transport in-process with fewer resources and no native-operation deadline.
Both modes use a dedicated native owner thread per client for creation,
operations and teardown; Windows COM initialization stays on that thread.

A parent Python process admits at most 32 live isolated workers. Each Input or
Output client uses one slot, even when used only for discovery; an input/output
pair uses two. Starts in progress and retiring children count until process exit
is confirmed. At the limit construction raises ResourceError before starting
another child. Startup failures release their reservation. These counts are
separate from native-client/connection limits and are not a machine-wide memory
budget or a guarantee that 32 workers fit on a Pi.
Closing a connection preserves its reusable client and worker. Release unneeded
clients and references from their connections when finished; asynchronous worker
cleanup may keep a slot occupied briefly afterward.

Callbacks must be synchronous and return promptly. Deferred results are reported
through `sys.unraisablehook`, without executing them; later delivery continues.
Callback exceptions are reported the same way. Blocking MIDI operations are
forbidden from callbacks and error hooks, including operations on other clients.
No internal Rust lock is held while calling user code. A callback must not wait
for its closer or a lock/queue held by its closer. Arbitrary Python or extension
code cannot be forcibly cancelled; callback draining can wait indefinitely.
`last_message_time` records Python monotonic seconds at callback start, separate
from native timestamps, and survives close.

Native receive admission bounds owned in-flight payloads to 8 MiB per input by
default. Each isolated receive queue additionally holds at most 128 packets and
8 MiB of payload. The same input byte-limit setting applies at each stage.
`dropped_packets` combines known native-admission and forwarding/dispatch queue
drops. While input is idle, the existing forwarding thread checks for changed
worker drop counts on its 100 ms queue wake-up and reports them without another
MIDI packet or command. This is a reporting cadence, not a latency deadline under
CPU/GIL contention or pipe backpressure. A worker crash can still lose the final
count, and driver loss is not measured.
Application-retained data, Python/queue overhead and OS buffers are outside these
budgets. Floods and allocation/process aborts remain possible.

Destruction and cyclic GC retire delivery and schedule cleanup without waiting
for a driver. Restoration is asynchronous; explicit close is the healthy reuse
path. Failed checked cleanup retains native state and its owner thread until
process exit without retry. The Rust core permits 32 native clients per process,
including retained clients; the Python cleanup registry permits 32 connections
and has four workers. Stalled callbacks or teardown can occupy all cleanup
workers. There is no global quota across isolated processes.

Importing transport initializes cleanup workers without opening a MIDI device.
Importing messages/events alone does not start them. Interpreter shutdown retires
callbacks and drains cleanup; isolated children that exceed their deadline are
terminated and reaped. In-process driver calls and parent callbacks can still
block shutdown. Forced termination bypasses graceful cleanup. Free-threaded
Python, subinterpreters and forking with imported/live MIDI state are unsupported.

## Build and qualification

`crates/meedee` is the Rust toolkit; the root crate binds it using PyO3 in one
private `meedee._core` extension. Rust use requires no Python. Both languages
use the same libremidi implementation; midir has been removed.

Source builds require Rust 1.87+, CMake 3.22+, a C++20 toolchain and CPython 3.11+.
Native libremidi sources are pinned and statically linked with no production
build-time download. macOS requires Command Line Tools and macOS 11+; Windows
requires the MSVC C++ tools. Linux requires ALSA development headers and
pkg-config for builds, and system `libasound.so.2` at runtime. Wheels do not
bundle ALSA. ALSA sequencer I/O also requires an accessible `/dev/snd/seq`.

Native OS backends are selected; optional JACK, PipeWire, network and keyboard
backends are disabled. CoreMIDI/UMP, available ALSA variants and WinMM are built.
Windows MIDI Services remains disabled pending pinned SDK/tooling integration
and native qualification. WinMM discovery checks capability-query failures;
its new regression targets still require Windows validation. Mac MIDI 1 virtual
loopbacks pass in Rust and both Python modes, along with typed capture/replay/
cancellation and a CoreMIDI UMP SysEx7 discovery/reply exchange. These passed in
the owner's Terminal; a direct Apple API probe isolates the agent's first-call
failure to its execution context. Physical hardware, fault recovery, Windows
and Pi qualification remain open. See [validation](plan/validation.md).

CoreMIDI retains one additional native client and notification-loop thread per
MIDI-using process until exit, avoiding Apple's last-client disposal/reopen hazard
and continuously servicing topology notifications. Existing per-client operation
threads remain separate. Virtual creation/disposal waits up to two seconds for
its matching local notification; SDK calls themselves can still block. A failed
CoreMIDI metadata read fails discovery without returning partial results or
preventing a later explicit discovery attempt. There are no automatic retries.
Ports and ordinary clients still close normally. Pure message/file imports do
not initialize this runtime. This is not recovery from a crashed/restarted MIDI service;
default isolation permits replacing the affected child, while in-process use
may require restarting the application. Observer disposal failures remain open.

Release CI remains triggered only by a published release, testing Python 3.11
and 3.15 on Linux, macOS arm64/Intel and Windows. It includes native regression
checks, Rust/Python checks, source/wheel builds and installed-package tests.
Configured jobs are not evidence of a passing release run.

The opt-in [qualification procedure](plan/qualification.md) provides bounded
synthetic workloads and supervised test-owned virtual-port measurements in Rust
and Python. Mac software baselines do not establish Pi performance, physical
device behavior or live MIDI 2.0 support. See the
[feature matrix and evidence](plan/validation.md).
The [Pi 5 handoff](plan/pi-qualification.md) separates inventory, device-free
builds, authorized software loopback and later physical qualification. Its OS
image and configuration remain unselected; no Pi qualification has run.

```sh
uv run --frozen pytest
uv run --frozen ruff check --select B,E,F,I meedee test
uv run --frozen ruff format --check meedee test
uv run --frozen ty check meedee test
cargo fmt --all --check
PYO3_PYTHON="$PWD/.venv/bin/python" cargo test --locked --workspace -- --test-threads=1
uv run maturin build --release --locked --sdist --out dist
```

On Windows set `PYO3_PYTHON` to `.venv/Scripts/python.exe`. Native software-port
checks in `test/manual/loopback.py` and `test/manual/windows.py` are explicit
opt-ins and require target-host authorization. No unit test selects hardware.

meedee is MIT licensed. See [third-party notices](THIRD_PARTY_NOTICES.md) and
[vendored provenance](third_party/libremidi/SOURCE.md). Publication is a separate
decision after the remaining qualification gates.
### Observed note activity

`meedee.notes` observes MIDI 1 bytes, MIDI 1 in UMP and native MIDI 2 without
opening a device. For example:

```python
from meedee import events, messages, notes

tracker = notes.Tracker(stream="keyboard")
tracker.feed(events.Event(
    stream="keyboard", message=messages.NoteOn(channel=1, note=60, velocity=100)
))
channel = tracker.snapshot().channels[0]
assert channel.notes[0].held == 1
```

Counts remain separate by source, UMP group, channel and protocol. `held` records
unreleased observed activity; `released` records matched releases since the last
boundary. Neither is a sounding-voice count. Repeated keys use counts without
arbitrary instance pairing. Raw sustain/sostenuto values retain native resolution.
An unmatched release marks uncertainty; capture may also begin mid-performance.
Loss diagnostics, clock-identity changes and explicit `invalidate()` discard
affected history. Nothing generates cleanup traffic or claims device ownership.

`detail_limit` bounds admitted sources/channels (128 by default), with at most
128 keys per channel and explicit `omitted_events`. File inspection exposes the
same state as `report.note_state`, keeping SMF tracks separate. CLI inspection,
monitor event rows and final summaries include it. Monitor filters affect display
only. Outstanding notes and unmatched releases do not change file completeness
or validation exit codes. See [the API contract](plan/api.md#observed-note-activity)
for reset, channel-mode and uncertainty policies.

### Static routing and patch bay

`meedee.routing` provides caller-driven Rust/Python routes, bounded arrival-order
merging and fan-out. Routes select exact sources and optionally filter/remap
channels and UMP groups. Channel voice is forwarded by default; other traffic
requires opt-in. Source/timestamp/span provenance stays attached to each delivery,
and native MIDI 2 values are preserved. There is no automatic protocol conversion,
timestamp scheduling, reconnect or cleanup send.

```python
from meedee import events, messages, routing

router = routing.Router(routes=[routing.Route(
    name="keys-to-synth", source="keys", destination="synth",
    protocol="midi1", remap_channel=2,
)])
router.feed(events.Event(
    stream="keys", message=messages.NoteOn(channel=1, note=60, velocity=100)
))
delivery = router.peek()
# The host sends delivery.event.message to the selected destination.
# After success: router.confirm(delivery.id)
# After a failed/uncertain send: router.fail(delivery.id, reason)
```

Install the optional CLI extra for offline archive preview or live forwarding:

```bash
meedee route-preview capture.meedee --stream keys --protocol midi1 --destinations synth --remap-channel 2
meedee patch "Keyboard" --protocol midi1 --destinations "Synthesizer" --remap-channel 2
```

Preview opens no ports and labels simulated deliveries `planned`. Patch uses exact
unique port names and isolation by default. It sends only when explicitly invoked;
changing routes requires a new session. `--include-nonchannel` permits supported
system/SysEx/UMP traffic; file-only/opaque UMP and compound commands are not
implicitly sent. Byte MIDI has no group options.

The queue defaults to 128 deliveries and 8 MiB of encoded payload. Capacity
admission is all-or-none for each fan-out; sending can stop after a partial fan-out.
Reports retain per-route capacity rejections, confirmed progress, pending copies
and uncertain destinations. Input loss, overload or output failure stops the whole
session without cleanup or retry. Already-sent notes can remain active. Object and
provenance overhead is outside the payload-byte budget; returned snapshots/examples
are caller-owned copies. External feedback loops and physical device responses
remain unqualified. See [the routing contract](plan/api.md#static-protocol-aware-routing).

### Port discovery and topology changes

The optional CLI can list ports or watch discovery without opening endpoints:

```bash
meedee ports --protocol midi1 --output json
meedee watch-ports --protocol ump --interval 1 --duration 60
```

Watch prints an initial baseline, changes and a final bounded summary. Unchanged
scans stay quiet. Discovery failure or port-limit overflow stops the session with
status 2 and retains the last complete snapshot; ordinary topology changes and
clean completion/Ctrl-C return 0. Failed initial discovery is explicitly marked
uninitialized rather than presented as an empty port list.

`meedee.topology` provides the same pure Rust/Python snapshot/change model.
Descriptors compare backend, protocol, direction, backend ID and name, preserving
duplicate counts. A rename is removal/addition, not proof of device continuity.
`Tracker.observe(...)` accepts complete descriptor lists; the first accepted scan
establishes a baseline without inventing arrivals.

`meedee.discovery.Poller` creates its existing isolated discovery clients only on
the first explicit poll, reuses them, and releases its references on close:

```python
from meedee.discovery import Poller

with Poller(protocol="midi1") as poller:
    report = poller.poll()
    if report.stopped:
        print(report.reason)
```

Defaults retain at most 256 port instances, 128 change samples per scan and 128
changed-scan history entries. These bound retained cardinality, not native
enumeration allocation or metadata byte sizes. Input and output enumeration is
serial; polling can miss transient changes and does not establish physical
identity or device health. No reconnect, route change or MIDI-state restoration
is performed. See [the topology contract](plan/api.md#topology-observations-and-discovery-polling).

### Explicit MIDI 1 SysEx transfers

Validate and preview a complete `.syx` file without opening MIDI ports:

```sh
meedee transfer-preview backup.syx --gap-ms 100 --output json
```

To send, choose a gap from the instrument's documented transfer requirements and
an exact, unique output name. Sending may change instrument state:

```sh
meedee transfer-send backup.syx --gap-ms 100 --port 'Exact output name'
```

The 100 ms value is an example, not a device recommendation. `--gap-ms` is
required; zero sends consecutive complete messages. No message splitting,
automatic retries or device acknowledgements are inferred. The file is fully
validated before output discovery. Defaults bound plans to 256 messages, 8 MiB
of framed bytes and 1 MiB per payload; CLI options can change these limits.
Preview samples are bounded by `--detail-limit` and `--preview-bytes`.

The shared Rust/Python `transfer` module supplies immutable plans and caller-driven
sessions; Python `transfer_output.run` delivers through an existing MIDI 1 typed
output and accepts `events.Cancellation`. Successful handoffs and uncertain sends
are reported separately. Handoff does not prove instrument acceptance; Ctrl-C or
cancellation cannot interrupt a blocked native call. See the
[transfer contract](plan/api.md#bounded-midi-1-sysex-transfers) for timing,
cancellation, progress fields and CLI status codes. No native device transfer
has been qualified by this implementation's automated checks.

### One MIDI-CI management scan

The optional CLI can discover CI participants on one explicitly selected route,
then ask format-2 peers for Product Instance IDs. Supply your local participant's
manufacturer, family, model and revision fields. For example, an explicitly
experimental local identity can be used during development:

```sh
meedee ci-browse --input-port 'Exact input name' --output-port 'Exact output name' \
  --protocol midi1 --manufacturer 7d0000 --family 1 --model 1 --revision 00000000
```

Those identity fields describe the local browser, not the device being sought.
For UMP, use `--protocol ump --group N` with the intended group (1..16). A fresh
random MUID is generated for each scan. Both ports must be uniquely named;
process isolation is enabled by default. `--output json` produces a final report.
The default discovery window is 3000 ms, endpoint timeout 1000 ms and overall
scan deadline 30000 ms, adjustable with `--discovery-ms`, `--endpoint-ms` and
`--timeout-ms`. The overall deadline does not interrupt blocked native calls.

This sends management traffic, including protocol replies and required collision
invalidations. It stops on loss/failure and never restarts or retries automatically.
It performs no Profiles, Property Exchange or instrument configuration writes.
An empty scan or timeout does not prove a device is offline. Peer identities and
capability bits are declarations; successful output handoff is not device
acknowledgement. Rust `ci_browser::Browser`, Python `ci.Browser` and `ci_live.run`
provide the same reusable workflow. See the [browser contract](plan/ci.md#one-scan-management-browser)
for bounds, cancellation, reporting, local identity and qualification limits.

### Deterministic transport tests

`meedee.testing` supplies in-memory inputs, recording outputs and an explicit
manual clock. It opens no devices and starts no threads or MIDI lifecycle:

```python
from meedee import events, testing

clock = events.Clock(identity="test-clock", origin="manual")
receiver = testing.Input(stream="keys", protocol="midi1")
receiver.inject(b"\x90\x3c\x64", clock=clock, timestamp_ns=10)
event = receiver.receive()  # Decoded by the production packet decoder.

sink = testing.Output(protocol="midi1")
if isinstance(event, events.Event):
    sink.send_message(event.message)
assert sink.take_packet() == b"\x90\x3c\x64"
```

Use `protocol="ump"` with UmpPacket/native UMP message values for MIDI 2 tests.
Queue limits drop newest input while preserving counters and loss diagnostics;
recorded-output capacity rejects sends without erasing evidence. Explicit
`lose`, `fail`, `close` and `fail_next_send(accepted=True)` exercise loss,
disconnection and uncertain delivery. The latter records a simulated sink effect
while raising SendError, so callers must retain uncertainty rather than retry.

`ManualClock.advance` changes time only when called. Empty receive returns None
immediately, even with a timeout; it never advances a clock. Hosts must arrange
explicit time progression. Rust has the same `testing` core. See the
[testing contract](plan/api.md#deterministic-in-memory-testing-transports) for the
supported connection subset, bounds and queue semantics. These tools do not
establish native driver or physical-device behavior.
