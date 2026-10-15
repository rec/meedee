# meedee API contract and representation prototype

Slices 1–4, updated 2026-10-14. This document records the approved API choices,
the original device-free prototype, and the MIDI 1.0 byte codec.
Platform qualification and higher-level protocols remain separate plan gates.

Additional work beyond the prompt

None.

## Approved language contract

Use native frozen PyO3 objects that own validated Rust values. Do not add
Pydantic or implement musical validation separately in Python. Concrete native
classes derive from a nonconstructible `Message` base. Supported concrete types
have no instance dictionary, writable fields, or Python-subclass extension
point. Immutability includes nested payload ownership.

Public definitions belong to these modules, without top-level re-exports:

| Module | Responsibility |
| --- | --- |
| `meedee.messages` | `Message` and concrete musical values/commands |
| `meedee.codecs` | Pure byte encoding and incremental decoding; no native I/O |
| `meedee.controls` | Optional per-source control and parameter observations |
| `meedee.events` | `Event`, `Timestamp`, and clock descriptors |
| `meedee.midi` | Discovery, transport capabilities, clients, connections and transport errors |

Keep `__init__.py` empty. Rust modules mirror responsibilities, using idiomatic
snake_case functions and typed structs. The current transport API remains in
place during this slice and may be replaced in slice 2 without aliases.

The installed binary is `meedee._core`. Public loaders register native values
without initializing MIDI for message/event imports. Both Python transport modes
now use the shared Rust libremidi wrapper. There are no old transport aliases.

Slice 2 transport choices, approved 2026-10-14:

- `meedee.midi.Input`, `Output`, immutable `Port` snapshots and connection types.
  Constructors take keyword-only settings; discovery refresh invalidates old
  snapshots. `backend_id` is metadata, not a unique persistent identity.
- Remove native naming parameters except the virtual endpoint name, and remove
  ID lookup. The pinned C API cannot honor the former and IDs cannot guarantee
  the latter. Keep default per-client process isolation and its in-process option.
- Callbacks take an immutable `meedee.events.ReceivedPacket`: exact optional
  signed `timestamp_ns`, connection `clock`, and raw `packet`. Bytes remain
  bytes; UMP uses an immutable UmpPacket with a tuple of 32-bit words.
- Raw chunks/batches are exposed now. Message framing and Ignore filters wait
  for the bounded codec slice; no unfinished SysEx assembly is introduced.
- Failed opens entering Rust invalidate the client in both modes. Ordinary
  Python validation happens first. Successful explicit close restores it.

### Construction, inspection and replacement

Python constructors are keyword-only. Integer fields require built-in `int`
values, rejecting booleans, floats, strings and integer subclasses. Rust uses
appropriately sized integer arguments with checked constructors. Neither
language silently clamps values or accepts invalid states.

Examples of the intended public construction style:

```text
NoteOn(channel=1, note=60, velocity=100)
ProgramChange(channel=1, program=1)
ControlChange(channel=1, controller=7, value=100)
SysEx(data=b'\x7e\x7f\x06\x01')
note.replace(velocity=80)
```

Expose readable fields, constructor signatures, field annotations, `repr`,
same-type value equality and stable hashes for deeply immutable types. Python
class patterns work through `__match_args__`; positional matching does not
permit positional construction. Objects need not satisfy `dataclasses.is_dataclass`.
`replace(**changes)` accepts existing fields only, revalidates through the same
Rust constructor, and returns a new value. Explicit `None` is an invalid value
for required integer fields, not a request to keep the old field. Rust checked
replacement likewise returns a new validated value; immutable fields stay private.

| Condition | Python | Rust core |
| --- | --- | --- |
| Wrong argument type, positional construction, unknown field | `TypeError` | Prevented by signatures/types where possible |
| Out-of-range integer, including arbitrarily large Python integers | `ValueError` | Validation error carrying field/range context |
| Invalid SysEx payload byte | `ValueError` | Payload validation error |
| Unsupported buffer shape/format | Buffer/type error at the input boundary | Caller supplies owned bytes |
| Invalid transport capability or native operation | Existing transport error families, refined in slice 2 | Distinct from pure value validation |

Do not freeze transport error representations or codec recovery policies based
on this small prototype. Encoding, decoding, allocation failure and native
failures will retain distinct error contexts at their respective boundaries.

### Numbering and protocol widths

Channels are 1–16 and Program Change numbers are 1–128 in both public APIs.
Codecs convert to 0–15 and 0–127 respectively, and restore public numbers on
decode. CC identifiers and seven-bit CC values remain 0–127. Bank values,
RPN/NRPN identifiers and MTS tuning bank/program fields retain their standard
ranges. These conventions apply to later MIDI 2.0 types too; wider controller
and velocity values retain their protocol widths.

Do not conflate a MIDI 1.0 type with its MIDI 2.0 counterpart. `NoteOn` has
seven-bit velocity; a later `Midi2NoteOn` has its own explicit fields. The
representation prototype initially implemented only `NoteOn` and `SysEx`.
Slice 3 adds the other primitive MIDI 1.0 values and byte codecs below.

### SysEx ownership

The prototype stores an owned Rust byte payload, excluding F0/F7 framing, and
requires every byte to be seven-bit. It accepts one-dimensional contiguous
unsigned-byte buffers, including bytes, bytearray and suitable memoryviews.
Mutable inputs are copied before the constructor returns. The public `data`
property returns immutable Python bytes; its prototype getter makes a copy.
This is measured representation behavior, not a zero-copy guarantee.

### Slice 3 byte codec, 2026-10-14

Approved choices: ordered messages/diagnostics with recovery, a configurable
1 MiB SysEx default, pure byte codecs with later timestamp integration, and
signed pitch bend centered at zero.

`meedee.codecs` is pure: importing it or constructing a decoder starts no MIDI
workers. The Rust equivalent is `meedee::codecs`, with immutable structs,
the `Message` trait and inline `AnyMessage` variants in `meedee::messages`.
No Python dependency is involved in Rust use.

The primitive inventory is NoteOn, NoteOff, PolyPressure, ControlChange,
ProgramChange, ChannelPressure, PitchBend, QuarterFrame, SongPosition, SongSelect,
TuneRequest, TimingClock, Start, Continue, Stop, ActiveSensing, SystemReset and
SysEx. Channel-mode bytes remain exact ControlChange values, including controller
identifiers 120–127; the codec does not interpret their effects or validate a
particular profile's controller semantics. NoteOn with zero velocity stays
NoteOn. PitchBend.value is -8192–8191 with zero centered. SongPosition.position
is 0–16383 MIDI beats, each six MIDI clocks; SongSelect.song remains 0–127.
QuarterFrame exposes the raw message_type (0–7) and value nibble (0–15), without
time-code assembly or validation of a complete time-code sequence.

`encode(message)` emits owned bytes for one primitive message, with its explicit
status and SysEx F0/F7 framing. It never introduces running-status compression.
`Decoder(*, sysex_limit=1048576)` is mutable, with one instance per independent
byte source. Its setting is a strict positive built-in integer in Python.
`feed(data)` accepts one-dimensional unsigned-byte buffers, including strided
memoryviews, snapshots them, and returns an ordered list of Message/Diagnostic
values. Empty feeds are harmless and arbitrary feed partitions have no semantic
effect. `Diagnostic` is frozen and exposes kind, offset and byte. Offsets count
bytes actually fed, starting at zero, across calls; finish uses the next offset
and byte=None. At the machine's maximum usize offset the counter saturates.

Diagnostics identify unexpected data/end bytes, undefined statuses, interrupted
or truncated short/SysEx messages, and SysEx overflow. Undefined F4/F5 clear
running status. F9/FD are diagnosed but preserve running status, partial messages
and SysEx, as do all defined real-time messages. SystemReset remains a wire value;
interpreting receiver reset effects is outside this codec. Output is ordered by
message completion or diagnostic detection, so interleaved real-time messages
can precede the longer message that began before them. No user error callback
executes inside the parser.

The current policy for non-F7 SysEx termination is deliberately conservative:
although the standard permits another non-real-time status to terminate SysEx,
the decoder discards that payload, reports InterruptedSysEx and processes the
new status normally. It cannot distinguish deliberate implicit termination from
an interrupted device dump. It never presents such a payload as complete SysEx.

The SysEx cap counts payload bytes, excluding framing and interleaved real-time
bytes. Exceeding it releases the accumulated payload, reports once, discards
data until F7 or another non-real-time status, and continues real-time delivery.
It never emits a truncated oversized payload. The cap bounds retained payload,
not allocator overhead, feed snapshots or returned lists. Divide very large
inputs into feeds appropriate to the application's memory and responsiveness
budget. Rust allocation failure can still abort the process; a Python allocation
failure after consumption does not roll back decoder state or promise partial
results. Discard/reset the decoder after such a failure.

`finish()` diagnoses unfinished input once and clears all parsing/running status;
later feeds can start another stream at the existing cumulative offset. `reset()`
clears state without a diagnostic or synthetic message, retaining offsets. Use
it after known transport loss or a deliberate boundary; unknown driver loss
cannot be detected from bytes alone. This decoder is for live MIDI 1.0 bytes,
not UMP or Standard MIDI Files.

Events/timestamps, transport integration and receive filters remain separate
from this codec. Transport callbacks continue to receive ReceivedPacket;
timestamped typed integration belongs to slice 10. Fine CC and RPN/NRPN are
provided by slice 4 below and tuning by slice 5; GM profiles and files retain later slices.

### Slice 4 compound commands and observations, approved 2026-10-14

Seven frozen Message subclasses are added in `meedee.messages`:
ControlChange14(channel, controller, value), RpnSet/NrpnSet(channel, parameter,
value, resolution), and RpnIncrement/RpnDecrement/NrpnIncrement/NrpnDecrement
(channel, parameter). All fields are keyword-only strict integers, with checked
replacement, annotations and class patterns as for primitive messages.
ControlChange14 uses controller 0–31 and value 0–16383. Parameter identifiers
are 0–16383 except RPN 16383, which is null selection and cannot be set/stepped;
NRPN 16383 remains a legitimate identifier. Set resolution is required and
exactly 7 or 14, with value 0–127 or 0–16383 respectively. Changing resolution
does not rescale a value. All commands retain channel numbering 1–16.

`codecs.expand(command)` returns a fresh list of frozen primitive ControlChange
messages. `codecs.encode` stays primitive-only and rejects commands. A fine-CC
command emits MSB followed by LSB. Parameter commands always send both selectors
MSB first, then Data Entry MSB (and LSB for resolution 14), or one Data Increment/
Decrement with data byte zero. They finish with RPN 127/127 to release selection,
including after NRPN operations. No selector caching or NRPN-null convention is
introduced. Generic values represent raw data bits, not profile-specific physical
units. Increment/decrement has no amount field or guessed absolute value.

Rust `Message` is now the common marker trait. The existing encoding method
moves to `PrimitiveMessage: Message`; method-style encoding callers import that
subtrait. Free `codecs::encode` retains its behavior. `Command: Message` exposes
expansion and `AnyCommand` stores the compound command families inline, separate
from primitive-stream `AnyMessage`. Pure Rust users need no Python interpreter.

`OutputConnection.send_command(command)` accepts these commands on MIDI 1.0
outputs, rejecting UMP before sending. Raw `send` remains unchanged. One native
owner-thread operation and the existing connection serialization cover the whole
sequence, including release; raw sends, other commands and close cannot run
between its steps on that connection. This is not atomicity against other
clients, processes or external device state. The default isolated deadline
covers the whole command, not each individual message. In-process calls retain
their existing unbounded native-driver wait.

`CompoundSendError` derives from SendError and exposes completed_messages and
total_messages. The count is the prefix accepted without a backend error, not
physical confirmation. The failed message may also have had effects. A worker
crash/timeout, lost reply or native owner-thread failure can make progress
unknown (None). If a complete success reply arrives before worker failure is
observed, the error preserves the fully accepted count instead. Original worker timeout/crash details are the raised exception's
cause; worker_failure retains the underlying worker diagnosis. Cancellation
still propagates KeyboardInterrupt/SystemExit. No retry or additional cleanup
send occurs after a failed step. Invalidation follows existing send failure
rules. In-process closed remains false until native ownership is disposed,
even though the failed connection already rejects sends. Release cannot be
guaranteed after failure. Python error construction occurs after dropping the
connection mutex, so import hooks/exception constructors cannot run under it.

`meedee.controls.Interpreter()` is pure and optional. Use one instance per source;
it has fixed state for 16 channels and no parameter cache or source registry.
`feed(primitive_message)` returns frozen ControlObservation, ParameterObservation,
ParameterStep and Diagnostic values; it does not return, mutate or replace raw
messages. Non-control primitives still advance its zero-based input index.
No SysEx payload is copied merely to ignore that input. Keep the raw stream
separately. Feeding a compound command is an error; expand or decode it first.

ControlObservation contains channel/controller/value/resolution and first_index/
last_index. A new coarse CC reports its exact seven-bit value and forgets the
previous fine component. A later fine CC reports a 14-bit value using that known
coarse component; further LSBs reuse it until a new MSB or reset. An LSB before
its MSB produces MissingCoarse and is never buffered for a future MSB. Unrelated
messages may intervene. Bank Select 0/32 stays raw, and Data Entry 6/38 belongs
to parameter interpretation rather than a generic knob observation.

ParameterObservation adds family (Rpn/Nrpn) and parameter. It emits coarse data
at resolution 7 and complete observed pairs at 14. ParameterStep has direction
(Increment/Decrement) instead of an absolute value or magnitude. CC 96/97 data
bytes do not become an amount. A step invalidates cached coarse data because
the new absolute value is unknown. Observation ranges include selector context
for parameters and the contributing coarse/fine inputs for generic controls.

Selector halves may arrive in either order. After a complete selection is edited,
or a family changes, both halves must be observed afresh. Previous selection/data
context is invalidated with a diagnostic. Data received while selection is
missing/partial yields MissingSelection/IncompleteSelection; a fine value without
coarse data yields MissingCoarse. A normal first selector simply waits for its
partner; the incomplete diagnostic is emitted if parameter data arrives too soon.
This intentionally declines compliant devices that change only one selector
half, rather than guessing inherited context. RPN-null clears both families.

`reset()` clears all source state and emits StateReset at the next input index
without advancing it, a boundary-position diagnostic. SystemReset does the same
at its input position; Reset All Controllers clears only its channel. No physical
default values are invented or sent. All Notes Off is not a controller reset.
Diagnostics expose kind, optional channel/controller, and first/last indexes.
Indexes count fed primitives, retain their position across reset, and saturate
at usize::MAX. No timeout or silent-loss detector invents stream boundaries.
Call reset after known loss/reconnect; also reset the byte decoder when needed.
Timestamped observations and device/profile-specific semantics remain later work.

The prototype does not interpret manufacturer IDs, enforce a complete vendor
message schema, assemble packets or implement SysEx8. Do not use its generic
payload validation as evidence that an arbitrary payload is a recognized
musical command. Native upstream allocation limits remain an independent audit.

## Event, clock and Rust stream design

Retain the main plan's separate immutable event model: `Event` contains a
message, stream identity and optional timestamp. Absence differs from a zero
timestamp. A timestamp holds exact integer ticks and clock identity; a clock
descriptor carries an exact rational tick unit, origin/mode and known resolution.
Clock identity must distinguish reconnect generations. Equality of musical
messages does not depend on event timestamps.

Define clock descriptor construction and concrete identity allocation alongside
the shared core in slice 2; capture defaults and cross-clock mapping require
the actual backend evidence before live integration. Do not hard-code
microseconds, unsigned time or a shared origin from the previous backend.
No file-format discriminator or persistence layout is frozen in this slice.

Rust uses validated immutable structs implementing the common `Message` trait
and a closed `AnyMessage` enum for heterogeneous streams. Store short messages
inline, avoiding one trait-object allocation per message. Payload-bearing
variants own their storage. The prototype separates Rust validation values
from their PyO3 wrappers so extraction can promote the same constructors into
the interpreter-independent core without reimplementing validation.

## Evidence and measurements

The native wrappers now live in `src/messages.rs`, registering production
classes in `meedee.messages` through the private `meedee._core` binary. Their
values and validation live in the dependency-free `crates/meedee` Rust toolkit.
The existing Python lifecycle harness runs the native representation test. Its Python fixture
is `test/native/meedee_messages.py`, using standard-library assertions because
an embedded interpreter does not automatically import the development venv's
pytest installation.

Checks cover abstract construction, native inheritance, keyword-only signatures,
annotations, readable fields, equality/hashing, pattern matching, mutation
rejection, unknown replacement fields, invalid types/ranges, large Python
integers, mutable-buffer snapshots and immutable replacement. Rust checks
cover primitive channel/note/velocity limits and seven-bit payload rejection.
No MIDI clients are constructed and no wire codec is implemented.

Run the focused prototype with the selected interpreter:

```sh
PYO3_PYTHON="$PWD/.venv/bin/python" CARGO_HOME="$PWD/target/cargo-home" \
  cargo test --locked messages::python_representation -- --nocapture --test-threads=1
```

Observations on macOS arm64, five runs of 10,000 constructions per sample:

| Interpreter/build and metric | Native `NoteOn` | Minimal frozen/slotted stdlib dataclass |
| --- | --- | --- |
| Python 3.11.7, debug construction | 1,387 ns/value | 374 ns/value |
| Python 3.11.7, release construction | 181 ns/value | 377 ns/value |
| Python 3.15.0, release construction | 180 ns/value | 461 ns/value |
| Python 3.11.7, `sys.getsizeof` for one object | 24 bytes | 56 bytes |
| Python 3.11.7, peak Python-tracked allocation for a 10,000-value list | 325,648 bytes | 645,864 bytes |
| Python 3.15.0, peak Python-tracked allocation for a 10,000-value list | 325,512 bytes | 645,806 bytes |

The canonical Rust note value occupies 3 bytes; the boxed SysEx payload
descriptor occupies 16 bytes, excluding its allocation. The baseline has no
range validation and is a measurement control, not a second production API.
`tracemalloc` does not measure Rust payload allocations or total resident memory.
Debug and release observations differ substantially; smaller Python allocation
alone is not evidence of faster construction. These observations qualify
the representation, not Pi Zero resource budgets or live callback throughput.
Repeat release/Pi and stream measurements when the real core and target board
are available; do not add timing thresholds to unit tests.

## Remaining implementation gates

- Integrate native public modules without starting transport workers for value use.
- Promote the prototype once into the shared Rust core and binding, removing
  the test-only implementation; retain the behavior checks.
- Add production Event/clock types, complete MIDI 1.0 types and wire fixtures
  in their planned slices rather than publishing placeholder classes.
- Qualify libremidi's C boundary and build before backend replacement.
- Select the exact Pi Zero model/image/ABI before board performance claims.
- Verify MIDI 2.0 group numbering, attribute fields and unsupported/opaque packet
  policy before its API slice; prototype types do not guess these choices.

Normative baselines and acquisition evidence are in [specifications.md](specifications.md).

### Slice 5 MIDI Tuning Standard, approved 2026-10-15

`meedee.tuning` is a pure loader for Rust-owned frozen values and typed messages;
importing it starts no MIDI runtime. `Pitch(semitone, fraction)` stores the exact
21-bit MTS frequency representation: semitone 0–127 and fraction 0–16383 in
1/16384-semitone units. The all-127 triplet is a separate `NoChange()` value,
not a pitch. `TuningTable(values)` has exactly 128 Pitch/NoChange entries indexed
by MIDI note 0–127. `NoteTuning(note, pitch)` identifies an individual update.
Collections accept bounded built-in lists/tuples, copy into Rust ownership,
and return immutable tuples. Repeated single-note entries retain wire order.

`Pitch.from_frequency(frequency_hz=...)` uses Hz; `Pitch.from_cents(note=...,
cents=...)` uses a cents offset from an equal-tempered MIDI note (A4 = 440 Hz).
Each returns a frozen `PitchConversion` with `pitch` and `error_cents`, defined
as represented minus requested pitch. There is no rounding to integer Hz or
integer cents: nearest/ties-to-even quantization occurs only when converting
to the finite protocol representation. Exact constructors never quantize.
Nonfinite and out-of-range requests are rejected, never clamped.

`ScaleOffsets(resolution, values)` has 12 signed integer offsets C through B.
Resolution 7 uses whole cents (-64 through 63); resolution 14 uses units of
100/8192 cents (-8192 through 8191). Its positive limit is therefore just below
100 cents. `ScaleOffsets.from_cents(resolution=..., cents=...)` returns a frozen
`ScaleConversion` with `offsets` and 12 signed `error_cents`, using the same
rounding/range policy. Changing resolution with `replace` does not rescale units.

Typed Message subclasses are `BulkDumpRequest`, `BankedDumpRequest`, `BulkDump`,
`KeyBasedDump`, `ScaleOctaveDump`, `SingleNoteChange`, `BankedNoteChange` and
`ScaleOctaveChange`. Constructors require keyword arguments. Device ID is always
explicit, 0–127; 127 deliberately addresses all devices. Tuning bank/program
identifiers remain 0–127, distinct from the 1–128 ProgramChange API. Dump names
accept up to 16 ASCII characters, right-padded with spaces; received 16-byte
names retain all characters, including padding/NUL. Original single-note changes
are real-time; banked changes and channel scale/octave changes require explicit
`realtime: bool`. Scale resolution is carried by ScaleOffsets. Channel lists use
1–16 and are canonicalized to ascending order; duplicate channels are rejected.
Zero changes or an empty channel mask represent no-op wire messages. A single
message contains at most 127 note entries; no implicit splitting occurs.

All these messages implement Rust PrimitiveMessage and Python Message, and work
with `codecs.encode`. Raw output uses `connection.send(codecs.encode(message))`;
there is no implicit transmission or tuning reset. `codecs.Decoder` still emits
ordinary SysEx. `tuning.decode(sysex)` explicitly returns a frozen `Decoded`
containing `message` and `checksum`, or None for unrelated/unsupported forms;
recognized malformed forms raise ValueError. Unknown forms remain available as
raw SysEx. The control interpreter ignores typed MTS primitives, as it does raw
SysEx; MTS does not synthesize RPN control observations.

Checksums have three observable states: NotPresent, Matches and LegacyMismatch.
Original bulk dumps accept mismatches because the specification documents
incompatible manufacturer conventions. Encoding uses XOR of all payload bytes,
excluding F0/F7 and the checksum. Later dump formats strictly require that XOR
checksum. Re-encoding a legacy mismatch generates the chosen checksum rather
than preserving the received checksum byte. Lengths, seven-bit payloads and
reserved scale channel bits are validated for recognized forms.

`ReplyTracker(request=..., now_ns=..., timeout_ns=...)` tracks one unicast dump
request, without sending, opening ports, sleeping or starting threads. Callers
supply monotonic unsigned 64-bit nanoseconds from one clock and a positive
bounded timeout. `observe(typed_message, now_ns=...)` returns True only for a
matching timely reply and leaves every message with the caller. An original
request matches an original bulk reply; a banked request can match a key-based
or scale/octave reply. Device, program and bank must match exactly. `poll` at or
past the deadline yields TimedOut, and `cancel` yields Cancelled while pending;
Completed/TimedOut/Cancelled are terminal. Backwards time and overflowing
deadlines are rejected. Poll/observe must be driven by the caller for expiry to
be noticed. Passing an already decoded dump deliberately accepts its reported
legacy checksum status; callers can enforce a stricter policy before observing.

MTS contains no transaction ID. Identical unsolicited or late replies cannot be
distinguished, even after recreating a tracker. Use separate tracking per source,
avoid overlapping identical requests, and retain the raw input and checksum
status when that matters. A reply is a dump, not acknowledgment that a preceding
retuning succeeded. Physical tuning and device support are not established by
these pure-codec tests. RPN fine/coarse tuning, MIDI 2.0 pitch and MPE remain
separate mechanisms; existing RpnSet can explicitly select MTS tuning programs
and banks without adding an automatic selection operation here.

### Slice 6 GM and selected profiles, approved 2026-10-15

Pure public modules are `meedee.gm`, `meedee.universal`, `meedee.mpe` and
`meedee.timecode`; named controllers and RPN/channel-mode helpers remain in
`meedee.controls`. Imports initialize no MIDI workers. Native message/command
classes inherit Message and retain frozen fields, strict keyword constructors,
checked replacement, equality/hash, annotations and pattern matching. Lookup
entries are frozen, non-constructible records; list results are caller-owned
copies. There are no package-root re-exports or compatibility aliases.

GM lookup has a single Rust table source. `program(program=...)` and `programs()`
return the 128 GM1 Program entries (program/name/family). GM1 defines no bank
address, so these entries expose no bank fields. `gm2_program(bank_lsb=...,
program=...)` and `gm2_programs()` return BankProgram entries for the 256
standard melodic sounds, with bank_msb=121 and their specified LSB. Programs
are 1–128 and bank bytes are 0–127; no general shift is applied to other IDs.
Families use the GM1 instrument-group labels. Invalid numeric ranges raise
ValueError; structurally valid but undefined GM2 addresses return None.

`find_programs(name)` searches GM1 and GM2 and returns every matching entry, GM1
first then GM2 table order. Matching trims surrounding whitespace and ignores
ASCII case; standard display names are preserved. It does not normalize internal
spacing or add nickname/fuzzy aliases. `drum_kits()` and `find_drum_kits(name)`
expose the nine GM2 kits in bank 120/0, with one-based program numbers.
`percussion(note=...)` returns GM1 note/name entries at 35–81;
`gm2_percussion(program=..., note=...)` returns the specified kit's entry or None
for a silent/undefined position or unknown kit. GM2 standard-set inheritance is
resolved from the Rust table; None does not emulate a sound generator's fallback.
Pan positions/exclusion-group synthesis rules are not implemented by these name
lookups. `GM1_DRUM_CHANNEL=10` is a convention, not a restriction on GM2 channels;
GM2 permits melodic channel 10 and rhythm channel 11, and optionally others.

`BankProgramSelect(channel, bank_msb, bank_lsb, program)` is a semantic command
in meedee.gm. Both bank values are mandatory, either explicitly chosen by the
caller or copied from a GM2 entry. It expands into CC0, CC32, ProgramChange in
that order. It never chooses a GM mode or inherits an unobserved bank. GM1
callers can send the existing ProgramChange primitive or deliberately specify
both bank bytes; the library supplies no invented GM1 bank. Mode controls are
separate Gm1On/GmOff/Gm2On primitive SysEx classes with explicit device_id.
Mode changes can mute notes and reset programs/controllers/tuning. GM2 specifies
completion within 100 ms after Gm2On; `GM2_ON_SETTLING_NS=100_000_000` records
that standard interval, not a runtime device-health guarantee. GM1's developer
guidelines discuss varying initialization times rather than a universal safe
constant. Callers schedule settling themselves; no codec/command inserts sleeps
or sends a mode/reset message on import/open/close.

The approved Rust change makes `Command.expand()` and `codecs::expand()` return
Vec<AnyMessage>, replacing the CC-only vector throughout. Python expand returns
a fresh list of primitive Messages; existing CC commands produce the same CC
objects and bytes. AnyCommand additionally carries BankProgramSelect and
mpe::ZoneConfiguration. `send_command` accepts both types through the same
in-process owner-thread operation or isolated worker protocol. Serialization,
known completed-prefix/unknown-progress reporting and no retries are unchanged;
there is no extra command-sending path. Raw send and primitive encode remain
separate. A command may partially select a bank or partially configure a zone
before failing. Other applications can interleave on the device, and backend
acceptance is not physical acknowledgment. UMP outputs reject these commands
without implicit translation.

`controls.Controller` names common standardized CC identifiers and exposes
`.number` plus `.message(channel=..., value=...)` returning an ordinary frozen
ControlChange. Raw values remain seven-bit; enum objects are not coerced into
integer fields. `all_sound_off`, `reset_all_controllers`, `all_notes_off`,
`local_control(enabled=bool)`, `omni_off`, `omni_on`, `mono_mode(channels=...)`
and `poly_mode` construct canonical channel-mode CC values. Mono channels are
0–16, where zero requests the receiver's voice count; GM2's supported mono mode
specifically uses one. These helpers do not emulate a synthesizer's controller
response. All Notes Off differs from immediate All Sound Off and can leave
sustained/releasing sound.

Named RPN factories return the existing RpnSet, using fresh selectors and the
existing RPN-null release. `pitch_bend_sensitivity(channel, semitones, cents)`
keeps both seven-bit fields exactly, without carry/normalization;
`channel_fine_tuning(channel, value)` is signed -8192–8191 in 100/8192-cent units;
`channel_coarse_tuning(channel, semitones)` is -64–63. `tuning_program` and
`tuning_bank` retain 0–127 identifiers. `modulation_depth_range(channel, value)`
keeps 0–16383 raw bits: in GM2 these are 1/128-semitone units, so its LSB is not
the cents byte of pitch-bend sensitivity. CA-026 alone assigns no universal
physical unit/default outside a specified profile. Devices may ignore bend
sensitivity cents and have smaller supported physical ranges. No helper claims
that constructing or sending a value establishes a receiver's response.

Universal primitives are IdentityRequest(device_id), IdentityReply(device_id,
manufacturer, family, model, revision), MasterVolume(device_id, value),
MasterFineTuning(device_id, value), MasterCoarseTuning(device_id, semitones).
All require explicit device IDs 0–127, with 127 deliberately broadcast.
Identity manufacturer bytes have one nonzero byte or three bytes starting with
zero; revision is exactly four seven-bit bytes in device-specific order.
Family/model are unsigned 14-bit values, serialized LSB first. Lists/tuples are
bounded and copied; getters return immutable tuples. Manufacturer registration
and revision meaning are not guessed. Master volume is 0–16383, master fine
is signed -8192–8191 in 100/8192-cent units, and coarse is -64–63 semitones.
Coarse's unused LSB must be zero; malformed input is rejected rather than
silently normalized. No request/reply wait or capability discovery is started.

`mpe.ZoneConfiguration(manager_channel, members)` accepts manager 1 or 16 and
0–15 members. It expands into a seven-bit RPN6 set plus the standard RPN-null
release. Zero disables that zone only; it does not necessarily leave MPE mode.
There is no automatic second-zone command, bend-range setup, voice allocation
or expression tracking. The receiver applies MPE's last-configuration-wins
rule to overlapping zones; the library maintains no shared zone registry.

`timecode.FrameRate` preserves the four MTC rate codes Fps24, Fps25,
DropFrame30 and Fps30. FullFrame(device_id, rate, hours, minutes, seconds, frames)
requires explicit broadcast device_id=127 per the full-message specification;
hours are 0–23, minutes/seconds 0–59, frames 0–23/24/29 according to rate.
The drop-frame code is preserved as a wire label, without a frame-count/time-line
conversion or a claim that skipped SMPTE labels are validated. There is no
quarter-frame assembly, two-frame latency correction, phase inference or
clock locking. Full-frame input does not imply that time is running.

`gm.decode`, `universal.decode` and `timecode.decode` explicitly interpret a
complete SysEx, returning their typed primitive or None for unrelated/unsupported
forms. Known malformed lengths/fields raise ValueError. The byte decoder keeps
raw SysEx and does not promote it. Callers retain raw bytes for unknown messages
or byte-for-byte evidence. Typed GM mode messages explicitly fed to the control
interpreter clear all cached channels with StateReset diagnostics, including
GmOff as a conservative context boundary; raw SysEx is ignored. Rust accepts
ModeMessage or the individual mode types as interpreter input. A caller selects
which addressed mode changes are relevant to that source, or calls reset after
known mode changes/loss; sending a value does not implicitly feed an interpreter.

Approved deferrals are GM2 controller-destination routing, key-based instrument
controls and global effect-parameter SysEx; MPE voice allocation/expression
tracking; quarter-frame MTC assembly/clock locking and high-resolution velocity
prefix interpretation. Generic raw messages remain available for these forms.
This selected coverage does not claim full GM2 message support, synthesis,
manufacturer extensions, device conformance or physical/native qualification.

## Slice 6.5 approved API and interpretation policies

`gm.ControllerDestination` (source 1=channel pressure, 2=poly pressure, 3=CC),
`gm.KeyBasedControl` and `gm.GlobalParameterControl` are frozen Message values.
They encode with the existing byte encoder and decode explicitly through
`gm.decode`. Entry order, duplicates and valid unknown IDs remain intact.
Controller-source and key-controller exclusions follow CA-022/023. Constructors
and decoding default to 1,024 entries, adjustable with strict `max_entries`;
checked replacement retains that resource policy. It does not affect wire/value
identity. Slot paths have at most 127 14-bit components. Global parameter IDs
are tuples of MSB-first seven-bit bytes, values tuples of LSB-first bytes; declared
widths are 1–127 bytes, preserving wide values without integer overflow. Named
reverb/chorus parameter helpers use GM2 slots 257/258. Raw values are not claims
of device support or manufacturer response.

One `mpe.Allocator` owns one explicit zone. It returns Voice tokens and NoteOn
values, chooses the lowest free member channel and rejects exhaustion. Release
returns NoteOff; sustain changes return manager-channel CC64. Released voices
remain reserved while sustain is held, a conservative allocation policy. There
is no implicit stealing, sending, expression reset or release-envelope estimate.
Tokens belong to that allocator; reset forgets state without sending silence and
does not reuse token numbers. An allocator is not a full MPE Mode 3 transmitter
which automatically shares occupied channels under exhaustion.

One `mpe.Tracker` observes an explicit zone of an independent source. Configure
clears state; callers manage interacting zones and feed configuration changes
explicitly. Fixed channel/note counters retain repeated pitches and distinguish
held and sustained counts, with checked count exhaustion. Channel expression is
reported separately; `ambiguous` flags several currently held notes instead of
inventing attribution. Member expression does not attach to sustained release
tails. Manager expression remains separate rather than applying an invented
synthesis-combination formula. Member-channel damper messages are ignored;
manager sustain/mode controls update affected member state. Channel snapshots
are frozen; returned note lists are copies. Reset is required after known loss.
Release-envelope duration and physical sounding-note state cannot be inferred.

`controls.VelocityInterpreter` implements CA-031 per source/channel. A prefix
survives intervening non-note messages, is replaced by a later prefix and is
consumed once by the next NoteOn/Off on that channel. Observations preserve
seven-bit coarse values when no prefix was received and fourteen-bit values
when paired. Zero-velocity NoteOn remains a note-off observation, ignores its
prefix and records its original form. Explicit reset, SystemReset and channel
Reset All Controllers invalidate pending context. Raw messages remain caller-owned.

`timecode.Position` validates SMPTE labels, including skipped drop-frame labels;
`advance(frames=...)` wraps at 24 hours in either direction. FullFrame remains a
wire value; the assembler validates its label before relocation. `Assembler`
requires an explicit positive `timeout_ns`, monotonic integer `now_ns`, and
caller-driven feed/poll. It returns frozen results with observations/diagnostics.
One cycle publishes a position; a second cycle advancing exactly two frames in
the same direction/rate acquires sequence lock. Gaps, malformed nibbles,
direction/rate/position discontinuities and silence deadlines invalidate lock.
Observations distinguish encoded and direction-compensated positions. FullFrame
relocates without acquiring lock. No extrapolation, wall clock, scheduling or
physical synchronization guarantee is supplied. Public reset retains the
monotonic caller-clock requirement.
