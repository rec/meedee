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
- Rust message values now carry optional timestamp/source/span metadata through
  the common Message trait, across MIDI 1, UMP, SysEx and device-management types.
  Builders and clamped note transposition preserve it. Equality/hash use only
  musical payload fields; expansion copies command provenance onto primitives.
  Verified by the cross-family metadata and command-expansion regressions.
- Removed the generic Rust Event wrapper without an alias. Live decoding yields
  Item::Message with a metadata-bearing Payload; archives, routing, inspection,
  observation, file timing and playback consume it directly. The Python bridge
  copies metadata between its message base and the same Rust value. Archive bytes
  and occurrence comparison are preserved; playback no longer invents a source.
  Verification: 123 Rust tests plus one doctest, core Clippy with warnings denied,
  independent archive byte fixtures, and 525 Python regressions pass after the
  extension rebuild. Two native-device tests remain ignored.
- Added root-level Output with lazy first-port discovery, unique name matching,
  explicit channel override, direct message/command/packet sending and heterogeneous
  send_all sequences. Duration remains elapsed time; Beats requires output tempo.
  Cumulative rational deadlines use the existing cancellable monotonic clock.
  Complete sequence validation precedes sending. Drop releases confirmed MIDI 1
  note counts and the agreed changed-state set; explicit close reports failures
  and closes even when cleanup fails. Device-free tests cover timing, channels,
  partial compound sends, cleanup failures and UMP protocol boundaries. The output
  doctest compiles without opening devices.
- Output::play now accepts archive record iterators, including Reader directly,
  infers the first message's time origin, and reuses the playback scheduler.
  Replay rejects loss/discontinuity/unknown records, unrelated clocks, regressions
  and implicit rounding; failures retain confirmed progress. Reader::open supplies
  strict defaults for a file path. Replay tests cover prior output activity,
  malformed archive completion, loss, regression and empty input without opening
  ports. A file-replay doctest compiles without running native transport.
- Added root-level Input with lazy first-port/name selection, explicit connection,
  channel/container filtering, and scoped close/Drop ownership. Default iteration
  yields metadata-bearing messages and prints diagnostics; with_errors retains
  diagnostics and terminal Results. Raw callbacks feed a packet/byte-bounded
  queue; caller-thread decoding resets on sequence gaps. Queue loss is reported
  after the accepted prefix even if the sender goes silent; remaining native tail
  loss and truncated messages are reported at close. Filters never remove
  diagnostics or system messages. Native health is checked while waiting for input. Device-free tests
  cover MIDI 1/UMP filtering, queue overflow, partial-message loss, callback-thread
  delivery, terminal decoding errors and ownership release.
- Input::record(Duration) now consumes/closes the input and returns a Recording
  that feeds Output::play directly. It retains declarations, message provenance,
  gaps and discontinuities, including errors and finalization diagnostics.
  Input::record_to captures the same records, then writes a strict archive with an
  explicit overwrite boolean. Memory grows with the retained capture, matching
  Python; receive admission remains bounded. Tests cover silent timeout, receiver
  release, diagnostic retention, file round-trips and overwrite preservation.
- Graded examples and the final migration audit remain
  pending. Full goal completion is not established.
