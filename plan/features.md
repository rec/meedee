# Suggested features for meedee

Prepared 2026-10-18. This is a collection of proposals for discussion, not an
implementation commitment or a replacement for [the existing plan](meedee.md).
Priorities below express likely usefulness to a working MIDI programmer or
music-programming hobbyist, rather than a fixed release sequence.

Additional work beyond the prompt

None.

## Starting point

meedee already provides immutable Rust/Python messages, MIDI 1 and UMP codecs,
fine controls and RPN/NRPN, MTS, General MIDI, selected universal messages, MPE
helpers and tracking, time-code assembly, standard files, exact event archives,
typed live I/O, recording/playback and MIDI-CI management. Suggestions here build
on those capabilities rather than proposing them again. Profiles and Property
Exchange are existing deferred work, identified separately below.

Keep shared musical behavior in Rust and expose it through Python. Optional
CLI, TUI and GUI tools should use that behavior rather than acquire separate
implementations. Library use should continue to work without an interface,
a display, a Python interpreter for Rust applications, or a network service.
A Pi Zero 2 W should remain a useful deployment target; heavier visual tools
can run on a desktop.

The most promising first additions are a MIDI inspector, musical transformations,
explicit routing and controller mapping. Together they support both practical
troubleshooting and small custom musical instruments.

## First candidates

### 1. MIDI inspector and diagnostic monitor

**Use:** Discover why a keyboard, synthesizer or script is doing something
unexpected without writing a fresh monitoring script each time.

Provide a reusable inspection layer that summarizes typed events, raw packets,
source identity, timestamp domains and diagnostics. Optional interpreted views
could show paired CC values, parameter changes, tuning messages and MPE activity
while retaining access to the original traffic.

A CLI could list ports and produce readable or machine-readable observations.
A TUI could filter a live stream, freeze a view and inspect individual messages.
A later GUI could add controller plots and a keyboard display. These are choices
of presentation, not three interfaces that must all be built initially.

Show queue losses, incomplete state and unknown values prominently. Make display
buffering bounded so a busy MIDI stream cannot exhaust memory. Observation
should send no traffic by default. Exporting a capture should reuse the archive
and recording facilities already present.

### 2. Musical transformations and filters

**Use:** Turn a keyboard into a split/layer controller, adapt an awkward device,
or build a small MIDI effect in a few lines of Rust or Python.

Offer composable operations for channel/group remapping, note-range filtering,
transposition, velocity curves, controller remapping and value scaling. Support
both event streams and file workflows without rewriting the same transformation
for each one. The first version can be a small set of explicit operations;
a general expression language is unnecessary.

Stateful transformations must remember the output corresponding to each input
NoteOn so that a later NoteOff still releases the right note after settings
change. Overlapping repetitions of the same note, zero-velocity NoteOn, sustain,
filtered releases and MIDI 2 per-note expression need deliberate policies.
Define out-of-range behavior rather than silently wrapping pitches or values.

Useful later additions include scale constraints, chord voicing and controlled
humanization. Any randomness should permit a seed for repeatable file processing
and regression tests.

### 3. Explicit routing, merging and splitting

**Use:** Replace a small patch bay, combine controllers, distribute one keyboard
to several instruments, or build a headless Pi MIDI processor.

Provide a reusable router that connects selected sources through transformations
to selected destinations. Start with direct routes, channel/group splits,
merging and fan-out. A CLI could run a saved setup; a TUI or GUI could eventually
make the connections visible and editable.

Preserve source identity when merging. Define ordering for equal-time events and
require explicit clock mappings where timing from different sources is compared.
Use bounded queues and report overload at the affected route. Prevent or identify
routing feedback; an ordinary MIDI message has no universal hop counter that
can safely be added to traffic.

Configuration changes need a clear policy for notes already sounding. Avoid
implicitly replaying old commands after reconnect or silently broadcasting
cleanup to destinations shared with other applications.

### 4. Controller mapping, learn and pickup

**Use:** Make inexpensive knobs, sliders, pedals and buttons useful with software
or hardware whose control assignments differ.

A mapping layer could bind observed controls to named application parameters,
with explicit input/output ranges, curves, inversion, toggle/momentary behavior
and supported relative-encoder conventions. Reuse existing 14-bit CC and NRPN
interpretation. Device-specific encoder conventions should be named choices,
not guessed from an ambiguous sequence.

A learn operation should observe traffic and propose a binding for approval.
Pickup/soft-takeover could avoid a sudden parameter jump when a physical knob
and the known target value disagree. Report when the target value is unknown;
observed outbound traffic is not proof of the instrument's actual state.

A small TUI would make learning and testing convenient. Motorized controls and
LED feedback can follow later, with explicit suppression of feedback loops.

### 5. Observed note and pedal state, with scoped cleanup

**Use:** Diagnose stuck notes, display playing activity and clean up a route that
the application itself owns.

Generalize useful note-state observation beyond the existing MPE tracker:
active note instances, held versus sustained notes, pedal knowledge and reset/loss
boundaries. Distinguish what was observed from what a synthesizer might still
be sounding, including its unknowable release envelope.

Offer an explicit cleanup plan derived from the messages a particular sender or
route owns. Let callers inspect and send that plan. A monitor could also offer
a clearly targeted panic action, explaining that channel-wide commands can affect
other performers. NoteOff, All Notes Off, pedal release and All Sound Off have
different musical effects; they should not be interchangeable hidden fallbacks.

Cleanup remains uncertain after lost traffic, a crash or competing writers.
Do not automatically add it to cancellation or connection disposal.

## Timing and musical construction

### 6. MIDI clock following and generation

**Use:** Synchronize an arpeggiator, sequencer or visualizer to a drum machine,
or make a Pi-based instrument provide tempo to other equipment.

Build on the existing Clock, Start, Continue, Stop and Song Position messages
with a caller-driven transport state and clock follower. Expose estimated tempo,
phase, running state, uncertainty and loss of lock. Keep MIDI beat-clock timing
separate from MIDI Time Code and native packet timestamps. Their roles differ,
as described in the [MIDI Association's message overview](https://midi.org/about-midi-part-3midi-messages).

Offer clock generation as an explicit scheduling operation. Decide how to handle
missing pulses, abrupt tempo changes, silence and late deadlines before exposing
it as a synchronization engine. Do not promise hard real-time performance or
silently emit a burst of overdue clock pulses.

### 7. Musical time, patterns and an event scheduler

**Use:** Express a small sequence in bars and beats rather than manually calculating
nanoseconds, and schedule live-generated events as well as existing recordings.

Extend existing tempo conversion and playback with a musical position/duration
model, time signatures and explicit tempo maps. Add a bounded deadline scheduler
for events submitted while playback is running, including cancellation and
lateness reports. Keep musical positions distinct from physical clock domains.

Pure pattern helpers could construct repeated phrases, Euclidean rhythms,
arpeggios, chords and controlled variations. They should return ordinary events
or scheduling actions so applications can inspect, edit and save the results.

Specify tempo-change behavior, equal-time ordering, looping boundaries and what
happens to sounding notes when a pattern changes. Swing and quantization are
optional musical transformations, not corrections applied to every recording.

### 8. Note-duration and performance editing views

**Use:** Edit a phrase as notes with durations, while keeping meedee's message
representation useful for low-level programming.

Add a derived note view over event sequences with explicit pairing policies for
overlapping same-pitch notes, missing releases and sustain. Preserve links to
the underlying events so edits can retain unrelated controllers and metadata.
A duration inferred from key release should remain distinct from sounding length.

Useful operations include trim, split, concatenate, repeat, quantize and
velocity editing. Preserve stable ordering and exact time where possible;
report rounding and unsupported export rather than hiding them.

A CLI could inspect, compare and transform files. A piano-roll GUI is a possible
later client, not a requirement to turn the library into a full DAW.

## Instrument and studio workflows

### 9. A tuning workbench

**Use:** Design a scale, inspect its tuning error and prepare a deliberate tuning
transfer to a compatible instrument.

Build on existing MTS values and codecs with scale construction, interval-ratio
helpers, keyboard mappings and optional Scala scale/mapping file interoperability.
Keep tuning descriptions separate from a specific device's transmission format.
Support an inspectable comparison between desired pitches and encoded results.

A CLI could convert files and explain quantization. A GUI could visualize tuning
and keyboard assignments. Auditioning would need a separate optional sound engine;
the MIDI library itself does not synthesize audio.

Sending a tuning should require an explicit destination and action. Preserve
original dumps for backup, and never describe a successful send as verified tuning
unless a device-supported response actually establishes it.

### 10. SysEx librarian and transfer workflow

**Use:** Back up a synthesizer, compare dumps and restore a selected patch without
inventing a separate ad hoc transfer script for every instrument.

Build transfer plans over the existing SysEx codecs/files, request tracking and
recording. Plans could express documented chunk sizes, pacing, replies, deadlines
and cancellation. Manufacturer/device support should be a collection of explicitly
supported formats, not a claim that arbitrary SysEx is self-describing.

Provide a generic byte/difference view even when a patch format is unknown.
A TUI or GUI could organize named backups and show transfer progress.
Do not resend an uncertain state-changing command automatically. A completed
byte send and a completed instrument restore are different outcomes.

### 11. Device definitions and reusable setups

**Use:** Give a controller or synthesizer meaningful parameter and patch names,
then reuse a setup across a script, a router and a desktop tool.

A device definition could describe documented bank/program names, CC/NRPN units,
encoder conventions, supported tuning mechanisms and transfer constraints.
Separate these descriptions from observed capabilities and current device state.
Keep user/vendor instrument definitions distinct from standardized GM tables.

A saved setup could describe desired routes and mappings. On load, show current
endpoint matches and require resolution of ambiguity. Names and backend IDs are
not durable proof of physical identity. Applying a setup must distinguish opening
routes from sending mode changes, patch selections or other musical commands.

Prefer a small declarative format if this becomes useful. Do not introduce a
plugin framework or executable configuration merely to store parameter names.

### 12. Topology watching and explicit recovery

**Use:** Keep a long-running musical application understandable when a USB device
is unplugged, rebooted or appears after startup.

Expose bounded topology-change observations and a recovery helper that retires
old interpretations and proposes fresh endpoint selections. Backend notification
support and discovery races need platform qualification; watching a port list
does not prove that a silent device is responsive.

Automatic reconnect could be an opt-in policy for a specifically identified route.
Restoring patch, tuning, controllers or notes must be a separate action. Do not
silently replay captured traffic, inherit stale parser state or claim to provide
a generic instrument heartbeat.

### 13. MIDI-CI browser and supported configuration

**Use:** Inspect a capable instrument's advertised properties and profiles instead
of hard-coding every capability into an application.

This extends the existing deferred Profiles and Property Exchange work, rather
than introducing another CI implementation. Once normative documents are acquired,
add the agreed bounded exchanges and expose a read-oriented browser. Configuration
writes and profile changes should be explicit actions with inspectable outcomes.

The [MIDI Association's Property Exchange overview](https://midi.org/midi-2-0-property-exchange)
describes access to device properties; its
[Profiles overview](https://midi.org/profiles) describes standardized behavioral
agreements. Neither overview replaces the detailed specifications needed for
implementation. A device definition from the previous proposal remains useful
for instruments without these facilities.

### 14. Machine-control helpers

**Use:** Control compatible recorders or studio transport equipment from scripts
and custom control surfaces.

Add typed values and codecs for a selected subset of
[MIDI Machine Control](https://midi.org/midi-machine-control), followed by explicit
request/reply handling where the selected commands require it. Reuse the existing
time-code values where applicable. Support should follow acquired normative
specifications and actual device tests.

A small transport-control interface could be useful, but record, locate and
other state-changing actions need clear targets. Keep this distinct from MIDI
clock synchronization and from a general-purpose DAW control protocol suite.

## Programming and diagnostic facilities

### 15. Virtual devices and deterministic application tests

**Use:** Test a musical application without requiring an attached synthesizer or
running a live OS MIDI session in every unit test.

Provide a small transport test double that records sends, injects input and
simulates time, loss, disconnects and uncertain send failures. It should exercise
the public musical behavior without pretending to qualify native drivers.
Reuse existing fake-clock and transport fixtures where practical; avoid creating
a second protocol implementation solely for tests.

For manual integration work, a CLI could expose a test-owned virtual keyboard,
controller or responder where the selected backend supports virtual endpoints.
Clearly distinguish these OS-visible tools from deterministic in-memory tests.

### 16. Capture comparison, validation and regression reports

**Use:** Check whether a new version of a script changed the notes, timing or
controller behavior it produces.

Compare captures semantically with explicit options for timestamp tolerance,
source matching and ignored fields. Explain differences in messages, ordering,
gaps, clock discontinuities and tuning/parameter observations. Compare raw
records when semantic interpretation is unavailable.

A file checker could report structure, integrity, protocol diagnostics and export
losses using existing readers/codecs. A CLI is a natural interface; the comparison
and validation logic should also be callable from Rust/Python tests. Never call
a timing difference harmless merely because it falls below a default tolerance.

### 17. Python interactive and notebook inspection

**Use:** Explore a recording or prototype an algorithm without manually unpacking
every immutable message object.

Offer concise inspection summaries and optional notebook views for note activity,
controller traces, tempo and loss diagnostics. Provide straightforward records
for analysis, retaining integer/rational timing and explicit clock identity.
Heavy plotting or data-analysis dependencies should remain optional integrations.

A notebook view should be passive unless the user explicitly opens a port or
starts playback. Reuse the same analysis used by the monitor and file tools.

### 18. Timing and load diagnostics for real setups

**Use:** Find out whether a particular Pi, USB interface and script can handle
a workload before using them in a performance.

Turn suitable qualification machinery into opt-in, reusable diagnostics for
observed throughput, queue occupancy, losses, scheduling lateness and process-tree
resource consumption. A loopback measurement can report round-trip timing;
one-way physical latency needs additional instrumentation or a justified clock
relationship. Separate measured facts from inferred bottlenecks.

Reports should identify backend, mode, clock domains and test conditions.
Tests that emit MIDI must use explicitly selected destinations and bounded runs.
This is a practical tool built on qualification work, not a substitute for the
remaining Windows, Pi and physical-fault validation.

## Longer-term candidates

- **Step sequencer or performance looper:** a useful client for patterns, routing,
  recording and editing, once loop-boundary and overdub policies are agreed.
- **Controller-surface editor:** combine mappings, named parameters and optional
  feedback into a desktop interface for a specific performance setup.
- **Opt-in network transport/bridges:** useful for studio or distributed Pi setups,
  but require a separate decision about protocol, clock relationships, discovery,
  security, loss and reconnect behavior. No network service should appear merely
  because the library is imported.
- **Interoperability adapters:** integrations with selected music-programming,
  notation or audio environments, chosen from actual user projects rather than
  a broad compatibility layer.

Do not begin with a full DAW, synthesizer, plugin host, notation editor or generic
remote-control platform. They can consume the project without improving its
central Rust/Python MIDI programming experience.

## Suggested order and decisions

| Stage | Candidate scope | Why this order |
| --- | --- | --- |
| First | Inspector plus capture validation/comparison | Immediately useful, exercises existing features and improves diagnosis |
| Second | Transformations, note-state observation and a small router | Enables practical MIDI effects and Pi applications |
| Third | Controller mapping, clock following and musical scheduling | Adds interactive instrument and sequencer building blocks |
| Fourth | Choose a tuning, librarian or editing workflow | Develop one complete musical use case before several partial tools |
| In parallel when available | Device/platform qualification and normative specification acquisition | Provides evidence needed for reliable live and standardized features |

These are candidate work packages, not new numbered slices or estimates. Before
selecting one, agree its musical policies and whether its first interface is
library-only, a CLI, a TUI or a GUI. Start with one interface where possible.
Do not choose CLI commands, widget layouts, dependencies or a configuration
language until a specific proposal is approved.

The existing native qualification gates remain release considerations. New pure
library features can be developed independently, but an attractive interface must
not obscure unqualified live behavior or imply that device support is universal.
