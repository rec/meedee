# Historical development artifacts

Project names and paths in these historical artifacts were normalized to `middy`
during the project rename. They are not new qualification results.

`2026-11-05-callback-lock.sample.txt` is the macOS `sample` report
collected on November 5, 2026, originally preserved from
`/tmp/middy-pytest-sample.txt`. The main-thread stack includes callback
failure-state mutex acquisition. This is evidence of that historical process
state, not a finding that the current implementation still has the same issue.
No new runtime or native MIDI qualification was performed while preserving it.

SHA-256: `fa0d29b61599f4a830ae93eeaade45e152855362c34c005a76bac12941e5ab69`

## Legacy UMP stub generator

`legacy-make-ump-stubs.py.txt` preserves the handwritten generator previously
left at `target/make-ump-stubs.py`. It reflects an earlier UMP API
and would overwrite the current `middy/ump.pyi` with an incomplete interface.
It is archived source, not a supported generator to run against today's checkout.

SHA-256: `e26df6f2906fa464e683b0ef9467bcc45cbe20bfa4b903ca39f0d63cc7ded9f9`

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
