# Remaining platform and resource qualification

Additional work beyond the prompt

None.

Qualification is specific to OS, architecture, backend, protocol, transport mode
and workload. No particular Linux board or host is required. Historical Mac
software-loopback and synthetic measurements remain in Git history; they do not
qualify current revisions, physical instruments or other platforms.

## Open gates

| Scope | Remaining evidence |
| --- | --- |
| macOS CoreMIDI MIDI 1 and UMP | Other versions/architectures, physical interoperability, faults and deployment resource behavior |
| Windows WinMM | Native regressions, ANSI/Unicode discovery tests, installed wheels and an agreed loopback route |
| Windows MIDI Services | SDK/build integration and native UMP qualification; currently disabled |
| Linux ALSA sequencer/raw and available UMP variants | Native build/tests, installed wheels and separately authorized runtime checks |
| Resources | Parent/child memory and CPU, thread/handle counts, sustained load and overload behavior against agreed application budgets |
| Release | Archive-format freeze, future compatibility promises and publication remain owner decisions |

See the staged [Linux procedure](linux-qualification.md) and
[open issues](issues.md). Compilation and device-free tests do not establish
native runtime or physical-device behavior.

## Setup and toolchain policy

Use the current installed Rust compiler locally. Minimum-version compatibility
belongs to release CI; do not install older compilers locally for qualification.
Record the actual compiler without making minimum-version claims from that run.

Use an available supported Python version and record its exact version. No local
interpreter matrix is required. Install non-editably with locked dependencies and
selected extras. Consult [build instructions](../README.ai.md) and the
[release workflow](../.github/workflows/ci.yml) for prerequisites and checks.
A missing prerequisite is an open gate, not permission to install tools.

Use a complete checkout and a new evidence directory. Record commit/dirty status,
OS/kernel/libc, CPU/RAM, storage/free space, C++/CMake/Rust/Python versions,
backend/protocol/mode, power/thermal settings and background load.
Run Python suites and linked Cargo tests sequentially because callback tests
rebuild binding artifacts. Build the wheel from the sdist and test it outside
the checkout. Retain logs, dynamic dependencies, license/provenance contents,
wheel identity and all pass/fail/skip counts.

## Existing measurement tools

- [Rust codec/file workloads](../crates/middy/examples/qualification.rs) measure
  batch wall time for messages, commands, SysEx, tuning, Clip and archives.
- [Python workloads](../test/qualification/measure.py) measure imports, codecs,
  files and paced/burst recording. Default pytest does not collect these files.
- The same Python file supervises native cases through
  [native.py](../test/qualification/native.py), with child-process deadlines.
  Build the [Rust transport example](../crates/middy/examples/qualification_transport.rs)
  before selecting its case. A missing binary or backend is a skip, not a pass.
- [CoreMIDI access](../test/qualification/access.py) and
  [discovery/reopen](../test/qualification/reopen.py) probes diagnose macOS
  access/discovery failures; they are not recurring release requirements.

For an authorized device-free baseline:

```bash
uv sync --frozen --no-editable --python 3.13 --all-extras
cargo build --locked --release -p middy --examples
target/release/examples/qualification
uv run --no-sync pytest -q -s test/qualification/measure.py -k 'not native'
```

Record first-run costs and build profiles. Debug-versus-release measurements
are not language-overhead comparisons. Batch throughput is not per-message
latency, and filesystem-cache timing is not sustained disk throughput.

Parent peak RSS is a lifetime high-water mark, not incremental memory or an
isolated-worker total. Measure the whole process tree with suitable host tools
and report unavailable measurements and sampling blind spots.

## Separately authorized native checks

Fixtures must select unique test-owned endpoints, never a pre-existing instrument.
Use the supervisor rather than launching a native child directly when backend
liveness is unknown. Software endpoints do not authorize physical traffic,
instrument tuning/reset writes, service restarts or fault injection.

- MIDI 1 loopback does not qualify UMP; ALSA sequencer does not qualify ALSA raw.
- WinMM needs an independently arranged loopback route; do not install a driver
  implicitly.
- Typed capture/replay, metrics, SysEx and synthetic MTS queries are distinct
  workloads and do not establish physical instrument support.
- Reopen cases use fresh high-level resources for every cycle; closure is
  terminal. Include process-lifetime CoreMIDI keeper/notification resources in
  accounting.

If CoreMIDI creation fails in the agent context, compare the maintained direct
SDK access probe in an ordinary Terminal before changing code or restarting
services. An access-context failure is not evidence of a library defect.

## Application acceptance

Agree stream counts, rates, maximum payloads, capture duration/storage and
resource/latency budgets after measuring the intended host. Rerun normal and
overload workloads. Inspect loss and completion reports; structural file
completion does not imply loss-free capture. Queue size alone cannot establish
sustained throughput or latency.

Physical tests require named equipment, authorization and a recovery procedure
for unplug/replug, reboot, driver faults and uncertain sends. Keep codec, file,
state-machine, software-loopback and physical claims separate. This document
authorizes no runtime test, installation, publication or hardware operation.
