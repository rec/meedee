# Rust API alignment

Goal: bring the Rust-facing API into line with the simplified Python API and the
agreed concepts. This is an API migration, not an additional legacy-compatible
constructor family. The existing low-level transport remains the backend.

Additional work beyond the prompt: None.

## Decisions

- Replace explicit MIDI 1 message constructors with payload-first defaults and
  validated `with_*` builders; migrate all repository callers and Python bindings.
- Use `std::time::Duration` for elapsed durations and a typed `Beats` value for
  musical durations. Tempo belongs to the output. No unit-string parser.
- Unassigned channels remain distinguishable from explicit channel 1. An explicit
  output channel overrides MIDI 1 message channels; an unspecified output preserves
  assigned channels and supplies channel 1 only when necessary.
- Message timing/source/span metadata is optional, survives transformations and
  does not participate in equality/hash. Remove the public generic Event wrapper;
  retain low-level SMF file records and internal archive representation as needed.
- Default input iteration yields messages and reports diagnostics; explicit error
  iteration retains diagnostics. Terminal input failures stop iteration. Output
  failures remain checked Rust Results.
- Full Note values never arise from input. A Note sends attack/release and advances
  by its duration; Rest advances without sending. Low-level attacks may overlap.
- Match Python's explicit cleanup set and avoid guessing defaults for other state.
  Drop is best effort; explicit close reports errors. No native device testing.

## Implementation and completion evidence

1. Typed musical durations and full Note/Rest values, including exact beat timing,
   validation and default note parameters. Unit tests establish timing arithmetic.
2. Message constructors, channel resolution, metadata and transformations. Migrate
   codecs, file/stream consumers and bindings; test payload equality, wire bytes,
   metadata preservation and unresolved-channel handling. No parallel constructors.
3. The ergonomic Rust Input/Output API: discovery/selection, iteration/filtering,
   sending, scheduling, recording to memory/files, replay and RAII cleanup. Reuse
   existing decoders, archives and scheduler. Device-free transport tests establish
   lifecycle, failures, cleanup, ordering and file overwrite behavior.
4. Graded Rust examples and updated contracts. Compile examples without executing
   native flows. Verify the full Rust test suite, Python regressions and strict docs
   build. Audit every item above against current sources before declaring the goal
   complete. Commit and push coherent verified changes as the migration progresses.

## Progress

- Initial inspection and constructor/duration decisions completed.
- Typed duration/full-note foundation implemented: root-level Note, Rest, Beats,
  NoteDuration; exact rational beat-to-time conversion; note defaults and validated
  channel/velocity builders. The Rust crate test suite passes (including the new
  duration/default regressions); hardware tests remain ignored.
- MIDI 1 constructors now take payload-first defaults with validated builders and
  optional channels. Encoding and command expansion return checked Results for
  unresolved channels; transpose clamps notes. Core callers, archive decoding and
  Python bindings use the new constructors. Native MIDI 2 constructors retain
  their explicit group/channel API. Independent MIDI/archive fixtures and 117 Rust
  tests pass (two native-device tests ignored), as do the Rust constructor doctest,
  core Clippy with warnings denied, and all 525 Python regressions after rebuilding
  the extension. The bindings now use the core optional channel directly.
- Message metadata migration, ergonomic Input/Output, scheduling,
  recording/playback integration, cleanup integration and graded examples remain
  pending. Full goal completion is not established.
