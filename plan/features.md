# Suggested features and integrations for meedee

Updated 2026-10-18 after reviewing uFor, arpeg, enveLFO and enge. These are
proposals for discussion, not implementation commitments or a replacement for
[the existing plan](meedee.md). Priorities reflect practical usefulness to MIDI
programmers and hobbyists, rather than a fixed release sequence.

Additional work beyond the prompt

None.

## Existing capabilities and ownership

meedee already provides immutable Rust/Python messages, MIDI 1 and UMP codecs,
fine controls and RPN/NRPN, MTS, General MIDI, selected universal messages, MPE
helpers and tracking, time-code assembly, standard files, exact event archives,
typed live I/O, recording/playback and MIDI-CI management. Build on these rather
than implementing them again.

The surrounding projects already supply much of the musical layer:

| Project | Existing responsibility | Boundary to preserve |
| --- | --- | --- |
| uFor | Portable musical definitions, tuning/scales and Scala conversion, bindings, sequence planning and reference calculations | Definitions do not imply live device adapters or complete runtime support |
| arpeg | Python/Rust pattern execution, rhythms, swing/groove, phrase capture, musical transformations, note scheduling and MIDI clock following | Musical decisions and owned-note policy remain in arpeg |
| enveLFO | Python/Rust envelopes, LFOs, control graphs, delayed events and retained state | Hosts supply clocks, observation cadence and output delivery |
| enge | Synthesis, sampling, modulation realization and audio voice execution | Hosts supply device I/O; audio lifetime is not MIDI note-state observation |
| meedee | MIDI protocols, device transport, files, timestamp identity and protocol diagnostics | No second composition, pattern, modulation or audio engine |

uFor's [capability matrix](../../ufor/doc/capabilities.md) distinguishes definitions,
reference behavior and host work. arpeg's [engine description](../../arpeg/README.md)
and [enveLFO bridge contract](../../arpeg/doc/envelfo.md), and enveLFO's
[ownership audit](../../envelfo/doc/extraction-audit.md), describe execution already
available. These sibling-checkout links record the local review context; they
are not runtime dependencies or assumptions about installed package locations.

Keep meedee's core MIDI behavior shared in Rust and exposed through Python.
Integrations should be optional, with placement decided for each concrete use
case. Do not make the standalone Rust library require Python or make ordinary
Python MIDI use import every musical engine. No new mandatory dependencies or
workspace restructuring are proposed here.

CLI, TUI, GUI and notebook tools may present the same underlying operations.
Start with whichever interface serves a selected workflow; there is no need to
build every interface. A Pi Zero 2 W remains a useful headless target, while
heavier visualization can run on a desktop.

## Independent meedee features

### 1. MIDI inspector and diagnostic monitor

**Implemented:** source-local timing diagnostics, optional file inspection and
validation CLI, and passive single-input monitoring with bounded display history,
filters, existing control/velocity interpretation and explicit loss/failure
reporting. Broader interactive views remain proposals.

**Use:** Explain unexpected keyboard, synthesizer or script behavior without
writing a fresh monitoring script each time.

Summarize typed events, raw packets, source identity, timestamp domains and
diagnostics. Reuse existing interpretation for fine CC, parameter changes,
tuning messages and MPE, while keeping original traffic available. Useful
views include filters, a frozen snapshot, controller traces and note activity.

A CLI or TUI is a natural first interface; a GUI or notebook can follow using
the same inspection logic. Bound display history, show losses and unknown state,
and send no traffic by default. Capture export should use existing recording
and archive facilities.

### 2. Capture comparison and file validation

**Implemented foundation:** `meedee.inspection.inspect_file` and the shared Rust
`inspection::inspect` summarize archive/SMF/SYX/Clip files and report verified
prefixes, completion, capture gaps/discontinuities, source/clock descriptors and
opaque content. Same-format comparison now supports explicit source/clock maps,
timing policies and bounded differences through Rust/Python APIs and the optional
CLI. Cross-format musical comparisons and interactive editors remain separate.

**Use:** Determine whether changing a script altered its messages, ordering or
timing, and diagnose damaged or unsuitable MIDI files.

Compare captures with explicit source matching, timestamp tolerances and ignored
fields. Explain gaps, clock discontinuities, protocol differences and export
losses. Reuse readers and codecs; compare raw records when interpretation is
unavailable. A tolerance is a user-selected comparison policy, not proof that
a difference is musically harmless.

Expose reusable Rust/Python reports and an optional CLI. This is inspection and
regression support, not a new musical sequence editor.

### 3. Protocol-aware routing, merging and splitting

**Use:** Connect controllers and instruments, combine sources or run a small
headless MIDI patch bay.

Start with explicit routes, channel/group selection and remapping, merging and
fan-out. Preserve source identity, bound queues, report route-specific overload
and define equal-time ordering. Comparing timestamps from different sources
requires explicit clock mappings. Detect or prevent known routing cycles without
inventing a hop counter inside ordinary MIDI messages.

Musical transformations should be supplied by existing engines or approved
adapters. Do not recreate arpeg's scale mapping, voicing, swing or note scheduler.
Use uFor's composition/binding vocabulary where it fits rather than inventing
a second general graph or setup language. A transport-specific router can still
be independently useful without a uFor dependency.

Changing a route while notes are active requires an explicit ownership/cleanup
policy. No automatic replay after reconnect or channel-wide reset on a shared
destination. A CLI could run selected routes; a TUI/GUI could display them.

### 4. Observed MIDI state and explicit scoped cleanup

**Implemented observation:** shared Rust/Python bounded per-source/group/channel
note counts, native pedal values, channel-mode and loss/reset boundaries, file
inspection summaries and passive monitor views. Counts describe observed traffic,
not sounding voices. Cleanup generation and routing ownership remain proposals.

**Use:** Diagnose stuck notes and inspect what a source or route has actually
sent or received.

Extend protocol observation beyond existing MPE views where a concrete need
exists: active note instances, pedal knowledge and reset/loss boundaries. Reuse
existing trackers and established pairing policies before adding new state.
This is distinct from arpeg's generated occurrence ownership, uFor's semantic
trigger planning and enge's sounding voices or release tails.

An explicit cleanup plan could identify releases for traffic owned by one route.
Let callers inspect and send it. A targeted panic action may also be useful,
with its broader effects clearly described. NoteOff, pedal release, All Notes
Off and All Sound Off are different actions. Lost traffic and competing writers
limit certainty; cleanup must not become an implicit cancellation/disposal action.

### 5. Topology watching and explicit recovery

**Use:** Keep a long-running application understandable when devices appear,
unplug, reboot or change endpoint identity.

Expose bounded topology observations and support selecting fresh endpoints after
loss. Platform notification/discovery races require qualification. A visible
endpoint does not prove a responsive instrument, and names/backend IDs are not
durable physical identities.

An opt-in reconnect policy could restore a specifically selected route. Restoring
patches, tuning, controllers or notes is a separate action. Reset affected
interpreters after known loss; never silently replay uncertain operations.

### 6. SysEx transfer execution and diagnostics

**Use:** Back up or restore an instrument using documented transfer rules.

Build bounded transfer plans over existing SysEx files/codecs, deadlines and
request tracking. Plans may express device-specific pacing, chunks and expected
replies. Retain original bytes and show progress and uncertainty. Do not infer
arbitrary manufacturer protocols or retry uncertain state-changing commands.

Device descriptions and librarian organization belong to the integration work
below. meedee's independent contribution is transport and transfer execution,
plus generic packet inspection. Successful byte delivery does not establish
that an instrument accepted or completed a restore.

### 7. MIDI-CI browser and supported configuration

**Use:** Inspect supported device profiles and properties without hard-coding
every instrument capability.

Continue existing deferred Profiles and Property Exchange work once the required
normative documents are acquired. Reuse the current CI management/session path;
do not create another CI implementation or advertise unsupported categories.
A read-oriented browser can precede explicit configuration writes.

Dynamic CI information can complement uFor instrument descriptions. Neither
advertised properties nor a stored definition proves device health. Detailed
standards and physical interoperability evidence remain implementation gates.

### 8. Selected machine-control helpers

**Use:** Control compatible studio recorders and transport equipment.

Add typed values/codecs for a bounded MIDI Machine Control subset, with explicit
request/reply handling where required. Reuse existing time-code values and
obtain the normative command specifications before implementation. This is
protocol coverage, separate from arpeg's beat-clock follower and from a new
studio automation engine. A small interface could expose explicitly targeted
transport actions.

### 9. Deterministic transport tests and virtual-device tools

**Use:** Test applications without a physical synthesizer, and exercise real
integration separately when a native backend is available.

Expose a small transport test double for injected input, recorded output, loss,
disconnection and uncertain failures, reusing suitable existing fixtures.
Combine it with engine conformance cases without duplicating protocol parsers
or musical behavior. In-memory tests do not qualify drivers.

Optional manual tools could provide test-owned virtual endpoints where supported.
Reuse arpeg's existing keyboard/browser instrument for musical input where
appropriate instead of building another general-purpose playable instrument.

### 10. Timing and load diagnostics

**Use:** Establish whether a particular Pi, interface and application can handle
a workload before a performance.

Make suitable qualification measurements reusable: throughput, queue occupancy,
loss, scheduling lateness and process-tree resources. Include backend, mode and
clock conditions. Round-trip loopback timing is not one-way physical latency;
the latter needs additional instrumentation or a justified clock relationship.

Reuse arpeg/enveLFO performance work for their engines. meedee should measure
its transport and integration contribution. Any emitted MIDI needs explicitly
selected destinations and bounded runs. This does not replace open Windows,
Pi or physical-fault qualification.

## Integration work

### A. Use meedee for arpeg's MIDI device boundary

**Use:** Exercise meedee with a substantial real musical application while
keeping arpeg's existing engine and interfaces.

arpeg's [Rust live host](../../arpeg/crates/arpeg-midi/src/live.rs) currently uses
midir; its [Python MIDI host](../../arpeg/arpeg/midi.py) uses Mido/RtMidi. Propose
replacing those device paths with meedee after agreeing a bounded migration.
The resulting host should use one selected transport path, not retain parallel
production backends solely for compatibility.

Preserve held/latched/history behavior, expression ordering, bounded admission,
output pacing and owned-note cleanup. arpeg already follows MIDI clock, handles
transport and schedules notes; the migration should not move those decisions
into meedee. Preserve its existing CLI/browser roles rather than recreate them.

Map timestamp origins and units explicitly: meedee's signed native nanoseconds
and clock identity cannot simply be relabeled as arpeg's host-relative time.
Test chunk framing, callback/process-mode behavior, disconnects and uncertain
sends. Initial work can preserve current MIDI 1 behavior; MIDI 2/MPE expansion
requires a separate musical-policy decision.

### B. Adapt meedee events to uFor performance bindings and enge

**Use:** Drive the same instrument from a MIDI file or live input using declared
musical rules.

uFor's [performance binding](../../ufor/doc/performance-binding.md) defines channel
selection, repeated-key release policy, tuning and named controller conversion,
but leaves the live adapter to a host. Implement that missing execution boundary
using meedee's decoded events, preserving source identity, trigger ownership,
unknown state and loss/reset handling. Respect binding semantics instead of
adding another general mapping model.

[enge's MIDI importer](../../enge/enge/midi.py) currently uses Mido for a restricted
file-to-performance conversion. An integration could replace that parsing path
with meedee's file readers and reuse the same binding conversion for live input.
Do not change existing interpretation silently: its FIFO pairing, channel-based
parts, controller subset, bend assumptions and ignored program changes must be
reviewed against the selected binding behavior.

Channel numbering and conversions to exact frame boundaries need explicit tests.
Rounding belongs at the destination boundary and should be reported where
material. uFor semantic events and meedee protocol/archive events retain different
purposes; integration need not replace either representation. enge continues to
own audio realization and voice retirement.

### C. Realize enveLFO outputs as MIDI controls

**Use:** Modulate external instruments with existing envelopes, LFOs and control
graphs rather than add another modulation engine.

Map selected named outputs to existing meedee CC, fine-CC, parameter or appropriate
UMP messages. Agree ranges, quantization, observation cadence, update suppression
and output-rate limits. Omit no required command ordering, and report overload
rather than silently claim all intermediate values reached hardware.

Reuse the optional [arpeg/enveLFO bridge](../../arpeg/doc/envelfo.md) for their
combined musical behavior. It already separates scheduling and realization so
expression can precede NoteOn. Preserve that ordering rather than adding a
second bridge with different onset semantics.

The host owns clock mappings and merges engine deadlines with input and delivery.
enveLFO does not acquire MIDI clock; arpeg already has a particular clock-following
policy. Assess reuse of that policy before proposing a more general clock service.
Do not introduce another autonomous scheduler or advance a graph implicitly when
encoding a MIDI message.

### D. Connect uFor tunings to MTS, with optional enge audition

**Use:** Design a tuning once, audition it and prepare an explicit transfer.

**Implemented foundation:** the optional Python
[`meedee.ufor_tuning` bridge](ufor-tuning.md) previews selected keys and constructs
ordinary/banked per-key MTS messages with explicit preservation and error policy.
It opens no ports. Explicit transfer and optional enge audition remain separate
integration work.

Use uFor's existing tuning/scales and Scala conversion. Sample or resolve the
selected pitches into meedee's MTS values, reporting range limitations, unmapped
keys and protocol quantization. Additional keyboard-map formats should be separate
gap-driven additions to the appropriate owner, not a second Scala/tuning stack.

A CLI or GUI could compare intended and encoded pitches. Optional audition
should use enge's existing synthesis/sampling. Sending tuning requires an explicit
destination and action; it is not a side effect of previewing or loading a scale.

### E. Reuse uFor device descriptions, mappings and sequence plans

**Use:** Share meaningful instrument parameters and saved material across musical
applications without another format for the same information.

Build on uFor's parameter mappings, bindings, instrument definitions and partial
SysEx descriptions. Its [VL70m proof of concept](../../ufor/doc/vl70m-sysex-example.md)
already supports lossless dump inspection and bounded relocation; a complete
librarian score or universal device-description engine is not thereby implemented.
Use those established contracts and identify actual missing pieces before adding
schema or device codecs. meedee can supply protocol execution and transfer reports.

Controller learn could propose a supported uFor binding; pickup and hardware
feedback would need explicit runtime ownership and known/unknown target state.
Do not implement a second curve evaluator, parameter model or modulation graph.
Use enveLFO for applicable time-varying control execution.

For file editing and looping, adapt selected meedee records to uFor's existing
sequence planning where semantics fit. Its raw captures are inert, while semantic
triggers have ownership and cleanup rules. Do not blindly send every raw record
or infer sounding duration from a release. An editor/looper may be a future
integration client, not a second meedee composition or playback-planning engine.

## Interface opportunities

A monitor, capture checker, patch bay, tuning workbench or librarian could justify
a CLI, TUI or GUI. Notebook views can reuse inspection reports without mandatory
plotting dependencies. These are workflow clients of the libraries above.

Use existing arpeg interfaces for arpeggiation and enge for audio audition. No
new DAW, synthesizer, pattern engine, envelope engine or generic plugin framework
is proposed. Network bridges remain a separate future decision about transport,
clock relationships, security and recovery, rather than part of these integrations.

## Suggested order and decisions

| Stage | Candidate scope | Why this order |
| --- | --- | --- |
| First | Inspector and capture validation/comparison | Useful independently and supports integration regression evidence |
| Second | One bounded arpeg host migration | Tests meedee against an existing real application without new musical behavior |
| Third | uFor performance-binding adapter and enge input | Connects file/live MIDI to established instrument semantics |
| Fourth | enveLFO control output or uFor-to-MTS workflow | Choose one complete musical integration before several partial ones |
| As needed | Routing, topology and instrument transfers | Add concrete transport features demonstrated by those clients |
| In parallel when available | Platform/device qualification and normative document acquisition | Supplies evidence required for reliable live and standardized features |

These are candidate work packages, not new numbered slices or estimates. Agree
placement, dependencies, musical policies and the initial interface before
implementing one. Preserve language boundaries: Python integrations may use
uFor's Python models, while standalone Rust hosts use existing native engine
contracts or explicitly prepared data. Do not require Python just to interpret
a MIDI packet or run an existing Rust engine.

Core library imports must not start devices, audio, interfaces or network services.
The existing platform qualification gates remain release considerations. New
interfaces and integrations must expose losses, uncertainty and unsupported
capabilities rather than imply universal device support.
