# Consolidate recordings

## Objective

Use `archive.Recorder` for both duration-based memory capture and explicitly
controlled archive recording in Python and Rust. Remove the independent record
accumulation path from `Input.record()`. Defer callbacks until their contract is
specified.

## Agreed API and behavior

- Support a memory destination in `archive.Recorder` and `Input.start_recording`.
- Keep `Recorder.stop()` returning `RecordingReport` for both destinations.
  Expose captured memory records separately after stopping. File recorders do not
  retain a second copy of the recording.
- Keep `Input.record(duration)` returning its existing captured-record collection,
  closing the input and draining accepted events at the boundary. Preserve optional
  saving and explicit overwrite behavior.
- Keep `Input.stop_recording()` leaving the input open and performing the shared
  admission, diagnostics, finalization and detach sequence.
- Preserve memory capture's retention of gaps and discontinuities and continued
  capture after diagnostics. Report diagnostic failures as incomplete recording.
  Keep the existing file recorder failure policy and bounded writer queue.
- Preserve buffered events, event timestamps, source metadata and clock declarations
  without duplicating events when switching to the shared recording path.
- Keep memory retention unbounded, as it is today; document this explicitly.

## Implementation

1. Extend the Rust recorder with memory storage and final record access. Share
   recorder lifecycle and report semantics across destinations while retaining the
   existing file writer.
2. Expose the corresponding Python constructor, record access and type hints.
3. Allow input attachment to select memory or file recording. Route duration capture
   through the attachment and remove superseded accumulation helpers. Ensure
   diagnostics and failure boundaries are recorded once, including close/drain.
4. Update Python and Rust documentation/examples and focused regression tests for
   memory/file equivalence, explicit stop, buffered events, clock changes, loss,
   terminal failures and finalization. Reuse existing coverage where possible.
5. Run the repository's Python checks and relevant Rust tests/checks. Review the
   final changes for parity, lifecycle regressions and duplicate recording paths.
6. Remove this plan in the implementation commit.

## Commits

Commit this plan first. Implement it and remove the plan in a separate commit.
Fetch/rebase against main before committing and pushing each commit.

## Additional work beyond the prompt

None.
