# Suggested features and integrations for meedee

Updated 2026-10-21 against meedee and the sibling integration sources. Implemented
foundations are identified below; remaining proposals are for discussion, not
implementation commitments or a replacement for
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
build every interface. meedee targets macOS, Windows and Linux; headless and
interactive applications share the same core. No particular board or machine
is a project requirement.

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

**Implemented static foundation:** Rust/Python one-hop routes select sources and
channels/groups, remap channel-voice addresses and fan out in caller arrival
order. A bounded queue admits complete fan-outs, with per-route capacity and
confirmation reports. Archive preview and an optional one-input live patch-bay
CLI reuse this core. Routes remain fixed; loss/overload/failure stops the session.
Scheduled merging, dynamic routes, recovery and ownership-based cleanup remain
separate proposals. External physical feedback loops are not detected.

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

**Implemented observation:** Rust/Python complete-descriptor multiset snapshots,
bounded change samples/history, a caller-driven Python discovery poller, and
read-only `ports`/`watch-ports` CLI commands. Discovery failure or port-limit
overflow stops polling and retains the last complete snapshot. Polling is not
an atomic OS view and can miss transients. Native subscriptions, route recovery,
reconnection and MIDI-state restoration remain separate proposals.

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

**Implemented MIDI 1 foundation:** bounded complete-message transfer
plans, strict whole-file SYX validation, caller-driven paced Rust/Python sessions,
a synchronous Python output adapter, and separate preview/send CLI commands.
Progress distinguishes handoff from uncertain delivery; cancellation never
retries. Passive MIDI 1 capture now collects an explicit expected SysEx count
within an overall timeout, with bounded retention and exclusive post-success
SYX export through Rust/Python APIs and the `sysex-receive` CLI. Loss or decoding
failure stops capture; success does not infer device-specific dump completeness.
One-shot ordinary/banked MTS dump queries now reuse the existing reply tracker
and decoder through Rust/Python APIs, a caller-thread host and preview/live CLI.
Original reply bytes and checksum status survive export; uncertain sends never
retry. UMP, device-specific chunking, acknowledgements and other request/reply
protocols remain separate work requiring explicit rules.

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

**Implemented management browser:** bounded Rust/Python discovery followed by
sequential Product Instance ID inquiries, a typed MIDI 1 / UMP SysEx7 host, and
an explicit one-scan CLI. Local identity is supplied; the CLI chooses a fresh
random MUID. Existing management responses and collision invalidations are
preserved. Profiles, Property Exchange and configuration writes remain deferred.

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

**Implemented in-memory foundation:** public Rust/Python testing inputs and
recording outputs, caller-driven time, bounded drop-newest queues, explicit
loss/failure/close and accepted-before-error send simulation. Production codecs
and decoders are reused. Routing, transfer and CI conformance tests exercise
these doubles. Native virtual-device tools and driver qualification remain separate.

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

**Implemented observational foundation:** opt-in playback dispatch-lateness
histograms, recording admission/peak reservation metrics and passive monitor
host delivery rates/intervals. Public Rust/Python accumulators retain fixed
histograms and exact aggregates with bounded memory. Queued and in-flight writes
both count toward recording pressure. Process-tree resources and native/physical
qualification remain separate.

**Use:** Establish whether a particular host, interface and application can handle
a workload before a performance.

Make suitable qualification measurements reusable: throughput, queue occupancy,
loss, scheduling lateness and process-tree resources. Include backend, mode and
clock conditions. Round-trip loopback timing is not one-way physical latency;
the latter needs additional instrumentation or a justified clock relationship.

Reuse arpeg/enveLFO performance work for their engines. meedee should measure
its transport and integration contribution. Any emitted MIDI needs explicitly
selected destinations and bounded runs. This does not replace open Windows,
Linux or physical-fault qualification.

## Integration work

### A. Use meedee for arpeg's MIDI device boundary

**Use:** Exercise meedee with a substantial real musical application while
keeping arpeg's existing engine and interfaces.

**Implemented in arpeg:** its [Rust live host](../../arpeg/crates/arpeg-midi/src/live.rs)
and [Python MIDI host](../../arpeg/arpeg/midi.py) now use meedee/libremidi.
The committed sources at arpeg `ccf219a` contain both migrations; the checkout
also has unrelated ongoing edits, which this review does not qualify or change.
See [migration evidence](../../arpeg/doc/performance.md#meedee-host-migration-2026-10-18)
and [Python migration evidence](../../arpeg/doc/performance.md#python-meedee-host-migration-2026-10-19).
There is no remaining midir/Mido host replacement to implement here.

Preserve held/latched/history behavior, expression ordering, bounded admission,
output pacing and owned-note cleanup. arpeg already follows MIDI clock, handles
transport and schedules notes; the migration should not move those decisions
into meedee. Preserve its existing CLI/browser roles rather than recreate them.

The hosts use explicit host arrival time rather than relabel native timestamps.
Their framing, bounded admission, loss, output pacing and failure policies remain
arpeg responsibilities. MIDI 2/MPE expansion and additional physical/platform
qualification are separate from the completed MIDI 1 transport migration.

### B. Adapt meedee events to uFor performance bindings and enge

**Use:** Drive the same instrument from a MIDI file or live input using declared
musical rules.

**Implemented in enge:** at `8c93502`, `enge.performance.MidiConverter` executes
uFor's channel, repeated-key release, tuning and named-controller bindings over
decoded meedee messages. [File/live dispatch](../../enge/enge/midi.py) uses the
same converters. See [the binding contract](../../enge/doc/performance-binding.md).
The importer uses meedee for synchronous PPQN SMF 0/1, with explicit rational
timing/frame rounding. Caller-polled `LiveInput` assigns the caller's frame;
it does not pretend native timestamps are audio frames. This is no longer a
missing Mido-to-meedee migration or a missing performance-binding converter.

Channel numbering and conversions to exact frame boundaries need explicit tests.
Rounding belongs at the destination boundary and should be reported where
material. uFor semantic events and meedee protocol/archive events retain different
purposes; integration need not replace either representation. enge continues to
own audio realization and voice retirement.

### C. Realize enveLFO outputs as MIDI controls

**Use:** Modulate external instruments with existing envelopes, LFOs and control
graphs rather than add another modulation engine.

**Implemented in enveLFO:** at `b892317`, optional Python `envelfo.midi` and Rust
`envelfo-midi` map observed named values to meedee CC, fine-CC and RPN/NRPN Set
messages. Explicit ranges, integer quantization, wire-byte admission, confirmed
suppression and uncertain-send recovery are covered by
[the MIDI integration contract](../../envelfo/doc/midi.md). The mapper owns no
ports, clock or scheduler. UMP mapping remains outside that implemented scope.

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
It opens no ports. Bounded bulk transfer-plan composition and optional offline
CLI preview/exclusive SYX export now accept existing uFor Tuning JSON and Scala
sources. Exported files use the existing explicit transfer workflow; no second
sending host is added. Optional enge audition remains separate integration work.

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
| Implemented | Inspector, capture comparison/validation, note observation | Shared Rust/Python foundations and optional CLI |
| Implemented in siblings | arpeg MIDI hosts, enge performance conversion/input, enveLFO MIDI mapping | Existing owners supply musical execution; do not duplicate them |
| Implemented | uFor bulk tuning preview/export and explicit transfer/query tools | Source resolution stays in uFor; delivery stays in meedee |
| Implemented foundation | Static routing, polling topology, bounded diagnostics and in-memory transports | Recovery and ownership cleanup are not implied |
| Qualification gates | macOS/Windows/Linux backends, physical behavior and resource budgets | Build/software-loopback evidence cannot replace platform/backend evidence |
| Standards gates | Deferred CI Profiles/Property Exchange, selected machine control | Acquire normative documents before dependent protocol implementation |
| Optional proposals | Scoped cleanup, reconnect, interactive interfaces and audition | Require their own ownership, recovery or product decisions |

Implemented rows record source-level progress, not universal runtime support.
The remaining rows identify candidate work packages and qualification gates,
not new numbered slices or estimates. Agree placement, dependencies, musical
policies and the initial interface before implementing another feature.
Preserve language boundaries: Python integrations may use
uFor's Python models, while standalone Rust hosts use existing native engine
contracts or explicitly prepared data. Do not require Python just to interpret
a MIDI packet or run an existing Rust engine.

Core library imports must not start devices, audio, interfaces or network services.
The existing platform qualification gates remain release considerations. New
interfaces and integrations must expose losses, uncertainty and unsupported
capabilities rather than imply universal device support.
