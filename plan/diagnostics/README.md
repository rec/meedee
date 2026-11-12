# Historical development artifacts

Project names and paths in these historical artifacts were normalized to `middi`
during the project rename. They are not new qualification results.

`2026-11-05-callback-lock.sample.txt` is the macOS `sample` report
collected on November 5, 2026, originally preserved from
`/tmp/middi-pytest-sample.txt`. The main-thread stack includes callback
failure-state mutex acquisition. This is evidence of that historical process
state, not a finding that the current implementation still has the same issue.
No new runtime or native MIDI qualification was performed while preserving it.

SHA-256: `b2b620b3505fed993f118ea0949c2a813db446b61f0127e4b5f0ec695d143950`

## Legacy UMP stub generator

`legacy-make-ump-stubs.py.txt` preserves the handwritten generator previously
left at `target/make-ump-stubs.py`. It reflects an earlier UMP API
and would overwrite the current `middi/ump.pyi` with an incomplete interface.
It is archived source, not a supported generator to run against today's checkout.

SHA-256: `03af78d12506144790b4dc5fc091dbd1ec66931c608eac3826c679e4f97c5933`

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
