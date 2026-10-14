# meedee: immutable Rust and Python MIDI toolkit

## 1. Purpose and scope

Create a new Rust MIDI toolkit and a matching Python API, using libremidi for
native MIDI transport. The design starts from musical concepts and the MIDI standards.
It has no interface-compatibility requirement with another Python MIDI library.

The toolkit targets Linux, macOS, and Windows, including resource-conscious
Linux applications on the Pi Zero. Rust users need no Python interpreter.
Python users receive the same musical semantics through a binding, rather than
a separately implemented parser, tuning engine, or file format.

The main requirements are:

- MIDI 1.0 and MIDI 2.0 message models and codecs, with honest transport limits.
- MIDI Tuning Standard (MTS), General MIDI, fine controls, NRPNs, and RPNs.
- SysEx, expressive/per-note controls, and other relevant standardized features.
- An immutable abstract `Message` with concrete types such as `NoteOn` and
  `SysEx`; Python objects should feel like frozen data classes.
- Optional original timestamps from libremidi, with explicit units and origins.
- Standard MIDI/SYX file interoperability and a custom precision-preserving
  event file format.

This plan is for review, updated 2026-10-13. The backend choice is libremidi,
with our Rust toolkit providing musical features and our own Python bindings
exposing them. It does not authorize implementation, dependency changes,
workspace restructuring, hardware/network runtime tests,
or publication. The deleted prior plan is not restored or used as an API target.
Only this file is updated by the current request.

Additional work beyond the prompt

None.

## 2. Protocol baseline and backend capabilities

The selected transport is libremidi, a cross-platform C++20 library with MIDI
1.0 byte and native UMP APIs. The Rust toolkit and Python binding now use this
transport; native platform qualification remains a separate completion gate.
The source pin and selected build requirements are recorded below.

The owner selected bundled, statically linked sources with CMake; native OS
backends only; MIDI 1.0 packets by default with explicit UMP selection; and
Absolute input timestamps by default with explicit capture disabling. The Pi
target is Zero 2 W with 64-bit Raspberry Pi OS. The selected vendored source is
now the owner's fork at `53a94516cc280fa63f6de12bfe279ba3c26d02b9`, including
two separately committed CoreMIDI property fixes and checked input teardown
for CoreMIDI and Windows, checked CoreMIDI output disposal and initialized
WinMM output storage. Windows native qualification is still outstanding. See
[source provenance](../third_party/libremidi/SOURCE.md). Backend integration
and cross-platform build qualification remain unfinished.

libremidi provides MIDI 2.0 transport through CoreMIDI UMP, ALSA UMP, and
Windows MIDI Services. Availability depends on the OS version, installed
services/libraries, compiled backend, and endpoint capabilities. A working
MIDI 1.0 backend is not evidence of native UMP support. See the
[libremidi documentation](https://celtera.github.io/libremidi/) and
[C API](https://github.com/celtera/libremidi/blob/master/include/libremidi/libremidi-c.h).

The plan separates message support, file support, and live transport support:

| Feature | Rust/Python model and codec | Live through selected libremidi backend |
| --- | --- | --- |
| MIDI 1.0 channel/system messages | Required | Yes, subject to backend behavior |
| MTS, GM, and other MIDI 1.0 SysEx/CC protocols | Required | Their MIDI 1.0 encoding, where the receiver supports it |
| MIDI 1.0 messages carried in UMP | Required UMP codec/file support | Native UMP where available; explicit byte conversion otherwise |
| MIDI 2.0 Channel Voice and other UMP messages | Required model/codec/file support | Native UMP on qualified backends/endpoints |
| SysEx8 and UMP-specific features | Required in the appropriate codec slice | Requires compatible native UMP transport and receiver |
| MIDI-CI discovery/profiles/property exchange | Staged standardized support | Applicable SysEx exchanges can use a MIDI 1.0 transport |

Use libremidi's byte and UMP APIs for their respective encodings. Do not pass
serialized UMP words into a MIDI-byte send operation. Unsupported live operations
must report a capability error.
Translations that lose information must be explicit and return a loss report;
unsupported per-note/group features must not disappear silently.

**Live MIDI 2.0 remains a required qualification gate.** libremidi is the chosen
implementation route, not proof that every deployment supports UMP. Verify
byte and UMP I/O separately on each target platform, including the selected
Pi image. Report missing capabilities without silently switching protocols
or introducing another transport library.

Pin the normative specification revisions before implementing codecs. The
public overview pages establish scope, not every bit layout. Obtain the full
specifications legitimately; do not reverse-engineer missing packet details
from marketing examples or implement an obsolete negotiation scheme.

Primary standards references:

- [MIDI 1.0 detailed specification](https://midi.org/midi-1-0-detailed-specification).
- [UMP and MIDI 2.0 protocol](https://midi.org/universal-midi-packet-ump-and-midi-2-0-protocol-specification).
- [MIDI 2.0 specification family](https://midi.org/midi-2-0).
- [MIDI Tuning updated specification](https://midi.org/midi-tuning-updated-specification).
- [Control Change/RPN reference](https://midi.org/midi-1-0-control-change-messages).
- [General MIDI](https://midi.org/general-midi),
  [MPE](https://midi.org/mpe-midi-polyphonic-expression), and
  [MIDI 1.0 addenda](https://midi.org/midi-1-0-addenda).

The detailed specifications are the authority for channel-mode interactions,
parameter selection, tuning sentinels, UMP translation, checksums, and reserved
fields. Track document versions and test-vector provenance with the code.

## 3. Three library layers and shared responsibilities

There are three libraries/crates, not three production executable binaries:

1. **libremidi**, the third-party C++ OS MIDI transport library.
2. **meedee**, our Rust toolkit: immutable values, codecs, stateful interpretation,
   tuning, profiles, files, timing, and port integration.
3. **Our Python binding**, exposing those Rust features through Python values,
   exceptions, iterators, and the existing optional process isolation.

The Rust toolkit must not depend on PyO3 or an embedded interpreter. Domain
validation, wire encoding/decoding, controller state machines, tuning math,
file interpretation, and integer-time conversion have one implementation in
Rust. Python adapts objects and language protocols; it does not create a second
musical engine or a permanently mirrored mutable message store.

Use a small safe Rust wrapper around libremidi's C API for native transport.
Keep unsafe FFI confined to that boundary. Specify handle ownership, callback
buffer lifetimes, callback retirement before freeing user data, thread affinity,
and error translation. Copy borrowed input before a callback returns when it
must outlive that callback. Contain Rust panics at the foreign boundary and
ensure C++ exceptions cannot escape into Rust. Verify that the C API covers
required byte/UMP operations, discovery, timestamps, and lifecycle controls
before committing to the bridge design.

Do not depend on `pylibremidi` or expose its flat namespace as our public API.
Both Rust and Python use our Rust toolkit and the same libremidi transport
wrapper. The C++ build/link step is an explicit requirement for Rust binaries
and Python wheels; Rust-only applications still require no Python runtime.

Use a Cargo workspace with the toolkit and binding as separate packages.
Package names are `meedee` and `meedee-python`; the selected private Python
extension identity is `meedee._core`. Small public-module loaders register the
native definitions in `meedee.messages`, `meedee.events`, and `meedee.midi`;
pure message/event imports must not initialize MIDI workers. Keep the
Python package at the repository root. Preserve current dependency versions
unless an approved requirement forces a change.

Separate pure protocol/file modules from native I/O so constructing a message
or reading a file does not initialize an OS MIDI client or start transport
workers. Add modules when used, not as empty extension points. Pure Rust
applications link the toolkit directly and start only the resources they use.

The existing raw binding is an implementation asset, not the new public API
contract. Reuse its relevant tests and failure-containment policy, but replace
the midir transport completely and revalidate backend-dependent assumptions.
Do not retain two production transport implementations. Decide
its public placement and the breaking-release policy before replacing it; do
not maintain duplicate implementations or unsolicited compatibility aliases.

## 4. Immutable message and event model

### Abstract Message and concrete types

Python `Message` is abstract and cannot be instantiated. Every supported
musical message/command type derives from it. Concrete objects have explicit
typed fields, validated constructors, readable field-based representations,
value equality, and immutable state. No mutable `type` string selects unrelated
payloads in a generic bag of attributes.

Examples of the intended construction style:

```text
NoteOn(channel=1, note=60, velocity=100)
NoteOff(channel=1, note=60, velocity=64)
ProgramChange(channel=1, program=1)
ControlChange(channel=1, controller=7, value=100)
ControlChange14(channel=1, controller=7, value=10000)
NrpnSet(channel=1, parameter=1234, value=10000, resolution=14)
SysEx(data=...)
```

Names and complete signatures are provisional until the first API slice. Use
channel numbers 1–16 consistently in both Rust and Python, including decoded
messages and semantic operations. Reject channel values outside that range.
Codecs convert internally to the wire field's 0–15 values on encoding and back
to 1–16 on decoding; backend calls use the appropriate wire representation.
Program Change program numbers use 1–128 in both languages, including MIDI 1.0
and MIDI 2.0 types, decoded messages, and GM lookup results. Codecs convert
internally to wire values 0–127 and back. GM program 1 is Acoustic Grand Piano;
reject public Program Change numbers outside 1–128. Some device manuals display
programs as 0–127, so document this API convention explicitly.

Control Change numbers remain the standard 0–127 identifiers without an offset:
CC 0 is Bank Select MSB, CC 7 is channel volume, and CC 64 is sustain. Controller
values also remain 0–127 for seven-bit controls. Bank values, note numbers,
RPN/NRPN identifiers, and MTS tuning bank/program fields retain their specified
protocol ranges; the Program Change convention does not renumber those fields.
See the [MIDI Association's controller table](https://midi.org/midi-1-0-control-change-messages).
Specify units and
bit widths in annotations/docs. Do not use a float in 0–1 as the canonical
storage for exact integer controller or velocity data.

The Rust equivalent uses immutable typed structs and a `Message` trait; an
`AnyMessage` enum represents heterogeneous streams without heap-allocating
one trait object for every short message. Rust has no class inheritance, but
the same named types implement the common trait. Keep fields private where
needed to prevent invalid mutation. Construction and checked replacement
produce new values; readers/getters do not expose writable backing storage.

### Python data-class experience

Provide named constructor fields, annotations, introspectable signatures,
field-style read access, readable `repr`, equality, and a checked `replace`
operation returning a new object. Reject invalid fields and ranges at creation
and replacement. Pattern matching should be considered in the representation
prototype, without relying on mutable dictionaries or dynamic aliases.

The owner selected native frozen PyO3 values on 2026-10-13, preserving Rust-owned
validation without adding a Python runtime dependency. Python constructors are
keyword-only and reject coercion, including booleans for integer fields.
Wrong types raise `TypeError`; invalid values raise `ValueError`.
The slice-1 [API contract and prototype](api.md) verifies the base/subclass
contract and records construction/memory observations. Production module
integration and stream/Pi measurements remain later qualification gates.

Immutability must be deep. A frozen model containing a list or dict still has
mutable contents. SysEx payloads use immutable bytes. Tuning/other aggregates
need immutable owned values and read-only access or defensive copies; a caller's
mutable input must be snapshotted. Any exception to the usual model collection
rules must be reviewed rather than hiding mutable lists inside `Message`.
Hashability is offered only when the complete value has a stable hash.

### Timestamped events are separate

A `Message` describes musical content, independent of when it was captured,
stored, or played. An immutable `Event` holds a message, a stream identity, and
an optional `Timestamp`. The same message can be reused in different events.
Do not overload a message field to mean file ticks in one context and hardware
microseconds in another.

Raw receive returns typed events. Optional semantic interpretation produces
separate derived updates with their source timestamp range; it must not destroy
or overwrite the primitive events needed for exact capture and diagnosis.
Recording raw input and requesting a semantic view are different operations,
not two simultaneously maintained copies of one mutable message stream.

## 5. Base MIDI and expressive message coverage

### MIDI 1.0

Cover Note On/Off, polyphonic and channel pressure, CC, program/bank selection,
pitch bend, system common messages, realtime clock/start/continue/stop, active
sensing, reset, and channel-mode commands. Note On with velocity zero remains
representable as received; an explicit semantic helper may interpret it as
release, but round-trip encoding must not silently rewrite it.

Use a real incremental byte parser with running status, realtime interleaving,
partial-message state, defined malformed-input recovery, and bounded SysEx
assembly. Feed boundaries are arbitrary. Undefined/reserved inputs are handled
by a documented error/preservation policy, never by unchecked slices or panics.
File parsing has its own applicable running-status rules.

### MIDI 2.0 and UMP

Provide explicit MIDI 2.0 types, for example `Midi2NoteOn`, rather than changing
`NoteOn.velocity`'s resolution implicitly. MIDI 2.0 values retain their native
widths, group/channel addressing, note attributes, and controller semantics.
Do not make every message carry a group when its UMP class has no group field.

The UMP codec inventory must cover applicable utility, system, MIDI 1.0 channel,
MIDI 2.0 channel, data/SysEx7/SysEx8, Flex Data, and stream-management families
from the chosen specification revision. Validate packet lengths, statuses,
reserved fields, continuation order, and byte order. Preserve unknown packets
in an explicit opaque representation when requested, without claiming they can
be interpreted or transmitted through every backend.

Include high-resolution channel controls, registered/assignable controllers,
per-note controls and pitch, relative adjustments, note management, and note
attributes as specified. MIDI 2.0 controller messages are not automatically
identical to MIDI 1.0 NRPN sequences. Translation tables must identify exact,
quantized, unsupported, and profile-dependent cases. No automatic lossy downgrade.

### Other standardized features to include in the feature inventory

| Family | Scope |
| --- | --- |
| RPN | Named tuning/pitch-bend-sensitivity helpers and generic parameter messages |
| MPE | Zone configuration and expressive channel/per-note interpretation; explicit voice/channel allocation policy |
| MIDI-CI | Discovery, profile configuration, and bounded property exchanges; replies are not evidence of universal device support |
| Universal SysEx | Identity, supported master volume/tuning messages, and relevant machine/time-code commands |
| High-resolution velocity prefix | MIDI 1.0 prefix interpretation kept distinct from paired fine CC and MIDI 2.0 velocity |
| Bank/program selection | Ordered bank MSB/LSB plus program messages, including GM profiles |
| MIDI clock and time code | Typed messages and explicit conversions/assembly; no assumption that receive timestamps are musical clock phase |
| Standard UMP files | MIDI Clip/other current standardized UMP file support after normative format access and version review |

These are identified standards work, not a promise of every device-specific
profile or vendor SysEx dialect. Mark each family as implemented, deferred,
model-only, codec/file-only, or live-validated. Obtain specifications and select
bounded subscopes before claiming broad coverage.

## 6. Fine controls and parameter messages

### Paired 14-bit CC

`ControlChange14` is one semantic command with a base controller and an exact
14-bit integer. Its MIDI 1.0 encoder emits the ordered coarse/fine pair, using
the standardized controller pairing. It is not a new on-wire MIDI 1.0 message.
Controller-specific protocols such as bank selection and data entry must retain
their own meaning rather than being indiscriminately presented as knobs.

Sending a compound command holds the output's sequence lock across all emitted
messages. Another send on that output cannot interleave its bytes. This does
not guarantee atomicity against other programs using the device. A partial
failure reports what completed; it is never silently retried.

The optional receive interpreter tracks coarse/fine observations per source and
channel/controller. It reports an incomplete/coarse observation separately
from a complete 14-bit value. Do not invent an unobserved fine part, combine
different sources, or require the two CCs to arrive next to each other. Define
repeated-LSB, reordered-pair, new-MSB, reset, stream-loss, and stale-state policy
against the detailed specification and measured devices. Reset/loss must not
leave an apparently trustworthy composed value based on forgotten context.

### NRPN and RPN

Provide useful operations at the semantic level: set a selected parameter at
an explicit 7-/14-bit resolution, and represent increment/decrement commands.
A generic numeric NRPN has no universal physical unit; device definitions may
supply labels/scaling separately. Do not infer semitones, hertz, or signedness
from an unregistered parameter number.

A send selects the parameter, sends its data, and applies a documented selection
release policy. The conservative proposal sends selection each time rather
than caching the receiver's state across independent operations. Suppression
of repeated selectors is a later explicit optimization. Increment/decrement
must not assume the CC data byte is an arbitrary increment magnitude.

The receive interpreter is stateful per source/channel and selection family.
It handles partial selection, changing NRPN/RPN selection, coarse/fine data
entry, relative changes, and defined deselection. Keep raw CCs observable.
Do not invent an NRPN-null convention by assuming a legitimate parameter value
is a sentinel; implement standard selection/deselection rules precisely.

A relative update is a relative update, not a fabricated absolute value when
the previous value is unknown. On known message loss, reset, reconnect, or
selection ambiguity, invalidate relevant state and emit an observable diagnostic.
Interpretation cannot reconstruct messages dropped upstream.

## 7. MIDI Tuning Standard

Make tuning a first-class feature, rather than a collection of magic SysEx
byte strings. Include typed messages for the specified bulk requests/dumps,
single-note tuning changes, bank/program extensions, and scale/octave forms,
with real-time/non-real-time behavior explicit in the type or fields.

Provide immutable tuning tables and note-pitch values. Support convenient
construction from frequencies in hertz, cents offsets, or exact supported
pitch values. Separate those convenience inputs from the canonical lossless
protocol representation. Range checks, quantization, rounding, checksums, device
addressing, tuning names, and no-change/undefined-pitch sentinels come from the
normative MTS revision, not guesses from examples.

Expose the quantization/error result when a requested frequency cannot be
represented exactly. Do not silently clamp an out-of-range pitch. Preserve
sentinels as explicit values instead of treating them as extreme frequencies.
Validate bulk table lengths and the legal note/bank/program ranges.

Sending a tuning command does not prove the receiver implements it or changed
its tuning. Sending a request must be explicit and use a bounded reply wait;
requests and replies need correlation, timeouts, cancellation, and separation
from unrelated SysEx. No tuning query or dump request runs on import/open.

MTS, channel fine/coarse tuning, MPE pitch bend, and MIDI 2.0 note pitch are
separate mechanisms with different scope and resolution. Conversion between
them requires an explicit policy; do not collapse them into a single global
float tuning state. Tests cover boundaries, sentinels, and interoperability
vectors before any instrument receives a tuning operation.

## 8. General MIDI

Support General MIDI as a musical profile layer, not as a substitute for the
wire protocol. GM1/GM2 functionality does not imply a synthesizer, soundfont,
or audio renderer, and does not make an arbitrary MIDI endpoint GM-compatible.

Include:

- Standard instrument-family/program lookup and reverse lookup using public
  program numbers 1–128, with internal conversion to wire values 0–127.
- GM1 percussion-note mappings and the drum-channel convention, represented as
  `channel=10` in both Rust and Python.
- GM2 standardized variations, drum kits, bank/program selection, controls,
  and mode requirements, derived from its full specification.
- Typed General MIDI mode/reset messages and helpers for deliberate setup.
- Named common controllers and channel-mode operations usable without memorizing
  CC numbers, while preserving access to their exact encoded values.

Store authoritative tables in Rust and expose the same names/values to Python.
Return immutable entries or copies; callers cannot mutate a global lookup map.
Do not confuse GM2 with the MIDI 2.0 protocol. Vendor GS/XG additions are separate
possible profile work, not assumed General MIDI support.

Mode/reset changes can affect programs, controllers, parameter interpretation,
and tuning. They must be explicit, ordered operations, with device timing
requirements respected and decoder state invalidated where appropriate. Opening
or closing a port does not automatically place a device into GM mode. File
reading preserves embedded mode messages without executing them.

## 9. SysEx and transport-independent codecs

`SysEx` represents an immutable MIDI 1.0 payload; framing belongs to its codec.
Validate seven-bit payload requirements and handle manufacturer/universal IDs
through explicit typed helpers. Do not put an eight-bit SysEx8 payload into
this type. UMP SysEx7/SysEx8 have their own packetization and stream-state rules.
Typed MTS/GM/identity messages remain `Message` subclasses and use the shared
SysEx machinery for their encoding rather than separate ad hoc serializers.

A codec maps a typed message to a sequence of wire packets. Ordinary short
messages yield one packet; fine CC/NRPN/GM setup commands can yield several.
Explicitly distinguish semantic commands, primitive wire messages, and raw
packets. A caller recording native input retains primitive messages and timing;
derived multi-message observations carry source ranges instead of replacing
that evidence with one invented instantaneous command.

Normalize upstream fragments before exposing a complete SysEx. Bound assembly
by size and reset state on interruption or observed loss; malformed or oversized
input yields a defined error/drop diagnostic. Backend assembly before our
callback remains outside these bounds. Audit the selected libremidi backend's
fragment delivery and assembly/allocation behavior; a receive queue limit alone
does not cap upstream storage. Record any remaining limit rather than carrying
forward a midir-specific defect as an established libremidi defect.

Arbitrary raw packets can be inspected/preserved, but cannot bypass transport
capability checks or cause unsafe encoder assumptions. Unsupported opaque data
is rejected on send unless the chosen transport explicitly supports it.

## 10. Optional high-precision timestamps and clock domains

Preserve libremidi's input timestamp exactly as an integer in the configured
timestamp mode and unit when capture is enabled. libremidi uses nanoseconds by
default and offers several timestamp modes; verify the selected C API's units,
signedness, origin, and backend behavior before defining the Rust conversion.
Disabling capture yields `timestamp=None`; it does not introduce a synthetic
zero or clock with a different meaning.
Timestamp selection is independent of musical messages and protocol selection.

`Timestamp` contains integer ticks and a clock identity. `Clock` records the
exact tick unit and origin kind, with rational unit representation where
necessary. Record the selected relative, absolute/monotonic, or other supported
mode explicitly; do not assume timestamps are UTC or comparable across ports.
Native clock IDs distinguish connection generations. Host-monotonic capture
is a separately named source,
not a replacement silently labeled as native time.

Represent duration/ordering conversions with integer or rational arithmetic.
A nanosecond storage unit cannot manufacture nanosecond accuracy from a source
with coarser resolution. Record known clock resolution separately from its
unit; unknown accuracy stays unknown. Repeated timestamps are valid. WinMM's
underlying timestamp rollover and per-backend differences require explicit
validation; no guessed cross-port synchronization or wrap extension.

Native timestamps describe receipt as implemented by the backend, not hardware
execution time at the instrument. For SysEx, document which fragment/time is
attached to a reconstructed event. A derived control update carries the first
and last contributing timestamps. Comparisons across clocks require an explicit
mapping, including uncertainty; never subtract unrelated clock ticks.

Playback uses relative times anchored to a selected monotonic playback clock.
Reject an unmapped multi-clock recording instead of merging it with arbitrary
origins. OS scheduling and Pi load affect actual delivery. Do not promise
sample-accurate or hard-real-time execution from timestamp precision alone.

## 11. Standard file interoperability

Provide Rust-owned read/write APIs for Standard MIDI Files and SysEx dumps.
Evaluate libremidi's existing C++ SMF reader/writer against the requirements
below before writing a duplicate codec. Reuse it through a narrow bridge if it
meets the semantics and resource limits; otherwise document the concrete gaps
that require a Rust codec. The transport C API does not itself expose those
C++ file classes. Our Python binding exposes the Rust file API in either case.
See libremidi's [reader](https://github.com/celtera/libremidi/blob/master/include/libremidi/reader.hpp)
and [writer](https://github.com/celtera/libremidi/blob/master/include/libremidi/writer.hpp).
Use typed events, tracks, tempo/time-signature metadata, and explicit musical
beat/tick time. File metadata such as tempo and track names is separate from
wire `Message` commands; output cannot accidentally send a file-only record.

SMF support includes its applicable file types, variable-length values, running
status, metadata, SysEx handling, and deterministic event ordering. File
read/write APIs accept streams as well as paths; caller-owned streams remain
caller-owned. Offer iterator-based reading where the format permits it rather
than requiring a whole large file to fit in Pi memory.

High-resolution controllers/MTS/GM commands are represented by their appropriate
MIDI 1.0 sequence in a conventional MIDI file. Standard MIDI Files do not
preserve arbitrary native clock identities/timestamps or every MIDI 2.0 message.
Export requires an explicit clock/tempo conversion and reports rounding and
unsupported content. Lossy export is opt-in; unsupported data is not discarded.

Standardized UMP/MIDI Clip file support is a separate named capability, with
normative format revision and test vectors recorded. A custom timestamp file
is not advertised as one of those standards. Preserve unknown metadata where
safe, reject malformed lengths before allocating, and document whether a writer
normalizes wire encoding or preserves an original packet representation.

## 12. Custom precision-preserving event format

### Proposal: one canonical binary format first

Use a versioned binary event stream, provisionally `.mrp`, as the canonical
custom format. This favors compact storage and streaming on the Pi. Do not
implement an independently evolving binary format and text format at once.
A human-readable export can be selected later if it uses the same record schema.
This satisfies the requested binary-or-text choice; the owner can choose text
instead before the format slice begins.

Its version-1 schema must contain only information needed for lossless event
storage:

- A recognizable magic value and format version.
- Clock descriptors: identity, exact tick unit, origin kind, and known resolution.
- Stream descriptors sufficient to distinguish input streams/generations; saved
  port labels/IDs are descriptive, not instructions to reopen hardware.
- Ordered event records: stream, optional exact integer timestamp, and an explicit
  message/record discriminator plus payload.
- Explicit metadata or diagnostic records needed to preserve tempo, loss, and
  clock-discontinuity information when present.

Represent absent timestamps distinctly from zero. Use fixed documented byte
order, checked record lengths, and a compact encoding defined by the schema.
UMP words have a documented file byte order independent of the host machine.
Do not use Python pickle, language-specific object layouts, floating-point
seconds, or an unversioned dump of Rust structs as the format.

Message discriminators are file-level stable IDs, not unstable Rust enum-layout
numbers. The format can hold primitive messages, semantic commands, and supported
UMP values without silently expanding and thereby losing the saved meaning.
Unknown records can be preserved opaquely when requested, but cannot be executed
as understood messages. Unsupported format versions fail clearly.

File order preserves equal-timestamp ordering and events from independent
streams. Multiple clocks can be archived without implying a single sortable
physical timeline. Do not require a global timestamp ordering that silently
reorders native input. A capture writer uses bounded buffering and reports
recording overflow; a valid prefix does not mean capture was complete.

### Integrity and resource behavior

A reader validates record lengths before allocation and distinguishes a complete
file from an interrupted/truncated capture. Define completion/trailer semantics
in the format specification. If checksums are needed for corruption detection,
select and document one scheme at that point rather than adding redundant
checksumming/compression/index layers speculatively.

Readers/writers operate incrementally with bounded working memory. They report
full-disk, interrupted writes, malformed records, unsupported message versions,
and input errors with record/byte location. A recording error stops or marks
capture explicitly; it must not look like successful silent data loss. File I/O
and logging must not block the native callback thread. No compression, database,
network storage, recovery rewrite, or indexing subsystem is required initially.

Round trips preserve exact ticks, clock identity, timestamp absence, source
ordering, protocol-width values, SysEx payloads, and opaque records. Conversion
to a text view or SMF is a separately identified export operation with explicit
precision/loss accounting. Freeze the on-disk version only after fixtures cover
all included message families and a documented example file is independently
readable from both Rust and Python.

## 13. Live API, ownership, and failure handling

Keep discovery/port opening explicit and separate from pure musical values.
The new high-level input delivers `Event`; output sends a `Message` and resolves
its packet sequence against advertised transport capabilities. Timestamps do
not imply scheduled send; scheduling is a separate operation with a selected
clock and cancellation policy.

Retain meedee's existing default Python process isolation and its in-process
option while evaluating Pi costs. Rust-only I/O initially runs natively with
clear native-hang limitations. It must not launch a Python interpreter as its
isolation mechanism. A Rust helper process would be a separate architecture
choice, not an assumed fourth executable.

Expose bounded queues and known loss/failure diagnostics. Compound decoders
invalidate state on observable gaps. A successful send means backend acceptance,
not physical delivery or acknowledgment. Capabilities distinguish codec support,
transport support, and observed peer support; discovery is not an implicit
negotiation, and idle input is not proof of failure.

Specify polling, blocking receive, cancellation, callback retirement, worker
failure, and close wakeup. Application callbacks run outside internal locks;
long-running callbacks cannot be forcibly cancelled safely. Callback forwarding
and self-close need deliberate semantics and tests before changing the current
blocking-thread guard. Preserve existing shutdown/GC/panic-containment coverage
when extracting code; no silent weakening for a more convenient API.

No automatic retries of tuning, mode changes, or parameter writes. A failed
compound send may already have changed device state. Report partial completion,
require a deliberate recovery decision, and invalidate any local output-state
optimization. Native and user-callback failures are different categories.

Existing upstream and validation findings are recorded in [issues.md](issues.md),
[validation.md](validation.md), and [midir-suggestions.md](midir-suggestions.md).
The midir report is historical evidence for the old backend, not a libremidi
audit. Reassess each backend-dependent finding during replacement and retain
applicable binding-level tests and guarantees.
Process isolation does not repair a shared OS MIDI service, unsafe native
rollback, or native assembly that occurs before our admission limits.

## 14. Cross-platform and Pi Zero requirements

Use standard Rust on Linux, macOS, and Windows. Pi Zero deployment requires
Linux/ALSA, not bare-metal firmware or `no_std`. Build libremidi with a compatible
C++20 toolchain and verify linking, required system libraries, license notices,
and wheel packaging on all three platforms. Confirm original Pi Zero versus
Zero 2, OS image, ABI/bitness, compiler target, libc, Python installation, and
chosen MIDI endpoint before selecting build commands or claiming hardware support.

Keep short message values compact and avoid per-message heap allocation in the
Rust path where possible. SysEx/tuning/file aggregates use checked owned storage.
Avoid per-message threads/timers/processes. Pure message and file use does not
start MIDI workers. Python has explicit conversion/object overhead to measure;
choosing Rust internally is not proof of low Python latency or memory use.

Measure resident and peak memory, startup/import time, binary/wheel size, idle
CPU, process/thread/handle count, throughput, overflow, and latency distributions
for Rust native, Python native, and Python isolated operation. Include dense CC,
NRPN sequences, tuning dumps, long SysEx, UMP files, and multi-stream recording.
Publish workload sizes and set budgets from real Pi measurements with the owner.
Cross-compilation success is not proof of runtime performance or ALSA permissions.

Python's supported-version floor remains 3.11 unless changed explicitly. Release
CI continues to run only on published releases and tests oldest/newest versions,
currently 3.11 and 3.15, across existing desktop platforms. Pi build/run artifacts
and native UMP transport qualification are additional distinct validation gates.
No toolchain or dependency changes are made by this planning request.

## 15. Proposed slices and completion gates

Each slice can contain several commits. The ordering establishes shared values
and wire evidence before stateful interpretation or live writes.

1. **Specification/API prototype (completed).** Obtain normative revisions; define value
   types, deep immutability, message hierarchy, units, errors, namespace, and
   language mappings. Prototype Python subclass/field behavior and Pi-relevant
   object overhead. Gate: no unacknowledged dependency or transport assumption.
   Evidence: [API contract](api.md), [specification baselines](specifications.md),
   and the native representation contract. Slice 2 promotes NoteOn and SysEx
   to public `meedee.messages` values.
2. **Rust toolkit extraction and libremidi transport replacement (in progress).**
   The dependency-free `crates/meedee` crate owns the prototype NoteOn/SysEx
   values; Python wrappers use those values. The root package is `meedee-python`.
   Vendored CoreMIDI checked input teardown passes focused normal/sanitizer tests.
   Native transport ownership uses one dedicated Rust thread per client, as
   selected by the owner; creation, operations and teardown stay on that thread.
   The Rust wrapper now uses this ownership model and statically builds the
   pinned C++ source. Its byte/UMP FFI paths have device-free macOS fixture tests;
   real CoreMIDI service access, Windows/Services and Pi qualification remain open.
   Python transport now uses this wrapper for both isolated and in-process I/O;
   midir and its old filtering/naming/ID-lookup API have been removed.
   Raw callbacks carry immutable ReceivedPacket values with signed nanoseconds,
   distinct connection clocks and byte/UMP payloads. Framing is in the pure
   slice-3 codec; receive filters and typed transport remain later integration.
   Separate the shared core and our Python binding; implement the safe C API wrapper,
   native build integration, and complete native qualification throughout the
   transport path.
   Retain applicable lifecycle checks and revalidate callback/clock assumptions.
   Gate: Rust use requires no Python; FFI ownership, shutdown, byte/UMP capability
   detection, Python 3.11/3.15 builds, and selected Pi cross-build checks pass.
3. **MIDI 1.0 values and byte codec (implemented).** Implement base message families, validated
   encoding, incremental parsing, running status, and bounded SysEx. Gate:
   immutable Rust/Python values, exact wire fixtures, malformed-input recovery.
   The pure codec and 18 primitive message families are available in Rust/Python.
   Signed pitch bend, ordered diagnostics/recovery and a configurable 1 MiB
   SysEx payload limit follow the approved choices. The decoder deliberately
   discards SysEx lacking F7 on a new status, with a diagnostic. Timestamps,
   receive filters and typed transport integration remain later work. See
   [the API contract](api.md) and shared wire fixtures for the precise scope.
4. **Fine CC and RPN/NRPN (implemented).** Add compound output and source/channel stateful
   interpretation. Gate: split/reordered/missing pairs, relative updates,
   resets/loss, selector changes, and partial sends have defined tested results.
   Seven frozen commands expand into primitive CCs; send_command serializes all
   steps including RPN-null release and reports known/unknown progress without
   retries. The optional pure interpreter has one fixed 16-channel state table
   per source, indexed immutable observations, conservative fresh selector halves
   and explicit reset/loss diagnostics. Generic CC pairing follows wire order;
   relative parameter steps do not invent absolute values. The API contract
   records the approved Rust Message/PrimitiveMessage/Command trait split.
5. **MTS.** Implement tuning values and the selected standard message families.
   Gate: normative vectors, quantization reporting, checksums/sentinels, and
   request/reply state pass before any authorized instrument operation.
6. **General MIDI and other MIDI 1.0 profiles.** Add GM1/GM2 maps/mode messages,
   bank/program helpers, and selected universal/MPE/time-code features. Gate:
   one Rust table source, exact numbering, explicit mode effects, no auto-reset.
7. **UMP and MIDI 2.0 core.** Implement packet/value families and explicit
   translation. Gate: shared codec fixtures, preserved wide values, SysEx7/8
   segmentation, capability checks, and explicit live-qualification status.
8. **Standard files.** Evaluate/reuse libremidi's SMF codecs or implement the
   justified Rust codec; implement SYX and selected standardized UMP formats.
   Gate: streaming/resource checks, unknown metadata, exact semantic round trips,
   and explicit precision/unsupported-content export reports.
9. **Custom event format.** Freeze the minimal version-1 schema and implement
   incremental read/write. Gate: exact clock/tick round trips, unknown records,
   equal-time order, absent time, truncation, overflow, and full-disk failures.
10. **High-level live input/output and timed capture/playback.** Integrate typed
    events, semantic views, capabilities, bounded recording, receive/callback
    lifecycle, and clock-aware replay. Gate: fake transport/clock checks first,
    then authorized native tests without new driver-safety claims.
11. **MIDI-CI and native MIDI 2.0 qualification.** Implement the selected bounded
    capability/profile/property exchanges and qualify libremidi's native UMP
    backends. Gate: actual native UMP evidence on chosen platforms before
    advertising live MIDI 2.0 support; unsupported deployments remain explicit.
12. **Pi and cross-platform qualification.** Run selected board workloads and
    desktop checks, record resource budgets and native gaps, and review partial
    versus complete feature claims before an explicitly authorized release.

A release can be staged, but its feature matrix must state what works as values,
codecs, files, and live transport. The custom format must not be frozen with
unsupported families accidentally assigned ambiguous IDs. Deferred standards
must not be represented by public stubs that claim working support.

## 16. Verification and decisions before implementation

Use shared Rust/Python fixtures derived from normative standards and independent
known vectors. Cover every legal boundary and invalid range; do not generate
expected bytes solely from the encoder under test. Verify immutable input
snapshots and aliases, hierarchy/ABC behavior, replacement validation, hashes,
protocol-width preservation, parser feed partitions, and exact file ordering.
Verify channel 1 encodes as wire value 0 and channel 16 as wire value 15, with
inverse decoding and rejection of public channel values 0 and 17 in both languages.
Verify Program Change numbers 1 and 128 encode as wire values 0 and 127, with
inverse decoding and rejection of public numbers 0 and 129. Verify CC 0, 7,
64, and 127 retain their exact identifiers, and reject CC numbers outside 0–127.

Stateful tests use controlled sources and clocks: CC pairing, NRPN/RPN selection,
loss invalidation, MTS request/reply, GM mode effects, partial compound send,
callback self-close/forwarding, cancellation, and clock discontinuities. File
fault tests use controlled short writes/read failures and resource limits.
Use fake transports for unit tests; no arbitrary hardware probing in release CI.

Native tuning, GM reset, unplug/replug, driver-fault, and UMP tests require
explicit authorization and named endpoints. Use externally supervised processes
for hang tests. Existing virtual-port permission does not authorize sending
configuration/tuning to physical instruments. Keep failed native initialization
and untested platforms visible rather than interpreting unit tests as hardware
validation.

Decisions to settle before dependent implementation:

- The libremidi source pin and static CMake build are selected; verify C API
  coverage, offline native build/packaging inputs, and byte/UMP backends on each
  platform. Codec/file support alone does not meet the live MIDI 2.0 requirement.
- Native frozen Python representation is selected; integrate it into the
  production modules and qualify stream/Pi costs using the prototype evidence.
- Public namespace is `meedee.messages`, `meedee.events`, and `meedee.midi`,
  without top-level re-exports. Current transport can be replaced without
  compatibility aliases; keep `__init__.py` empty.
- Pi Zero 2 W and a 64-bit Raspberry Pi OS ABI are selected; establish the exact
  image/sysroot and acceptable workloads before cross-build and performance claims.
- Fine-control/parameter observation policies and initial MTS/GM2/CI subscopes,
  after access to the detailed normative documents.
- Canonical custom file choice: proposed binary first versus text instead;
  concrete schema, completion semantics, and versioning before implementation.
- Absolute input timestamp capture is enabled by default, with an explicit off
  option. Validate backend origins/fallbacks and explicit clock-mapping policies.

These decisions do not block preparing this plan. They prevent later code from
silently inventing behavior, promising unavailable transport, or relaxing the
immutable message contract.

## 17. Completion reminder

- [ ] When all work here is finished, remind Tom to send the libremidi fork
  commits upstream for review, with their regression tests and validation
  limitations. Use the commit history recorded in
  [source provenance](../third_party/libremidi/SOURCE.md). This reminder is for
  project completion, not merely completion of slice 2. Do not submit the
  commits or contact upstream developers automatically.
