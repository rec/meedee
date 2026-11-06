# Python and Rust interface parity

Additional work beyond the prompt: None.

## Approved scope

- Use Python's public module organization and corresponding names as the canonical
  interface. Replace old paths instead of retaining compatibility aliases.
- Add Rust live workflow drivers, file conveniences and advanced playback controls.
  Python CLI, process isolation and uFor integration remain language-specific.
- Default Python input to in-process operation, retaining explicit isolation.
- Unify Python's high-level clients and connections. Connection selection returns
  the same object; closure is terminal and reconnection requires a new object.
- Match Python's report snapshots and field names in Rust, including Distribution.
- Preserve typed Rust durations, builders, enums, Results and scoped Drop cleanup.
- Restore callable Python output. Use stream and SourceSpan consistently.
- Update examples, contracts and callers; verify without opening native ports.

## Additional approved decisions

- Typed high-level Rust errors preserve transport categories and underlying causes.
- Output::with_playback configures subsequent Rust play calls; Python uses keywords.
- Live drivers require fresh sessions and own their monotonic clocks.
- Channel filters preserve system messages. Split examples forward them once.
- ci_live::Options carries cancellation and assembly limits with Python's defaults.

## Work inventory

- [x] Canonical module paths and names; migrate Rust callers and Python bindings.
- [x] Unified Python Input/Output lifecycle, named output sending and port selection.
- [x] Consistent read-only report snapshots and metadata terminology.
- [x] Rust file conveniences and advanced playback options.
- [x] Rust CI, tuning, SysEx capture/send and discovery drivers.
- [x] Error classification preserving categories and causes.
- [x] Review public implementation helpers and language-specific exceptions.
- [x] Update graded examples and API documentation.
- [x] Focused regressions and full required checks; commit and push this migration.

The public-symbols.txt inventory is user-owned, untracked and must not be committed.
Completion requires finishing this inventory, not merely aligning symbol counts.


## Canonical surfaces

| Python | Rust | Language adaptation |
| --- | --- | --- |
| midi.Input, midi.Output | midi::Input, midi::Output | Python send; Rust send and consuming connect |
| messages.Note, Rest | messages::Note, Rest, Beats | Python unit strings; Rust Duration and Beats |
| Message.stream, span | Message::stream(), SourceSpan | Metadata survives transformations and is ignored in payload equality |
| events packet decoding/playback | events packet decoding/playback | Rust enums and typed Results |
| archive.Recorder, RecordingReport | archive::Recorder, RecordingReport | Rust generic readers/writers |
| ci.Session, Browser, BrowseReport | ci::Session, Browser, BrowseReport | Rust Browser takes an explicitly constructed Session |
| tuning.Query, QueryReport | tuning::Query, QueryReport | Same request/progress semantics |
| transfer_input/output, tuning_live, ci_live | Same module names and run drivers | Caller-owned connections and monotonic clocks |
| discovery.Poller | discovery::Poller | Caller-driven full snapshots; stop on failed scans |
| inspection reports/comparison | inspection reports/comparison | Immutable getters use Python property names |
| metrics.Distribution | metrics::Distribution | Detached snapshot, separate Histogram accumulator |
| smf/clip timed readers | smf/clip TimedReader and Timeline | Rust enums retain file-specific records |

Python-only surfaces remain the CLI, process isolation, unit-string parsing and
uFor integration. Rust's transport and file_io modules remain advanced native and
streaming primitives. Private implementation modules are not alternate public paths.
The old inspection timing integration test moved into the core unit tests so the
mutable timing accumulator no longer needs a public module.

## Verification

- Rust workspace: 201 tests passed, five opt-in tests ignored; no native devices.
- Core Rust Clippy: all targets, warnings denied, passed.
- Rust guide: all 19 snippets compile without executing native operations.
- Strict documentation build passed with no issues.
- Python Ruff, formatting, pyupgrade for 3.13 and ty check meedee passed.
- Full final Python suite: 526 tests passed.

Native qualification scripts were migrated to named output sending and new resources
per open/close cycle, but were not executed. No macOS, Windows or Linux hardware
qualification is claimed by these device-free checks. public-symbols.txt remains
untracked and excluded from the commit.


## Follow-up: lazy opening boundary

The side-by-side symbol audit found remaining differences despite the completed
module migration above. The user approved lazy construction and device-free empty
playback in both languages. Python now delays native clients and isolated workers
until discovery/connect or actual I/O. Initialization errors and worker admission
limits apply at first use; closing an unused object creates nothing. Playback
examines the source before opening, matching Rust's existing boundary.

The remaining pattern, high-level input/raw-mode, snapshot and CI-constructor
design choices are still pending. This follow-up does not claim full parity.
Additional work beyond the prompt: None.

Verification for the lazy-opening follow-up: 534 Python tests passed; the Rust
workspace passed 201 tests with five opt-in tests ignored, including its existing
empty-replay/no-port regression. Python lint, format, type and 3.13 pyupgrade checks
passed. The strict documentation build reported no issues. No native device flows
were run. public-symbols.txt marks G5 resolved and remains uncommitted.
