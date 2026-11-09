# Historical process diagnostics

`2026-11-05-callback-lock.sample.txt` is the original macOS `sample` report
collected on November 5, 2026, preserved byte-for-byte from
`/tmp/meedee-pytest-sample.txt`. The main-thread stack includes callback
failure-state mutex acquisition. This is evidence of that historical process
state, not a finding that the current implementation still has the same issue.
No new runtime or native MIDI qualification was performed while preserving it.

SHA-256: `6e0a45029995af86a47d5721082f390786c919ecfb2bfc621881e172c2263b72`
