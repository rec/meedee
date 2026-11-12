# Historical development artifacts

`2026-11-05-callback-lock.sample.txt` is the original macOS `sample` report
collected on November 5, 2026, preserved byte-for-byte from
`/tmp/meedee-pytest-sample.txt`. The main-thread stack includes callback
failure-state mutex acquisition. This is evidence of that historical process
state, not a finding that the current implementation still has the same issue.
No new runtime or native MIDI qualification was performed while preserving it.

SHA-256: `6e0a45029995af86a47d5721082f390786c919ecfb2bfc621881e172c2263b72`

## Legacy UMP stub generator

`legacy-make-ump-stubs.py.txt` preserves the handwritten generator previously
left at `target/make-ump-stubs.py`, byte-for-byte. It reflects an earlier UMP API
and would overwrite the current `meedee/ump.pyi` with an incomplete interface.
It is archived source, not a supported generator to run against today's checkout.

SHA-256: `bbdf8db3b0141ecfdd548eaff741a4035e8321cb243b7a21ca4b6cdb4013905f`

## Target cleanup, November 12, 2026

Removed 262 routine top-level logs, superseded experimental CMake builds,
old wheel-test sandboxes, package artifacts, saved extension binaries, generated
inventory/requirements files and completed pytest outputs. The old packaging
sandboxes' handwritten Cargo manifests and release tests already exist in Git
history. The scratch CoreMIDI access probe differs from the maintained
`test/native/coremidi_access.cpp` only in formatting and its explanatory comment.

Retained the main Rust debug/release caches, dependency caches, current
`native-tests` build, generated documentation, specifications and qualification
evidence. Upstream source trees and the clean `libremidi-teardown` Git checkout
were retained; only that checkout's obsolete build directory was removed.
No library code changed and no native MIDI runtime was started for this cleanup.
