# Remaining feature proposals

Additional work beyond the prompt

None.

These are unfinished proposals, not implementation commitments. Reuse the
existing MIDI codecs, file APIs, workflow drivers and state machines.

## Standards-dependent protocol work

- MIDI-CI Profiles and Property Exchange remain deferred until the full selected
  normative documents are available. Process Inquiry engines and individual
  instrument-profile semantics are also deferred. Reuse the existing management
  session and host; do not advertise optional categories before implementing them.
- A bounded MIDI Machine Control subset needs normative command specifications
  and an agreed request/reply scope before implementation.
- MIDI Clip profile negotiation and semantic interoperability need qualification.
  The separate MIDI Container format is not implemented.
- MDS has packet-level support; a dataset-transfer engine needs its own lifecycle
  and protocol decisions.
- Additional UMP dump workflows and manufacturer recipes require documented
  addressing, matching, completion and failure rules. Existing
  [SysEx coverage](../doc/sysex-dumps.md) identifies unsupported collections and
  unresolved manufacturer-document inconsistencies.

The reviewed sources for existing codecs are recorded in
[specification baselines](../doc/specifications.md). Acquire missing documents
before deriving new layouts or transaction semantics.

## Routing and recovery

Remaining proposals are scheduled multi-source merging, dynamic routes, explicit
recovery and route-owned note cleanup. Define equal-time ordering and explicit
cross-source clock mappings before scheduled merging. External physical feedback
loops are not detected.

Changing routes while notes are active requires an ownership policy. An
inspectable cleanup operation must distinguish NoteOff, pedal release, All Notes
Off and All Sound Off. Competing writers and lost traffic limit certainty.
Do not add implicit replay, channel-wide resets on shared destinations or
automatic recovery after an uncertain send.

Native topology subscriptions and explicit reconnect/state restoration remain
proposals. Endpoint names and IDs are not durable physical identities, and
rediscovery does not establish instrument health.

## Inspection and workflow clients

- Cross-format musical capture comparison remains separate from implemented
  same-format comparison.
- Frozen monitor views, controller traces, notebook views and librarian/editor
  interfaces need a selected workflow before implementation.
- Native virtual-device tools and driver qualification remain separate from
  in-memory testing transports.
- Child-inclusive memory, CPU, thread and handle measurements remain open;
  payload budgets and parent-only metrics do not establish total resource use.

Reuse existing reports and file APIs rather than adding another engine or
general setup language.

## Optional integrations

The arpeg MIDI host migration, enge performance-binding converter and enveLFO
MIDI control mapping are implemented in their owning projects. Check those
projects' current sources before extending them.

Remaining candidates:

- Native UMP control mapping in enveLFO, with explicit quantization, admission
  and uncertain-send policy.
- Optional enge audition and intended-versus-encoded pitch views for tuning.
- Controller learn, pickup/feedback and librarian workflows based on uFor's
  instrument descriptions, mappings, bindings and sequence planning.
- Additional MIDI 2/MPE and platform/hardware cases in arpeg/enge integrations.

uFor owns musical definitions, arpeg scheduling, enveLFO modulation execution,
enge audio realization and voice lifetime, and middy MIDI protocols/transport.
A VL70-m decoder does not establish a universal device-description engine.
No second composition engine, synthesizer or autonomous scheduler is proposed.
Network bridges need separate transport, clock, security and recovery decisions.

## Qualification and release decisions

See [qualification](qualification.md), the [Linux procedure](linux-qualification.md)
and [open issues](issues.md). Windows/Linux runtime evidence, physical faults,
resource budgets, archive-format freeze and publication remain separate gates.
This list authorizes no implementation, hardware operation or publication.
