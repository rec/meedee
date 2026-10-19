# Conversation handoff

This note records context that is not recoverable from meedee's commits and
existing design documents. It is not another feature plan or validation ledger.

## Where the conversation stands

The latest implementation request approved all recommendations for the optional
uFor-to-MTS preview/construction bridge. That request is complete. There is no
approved next implementation slice yet. Do not interpret the feature proposals
as permission to add device transfers, audition or an interface automatically.

The work has been moving across sibling repositories, not just meedee. The
conversation has already covered both arpeg host migrations, enge's file-import
migration, a shared MidiConverter, its file and live-input integrations, and
shared offline velocity support. Some integration descriptions in
`features.md` still describe these as proposals. Before recommending or repeating
one, inspect the current sibling checkout and its commits. This is a warning
about the proposal document's scope, not a replacement progress inventory.

## Native tests: avoid repeating the Terminal handoff

Earlier agent-launched CoreMIDI probes failed while the same probes worked in
Tom's ordinary Terminal. Many rounds of asking Tom to run commands followed.
The later investigation established a usable execution route: run native MIDI
tests with **standalone `uv run ...` commands**. Tom explicitly asked that this
route be used in future rather than handing the commands back to him.

Keep `uv run` as the command being executed. Avoid embedding it in a larger shell
script, wrapping it in another interpreter, or prefixing the native command with
environment assignments when relying on the existing command approval. Those
forms may be evaluated differently by the execution environment. Use the normal
approval mechanism if the standalone command needs it. Do not request full
machine access as the default solution.

Historical statements that ordinary Terminal execution is required describe the
earlier execution context. They should not override this later instruction.
An initial service-access failure is not evidence of a transport regression.
Neither this workaround nor the historical error alone proves the precise OS
mechanism responsible for a future failure.

## Standing choices from the conversation

- Dependencies on Tom's GitHub projects should use **HTTPS and `main`**. Do not
  pin revisions in `pyproject.toml`; normal resolved revisions in lockfiles are
  expected. This is an explicit dependency-source preference, not a request to
  change Git remote URLs.
- Never ask Tom to run `gh auth` again. His account authorization and the agent's
  credential access are separate. Report the actual denied operation if agent
  access is insufficient.
- Ask about substantive API or musical-policy decisions before implementing
  them. Approval of recommendations applies to the questions actually answered,
  not to every later integration idea.
- Permission to change or push a sibling repository has been requested for
  individual changes. Do not treat those approvals as unrestricted authorization
  for future cross-repository work.
- Runtime permission has covered bounded, supervised tests using test-owned
  virtual MIDI endpoints. It does not authorize physical instruments, tuning or
  reset transmissions, device configuration, or audio playback for a new slice.

## Additional work beyond the prompt

None.
