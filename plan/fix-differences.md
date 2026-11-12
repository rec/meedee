# Finish Python and Rust interface alignment

Additional work beyond the prompt: None.

## Scope and starting point

This plan covers the 60 Rust paths classified in `public-symbols.txt` as
representation (35), keyword arguments (9), existing operation (9), or other
module (7). It does not add the excluded low-level helpers or reopen the separate
exposure-candidate and partial-facade lists.

The inventory identifies mappings, not 60 missing features. Its source baseline
is commit `49e7d28`; this plan was checked against `b56f318` on 2026-11-05. Later
work already added Python file timelines/helpers, Rust virtual ports, raw input,
callbacks and retained diagnostics. Do not reimplement those changes from the
older inventory notes.

The goal is equivalent user operations and behavior, with an explicit mapping
where the languages need different representations. It is not equal symbol
counts. Python subclasses and unions, Rust enums, Python keyword arguments and
Rust options/builders are already permitted by AGENTS.md. Adding Python wrapper
classes just to reproduce every Rust enum would make the basic API harder to use.

The user approved D1, D2 and D3. The implementation and evidence below record
completion of all four slices. Keep public-symbols.txt uncommitted.

## What completion means

For each numbered entry below, record one of:

- Fixed: a concrete operation, field, default, validation rule or error differed,
  and the implementations and callers now agree.
- Verified language adaptation: users can perform the same operation and obtain
  equivalent information, expressed idiomatically in each language.
- Approved exception: behavior remains different for a stated reason approved by
  the user. Existing exceptions do not need to be approved again.

An exact-name match alone cannot close an entry. A retained enum is not an
unfixed defect when its cases correspond to the Python return values. Conversely,
an enum label must not conceal missing payload fields, validation or error detail.

## Slice 1: Message families, observations and return values

Keep the existing Rust sum types where Python returns concrete objects. Review
every case against the Python constructor, returned object and binding conversion.
Correct missing fields, defaults, validation and provenance in the shared Rust
implementation, then keep the Python binding thin. Preserve wire bytes and the
agreed metadata/equality rules.

GM decoding is a confirmed operation mismatch: Python `gm.decode()` handles mode
and extended messages, whereas Rust `gm::decode()` handles modes and
`gm::decode_extended()` handles extensions. Subject to decision D1, replace that
split with one decoder and one result family. Migrate the binding, interpretation
code, tests and examples; do not retain the superseded public decoder as an alias.

| ID | Rust path | Counterpart and planned resolution |
| --- | --- | --- |
| R01 | `archive::Record` | Python archive record classes, Clock and Message. Keep the enum; verify all record cases, unknown-record preservation, completion and occurrence comparison, including timestamp/source metadata. |
| R02 | `ci::Body` | Concrete Python CI messages. Keep the enum; compare every body's fields, optional values and version-dependent rules. |
| R03 | `ci::Management` | Python CI objects combine header/body. Keep the validated envelope; verify header, extensions and message provenance survive construction, decoding and encoding. |
| R04 | `ci::Update` | Python session returns Action or Observation. Verify Send/Observe carry equivalent values and ordering. |
| R05 | `codecs::DecodeItem` | Python Message or codecs.Diagnostic. Verify incremental decoding, diagnostic payloads and recovery return the same sequence. |
| R06 | `controls::Observation` | Python ControlObservation, ParameterObservation, ParameterStep or Diagnostic. Keep the union/enum mapping; verify source binding, reset behavior and metadata. |
| R07 | `events::InputItem` | Python Message/raw packet or StreamDiagnostic, depending on input mode. Verify decoded and raw cases, filters and diagnostic delivery; use the newly implemented Rust raw input. |
| R08 | `events::ObservationValue` | Concrete Python Observation.value. Verify all merged interpreters/trackers return equivalent values and reset diagnostics. |
| R09 | `events::Payload` | Python Message subclasses. Keep the Rust dispatch enum; verify primitive, compound and UMP coverage, metadata and conversion boundaries. |
| R10 | `events::PlaybackCause` | Python playback exceptions and their causes. Preserve typed Rust causes; verify failure reason, confirmed progress, cancellation and metrics, rather than matching exception text. |
| R11 | `events::StreamDiagnosticKind` | Python StreamDiagnostic.kind strings. Keep the enum/string adaptation; document and verify the exact case-to-string mapping. |
| R12 | `gm::ExtendedMessage` | Python extended GM classes. Replace as a public split result family under D1. |
| R13 | `gm::GmMode` | Unified in both languages with an explicit mode and device ID. |
| R14 | `gm::Program` | Unified catalogue result with optional banks in both languages; search keeps GM1 followed by GM2 ordering. |
| R15 | `inspection::Format` | Python format strings. Keep the enum/string adaptation; verify accepted formats and explicit-format behavior. |
| R16 | `messages::AnyCommand` | Python compound Message subclasses. Keep the enum; verify expansion order, channel resolution, provenance and partial-send reporting. |
| R17 | `messages::AnyMessage` | Python primitive Message subclasses. Keep the enum; verify constructors, defaults, version selection and encoding validation. |
| R18 | `messages::Beats` | Python beat values with bpm or explicit unit strings. Keep typed beats; verify the same rational timing and rejection of beats without tempo. |
| R19 | `messages::Metadata` | Python Message.timestamp/stream/span. Keep Rust's grouped storage; verify replace/builders/transpose preserve it and payload equality/hash ignore it. |
| R20 | `messages::NoteDuration` | Python numeric or unit-string duration. Keep typed elapsed/beat alternatives; verify numeric duration interpretation with and without bpm. No Rust unit-string parser. |
| R21 | `messages::ValueError` | Python TypeError/ValueError. Verify malformed values fail before encoding or sending and retain useful field/reason information. Do not add one Rust exception type per Python exception. |
| R22 | `midi::Error` | Python MIDI exception classes. Audit operation category and underlying cause, including Shared retained errors from the diagnostics change. Preserve the approved Result/exception adaptation. |
| R23 | `midi::InputErrors` | Python Input.with_errors() iterator. Verify accepted-prefix draining, diagnostic delivery and terminal-error behavior. Keep a named Rust iterator and Python generator. |
| R24 | `midi::OutputValue` | Python send's heterogeneous arguments. Keep the enum; verify message/note/rest/delay/packet support and validation before sending. |
| R25 | `midi::Recording` | Python Input.record() list. Keep the Rust iterable container; verify record order, declarations, errors, timing, final diagnostics and direct playback. |
| R26 | `routing::RouteReport` | Python Report.routes currently contains dictionaries. Subject to D2, expose the existing Rust snapshot as a Python RouteReport and migrate dictionary consumers. |
| R27 | `smf::Record` | Python Message, Meta, SysExFragment or UnknownChunk. Keep the enum; preserve track/delta context, fragmented SysEx, unknown data and file ordering. Retain low-level smf.Event as previously agreed. |
| R28 | `transport::Error` | Python MIDI exception categories. Map together with R22, preserving capability/state/resource/worker/compound-send distinctions and nested causes. Process-isolation-only failures remain Python-specific. |
| R29 | `transport::Packet` | Python bytes or midi.UmpPacket. Keep the enum; verify protocol checks and preservation of raw packets, including fragmented byte traffic. |
| R30 | `tuning::TuningMessage` | Concrete Python tuning messages. Keep the enum; verify every supported request/reply, checksum status and native value. Keep tuning.decode() for single messages. |
| R31 | `tuning::TuningValue` | Python Pitch/NoChange. Keep the enum; verify NoChange is preserved distinctly from a numeric pitch through building, encoding and decoding. |
| R32 | `ump::AssemblyItem` | Python AssembledData or AssemblyDiagnostic. Keep the enum; verify group/stream addressing, truncation detail, source changes and reset behavior. |
| R33 | `ump::DecodeOutput` | Python Message or DecodeDiagnostic. Keep the enum; verify unsupported/malformed words, diagnostic contents and native value widths. |
| R34 | `ump::UmpMessage` | Concrete Python UMP/message classes. Keep the enum; verify unified MIDI 1/2 messages, explicit conversion and fallible encoding of unresolved channels. |
| R35 | `universal::UniversalMessage` | Concrete Python universal messages. Keep the enum; verify supported cases, device IDs, payload fields and byte round-trips. |

Use existing byte fixtures and interpreter/assembler regressions first. Add only
missing cross-language behavior cases. Shared Rust code reduces duplication but
does not prove that Python accepts, returns or exposes the same values.

## Slice 2: Options, defaults and limits

Keep Rust options/builders and Python keywords. Make a field-by-field contract
for each entry, using current Python names and the existing approved defaults.
Fix divergent values or omitted controls, rather than introducing Python Options
classes. Review boundary values, invalid combinations, cancellation, capacity
limits and the point at which validation occurs.

| ID | Rust path | Python counterpart and required check |
| --- | --- | --- |
| K01 | `ci::Options` | ci.browse keywords. Match cancellation, SysEx limits and fresh-session driver behavior. |
| K02 | `inspection::ComparisonOptions` | compare_files keywords. Match exact/tolerant/ignored timing, explicit source/clock mappings, limits and bounded differences. |
| K03 | `inspection::MonitorOptions` | Monitor/run keywords. Match filtering, detail limits, note-state collection and opt-in metrics; filters must preserve diagnostics. |
| K04 | `inspection::Options` | inspect_file keywords. Match payload_limit, preserve_unknown and detail_limit, including default values. |
| K05 | `midi::PlaybackOptions` | Output.send keywords. Match origin, mappings, cancellation, rounding and metrics. Retain the approved persistent Rust with_playback builder versus per-call Python keywords, and explain that lifetime difference. |
| K06 | `routing::PreviewOptions` | preview_file keywords. Match queue/event/byte/payload/detail budgets and incomplete-input handling. |
| K07 | `events::with_gap` | Python item-or-float iterator and Rust GapItem<T> with typed Duration. SysEx uses ordinary record/send and strict file helpers. Preserve whole-message sends and no automatic retries. |
| K08 | `transport::InputOptions` | Input constructor/builders. Compare controls reachable through high-level Rust midi::Input, including raw/callback mode and receive budgets. Low-level transport options are not a replacement high-level API. Keep Python isolation explicit. |
| K09 | `transport::OutputOptions` | Output constructor/builders. Match backend/protocol selection through midi::Output; keep channel/tempo/port selection in the high-level API and Python isolation separate. |

Previously approved choices are constraints, not new questions: lazy construction,
terminal close, in-process defaults, typed Rust durations, persistent Rust playback
configuration and live drivers owning their clocks all remain in force.

## Slice 3: Equivalent operations and canonical paths

Make the everyday operation obvious in both guides. Retain useful advanced Rust
streaming and clock/sink operations without forcing their implementation types
into basic Python examples. They must use the same underlying behavior, not grow
independent implementations for testability.

One direct naming fix is appropriate: put exact timestamp conversion on Rust
Timestamp as seconds(), corresponding to Python's seconds property. Move the
existing implementation, migrate its Rust and binding callers and remove the
superseded public free function. Do not retain both public spellings.

| ID | Rust path | Python counterpart and planned resolution |
| --- | --- | --- |
| E01 | `events::MonotonicClock` | Python's internal playback clock. Retain Rust's clock implementation for advanced PlaybackClock users; ordinary users get automatic clocks in both languages. time.monotonic_ns() alone is not a full replacement for its waiting/cancellation contract. |
| E02 | `events::play` | Output.send(). Keep the low-level Rust clock/sink engine; Output shares its planner, clock, cancellation and report semantics. Document the advanced boundary, without adding a second Python playback API. |
| E03 | `events::send_message` | Output(message). Keep the lower-level Rust connection helper; verify expansion, validation and send-progress behavior agree with high-level sending. |
| E04 | `events::timestamp_seconds` | Timestamp.seconds. Replace the public Rust function with Timestamp::seconds() as described above, retaining exact Rational arithmetic and checked overflow. |
| E05 | `file_io::Source` | Python path/binary-reader handling. Keep the advanced Rust bounded-reader implementation; verify resource limits and offsets through the public file operations. Do not introduce a Python Source wrapper. |
| E06 | `gm::decode_extended` | gm.decode(). Consolidate under D1 with R12/R13; include max_entries and malformed/unknown message behavior. |
| E07 | `inspection::compare` | compare_files accepts binary readers in Python. Retain Rust compare(Read, Read, ...) and compare_files(path, path, ...) as the existing idiomatic streaming/file pair. Verify both use identical comparison rules. |
| E08 | `inspection::inspect` | inspect_file accepts a binary reader in Python. Retain Rust Read/path entry points and verify the same report and failure behavior. |
| E09 | `routing::preview` | preview_file accepts a binary reader in Python. Retain Rust Read/path entry points and verify the same route preview, capacity and failure results. |

The seven module mappings below already have canonical high-level Rust paths:
midi.rs publicly exposes backend/port/protocol selection, and events.rs exposes
ReceivedPacket. Do not move transport internals or add parallel wrapper types.
Use the canonical paths in everyday examples and describe transport as advanced.

| ID | Advanced Rust path | Canonical user-facing mapping |
| --- | --- | --- |
| M01 | `transport::Backend` | Python midi.Backend and Rust midi::Backend. Verify supported backend values and capability failures per platform. |
| M02 | `transport::Clock` | Python events.Clock. This is not merely another import path: Rust's native descriptor must be converted to the semantic events::Clock. Verify identity, units, mode, origin and resolution; keep the raw descriptor advanced. |
| M03 | `transport::Port` | Python midi.Port and Rust midi::Port. Verify direction, protocol, backend ID/name and selection semantics. |
| M04 | `transport::Protocol` | Python midi.Protocol and Rust midi::Protocol. Verify byte/UMP meanings and default selection. |
| M05 | `transport::ReceivedPacket` | Python events.ReceivedPacket and Rust events::ReceivedPacket. Verify sequence, optional time and conversion from native clock descriptor to semantic Clock. |
| M06 | `transport::TimestampMode` | Python midi.TimestampMode and Rust midi::TimestampMode. Verify disabled/absolute modes and timestamp availability. |
| M07 | `transport::compiled_backends` | Python midi.compiled_backends() and Rust midi::compiled_backends(). Verify values on the same build; do not require macOS, Windows and Linux to expose identical backend lists. |

## Slice 4: Verification, examples and final accounting

1. Update Python stubs/bindings, Rust docs and both example documents for the
   actual migrations. Keep the first examples short: automatic clock creation,
   Input/Output and concrete messages. Introduce sum types, readers and options
   only where the operation needs them.
2. Reuse device-free tests for message bytes, metadata, stream decoding, file
   records, defaults, reports, timing, lifecycle and failures. Exercise the Python
   boundary as well as Rust, especially conversions and returned snapshots.
3. For GM, cover modes and extensions through the same decoder, unknown input,
   malformed recognized input and the entry limit. For route reports, cover
   admitted/confirmed/rejected counts and detached snapshot behavior. For
   Timestamp.seconds, cover exact fractions, disabled timing and overflow.
4. Compile Rust examples without executing device operations. Run the required
   Python checks when Python/binding behavior changes, current-toolchain Rust
   tests/Clippy/format checks and the strict documentation build. Use CI for other
   platforms and old compiler versions; do not install or run old Rust locally.
   Device-free success does not establish physical-device qualification.
5. Reconcile every ID against the final sources. Refresh public-symbols.txt only
   as part of the later implementation, keep it uncommitted, and distinguish
   verified language adaptations from actual unresolved behavior. Do not claim
   full method/property parity from this module-scope inventory.
6. Commit and push coherent changes, migrating all callers of each superseded
   interface in the same change. No compatibility aliases or unrelated cleanup.

The accounting must cover R01–R35, K01–K09, E01–E09 and M01–M07: exactly 60 source
entries. The GM decoder is deliberately referenced by several IDs because one
coherent change closes all of those mappings; it is not several independent jobs.

## Approved design decisions

### D1. Approved: One GM decoder and result family

Recommendation: make Rust gm::decode cover all six Python result classes, return
one gm::GmMessage enum, and remove the public ModeMessage/ExtendedMessage split
and decode_extended entry point. Use an explicit max_entries argument in Rust,
corresponding to Python's keyword with its existing default of 1024. A single
limit does not justify another options class. Update all callers, including
interpretation code that currently recognizes modes through gm::decode.

This gives both languages one operation and complete result coverage, but changes
the Rust decoder signature and exhaustive matches. Keeping the split avoids that
migration but leaves the confirmed operation mismatch. Approve the consolidated
decoder, its result name and explicit Rust limit argument before code changes.

### D2. Approved: Typed per-route report snapshots in Python

Recommendation: expose routing.RouteReport in Python with read-only admitted,
confirmed and capacity_rejections properties, and change Report.routes to return
a dictionary of these snapshots, matching Rust's existing value type. Replace
dictionary indexing in callers and examples; do not support both result shapes.

This makes the report discoverable and typed, but changes access from
report.routes[name]["confirmed"] to report.routes[name].confirmed and introduces
one public Python class. Keeping dictionaries is simpler in symbol count and can
remain a documented representation adaptation, but retains weaker field typing.
Approve the snapshot shape before changing the binding.

### D3. Approved: Retain idiomatic representations and advanced reader operations

Recommendation: resolve the remaining mappings through equivalent behavior and
documented language adaptations, retaining Rust enums/options and its existing
advanced clock/sink and Read-based operations. Do not create matching Python
wrapper types or hide useful Rust operations just to remove unmatched symbols.

This follows the existing parity policy and preserves simple Python examples.
It means the finished inventory will still contain different public symbols,
clearly marked as verified adaptations. Requiring identical symbol sets would
instead require a broader redesign and more user-facing types. Confirm that
behavioral parity, rather than eliminating these valid representation entries,
is the completion criterion for this batch.


## Implementation audit

The remaining representations were checked at their Rust definitions, Python
stubs and binding conversions, then exercised by the existing device-free suites.
The table names test files under crates/middy/tests/ and test/ unless a core
unit-test location is stated. These checks establish the listed operations and
boundaries, not an exhaustive method-by-method audit or physical-device support.

Fixed entries replace the old API without aliases. Adaptation entries retain the
approved language representation. Exception entries retain previously approved
language-specific behavior.

| ID | Outcome | Source/binding review and regression evidence |
| --- | --- | --- |
| R01 | Adaptation | archive.rs / src/archive.rs; archive.rs and test_archive.py cover independent v1 bytes, unknown records, occurrence metadata and trailers. |
| R02 | Adaptation | ci.rs / src/ci.rs; ci.rs and test_ci.py cover management bodies, versions, extensions, malformed input and shared vectors. |
| R03 | Adaptation | ci.rs / src/ci.rs and message_metadata.rs; envelope validation and explicit message transformations retain metadata; bare SysEx decoding interprets payload. |
| R04 | Adaptation | ci.rs / src/ci.rs; ci.rs and test_ci.py cover actions/observations, pending sends, early replies and loss. |
| R05 | Adaptation | codecs.rs / src/messages.rs; byte_codec.rs and test_codecs.py exercise incremental framing and diagnostic ordering. |
| R06 | Adaptation | controls.rs, event_views.rs / src/controls.rs; event_views.rs, test_controls.py and test_event_views.py cover observations and boundaries. |
| R07 | Adaptation | live.rs, input_api.rs / src/live.rs and _clients.py; live.rs, coremidi_boundary.rs and test_live.py cover packets, filters and accepted-prefix diagnostics. |
| R08 | Adaptation | event_views.rs / src/event_views.rs and profile_engines.rs; event_views.rs and test_event_views.py cover all merged owner families and reset context. |
| R09 | Adaptation | events.rs / src/event_values.rs; message_metadata.rs, byte_codec.rs, ump.rs and test_messages.py cover dispatch, wire values and metadata. |
| R10 | Adaptation | playback.rs / src/live.rs; live.rs and test_live.py cover cancellation, confirmed progress, causes and timing. |
| R11 | Adaptation | live.rs / src/live.rs maps Loss, Malformed, ClockChange, Truncated to loss, malformed, clock_change, truncated; live.rs and test_live.py exercise the diagnostic paths. |
| R12 | Fixed | gm.rs and gm_extensions.rs now return GmMessage; profile_engines.rs and test_profile_engines.py cover extended cases and limits. |
| R13 | Fixed | gm.rs and src/profiles.rs use the same decoder; profiles.rs and test_profiles.py cover all three mode cases and reset policy. |
| R14 | Adaptation | gm.rs / src/profiles.rs; profiles.rs and test_profiles.py verify exact names, result families, bank addresses and ordering. |
| R15 | Adaptation | inspection.rs / src/inspection.rs parse_format; inspection.rs, comparison.rs and test_inspection.py cover all four explicit formats and argument failures. |
| R16 | Adaptation | messages.rs, gm.rs, mpe.rs / src/messages.rs and profiles.rs; byte_codec.rs, message_metadata.rs and test_messages.py cover expansion and metadata. |
| R17 | Adaptation | messages.rs / src/messages.rs; byte_codec.rs, ump.rs, test_messages.py and test_ump.py cover defaults, boundaries and MIDI versions. |
| R18 | Exception | sequence.rs / src/messages.rs, src/units.rs and _clients.py; sequence.rs and test_output.py verify exact beats/tempo. Unit strings remain Python-only. |
| R19 | Adaptation | messages.rs / src/messages.rs and event_values.rs; message_metadata.rs and test_event_views.py cover transformations, source/span validation and equality semantics. |
| R20 | Exception | sequence.rs / src/messages.rs, src/units.rs and _clients.py; sequence.rs and test_output.py cover typed beats/time and Python numeric duration interpretation. |
| R21 | Adaptation | messages.rs ValueError and binding value_error helpers; byte_codec.rs, test_messages.py and test_profiles.py cover pre-send rejection and checked fields. |
| R22 | Fixed | midi.rs / native error mapping and _clients.py; midi core unit tests, transport_tests.rs, coremidi_boundary.rs, test_contract.py, test_testing.py and test_live.py cover categories, nested compound causes and retained Shared errors. |
| R23 | Adaptation | input_api.rs / _clients.py; coremidi_boundary.rs and test_live.py cover raw/decoded iteration, callbacks, closure and terminal overflow after an accepted prefix. |
| R24 | Adaptation | output.rs / _clients.py and src/messages.rs; output core unit tests and test_output.py cover heterogeneous values, channels, timing and cleanup. |
| R25 | Adaptation | input_recording.rs / _clients.py; input core unit tests and test_live.py cover memory/file capture and playback. Retained output reuse is tested separately from temporary destruction. |
| R26 | Fixed | src/routing.rs exposes frozen RouteReport values; routing.pyi and _routing_cli.py migrated. test_routing.py verifies detached counts, read-only properties and rejected admission; routing.rs covers confirmation/failure. |
| R27 | Adaptation | smf.rs / src/files.rs; files.rs, file_timing.rs, test_files.py and test_file_timing.py retain tracks, metadata, fragments and unknown chunks. |
| R28 | Fixed | transport/mod.rs and midi.rs / native error mapping; midi core unit tests, transport_tests.rs, coremidi_boundary.rs, test_testing.py and test_contract.py cover structured failures. Worker now maps consistently to WorkerError; compound errors preserve typed __cause__. Python process-isolation errors remain exceptions to parity. |
| R29 | Adaptation | transport/mod.rs / src/packets.rs; coremidi_boundary.rs, test_packets.py and test_live.py cover bytes, UMP, fragments and protocol rejection. |
| R30 | Adaptation | tuning.rs / src/tuning.rs; tuning.rs and test_tuning.py cover shared wire forms, explicit decode and checksum status. |
| R31 | Adaptation | tuning.rs / src/tuning.rs; tuning.rs and test_tuning.py preserve Pitch/NoChange through encoding and bounded generation. |
| R32 | Adaptation | ump assembly and event_views.rs / src/ump*.rs; ump.rs, event_views.rs, test_ump.py and test_event_views.py cover group/stream bounds and retained truncation detail. |
| R33 | Adaptation | ump decoder / src/ump*.rs; ump.rs and test_ump.py cover word framing, diagnostic values and unknown packets. |
| R34 | Adaptation | ump message enum / src/messages.rs and ump*.rs; ump.rs, byte_codec.rs and test_ump.py cover native widths and explicit conversion/encoding failure. |
| R35 | Adaptation | universal.rs / src/profiles.rs; profiles.rs and test_profiles.py verify explicit interpretation, supported values and shared wire vectors. |
| K01 | Adaptation | ci.rs / ci.py and src/drivers.rs; ci.rs core tests and test_ci_browser.py exercise fresh sessions, cancellation, UMP assembly and loss. |
| K02 | Fixed | comparison.rs / src/comparison.rs; renamed Rust sources/clocks to source_map/clock_map. comparison.rs and test_comparison.py cover mapping, tolerances, limits and file completion. |
| K03 | Adaptation | inspection.rs / src/inspection.rs and inspection.py; inspection.rs core tests, test_inspection.py and test_monitor.py cover filters, typed callbacks, snapshots and opt-in metrics. |
| K04 | Adaptation | inspection.rs / src/inspection.rs; inspection.rs and test_inspection.py cover positive limits, prefix reports and preservation. |
| K05 | Exception | output.rs / _clients.py; output core replay_options_persist test and test_output.py cover repeated playback. Persistent Rust builder configuration versus per-call Python keywords remains approved. |
| K06 | Adaptation | routing.rs / src/routing.rs; routing.rs, test_routing.py and test_patch.py cover budgets, preview and confirmed/uncertain progress. |
| K07 | Adaptation | events.with_gap / events::with_gap insert waits, including after the final item; ordinary Output.send owns scheduling and confirmed progress. |
| K08 | Exception | input.rs, input_api.rs, transport/mod.rs / _clients.py; coremidi_boundary.rs and test_live.py cover reachable high-level raw/callback/filter/limit operations. Python isolation remains separate. |
| K09 | Exception | output.rs, transport/mod.rs / _clients.py; output core tests, coremidi_boundary.rs and test_output.py cover selection, protocol, defaults and closure. Python isolation remains separate. |
| E01 | Adaptation | playback.rs uses monotonic time and cancellable waits; src/live.rs supplies Python host timing. live.rs and test_live.py cover scheduling/cancellation, not merely clock reads. |
| E02 | Adaptation | output.rs delegates to playback::play; _clients.py delegates to native playback. Output core tests and test_output.py cover origin, progress, empty playback and retained-output reuse. |
| E03 | Adaptation | live.rs send_message and output.rs share codec expansion/transport sends; live.rs, output core tests and test_live.py cover no implicit conversion and partial handoff. |
| E04 | Fixed | Timestamp::seconds replaces timestamp_seconds, including all core/binding callers. archive.rs and test_archive.py cover exact fractions; disabled Rust clocks now return InvalidInput. Common Rational tests cover arithmetic overflow. |
| E05 | Adaptation | file_io.rs / src/files.rs; files.rs and test_files.py cover bounded reads, borrowed-reader ownership, short writes and failures. |
| E06 | Fixed | Only gm::decode is public; extension parsing is private. profiles.rs, profile_engines.rs and their Python counterparts cover both mode and extension families. |
| E07 | Adaptation | inspection::compare_files opens files and delegates to compare; src/comparison.rs calls the same core for paths/readers. comparison.rs and test_comparison.py exercise reports and mappings. |
| E08 | Adaptation | inspection::inspect_file delegates to inspect; src/inspection.rs shares that engine. inspection.rs and test_inspection.py exercise all formats and file errors. |
| E09 | Adaptation | routing::preview_file delegates to preview; src/routing.rs calls that engine for Python paths/readers. routing.rs and test_routing.py exercise incomplete files and retained progress. |
| M01 | Adaptation | midi.rs canonical re-export / src/packets.rs backend mapping; test_contract.py and test_packets.py check public values and native selection validation. |
| M02 | Adaptation | transport::Clock is an advanced native descriptor; input.rs and src/packets.rs convert native generations to events::Clock units/origin/mode. live.rs, coremidi_boundary.rs and test_live.py cover clock provenance. |
| M03 | Adaptation | midi.rs re-export / port binding; transport selection unit tests and test_contract.py cover strict handles and ambiguous/missing names. |
| M04 | Adaptation | midi.rs re-export / src/packets.rs; coremidi_boundary.rs and test_live.py exercise both protocol paths. |
| M05 | Adaptation | events.rs re-export / src/packets.rs; coremidi_boundary.rs and test_live.py cover sequence, optional time, raw fragments and semantic-clock mapping. |
| M06 | Adaptation | midi.rs re-export / src/packets.rs; archive.rs, test_archive.py and test_packets.py cover disabled timing, clock units and typed values. |
| M07 | Adaptation | midi.rs re-export / src/packets.rs; transport resolve_backend uses the compiled list. Platform-dependent backend availability remains explicit; no cross-platform runtime equivalence is inferred from the macOS checks. |

### Options contract checked

| Entry | Shared fields and defaults |
| --- | --- |
| K01 | cancel: fresh uncancelled token; message_limit: 1,048,576; byte_limit: 8,388,608. |
| K02 | payload_limit: 1,048,576; detail_limit: 128; preserve_unknown: false; ignore_timestamps: false; tick_tolerance: 0; source_map/clock_map: empty. |
| K03 | protocol: MIDI 1; types: empty; channel/group: unset; detail_limit: 128. Run duration/cancel/callback are optional in Python and explicit Rust arguments; metrics: false. Note-state interpretation belongs to the monitor, display history to the CLI. |
| K04 | payload_limit: 1,048,576; preserve_unknown: false; detail_limit: 128. |
| K05 | origin: inferred; mappings: empty; cancellation: initially clear; allow_rounding/metrics: false. Configuration lifetime is the approved language exception. |
| K06 | event_limit: 128; byte_limit: 8,388,608; payload_limit: 1,048,576; detail_limit: 128. |
| K07 | gap: explicit; message_limit: 256; byte_limit: 8,388,608; payload_limit: 1,048,576. Duration versus Python number/unit string is the established adaptation. |
| K08 | protocol: MIDI 1; backend: automatic; timestamps: absolute; receive bytes: 8,388,608. High-level event_limit: 128; sysex_limit: 1,048,576; no channel filter/port pattern; raw and callback delivery opt-in. Rust callbacks use a typed builder, Python connection callbacks. |
| K09 | protocol: MIDI 1; backend: automatic; channel/port/bpm: unset. Unassigned MIDI 1 channels resolve to 1; an explicitly configured output channel overrides assigned MIDI 1 message channels. |

### Additional decisions resolved during implementation

- Approved: reject disabled-clock Timestamp::seconds() with InvalidInput, matching Python's
  rejection of disabled-clock Timestamp construction.
- Approved: a retained output stays open after play() in both languages, until its normal
  close/context/destruction cleanup. Inspection confirmed Python already did this:
  its temporary-output regression observes destruction, not play() closing the
  output. A new Python reuse regression makes the distinction explicit.
- Approved: native and simulated Worker failures both map to Python WorkerError;
  ResourceError is reserved for explicit resource/capacity failures.
- CompoundSendError now retains its typed underlying __cause__, matching Rust's
  nested error information, rather than retaining only its text.

### Verification

- Full Python suite: 561 passed after rebuilding the extension.
- Rust workspace: 226 passed, five opt-in tests ignored, using the current compiler.
- All 33 Rust guide snippets compiled with no_run wrappers; no guide I/O executed.
- Strict core Clippy, cargo fmt, Python Ruff, 3.13 pyupgrade and ty check middy passed.
- Strict documentation build: no issues.
- Superseded GM decoder/result names and timestamp_seconds have no remaining
  source, example or test callers.
- All 60 mapping IDs have an outcome and evidence; public-symbols.txt was
  regenerated from Python declarations and fresh Rust documentation and remains
  untracked.

No physical devices, old Rust toolchains, publication or platform runtime
qualification were used. Workspace compilation reports existing Python-binding
warnings unrelated to these changes; strict core Clippy is clean.

## Method-level input follow-up

The user subsequently requested the five concrete method/behavior gaps remaining
after the module-scope audit. All six design recommendations were approved before
implementation: independent background recording with a shared recorder handle,
host ingress activity (Python seconds/Rust Instant), receive-only cancellation,
explicit closure errors, terminal callback failure, and retryable recorder stops.

Implementation reuses the archive recorder and Rust's input dispatcher, with a
shared internal input owner so controls can stop recording while a callback runs.
The dispatcher releases the input lock before waiting for ingress or calling user
code. Native ingress and decoded delivery remain bounded. No new transport,
scheduler, dependency or Python runtime isolation mechanism was introduced.

Coverage includes recording without receive calls; callback recording success and
panic; receive-capacity independence; no retroactive recording of decoded pending
messages; exact cancellation/closure behavior; retained activity; callback failure
before and after connection setup; original exception identity; and retry after
recorder timeout and input close. Both guides, the API contract and README were
updated. public-symbols.txt remains uncommitted.

Final verification:

- Full Python suite: 567 passed without warnings.
- Rust workspace: 233 passed, 5 opt-in tests ignored, using the current compiler.
- All 35 Rust guide snippets compiled with no_run wrappers; no guide I/O executed.
- Strict core Clippy, cargo fmt, Python Ruff, 3.13 pyupgrade and ty check middy passed.
- Strict documentation build: no issues.
- Concurrency regressions cover exception normalization outside native callback
  locks, overlapping automatic/explicit close, and callable-destructor reentry.
- Closed inputs retain stream/clock metadata and host ingress activity; cancellation
  preserves the accepted prefix and leaves the input reusable.

Physical-device and cross-platform runtime qualification remain separate; no old
Rust toolchain was run locally. Additional work beyond the prompt: None.


## Unified output operation follow-up

The user approved removing Python Output.__call__ and consolidating play/send into
send in both languages. They also approved rejecting mixed timed/untimed sources,
always returning a report, and adapting Rust source forms through OutputSource
while removing send_all. Python uses variadic arguments; Rust accepts values,
iterables and tuples of independent sources. Readers remain streamed and empty
sends remain device-free. Direct messages ignore timing metadata for scheduling.
Notes, rests and delays retain exact cumulative timing, with scalar rounding
included in the shared report. Both guides and workflow callers were migrated.
Python testing.Output uses send in place of __call__, matching its Rust sink.

Approved: Python preserves the original typed exception and attaches
report/completed_events after execution begins. Failures are not wrapped in
PlaybackError. Rust retains its report-bearing Failure with the typed cause.

Additional work beyond the prompt: None.

Verification of the working implementation:

- Full Python suite: 573 passed, with no thread warnings.
- Rust workspace: 236 passed, 5 opt-in tests ignored.
- All 36 Rust guide blocks compile without executing guide I/O; all 36 Python
  guide blocks parse.
- Strict core Clippy, cargo fmt, Python Ruff, ty and Python 3.13 pyupgrade pass.
- Strict documentation build passes. No physical device traffic was used.
- public-symbols.txt remains untracked and is excluded from the implementation
  commit.
