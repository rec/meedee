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

Resource follow-up approved 2026-10-15: each parent Python process admits at most
32 live isolated workers, separately from native-client and connection counts.
Each isolated Input/Output constructor consumes one slot, including discovery-only
clients. Starts in progress and retiring processes count until exit is confirmed;
admission raises ResourceError before spawning at capacity. Failed starts release
their reservations. This is a count limit, not a machine-wide memory budget.
Closing a connection restores its reusable client and does not terminate that
client's worker. Connections retain their clients; drop those references when
the client is no longer needed, and allow asynchronous retirement to complete.
CoreMIDI retains one additional keeper client and notification-loop thread per
MIDI-using process until exit; ordinary ports/clients still close. Per-client
operation ownership is preserved. The private notification callback stores no
application context. A fixed 256-entry sequence history bounds event tracking;
history loss fails explicitly rather than acknowledging an unobserved change.
Virtual endpoint creation and disposal wait up to two seconds on a steady-clock
deadline for the matching local add/remove event. Disposal success is recorded
before waiting, so delayed acknowledgement does not dispose an endpoint twice.
Timeouts report errors under the existing failed-open/unsafe-cleanup ownership
rules; they do not interrupt SDK calls or guarantee peer-process visibility.

CoreMIDI metadata-read failures return PortInfoError with no partial snapshot,
without poisoning the client or isolated worker. A later explicit ports() call
can succeed. Connection errors remain sticky, and other backends keep their
existing discovery-failure policy. There is no retry or error suppression.
The runtime does not detect/recover a MIDI-server restart and does not initialize
during pure codec/file use. Production macOS virtual-reopen qualification passes
three cycles in each MIDI 1/UMP and isolated/in-process combination, including
exact full-width MIDI 2 delivery and timestamps; see `validation.md`.

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

`OutputConnection.send(command)` accepts these commands on MIDI 1.0
outputs, rejecting UMP before sending. Encoded packets use `send`. One native
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
the representation, not host resource budgets or live callback throughput.
Repeat release and stream measurements when the real core and test host
are available; do not add timing thresholds to unit tests.

## Remaining implementation gates

- Integrate native public modules without starting transport workers for value use.
- Promote the prototype once into the shared Rust core and binding, removing
  the test-only implementation; retain the behavior checks.
- Add production Event/clock types, complete MIDI 1.0 types and wire fixtures
  in their planned slices rather than publishing placeholder classes.
- Qualify libremidi's C boundary and build before backend replacement.
- Record each test host's OS/architecture, ABI and hardware before performance claims.
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
mpe::ZoneConfiguration. `send` accepts both types through the same
in-process owner-thread operation or isolated worker protocol. Serialization,
known completed-prefix/unknown-progress reporting and no retries are unchanged;
there is no extra command-sending path. Explicit encoding through codecs.encode
remains independent of transmission. A command may partially select a bank or configure a zone
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
ZoneConfiguration performs no automatic second-zone command, bend-range setup,
voice allocation or expression tracking; separate engines are described below. The receiver applies MPE's last-configuration-wins
rule to overlapping zones; the library maintains no shared zone registry.

`timecode.FrameRate` preserves the four MTC rate codes Fps24, Fps25,
DropFrame30 and Fps30. FullFrame(device_id, rate, hours, minutes, seconds, frames)
requires explicit broadcast device_id=127 per the full-message specification;
hours are 0–23, minutes/seconds 0–59, frames 0–23/24/29 according to rate.
The drop-frame code is preserved as a wire label, without a frame-count/time-line
conversion or a claim that skipped SMPTE labels are validated. FullFrame itself supplies no
quarter-frame assembly, latency compensation or clock locking; Position and
Assembler below provide separate explicit interpretation. Full-frame input does not imply that time is running.

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

Originally deferred from slice 6 were GM2 controller-destination routing, key-based instrument
controls and global effect-parameter SysEx; MPE voice allocation/expression
tracking; quarter-frame MTC assembly/clock locking and high-resolution velocity
prefix interpretation. Slice 6.5 below implements these codecs/state engines.
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
reported separately; `ambiguous` flags several currently held member notes instead of
inventing attribution. Member expression does not attach to sustained release
tails. Manager expression remains separate rather than applying an invented
synthesis-combination formula. Member-channel damper messages are ignored;
manager sustain/mode controls update affected member state. Channel snapshots
are frozen; returned note lists are copies. Unobserved pitch bend, pressure,
timbre and manager sustain are `None`, including after reset. Member snapshots
report the manager pedal knowledge. Counts describe only observed held and known
pedal-sustained notes. Reset is required after known loss.
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

## Slice 7 UMP/MIDI 2.0 contract

The pure `meedee.ump` namespace exposes frozen native Message subclasses backed
by Rust values. It does not import transport, initialize workers, perform I/O or
attach host timestamps. `encode` returns an immutable tuple of 32-bit words;
`decode` requires exactly one UMP. Decoder.feed accepts word lists/tuples and
returns ordered typed values or frozen diagnostics with cumulative word offsets.
It retains at most three unfinished words, recovers at declared packet lengths,
and reports truncation on finish. Reset discards pending state, retaining offsets.

All scalar constructors are keyword-only and strictly reject coercion/bools for
integer fields. Groups/channels are 1–16 and programs 1–128; wire conversion is
internal. Pitch bend and per-note bend are signed -2^31 through 2^31-1, centered
at zero. Relative controllers are signed two's-complement integers. Velocity and
attribute data retain 16 bits; pressure and absolute controllers retain 32 bits.
Attribute types 0–3 are represented; type 0 requires zero attribute data. Reserved
registered per-note controller IDs are rejected by typed values for this baseline.
Per-note management flags use protocol bits (1=detach, 2=reset). Program options
use bit 0=bank valid; without it both bank bytes must be zero. A MIDI 2 NoteOn
velocity of zero is a sounding-note event, unlike MIDI 1 NoteOn zero.

| Packet family | Representation and scope |
| --- | --- |
| Utility | Noop, JrClock, JrTimestamp, DeltaClockstampTicksPerQuarter, DeltaClockstamp; no Group field |
| System/MIDI 1.0 channel | Midi1Packet(group, message) reuses immutable MIDI 1 values; SysEx uses separate packetization |
| MIDI 2.0 channel | Explicit Midi2NoteOn/Off, PolyPressure, ChannelPressure, ControlChange, ProgramChange, PitchBend, Registered/AssignableController, relative controllers, registered/assignable per-note controllers, per-note bend/management |
| Data64 | SysEx7Packet: format 0/1/2/3 = Complete/Start/Continue/End, at most six seven-bit bytes |
| Data128 | SysEx8Packet: format, stream ID 0–255, up to thirteen payload bytes; explicit aborted End; MixedDataSet header/payload packets retain their fourteen bytes |
| Flex Data | Group, optional channel, format, status bank/status and three data words; tempo/time-signature/metronome/key-signature factories and bounded UTF-8 packetization |
| Stream | Groupless format/status and fourteen bytes; endpoint/function-block/configuration/identity/clip factories plus endpoint/product/block text packetization |
| Opaque | Exact words of any structurally complete UMP, including assigned 96-bit reserved types; no interpretation or transmission guarantee |

Typed decoding checks lengths, known statuses, reserved fields, ranges, padding
and addressing. Default rejection is strict. `preserve_unknown=True` returns
OpaquePacket for complete layouts rejected by the typed decoder, including
malformed/reserved fields; it does not certify their validity. Framing truncation
is still a diagnostic. MIDI 2 CC values replaced by unified messages can be
preserved, without pretending the MIDI 2 receiver will apply their MIDI 1 meaning.
`bytes_to_words`/`words_to_bytes` require byteorder="big" or "little"; the codec
never silently selects host or transport byte order. JR clock/timestamp units
are 1/31250 second with 16-bit wrapping, separate from host clock metadata.

Assembler has configurable positive message_limit (default 1 MiB), total_limit
(default 8 MiB of retained payload) and max_streams (default 64). One instance
belongs to one independent source. Keys keep SysEx7 groups, SysEx8 group/stream
IDs and text addressing separate. Completed function-block names expose a
separate function_block field; stream_id identifies only SysEx8 streams.
Overflow discards the unfinished payload,
releases its allocation and retains bounded discard state until its end. Abort,
interruption, unexpected continuation and finish produce diagnostics. Incomplete
or discarded payloads are never returned as completed data. SysEx7 permits
realtime/groupless interleaving; Flex continuations enforce matching headers and
permitted realtime/JR clock interleaving. Flex text is at most 32 UMPs/384 bytes;
endpoint names at most 98 bytes, product IDs 42 ASCII bytes, block names 91 bytes.
Complete text validates UTF-8, absence of a BOM and terminal zero padding, even
when code points span packets. Melisma remains an empty lyric payload with its
status, not an invented syllable. Dates/language tags/chord synthesis semantics
remain caller interpretation; packet layouts and content are preserved.

Budgets count payload length, not allocator capacity, object overhead, returned
results or application-held values. The fixed framing buffer and bounded active
registry do not impose a hard process memory/CPU guarantee. Feed results/copies
are proportional to supplied input. There is no background timeout or discovery
registry; callers must reset after known packet loss or source/clock changes.
MDS header/payload support is packet-level, not a dataset transfer/assembly engine.

`Capabilities(midi1, midi2, sysex8_streams)` records explicit declarations. Its
supports(value) returns a protocol/stream compatibility fact, or None when these
declarations do not establish support for a family. It does not infer health,
synthesis behavior, group topology, live platform support or capability replies.
No discovery, MIDI-CI or profile transaction runs implicitly. Encode is a pure
operation, and does not require or assert a physical receiver's capability.

### Translation policy

`to_midi2(MIDI1_message, group=...)` and `to_midi1(UMP_message, allow_loss=False)`
return immutable reports: status, reason, routing group, converted and a fresh
list of messages. Min-Center-Max scaling follows M2-115-U 1.0.2, with truncating
downscaling; native integer values remain exact until conversion is requested.
Quantized reports contain no output without explicit allow_loss=True. That
permission permits numerical precision loss, not dropping attributes, guessing
context or inventing alternate profile/MPE mappings.

| Case | Report / output |
| --- | --- |
| MIDI 1 notes/pressure/bend and ordinary CC | Exact standard upscaling; every seven-bit velocity and fourteen-bit bend round trips |
| MIDI 1 NoteOn zero | Explicitly reported semantic normalization to MIDI 2 NoteOff, release velocity 0x8000 |
| MIDI 2 plain notes/pressure/bend/ordinary CC | Exact if representable; otherwise Quantized and requires permission |
| MIDI 2 NoteOn whose seven-bit result would be zero | Quantized to minimum sounding velocity 1 when permitted, preserving note-on meaning |
| Program/bank | MIDI 1 program-only explicitly retains the receiver bank; MIDI 2 bank-valid yields MSB, LSB, ProgramChange in order |
| Registered/assignable controller to MIDI 1 | Four primitive selector/data-entry messages with parameter-specific coarse fields where defined; RPN-null has no data-entry target and is Unsupported |
| MIDI 1 bank/parameter/velocity-prefix sequences or increment/decrement intent | ContextRequired; no source state, selector timing, bank inheritance or paired velocity is guessed |
| Note attributes/per-note MIDI 2 expression | ProfileDependent; no implicit attribute removal or alternate translation |
| Complete SysEx7 / MIDI 1 SysEx | Exact framing/packetization; fragmented SysEx7 must first be assembled |
| SysEx8, MDS, relative controllers, Flex/stream/utility/opaque | Unsupported for stateless non-UMP MIDI 1 conversion |

This is an explicit conservative stateless translator, not a complete default
translation engine for arbitrary stateful MIDI 1 streams. MIDI 1 fine CC halves
are individually preserved rather than automatically merged. RPN/NRPN output
lists use the standard four-message sequence, without implicit RPN-null cleanup;
selection may remain active. Lists are values, not atomic live operations and
provide no send-progress or delivery acknowledgment. Typed/timed sending and
source integration remain slice 10; profile negotiation remains slice 11.

## Slices 8–10 approved policies, 2026-10-15

Rust-owned codecs live in smf/syx/clip/archive, with Python modules of the same
names. Shared event/time values live in events. SMF types 0/1/2, PPQN/SMPTE,
SYX and MIDI Clip 1.0 are selected; other containers are out of scope. Type 2
sequences are independent. File writes preserve meaning/order, not byte-exact
running-status compression. Unknown metadata/chunks and F0/F7 fragments remain
file records rather than executable live messages. No implicit lossy export.

The standard file codecs stream one record at a time. SMF Reader.header exposes
format, track count, raw validated 16-bit division and header-extension bytes.
Each smf.Event has a zero-based file track index, absolute integer tick and
record. Record is a primitive channel Message, Meta, SysExFragment or
UnknownChunk. Unknown chunks are preserved at track boundaries. EndOfTrack is
explicit. Writers require contiguous tracks, nondecreasing ticks within tracks,
a valid terminal EndOfTrack and an explicit finish(). SMF output requires a
seekable destination to patch track lengths; it never buffers a complete track.
Writers may normalize running status. Known metadata may retain extension bytes.

SYX is a sequence of F0/payload/F7 dumps, rejecting other statuses and unfinished
dumps. Clip preserves individual UMP packets including clock and configuration
records; Entry.tick is the cumulative DCS position when known. Timing/configuration
records are storage, not instructions to send automatically. Readers/writers
check the initial zero DCS/DCTPQ and sequence start/end framing. Semantic profile
negotiation remains later work. Other MIDI containers are not claimed.

All file payload limits default to 1 MiB and are configurable; file length is not
capped. Python paths and binary streams are accepted, and borrowed streams stay
open. File codec finish flushes successful output; garbage collection does not
assert successful completion. File operations surface I/O and malformed-data
errors, including read byte positions. A failed writer cannot be resumed.

The custom `.meedee` archive is binary, versioned and length-prefixed, with
per-record CRC-32 and an explicit completion trailer. Normal reading requires
completion; recovery is explicit and returns only verified complete records.
Native clock values and absent timestamps remain exact, with rational units and
stable record IDs. Unknown records may be retained opaquely without execution.

TypedInput wraps the existing input transport. Python Output opens one
OutputConnection with send for message objects, compound operations and encoded
packets, and play for timestamped events.
Message and packet operations share connection ownership and process isolation;
protocol conversion remains explicit. Input selects callbacks or a bounded
receive queue at connection time.
Events use completion-packet time with first/last contributing packet spans;
primitive messages remain evidence and semantic observations remain separate.
Recording uses one bounded writer thread (128 records and 8 MiB payload by
default), stops incomplete on observed loss/overflow/write failure, and offers
explicit best-effort gap recording for input loss. Playback runs on the caller's
thread with monotonic deadlines/cancellation, sends late events immediately with
lateness reporting and stops on send failure without retry. Missing times,
unmapped clocks and unresolved discontinuities require explicit caller policy.
No automatic silencing, reset, connection close or restoration on cancellation.
Only supervised temporary virtual CoreMIDI endpoint tests are authorized; physical
instruments and Windows/Linux native validation remain open.


## Final slices 8–10 contracts

Rust modules smf/syx/clip/archive/events/live/recording/playback/file_timing and
source-scoped event_views contain the shared logic. Python uses the same native
implementations through focused modules; typed input composes the sole input
transport, and output unifies packet/message access on the same connection.
No implicit MIDI 2 downgrade or group collapse
is introduced. Rust native typed callbacks are available through connect_typed
and create_virtual_typed; Rust applications compose Decoder/Recorder/playback as
needed without starting Python. Rust transport remains in-process.

File readers close their owned handles explicitly with close(); borrowed streams
remain open. Writers require finish, and abandonment never certifies completion.
Blocking read/write operations release the GIL, but an application-supplied stream
may itself execute arbitrary Python. File creation/header work remains synchronous.
File I/O is not protected by MIDI child-process deadlines. Recording's writer uses
a native file handle, never a Python stream. A stop timeout leaves that handle and
thread owned until the blocked operation returns; callers must not rewrite the
same destination while it remains active. Flush does not imply fsync or power-loss
durability. Creating an output path truncates it using normal file-open semantics.

Raw ReceivedPacket.sequence counts native packets before admission, so an observed
sequence gap resets typed framing. Counters are checked again at stop/close to
catch a final dropped packet with no successor. This detects known software loss,
not driver loss. Completion timestamps remain optional signed native nanoseconds;
zero is valid. General Timestamp has signed ticks and a rational seconds-per-tick
Clock. Stream descriptors can link even a timestamp-disabled source to its clock.
Archive Metadata can carry an optional stream and exact timestamp independently
of neighboring events. Clock identities are generations, not synchronization.

PacketDecoder emits events and structured diagnostics, including malformed input,
loss, clock changes and unfinished messages. Typed callbacks use the existing
blocking-operation guard and run outside internal locks. Receive supports polling,
timeout, cancellation and close/failure wakeup, with at most 50 ms polling delay.
Its count/payload admission is independent of the recorder: a slow queue consumer
can fail while recording remains healthy. Valid queued prefix events remain
available before that failure is raised. Source-local semantic views consume these
events/diagnostics and reset on loss, clock change and relevant MIDI resets. Original
events remain untouched. Unknown contributor times stay None. Quarter-frame time-code
observations preserve the completion time; the current assembler does not expose a
precise first-quarter contributor, so their first range/time is None. Poll-only
expiry observations have no invented native event timestamp.

A recorder is admission, not a per-call disk acknowledgment: record returns False
after failure, and stop returns the final completion/failure report. Default mode
stops on input loss, malformed/truncated messages, queue overflow and write failure.
Best effort stores gap records after input loss; a clock discontinuity still stops.
Counts and bytes include the in-flight record. Capture start defines admission of
completed events; a source span can begin before recording was attached. Explicit
stop detaches capture but leaves input open. Closing input stops its attached
recording and reports incomplete cleanup; garbage collection is not success.

Playback requires an explicit Timestamp origin. SMF/Clip TimedReader.origin supplies
a file origin, preserving leading silence. To remove it, callers deliberately use
the first event's timestamp instead. Mapping applies target seconds = source seconds
* rate + offset; uncertainty is declared metadata, not a synchronization guarantee.
Mappings must have distinct source domains and one consistent target, with no
implicit chaining. Rational arithmetic is checked; conversion to host nanoseconds
requires exactness unless allow_rounding=True, which reports quantized events.
Equal-time input order is preserved. Regressing times, missing timestamps, gap or
discontinuity records and unknown executable records stop replay. Cancellation cannot interrupt a blocked source iterator, synchronous file call,
application callback or in-process driver operation. No file sorting,
first-event alignment, automatic cleanup, retry or state restoration is supplied.
A failed send may already have effects; PlaybackError exposes completed_events,
report and cause, and compound causes retain their completed-prefix information.
Streaming verification can discover an invalid trailer after earlier events were
sent. Preverify first when that is unacceptable.

SMF MergedReader/TimedReader require seekable input and default to 128 selected
tracks and 8 MiB lookahead payload. Independent track decoders each retain at most
the selected SysEx payload limit; that memory can multiply by selected track count.
Raw Reader does not impose those merge limits.
TimedReader uses exact tempo/SMPTE time and reports out-of-range signed event ticks;
raw readers can still retain longer unsigned file tick positions. Timed performance
views omit file metadata; preserve it separately with raw readers/archive Metadata.
SMF export uses an explicit origin, tempo, target division and optional clock mapping;
it returns a report without silently dropping unsupported messages. A UMP group
requires deliberate routing/translation before export. Clip configuration is excluded
from performance events by default; include_configuration=True explicitly includes
supported header packets. File clock records and clip markers never become live
sends. MIDI-CI profile negotiation remains slice 11.


## Slice 11 management scope

meedee.ci adds frozen management Message values and an explicit decode(SysEx),
plus a pure caller-driven Session backed by Rust ci/ci_session. Raw receive,
codec, Event and archive behavior stays primitive. Callers supply randomized
MUIDs and explicit routes; channels/groups remain 1–16. Format 1/2 management,
reserved identifiers, compatible future extensions and bounded management
transactions are covered. Session optional-category declarations are zero.

Profiles, Property Exchange and their JSON/compression dependencies are deferred
by the owner because their normative documents are unavailable. Process Inquiry
engines remain deferred. See ci.md for constructors, action/send-result ordering,
explicit deadlines/restart, text encoding, correlation limitations and resource
budgets. Native UMP qualification is separate from this software capability.

## File inspection and capture validation

`meedee.inspection.inspect_file(source, *, format, payload_limit=1048576,
preserve_unknown=False, detail_limit=128)` uses the shared Rust `inspection::inspect` pass over
the existing archive/SMF/SYX/Clip readers. Format is explicit. Inspection is
read-only, opens no MIDI resources and adds no parser, scheduler or dependencies.

The native frozen Report exposes decoded-prefix record/message counts, archive
clock/stream descriptors, SMF header/track counts, gaps, total known lost events,
discontinuities, missing times and opaque content. Collections are defensive
copies. UMP counts describe packet families, including clock/configuration;
compound archive commands stay commands and SMF SysEx fragments stay fragments.
No cross-clock sorting, timestamp synthesis or semantic promotion is performed.

The first format/limit/completion failure stops the pass and becomes `problem`;
counts never include the rejected record. `complete` requires successful EOF
validation, independently of gap/discontinuity/opaque counts. File I/O failures
propagate rather than being labeled malformed data. Unknown-content policy is
inherited from the selected reader. See README.md for counting/resource details.

Source-local timing summaries retain exact ticks, first/last/min/max ranges,
missing times and regression counts. Loss, discontinuity and clock changes start
new segments; they do not invalidate structural completeness. Segment/diagnostic
examples are bounded by detail_limit, with explicit omitted totals.

`compare_files` and Rust `comparison::compare` compare same-format records by
position with explicit one-to-one archive source/clock maps. Timestamp policy is
exact by default, with either a tick tolerance or ignoring timestamps. Metadata,
configuration, span packet indexes and non-timing content remain significant.
Both inputs must reach verified completion before equality is reported. There
is no automatic alignment, cross-format musical conversion or resynchronization.

The optional `cli` extra supplies Pydantic/Tyro inspect, validate, compare and
monitor commands, text/JSON output and 0/1/2 verdicts. Core imports remain free
of mandatory Python dependencies. The passive monitor selects one exact unique
input name and explicit protocol, with isolation enabled by default and existing
typed-queue/payload limits. Display filters do not filter interpretation or hide
loss/failure notices. Known loss resets views and continues; terminal transport
failure stops. History and payload previews are bounded. It sends no MIDI.

Pure `inspection.Observer` instances aggregate one live source using the same
Rust timing/message logic and return frozen snapshots, without file-completion
claims. Asynchronous transport drop totals may overlap sequence-gap diagnostics;
counter notices reset source context without double counting sequence loss.
README.md documents CLI selection, filters, resource bounds and examples.

## Observed note activity

`meedee.notes.Tracker(stream=..., detail_limit=128)` accepts `Event` or
`StreamDiagnostic` through `feed`, returns frozen `notes.Report` snapshots, and
offers explicit `invalidate()` for asynchronous loss. It owns no transport.
Rust `notes::Tracker` supplies the same state machine, taking an explicit source,
payload, optional clock identity and consistent positive detail limit from its
caller. Python rejects another source before mutation.

Snapshots separate source, optional UMP group, channel and MIDI protocol family.
Byte MIDI 1 and MIDI 1-in-UMP interpret zero-velocity NoteOn as release; native
MIDI 2 preserves it as NoteOn. Repeated keys retain counts, without occurrence
pairing. `held` counts observed unreleased activity. `released` counts matched
releases since the last boundary, including channel-mode releases, not sustained
voices or audio tails. Unmatched releases increment `unmatched_releases` and
mark `uncertain`. An arbitrary capture can start mid-performance: false
`uncertain` means no observed inconsistency, never a known initial device state.

CC64/66 retain raw sustain/sostenuto values at their native 7/32-bit resolution;
no voice retention or pedal-to-occurrence attribution is inferred. CC120 clears
outstanding held activity without adding releases; CC123-127 move held counts
to released. CC121 forgets pedal knowledge and retains note history. These are
observations of commands, not confirmation that a receiver obeyed them. Unknown
instrument Basic Channel, mode, other writers and note management remain outside
this observer's model. Raw traffic remains available through existing readers.

Source loss, discontinuity and known clock-identity changes forget affected note
and pedal history and mark uncertainty. SystemReset invalidates its source's
byte-MIDI scope or the addressed UMP group. Boundary totals remain visible.
Counts saturate rather than wrap; count exhaustion marks uncertainty. Up to
`detail_limit` sources and channel states are admitted, each with at most 128
keys. Capacity never evicts admitted state; `omitted_events` counts observations
excluded by source/channel capacity, including an excluded boundary. Snapshots
and their collection getters are independent copies.

`inspection.Report.note_state` includes the same report for archive sources,
separate SMF tracks, Clip packets or an empty SYX observation. SMF tracks are not
merged into an instrument-performance view. Verified prefixes retain their state
when file completion fails. Structural completeness and CLI validation exit
codes are unaffected by outstanding notes, unknown pedal state or unmatched
releases, since those can be legitimate partial captures.

CLI inspection and final monitor summaries include `note_state`; visible event
rows include matching channel summaries. Display filters run after state updates.
`inspection.Observer.channel_states(channel=..., group=None)` projects just the
addressed channel's protocol views, so live rows do not copy unrelated history.
History limits still bound stored rows, and no cleanup messages are constructed
or sent. The existing MPE tracker retains its separate zone/member semantics.

## Static protocol-aware routing

Rust `routing::Route` and Python `routing.Route` specify name, exact source and
destination identities, protocol, optional channel/group selectors and optional
channel/group remaps. Python protocol values are `"midi1"` or `"ump"`, allowing
configuration and preview without importing transport clients. Channels/groups
use 1-16; byte MIDI rejects group options. Python routes are frozen; Rust fields
are private and constructor-validated. No mandatory dependency is added.

`Router(routes=..., event_limit=128, byte_limit=8388608)` accepts 1-128 uniquely
named routes. A destination must have one protocol. `feed(Event)` selects routes
in configuration order, preserving caller event order without timestamp sorting.
Several matching routes may deliberately deliver multiple copies to one output.
No protocol conversion, command expansion, musical transformation or autonomous
scheduler is implied. `Delivery.event` retains the original stream, timestamp and
contributor span with its transformed message. Destination labels are never
recursively fed back as sources: routing is one hop, with no internal graph loop.
External device/wire feedback is not detected or inferred from endpoint names.

Only MIDI 1/2 channel-voice traffic matches by default. Explicit
`include_nonchannel=True` permits supported system, SysEx and UMP traffic.
Channel selection/remapping applies only to channel voice; opted-in non-channel
traffic bypasses the channel selector. A group selector excludes groupless UMP.
Group remapping touches only actual group fields, preserving utility/stream
headers and native payload bits. Flex Data channel addresses are not channel-voice
remap targets. Opaque/file-only UMP is never forwarded, even with opt-in. Compound
archive commands require explicit expansion before routing. Native MIDI 2 values
and zero-velocity NoteOn meaning remain unchanged.

All copies of an event pass event-count and aggregate encoded-payload-byte
capacity checks before any is admitted. Byte limits cover encoded payloads, not
object/provenance metadata overhead. Returned copies belong to the caller.
Overflow stops the router without admitting a partial fan-out, records per-route
`capacity_rejections`, and retains previously pending progress. Sending is not
atomic: some copies can be accepted before another send fails.

`peek()` returns the head without consuming it; `confirm(id)` advances only that
head after a successful whole-message handoff. `fail(id, reason)` records the
uncertain delivery, route and destination and stops everything. `stop(reason)`
stops without declaring a send uncertain. The first reason is retained, bounded
to 1024 Unicode characters. Stopped routers cannot feed, drain or confirm again.
Reports retain admitted/confirmed/pending counts, queue bytes and per-route totals.
Confirmation means caller-reported handoff, not instrument acceptance. Python
`feed(StreamDiagnostic)` stops for diagnostics belonging to configured sources;
Rust callers use `boundary(source, reason)`. There is no retry, replay, reconnect,
cleanup generation or reset send.

`routing.preview_file` and Rust `routing::preview` read an archive through its
canonical reader and simulate queue draining. Structural completion is verified
even after routing stops at a selected source's gap/discontinuity or admission
failure. Malformed/incomplete input retains verified prefix results; I/O failures
propagate. A configured source absent from a complete archive is an argument
error. Examples are bounded by both `detail_limit` and the aggregate byte limit,
with explicit omissions. Native preview confirmation is simulated acceptance;
the CLI labels these counts `planned`, never sent.

The optional CLI adds `route-preview` and `patch`, sharing protocol, destinations,
channel/group options, non-channel opt-in and bounds. Preview opens no ports and
changes no files. Patch selects one exact unique input and exact unique output
names, using isolated typed connections by default. It opens outputs before input
reception, checks input failure/loss before each send and closes opened connections
on completion, interruption or failure. Routes cannot change during a session.
One native send can delay every route; duration does not interrupt a blocked
transport call. Existing native client/worker admission limits still apply.

CLI status is 0 for clean completion/interruption, 1 for input diagnostics/loss,
capacity stop or a valid archive whose routing stops, and 2 for malformed input,
arguments/access/setup/transport failures or an interrupted uncertain send.
Interruption with pending deliveries stops and reports them without counting
unsent copies as confirmed. Already-sent notes can remain active after stopping.

## Topology observations and discovery polling

Rust `topology::Endpoint` and frozen Python `topology.Endpoint` contain backend
label, protocol (`midi1`/`ump`), direction (`input`/`output`), backend ID and name.
Equality compares all five fields. The backend label is observation metadata;
backend IDs and names do not establish physical identity or health. Empty names
are retained. Endpoints are inert descriptors, without native handles or
connectable discovery tickets.

`topology.Tracker(port_limit=256, detail_limit=128, history_limit=128)` accepts
complete endpoint lists with `observe`. Rust uses the same three constructor
limits. Repeated identical descriptors retain multiplicity through `PortCount`;
the port limit counts all instances, not only distinct descriptors. Generation
zero means no complete baseline exists. The first accepted scan establishes a
baseline with zero arrivals/removals. Later reports compare multisets, so a rename
is removal plus addition even when the backend ID is unchanged. Reordering alone
is not a change. Identical observations cannot prove uninterrupted presence.

Frozen `Report` values expose generation, total port count, grouped ports, the
last `Delta`, changed-scan history, omitted-history total, stopped state and reason.
Every accepted scan advances the generation. Only changed scans enter history;
eviction increments `omitted_history`. `added`/`removed` count port instances,
while `omitted_changes` counts descriptor-delta entries excluded by detail_limit.
The complete current baseline still advances when only diagnostic samples are
omitted. Changes have deterministic descriptor order, not inferred event timing.

An oversized scan stops the tracker and rejects the entire observation. Explicit
`stop(reason)` likewise retains the last complete baseline, delta and history;
the first reason is retained, capped at 1024 Unicode characters. No removals are
inferred from a failed/partial scan. `observe` after stopping raises an error.
Snapshots and collection getters return independent copies. Cardinality limits
bound retained observations, not metadata byte sizes or caller-owned copies.

`discovery.Poller` is a Python adapter with explicit protocol, optional backend,
isolation enabled by default and the same observation limits. Import and
construction start no transport clients. The first explicit `poll()` creates
one unconnected Input and one Output client; subsequent polls reuse them. Each
poll enumerates both directions through existing APIs and commits only when both
scans succeed within the total port limit. Existing discovery APIs enumerate full
results before this adapter can reject their size; they have no new allocation
budget. Input/output enumeration is serial, not an atomic topology transaction.

Known discovery/transport errors and limit overflow produce a stopped report and
close the poller, preserving the last complete baseline. `snapshot()` remains
available after closure; polling a closed poller raises ValueError. `close()` and
context exit release owned client references through the existing retirement
path, without promising synchronous native destruction. No endpoint is opened,
no traffic is sent, and no background polling thread is introduced. Rust hosts
can feed their existing discovery results to the shared tracker directly.

The optional CLI adds `ports --protocol midi1|ump` for one scan and `watch-ports`
for polling, with explicit backend/isolation choices and text/JSON output.
Watch defaults to a one-second interval, prints the initial baseline and actual
changes, and emits a final bounded summary on duration expiry, Ctrl-C or failure.
Unchanged scans produce no intermediate rows. Output explicitly distinguishes an
uninitialized failed/interrupted scan from a complete empty result. Status 0
means clean completion/interruption; status 2 means discovery failure or overflow.
Ordinary appearance/disappearance is not an error. Duration cannot interrupt an
in-flight transport operation. Existing backend/client qualification limits apply.
Polling can miss transitions between scans; no notification, reconnect, route
mutation, MIDI-state replay, cleanup or physical-identity guarantee is supplied.

## Bounded MIDI 1 SysEx transfers

Rust `transfer::Plan` and frozen Python `transfer.Plan` retain a nonempty sequence
of complete MIDI 1 SysEx messages in original order, without splitting, rewriting
or inferred manufacturer semantics. Python construction takes a list of `SysEx`
values. Rust construction accepts an iterator of fallible values. Both require
an explicit unsigned `gap_ns`, including zero. Default limits are 256 messages
and 8 MiB of wire bytes, including each F0/F7 pair. Plans expose message count,
wire size, gap and zero-based indexed message access; Python returns independent
frozen message values. Sessions share the immutable plan internally.

Rust `Plan::read` and Python `transfer.read_file` use the existing strict SYX
reader with a default 1 MiB per-message payload limit. They validate the entire
input before returning a plan: an empty, malformed, truncated or oversized file
cannot yield an executable prefix. Caller-owned binary streams remain open.
File construction failures are OSError in Python; invalid direct plans and
session operations are ValueError. Bounds constrain retained plan content;
parsing can temporarily hold one additional message up to the payload limit,
and caller-owned values, extracted arguments and copies are outside that bound.
Limits do not impose a native transport allocation budget.

Rust/Python `transfer.Session(plan)` performs no I/O and starts no threads. Callers
supply nondecreasing unsigned nanoseconds from one monotonic clock to `prepare`
and `confirm`. `prepare(now_ns)` reserves the next zero-based `Step` exactly once,
or returns None before its deadline. A second prepare while a send is reserved
is an error. `confirm(index, now_ns)` records one successful transport handoff.
The next message is eligible only at that confirmation time plus the selected
gap. There is no initial delay or final trailing delay, and no timing-accuracy
promise. Deadline overflow stops the session while retaining the successful
confirmation. A gap is host pacing, not a measured wire or device processing gap.

`fail(index, reason)` stops a reserved send and marks that index uncertain.
`cancel(reason)` stops an active session; a reserved send becomes uncertain,
while cancellation between sends does not imply delivery of the next message.
Terminal state is permanent and the first terminal reason is retained, capped
at 1024 Unicode characters. No retry, resume, replay, acknowledgement inference,
cleanup, reset, readback or device-specific request/reply exchange is provided.
A new explicit session is a new transfer attempt, not automatic recovery.

Frozen reports contain status (`active`, `completed`, `cancelled`, `failed`),
total messages/bytes, confirmed messages/bytes, remaining messages, in-flight and
uncertain indices, next eligible time and reason. Remaining includes every
unconfirmed message, including an in-flight/uncertain one; neither counts as
confirmed. Snapshots remain independent of later session operations. A successful
handoff does not establish that bytes reached the wire or that an instrument
accepted, applied or persisted the dump.

`transfer_output.run(session, output, cancel=None)` is a synchronous Python
adapter for an existing MIDI 1 `OutputConnection`. It uses monotonic time,
checks the existing `events.Cancellation` between sends and at most every 50 ms
while waiting, and does not own/close the output. Native calls cannot be forcibly
interrupted. Operational send failures return a failed report; Ctrl-C returns a
cancelled report, conservatively marking an interrupted send uncertain.
Unexpected exceptions propagate with the session available for inspection and
any reserved message marked uncertain. Importing the planner/output adapter
starts no native clients. Callers must serialize access to the session/output
and avoid interleaving unrelated sends if device pacing requires exclusivity.

The optional CLI adds `transfer-preview SOURCE --gap-ms N` and the distinct
`transfer-send SOURCE --gap-ms N --port EXACT_NAME`. Gap is a required nonnegative
integer number of milliseconds. Both fully validate the bounded SYX plan first.
Preview opens no ports and prints bounded payload samples with omission counts;
sending selects exactly one MIDI 1 output by name and defaults to process
isolation. It closes its connection and prints final known/uncertain progress.
Explicit backend selection is limited to existing MIDI 1 backends. Neither
command guesses device-specific pacing. Status 0 means valid preview/completed
handoffs, 1 means cancellation/interruption without uncertain delivery, and 2
means malformed input, access/argument/setup/close failure, execution failure
or uncertain delivery.
An interruption during shutdown remains visible even if all handoffs completed.
No physical device compatibility or native transfer qualification is claimed.

## MIDI-CI management browser

Rust `ci_browser::Browser`, Python `ci.Browser` and the optional `ci_live` host
coordinate one bounded discovery scan followed by sequential format-2 endpoint
inquiries. Frozen `ci.BrowseReport` / `ci.BrowseDiagnostic` values retain bounded
progress and peer declarations. They reuse the existing CI session and codecs;
no deferred profile/property support is added. The exact caller-driven methods,
limits, identity and collision policies, live byte/UMP adapter and `ci-browse`
CLI contract are documented in [ci.md](ci.md#one-scan-management-browser).

## Deterministic in-memory testing transports

The public Rust `testing` module and Python `meedee.testing` provide `ManualClock`,
`Input`, `Output` and independent frozen Python `InputReport` / `OutputReport`
snapshots. They initialize no native lifecycle, clients, threads, timers or sleeps.
Python constructors require `protocol="midi1"|"ump"` so even test construction
does not require importing the native MIDI module. Rust uses the existing
`transport::Protocol` and `Packet` values. This is a small receive/send-compatible
testing surface, not another selectable native backend or a complete replacement
for discovery, callback, recording, playback or compound-send connection methods.

`ManualClock(now_ns=0)` stores unsigned host nanoseconds. `advance(nanoseconds)`
changes only that clock and rejects overflow without changing its previous value.
Nothing advances it implicitly. A caller explicitly connects its value to host
clock hooks and chooses when to advance it. Input timestamps are separate signed
nanosecond values and may regress deliberately to exercise diagnostics.

`Input(stream=..., protocol=..., packet_limit=128, event_limit=128,
byte_limit=8_388_608, sysex_limit=1_048_576)` owns a bounded raw packet queue and a
bounded decoded-item queue over the existing source-local `live::Decoder`.
`inject(packet, clock=..., timestamp_ns=None)` accepts byte buffers or an existing
UmpPacket matching that protocol. The explicit Clock must use nanosecond ticks;
synthetic clocks need no claimed backend. Disabled clocks reject non-None times.
Packet content can deliberately be partial or malformed. Every valid injection
consumes a sequence number, even when the newest packet is dropped. False means
admission dropped that packet; protocol/clock/closed/failed-state errors do not
pretend to be packet loss. Caller buffers are copied.

`receive(timeout=None, cancel=None)` never blocks, sleeps or advances time,
regardless of a finite nonnegative timeout. It returns one Event/StreamDiagnostic
or None. A cancelled token raises InterruptedError. Decode order and completion
spans, running status, UMP words, clock changes and recovery use production logic.
When count or shared queued-byte capacity is exhausted, older queued content is
retained and new packets/items are dropped. Raw drops increase dropped_packets;
decoded-item drops separately increase dropped_events. Once admission of a
decoded item fails, the remainder of that packet's decoded tail is dropped too,
so later items cannot overtake a missing boundary diagnostic. Loss diagnostics
follow older retained items before later packets are decoded. Decoded loss uses
lost=0 and reports the dropped-item count in its reason, rather than relabel
items as lost packets. Partial interpretation resets at that boundary. Raw loss followed
by silence is still emitted; a later accepted packet does not report it again.
`lose(count)` explicitly skips positive raw sequence counts and increments loss.

Input snapshots expose next_sequence, dropped_packets, dropped_events, queued
packet/item counts, combined queued payload bytes, closed state and failure text.
`fail(reason)` latches the first simulated worker failure (maximum 1024 Unicode
characters), rejects later injection and exposes a WorkerError through `.failure`.
Already queued input/loss can be drained before receive raises that failure.
`close` discards queued input, resets interpretation and retires receiving; it
is idempotent. Counters and failure evidence survive close. Context exit closes.

`Output(protocol=..., packet_limit=128, byte_limit=8_388_608)` records accepted
packets without connecting them to any input automatically. Python `send` accepts
message objects, compound commands and encoded packets with the same preparation
as live output. It rejects implicit protocol translation. Compound failures raise
CompoundSendError with the known completed prefix and total message count. Capacity
exhaustion rejects without accepting or discarding a packet. `take_packet()`
returns the oldest recorded raw packet, releasing its capacity, or None.
`fail_next_send(accepted=False, reason=...)` arms one fault for the next otherwise
admissible send. It raises SendError and permanently fails the output, optionally
recording the packet first when accepted=True. This simulated ground truth lets
tests check uncertainty handling; the sending application still sees an uncertain
error. No automatic retry occurs. A second armed fault or arming a failed output
is an error. `close` preserves accepted packets for inspection and retires sends.

Output snapshots expose valid send attempts (including capacity rejection),
lifetime accepted packets, retained packet/byte counts, closed state and failure
text. Calls rejected for invalid arguments, protocol, closure or latched failure
are not new attempts. Reports remain independent when queues drain or close.
Bounds cover retained queue payloads/counts, not caller copies, metadata, object
headers, per-packet decoder temporary results or the separately bounded SysEx
assembly state. A synthetic loss diagnostic returned directly is not queued.

Callers serialize access and explicitly inject replies, drain outputs, schedule
faults and advance time. Empty receive cannot drive an unchanged-clock host loop
to completion on its own. There is no script interpreter, virtual-device creation,
background delivery, automatic loopback, ownership cleanup or native backend
selection. These doubles exercise toolkit logic; they do not qualify native
lifecycle, driver queue behavior, physical timing, thread safety or devices.

## Timing and load diagnostics

Diagnostics are opt-in and observational. Existing timing/loss diagnostics and
file-completeness rules remain unchanged. No metric proves physical latency,
device acknowledgement, backend capacity or loss-free delivery. No process-tree
sampling, native loopback or traffic generator is introduced here.

Rust `metrics::Histogram` and Python `metrics.Histogram` accept unsigned u64
nanoseconds through `observe`. Python `snapshot()` returns an independent frozen
`Distribution`; Rust callers clone the accumulator. Both expose count, exact u128
sum_ns (a Python int), optional minimum_ns/maximum_ns and nine lifetime bucket
counts. Inclusive upper bounds are `[0, 1000, 10000, 100000, 1000000, 10000000,
100000000, 1000000000]` nanoseconds; each bucket excludes its predecessor's upper
bound, and the last contains values above one second. Empty distributions have
zero counts/sum and absent extrema. Snapshots copy bucket/bound lists. No samples
or exact percentiles are retained. Invalid/overflowing observations fail without
changing the previous distribution.

`metrics.Delivery(start_ns=...)` uses explicit caller-supplied unsigned host times.
`observe(now_ns)` counts one event; intervals begin with the second event, so
simultaneous events contribute zero intervals. `snapshot(now_ns)` returns events,
elapsed_ns since start and an interval Distribution. Initial/final idle belongs
to elapsed time, not to the interval histogram. Delivery observations cannot
precede start or the previous event; snapshots cannot precede the last event.
Snapshots do not advance the accumulator. Rate is events/elapsed time and is
undefined for zero elapsed time. These accumulators initialize no native client,
worker, timer or background sampler.

Python `output.play(..., metrics=True)` and Rust `playback::play`'s collect_metrics
argument populate Report.lateness. The default is None. It measures dispatch
lateness at the existing pre-send host-clock sample, relative to the planned
deadline, and includes zero values. A sample is retained only after a successful
whole-event handoff, matching sent_events and the existing max_lateness_ns field.
A failed send contributes no sample because acceptance is uncertain; errors and
cancellation retain the successful prefix. Send duration, physical output timing
and device processing are not measured. Enabled empty playback has an empty
histogram, distinguishable from disabled collection.

`input.start_recording(..., metrics=True)` and `archive.Recorder(..., metrics=True)`
expose a live `.pressure` snapshot and final RecordingReport.pressure. Rust
`recording::Recorder::new` takes the corresponding collect_metrics argument.
Disabled collection returns None. accepted_records counts admitted caller records,
not initial archive descriptors or successfully written records. capacity_rejections
counts record/payload/byte-budget or queue-capacity rejections; semantic invalidity,
input loss/discontinuity, prior failure, closure and worker disconnection retain
existing failure behavior without being relabelled capacity rejection. Peaks use
existing reservations, including queued and currently writing records. Bytes are
reserved archive payload bytes, excluding framing, metadata and object overhead.
Live snapshots are independent values. Final metrics survive failed/incomplete
recording and do not change structural completeness or best-effort rules.

`meedee monitor --metrics` adds a `metrics` object to its final summary. It counts
all decoded Events delivered to the host before display filters, excluding
StreamDiagnostic items. `clock="host_monotonic"`, events, elapsed_ns,
events_per_second and delivery_intervals describe host receive observations;
source event timestamps remain untouched and are diagnosed separately. The window
starts before receiving and ends after connection closure and inspection-summary
preparation, before final emission. It includes initial/final idle, interpretation,
display and shutdown work. Rate is not raw packet or wire throughput; intervals
are not source-clock jitter. No host timestamp calls or extra summary fields are
added when disabled. CLI defaults, exit status, filters and loss handling remain
unchanged.

## Passive MIDI 1 SysEx capture

Rust `transfer::Capture` and Python `transfer.Capture` collect whole SysEx messages
from one explicit stream. Construction requires stream, expected_messages and
positive timeout_ns, with byte_limit=8_388_608 and payload_limit=1_048_576 defaults
in Python. The byte budget includes F0/F7 framing; the expected count also bounds
retained message objects. Impossible counts exceeding byte_limit/2 are rejected.
Budgets exclude object/metadata overhead, decoder state, caller copies and native
queues. This is passive MIDI 1 byte capture, not UMP or a dump-request protocol.

`start(now_ns)` starts a fresh session and checks deadline arithmetic. `poll(now_ns)`
notices expiry, and `feed(item, now_ns)` consumes existing typed Event or
StreamDiagnostic values using the same unsigned, nondecreasing host clock.
Source timestamps are preserved in the input events but do not set deadlines.
At or after the overall deadline, the session times out before accepting another
message. Timing begins at start, not at the first SysEx; no implicit timer or
background polling is created. Starting twice or feeding/polling a non-active
capture is an error. Invalid clock operations leave the prior observation intact.

Every whole SysEx from the selected source counts, including empty payloads and
unrelated manufacturer traffic. Non-SysEx MIDI 1 events increment ignored_events;
UMP/compound events and mismatched sources fail. All decoder diagnostics stop
capture, including known loss, malformed/truncated input and clock changes.
Payload or aggregate-byte exhaustion fails before retaining the offending
message. Reaching the expected count before the deadline completes the capture.
It proves neither device-specific dump completeness nor the absence of later
traffic or unobservable driver loss. It does not correlate replies, infer an idle
end marker, filter manufacturer IDs or send any requests/acknowledgements.

`fail(reason)` / `cancel(reason)` stop ready/active sessions and retain the first
terminal outcome and received prefix. Independent frozen Python CaptureReport
snapshots expose status (`ready`, `active`, `completed`, `failed`, `cancelled`,
`timed_out`), expected_messages, received_messages, wire_bytes, ignored_events,
deadline_ns and an optional reason capped at 1024 Unicode characters. `message(i)`
returns a received SysEx even after failure. No automatic retry or state cleanup
is implied.

`write(destination)` exports only a completed capture through the existing SYX
writer, preserving retained payload bytes and order. Rust accepts a caller-owned
writer. Python paths use exclusive creation and never overwrite an existing file;
caller-owned binary streams remain open and write at their current position.
Writes flush without fsync. Errors propagate and can leave a partial destination;
export does not change the capture outcome or claim power-loss durability.

`transfer_input.run(capture, receiver, cancel=None)` starts a fresh capture on the
caller thread. Supply an existing queued MIDI 1 typed input for the same stream;
its already queued events are eligible and are not drained automatically. The
adapter checks failure/loss counters before and after receives, polls the deadline
and waits at most 50 ms per receive. Cancellation/Ctrl-C retain a cancelled prefix;
known transport errors retain failed progress. Unexpected exceptions propagate
after retiring an active capture. The caller owns and closes the input, serializes
access and configures its existing queue/decoder budgets. No native output, timer,
worker or additional decoder is created. Uninterruptible in-process driver or
Python work can exceed the observation deadline.

`meedee sysex-receive DESTINATION --port NAME --expected-messages N --timeout-ms MS`
requires one exact, unique input name and uses process isolation by default.
Start a dump on the instrument after opening capture. Backend, receive-byte,
capture-byte and payload limits are explicit options. The capture deadline starts
after connection setup. Setup itself uses existing transport operation deadlines.
The input is closed before export, and a close failure prevents writing. Existing
destinations are rejected before discovery and exclusive creation also catches
files appearing later. Failed/cancelled/timed-out receives create no destination.
Final write/flush failures may leave a partial file. The final text/JSON summary
separates capture status, exported, failure and interrupted evidence; completed
capture with exported=false is not a saved backup. Exit status is 0 for successful
export, 1 for timeout/cancellation/interruption and 2 for setup, input, limit or
export failure. No physical instrument was used to qualify this workflow.

## One-shot MIDI 1 MTS dump queries

Rust `tuning_query::Query` and Python `tuning.Query` combine the existing MTS
request codecs, ReplyTracker and decoder. Construction requires one typed
BulkDumpRequest or BankedDumpRequest, an input stream and a positive timeout_ns.
Broadcast device ID 127 is rejected; device IDs are 0..126, programs/banks 0..127.
Banked requests match either KeyBasedDump or ScaleOctaveDump using the existing
tracker's device/bank/program rules. Ordinary requests match BulkDump. No new MTS
wire format, generic transaction engine or instrument configuration write is added.

`prepare(now_ns)` reserves the only send and returns its ordinary SysEx request.
It starts the overall deadline before the caller attempts transport handoff;
overflow fails without starting. `confirm(now_ns)` records a successful whole
message handoff and enters waiting, or timed_out if the send consumed the deadline.
`poll(now_ns)` checks expiry while sending/waiting. All time comes from one
caller-supplied unsigned, nondecreasing host clock, independent of event timestamps.
`feed(item, now_ns)` requires a confirmed send and checks expiry before accepting
an Event/StreamDiagnostic. A reply at or after the deadline cannot complete.
Invalid operations/clock regression raise without silently restarting the query.

All decoder diagnostics, wrong-source events, UMP/compound events and recognized
malformed MTS messages fail the query. Unrelated well-formed MIDI 1 events/SysEx
and nonmatching decoded MTS increment ignored_events without retaining history.
Only one matching raw SysEx and its decoded MTS value are retained. The existing
supported dump layouts bound retained reply content; this does not bound caller
input, packet queues, decoder working data, metadata or copies. Input adapters
must use the existing bounded decoder and queue settings.

A matching original BulkDump with LegacyMismatch completes and retains both its
original checksum bytes and the decoder's explicit status. Modern malformed or
bad-checksum banked dumps still fail under existing decoder rules. Export never
re-encodes the decoded tuning value: doing so could normalize the checksum and
change the received message. Success establishes a matching observed declaration,
not authenticity, device health, physical receipt of the request or tuning state.
MTS has no transaction ID; an identical unsolicited or late dump can match.

`fail(reason)` and `cancel(reason)` stop ready/sending/waiting queries, preserving
the first terminal outcome. Frozen independent Python QueryReport snapshots
contain status (`ready`, `sending`, `waiting`, `completed`, `failed`, `cancelled`,
`timed_out`), request_sent, uncertain_send, ignored_events, deadline_ns, optional
reply/decoded and a reason capped at 1024 Unicode characters. request_sent means
a confirmed transport handoff, not acknowledgement. uncertain_send is set on a
terminal outcome reached with a send pending; it is conservative even if a test
transport happens to know acceptance. Nothing retries or resumes a terminal query.
A known handoff followed by input failure remains request_sent=true without
inventing send uncertainty. `write` requires completed status, writes the original
reply using the existing SYX writer and flushes. Rust accepts a caller-owned writer;
Python paths use exclusive creation and binary streams remain open. Final write
errors may leave a partial file; flush is not fsync or power-loss durability.

`meedee.tuning_live.run(query, receiver, output, cancel=None)` drives a fresh query
on the caller thread using existing MIDI 1 queued typed input/output connections.
The caller supplies the query's selected stream and owns/closes both connections.
Known loss/failure is checked before sending and before/after receive. Receive
waits are at most 50 ms, with cancellation/Ctrl-C and uncertain sends represented
in the supplied query. Unexpected exceptions propagate after retiring an active
query. No extra decoder, timer, worker or cleanup traffic is created. Already
queued input is eligible; there is no automatic drain or time alignment. Blocking
in-process drivers or arbitrary caller code can exceed the observation deadline.

`meedee tuning-preview --device-id ID --program P [--bank B] --timeout-ms MS`
prints the exact request bytes and deadline duration without importing native MIDI
or opening ports. `meedee tuning-query DESTINATION` takes those same fields plus
required `--input-port NAME --output-port NAME`. It uses process isolation by
default and resolves both exact, unique names before opening either endpoint.
Only one request is sent, after both connections are ready; no tuning changes,
program selection, broadcast scan or automatic retry is sent. Both connections
close before exclusive export. Existing destination files are rejected before
discovery and files appearing later are preserved by exclusive creation. Setup,
input and close failures prevent export. Final write/flush failures may leave a
partial destination. The final text/JSON summary separates query status, known
handoff, uncertainty, checksum, export and failure evidence.

Exit status is 0 for a saved matching reply with a matching checksum, 1 for saved
LegacyMismatch or timeout/cancellation/interruption, and 2 for setup, malformed
input, loss, send, close or export failure. A completed query with exported=false
is not a saved backup. No physical instrument interoperability is qualified by
this workflow's device-free tests.
