# Unify MIDI 1 and MIDI 2 message handling

## Objective and agreed decisions

Provide one semantic message API and one event pipeline for MIDI 1 and MIDI 2,
with explicit byte-stream and UMP codecs at the edges. Preserve native integer
values and protocol-specific meaning through capture, manipulation, storage and
playback.

The user agreed to:

- Keep explicit native values rather than add automatic normalization/scaling.
- Replace public names and payload variants and migrate repository callers.
- Put optional UMP groups on shared MIDI 1 messages; decode MIDI 1 UMP messages
  into those shared classes rather than a nested message wrapper.
- Apply `Output(channel=...)` consistently to all channel-addressed semantic
  messages, including MIDI 2 per-note messages.
- Make a clean replacement without backward-compatibility requirements.

This task writes the plan. Implementation is a subsequent task. Migrate Python
and Rust together, including examples, fixtures, bindings and public module
ownership. Retire superseded interfaces completely. Use one current archive
format rather than introducing aliases, legacy readers or migration tools.

## Audit and evidence

Source audit on 2026-11-12 at `e32aa3b`. Findings below are based on source and
existing test coverage; this documentation task does not execute the program or
rerun tests. Paths are relative to the repository root.

| Area | Current implementation | Consequence |
| --- | --- | --- |
| Shared musical types | `crates/meedee/src/messages.rs`: `ChannelFormat`, `channel_values!`, `ControlChange`, `AnyMessage`. Python mirrors these in `src/messages.rs` and `meedee/messages.pyi`. | NoteOn/Off, pressure, ControlChange, ProgramChange and PitchBend already share their real implementations. Keep them instead of creating a second universal hierarchy. |
| Addressing | Shared values store a group only in `ChannelFormat::Midi2`. `with_group` rejects MIDI 1. `ump::Midi1Packet` separately holds a group and an `AnyMessage`. | MIDI 1 UMP needs a wrapper even though its musical meaning is unchanged. Group currently appears to imply message version 2 when it actually belongs to UMP addressing. |
| Event payload | `events.rs::Payload` has `Midi1(AnyMessage)`, `Ump(UmpMessage)` and `Command(AnyCommand)`. `AnyMessage` already includes versioned MIDI 2 values. | Variant names mix message semantics, wire representation and expansion intent. Consumers must inspect wrappers to discover musical meaning. |
| UMP types | `ump.rs::UmpMessage` duplicates the seven shared channel-message enum arms and combines them with eight MIDI 2-only musical classes, utility messages, data packets, Flex Data and stream packets. | A wire-codec module owns musical operations as well as packet families. The same shared class travels through different envelopes. |
| Conversion into payloads | Rust `output.rs::From<AnyMessage>` chooses Midi1/Ump by version. Python `src/event_values.rs::payload` probes primitive, command and UMP extraction in order. | Classification is distributed. This is not evidence that Python archives reject MIDI 2: `resolved()` explicitly rejects MIDI 2 in the MIDI 1 extractor, so the UMP extractor handles it. `test/test_archive.py` covers MIDI 2 pitch/controller round trips. Replace the indirect classification rather than treating this working path as a confirmed bug. |
| Compound values | Fourteen-bit MIDI 1 ControlChange is representable in both AnyMessage and AnyCommand; output and archive code separately recognize it as a command. ParameterSet/ParameterStep are semantic MIDI 1 commands. | Classification and expansion rules are repeated. A wider controller value is not sufficient evidence of MIDI 2, and a compound operation is not a primitive packet. |
| Receive | `live.rs::Decoder` turns byte messages into Payload::Midi1 and UMP messages into Payload::Ump. The byte and word decoders maintain different framing state and diagnostics. | Share the decoded value boundary; keep these necessary framing implementations separate. |
| Output | `output.rs::prepare_message` branches by payload wrapper and connection Protocol. `_clients.py::_prepare_sequence` and native bindings maintain their own supported-type lists. UMP channel override matches only the seven shared classes. | Equivalent semantic operations follow different paths. Per-note MIDI 2 classes do not receive the same explicit channel override as shared MIDI 2 notes. |
| Downstream operations | `routing.rs::classify/remap/wire_bytes`, `notes.rs::action` and `event_views.rs::midi1` unwrap the same families separately. Routing often encodes, edits header bits and decodes again to remap an address. | Centralize semantic access and address changes. Preserve decoder-specific group selection, MIDI version distinctions and unsupported-operation errors. |
| Persistence and files | `archive.rs` tags events as MIDI 1 bytes, one UMP or a semantic command. `file_io.rs::WriteRecord` encodes primitive values into MIDI 1 bytes and decodes them again. `file_timing.rs` selects conversion by payload wrapper. | Storage and file adapters need the new semantic boundary, not another byte-only normalization round trip. Grouped complete SysEx also needs an explicit storage representation. |
| Existing strengths | Common metadata, shared playback clocks/reports, bounded packet assembly, translation reports and explicit raw `transport::Packet`/ReceivedPacket access already exist. | Reuse these. There is no reason to replace the scheduler, recorder, transport backends or all message structs. |

Audit owners: `src/ump.rs`, `src/event_values.rs`, `src/output.rs`,
`src/packets.rs`, `crates/meedee/src/{messages,events,ump,live,output,archive,
routing,notes,event_views,file_io,file_timing,ump_translation}.rs`,
`crates/meedee/src/ump_{values,stream}.rs`, `meedee/_clients.py` and the matching
Python stubs. Tests providing current contracts include
`test/test_{messages,ump,archive,output,routing,notes,event_views,file_timing}.py`
and `crates/meedee/tests/{message_metadata,archive,controls}.rs`.

## Target ownership and value model

### Semantic messages and commands

`messages` owns musical message classes and their protocol-specific values.
Keep the existing shared channel classes and add the MIDI 2-only musical types
to the same module and `AnyMessage` enum:

| Retired `ump` name | Canonical `messages` name |
| --- | --- |
| Midi2RegisteredPerNoteController | RegisteredPerNoteController |
| Midi2AssignablePerNoteController | AssignablePerNoteController |
| Midi2PerNoteManagement | PerNoteManagement |
| Midi2RegisteredController | RegisteredController |
| Midi2AssignableController | AssignableController |
| Midi2RelativeRegisteredController | RelativeRegisteredController |
| Midi2RelativeAssignableController | RelativeAssignableController |
| Midi2PerNotePitchBend | PerNotePitchBend |

Retain each moved type's native widths, signedness, required constructor
arguments, validation and metadata. Its protocol version is inherently 2;
expose that fact consistently without a redundant mutable version field.
Remove old module registrations and names rather than re-exporting them.
Keep package-root exports limited to their current intended role.

MIDI 1 ParameterSet/ParameterStep and MIDI 2 registered/assignable controllers
remain distinct where their data meanings differ. Parameter-specific conversion
does not become ordinary numeric scaling. Keep high-level duration-based Note
and Rest behavior unchanged; this refactor does not invent a normalized musical
layer or silently upgrade their MIDI 1 defaults.

### Addressing

Allow an optional group on shared MIDI 1 channel/system messages and complete
SysEx, and on MIDI 1 compound commands when addressing them over UMP. None
means ungrouped legacy addressing; a supplied group is 1 through 16. Shared
MIDI 2 classes retain their existing group defaults and requirements. Group is
an address, not provenance metadata and not a signal to upgrade the protocol.

Keep channel/program numbering and existing native value ranges unchanged.
Do not silently change the moved per-note constructors to use a new channel
default. Output resolves its normal channel default where applicable and
applies an explicitly configured channel override to every channel-addressed
semantic message or command. It does not rewrite group, note number or note
attributes as a side effect. Utility/stream packets without channels remain
unaffected.

Address changes preserve timestamps, source and packet span. Equality and
hashing continue to include musical addressing and ignore occurrence metadata.
Changing protocol version through `replace` remains forbidden; translation is
the explicit way to change native representations.

### Canonical payload

Replace the current split with these responsibilities:

```rust
enum Payload {
    Message(messages::AnyMessage),
    Ump(ump::PacketValue),
    Command(messages::AnyCommand),
}
```

`Message` covers shared MIDI 1/MIDI 2 semantics. `PacketValue` contains the
UMP-specific utility, SysEx7/SysEx8 fragment, MixedDataSet, Flex Data, stream
and opaque families. It does not contain another set of channel-message arms
or a Midi1Packet wrapper. Remove the superseded UmpMessage enum and Midi1Packet
API; `ump.decode` and its incremental decoder return the canonical payload
boundary in Rust and the corresponding native message/packet classes in Python.

One core classification/conversion path is used by authored values, byte/UMP
decoding, bindings, output, recorders and file readers. In particular, classify
MIDI 1 fourteen-bit ControlChange as Command, MIDI 2 ControlChange as Message,
and packet-only values as Ump. Reject deliberately constructed inconsistent
low-level payloads at the boundary. Do not rely on repeated encode failures or
exception probing to discover a value's family.

Keep the Message metadata trait and existing Python Message base. Packet values
can retain that common metadata interface without pretending every packet is a
musical operation. Preserve explicit raw bytes/word access and opaque values.
Decoded messages do not promise original running status, callback packet
boundaries or malformed reserved bits; raw packet APIs provide that evidence.

## Encoding, conversion and output

Replace public `midi.Protocol { Midi1, Ump }` with
`midi.Encoding { Bytes, Ump }`, and rename corresponding `protocol` connection
and route fields to `encoding`. Message version remains 1 or 2. Migrate Python
configuration, Rust options/builders, native adapters, worker serialization,
CLI arguments and examples together. Backend/native ABI identifiers can remain
internal implementation details. UMP capability declarations still distinguish
support for MIDI 1 and MIDI 2 messages.

Keep byte encoding in `codecs` and UMP encoding/framing in `ump`. Existing
encoding traits may remain when they express these capabilities clearly;
their implementations must consume canonical values. Do not add a generic
`encode()` that guesses a wire format from an integer's magnitude.

| Value | Byte output | UMP output |
| --- | --- | --- |
| MIDI 1, no group | Encode native MIDI 1 bytes. | Require explicit group selection. |
| MIDI 1, group supplied | Require explicit group selection/removal before legacy output; do not silently discard it. | Package as MIDI 1 UMP; keep the native values. |
| MIDI 2 | Require explicit supported translation to MIDI 1. | Encode native MIDI 2 UMP. |
| Complete seven-bit SysEx with group | Require explicit removal of group for legacy output. | Packetize as SysEx7 with the selected group. |
| MIDI 1 compound command with group | Require explicit removal of group for legacy output. | Expand into MIDI 1 primitives and package those primitives, without promoting them to MIDI 2. |
| UMP-only packet family | Unsupported unless an existing explicit conversion supports it. | Preserve its packet-family rules and existing send restrictions. |

Store group selection on the semantic value rather than add duplicate group
arguments to several encoding layers. Update SysEx7 packetization to consume
the grouped complete SysEx value. Preserve the distinct eight-bit data and
stream-ID rules of SysEx8; do not reinterpret it as seven-bit SysEx.

Use the existing transport's validated UMP-stream handoff for a logical value
that packages into several UMPs. Bound the complete prepared output before
sending. A logical value counts as sent only after its handoff succeeds; raw
packet inputs retain their own event granularity. Preserve known compound
prefix information when supplied by a backend and report unknown progress as
unknown. Neither batching nor a successful handoff certifies physical delivery.
Do not introduce automatic retries or arbitrary chunking to fit a backend limit.

Keep byte-stream running-status parsing and UMP word framing separate, including
their diagnostic offsets and truncation rules. Preserve explicit assembly:
the ordinary receive path must not begin coalescing SysEx7/8 fragments or text
into larger events merely because semantic channel messages are unified.
The existing assembler keeps per-source/group/stream state, bounds and loss
handling; assembled SysEx7 data can construct the shared grouped SysEx value.

Keep translation outcomes Exact, Quantized, Unsupported, ProfileDependent and
ContextRequired. Use the existing normative scaling rules and explicit loss
permission. Preserve these semantic distinctions:

- MIDI 1 NoteOn zero is note release; MIDI 2 NoteOn zero is a note-on event.
- Down-conversion may quantize velocity, pressure, bend and controller values.
- Bank and parameter sequences need context; raw CCs are not automatically
  promoted into higher-level intent during decoding.
- Per-note expression and attributes do not silently become MPE or disappear.
- System messages carried by UMP do not acquire MIDI 2 channel semantics.

Migrate translation helpers to a neutral `translation` module because protocol
conversion is independent of UMP framing. Keep their report contents and loss
policy, and remove the old `ump` entry points after migrating callers.
Preserve current MIDI 1 close/cleanup behavior and explicit UMP behavior; adding
UMP cleanup, reconnection or discovery-driven conversion is outside this plan.

## Storage, consumers and API parity

Make archive event encoding describe the canonical semantic/packet/command
categories and retain native protocol values, optional group, opaque words and
occurrence metadata. A grouped complete SysEx value must be stored as one
logical value without being confused with a single SysEx7 fragment. Revise the
current archive layout/version and its specification together when needed;
use stable explicit representation IDs rather than Rust discriminants or
Python class names. Regenerate current fixtures. Implement one current format,
without backward readers, aliases or migration machinery.

Remove encode-to-MIDI-1-then-decode normalization from generic file writing.
Archive writers accept canonical MIDI 1 and MIDI 2 values directly. `.syx`
continues to mean complete seven-bit SysEx without group/timing storage; `.mid`
and `.clip` retain their own file-record and timing contracts. Export requiring
address removal, translation or clock mapping stays explicit. This plan does
not add another file format or broaden the generic reader to every container.

Update routing, notes, views, inspection, comparison and file timing to use
common semantic accessors for version, group, channel, note/control kind and
native value. Replace address remapping through encode/edit/decode with checked
value transformations. Keep interpretation state scoped by source, protocol,
group and channel as appropriate; unification must not merge unrelated streams
or selector/note state. Keep existing explicit group selection for views that
interpret one UMP group's MIDI 1 traffic.

Calculate admission and prepared-output costs for the selected encoding.
Three-byte MIDI 1 channel messages occupy four bytes in UMP; MIDI 2 messages
and packetized data have other costs. Commands include their whole expansion.
Update encoded-size helpers, route limits, receive/recording bounds and archive
payload sizing together. Keep unknown/file-only packets on their existing
deliberate raw path, and preserve cancellation, typed failures, confirmed
progress, source spans and clock behavior.

Python and Rust must expose the same operations, defaults, validation, units,
address rules and reports, with idiomatic constructors, traits and enum
differences. Preserve the established Python-only CLI, unit-string parsing and
uFor exceptions. Do not introduce a second Python implementation of canonical
classification or another scheduler/recording engine.

## Implementation sequence

1. Freeze source-based wire and behavior fixtures for the existing shared
   classes, metadata, translation, packet-only types and output reports.
   Specify the new payload, group and archive contracts in the API documents.
2. Extend shared value addressing, relocate the eight MIDI 2-only semantic
   classes and define PacketValue. Update the byte/word codec boundaries and
   native Python registration/extraction together. Replace old names and
   wrappers and migrate direct callers in the same coherent change.
3. Route Input/Output, bindings and worker adapters through canonical payload
   classification. Implement Encoding naming, explicit UMP packaging, uniform
   channel override and bounded multi-packet handoffs. Retain translation as a
   separate explicit operation and move its public helpers to translation.
4. Migrate archive/file I/O and their specification/fixtures, then routing,
   note tracking, views, inspection/comparison and timed file adapters. Remove
   duplicated family switches and special command classification. Keep each
   intermediate commit coherent and passing; combine tightly coupled steps
   rather than introduce temporary compatibility interfaces.
5. Update both example documents and API documentation, including authored and
   received MIDI 1-over-UMP, native MIDI 2, complete versus fragmented SysEx,
   grouped commands and lossy conversion. Replace stale names throughout
   tests, optional scripts and distribution checks. Keep public-symbols.txt
   uncommitted unless explicitly requested.
6. Run the focused parity cases below, then the required Python checks and
   relevant Rust tests/workspace checks. Review the final diff for duplicate
   paths, accidental protocol conversion and loss of addressing/provenance.

## Verification and acceptance

Use shared, independently stated wire fixtures and fake transports/clocks.
Cover both authored and received values through Python and Rust, including
in-process and isolated adapters where existing tests support them.

| Case | Required assertion |
| --- | --- |
| MIDI 1 bytes and MIDI 1 UMP | Same semantic class/native values; UMP retains its group. Known packet words round trip through the packet codec. |
| MIDI 2 shared and newly relocated classes | Native widths/attributes, strict bool/range checks, signed values and metadata survive without scaling or alias types. |
| Group/channel edits | Correct fields change for channel/system/per-note values; group zero/out-of-range and groupless packet misuse fail. Explicit output overrides are consistent. |
| NoteOn zero and pressure/control interpretation | Correct version-specific actions and independent source/group/channel state remain intact. |
| Command versus primitive | Seven-/fourteen-/thirty-two-bit CC and ParameterSet/Step are classified once; expansion order, native values and confirmed failure prefixes remain correct. |
| Conversion | Every seven-bit velocity and fourteen-bit bend round trips under existing rules; quantized/context/profile/unsupported cases keep their report and permission behavior. |
| SysEx and packet families | Seven-bit versus eight-bit validation, empty payloads, fragmentation/interleaving, aborts, limits, opaque words and file-only restrictions remain explicit. No silent receive assembly. |
| Output and budgets | Byte/UMP cost boundaries are correct; preparation failures send nothing, cancellation preserves admitted progress, partial/uncertain sends are not retried and batch success is not called physical acknowledgment. |
| Storage and files | Current semantic, grouped complete SysEx, fragment, command and opaque categories round trip with clocks/spans; CRC, truncation, unknown data and format restrictions remain tested. |
| Language boundary | Rust and Python classify equivalent values identically before output, archive writing, routing and observation; introspection/module names match the new public ownership. |

For implementation changes, use the repository's required pytest, Ruff check
and format, ty and pyupgrade workflow, plus relevant Cargo tests, formatting and
`cargo check --workspace --all-targets`. Avoid simultaneous Cargo builds while
the Python callback test is compiling into the same target directory. The
embedded-Python harness currently needs the virtual environment's site-packages
on PYTHONPATH to find runtime dependencies; record the actual verification
environment. Rely on existing CI for unavailable platforms without manually
starting workflows per commit. Hardware qualification remains separate.

Acceptance is one shared semantic boundary, explicit encoding and conversion,
consistent addressing, bounded lossless handling of native values and packet
data, migrated callers in both languages, and passing behavior-focused checks.
No superseded public API, archive migration layer or parallel message implementation
remains.

## Standards and limits

Use the acquired baselines in [specifications.md](specifications.md), including
M2-104-UM 1.1.2 and the existing translation/scaling baseline. Recheck required
errata before changing wire rules. This plan reorganizes the implementation;
it does not adopt new protocol features merely because a newer document exists.

- [MIDI Association UMP/MIDI 2 specification](https://midi.org/universal-midi-packet-ump-and-midi-2-0-protocol-specification): UMP carries both MIDI 1 and MIDI 2; container choice and semantic version are distinct.
- [Microsoft UMP primer](https://microsoft.github.io/MIDI/kb/ump-data-format-primer/): packet widths, MIDI 1/system message families, groups, and distinct SysEx7/SysEx8 representation.
- [Windows implementation details](https://microsoft.github.io/MIDI/kb/midi2-implementation-details/): endpoint discovery/negotiation and JR handling have platform-specific rules. Keep MIDI-CI, universal identity inquiry and UMP endpoint management distinct; this refactor does not start any of them implicitly.

## Additional work beyond the prompt

None.
