# Slice 12: qualification procedure and evidence

Additional work beyond the prompt

None.

## Status and boundaries

The owner authorized bounded synthetic workloads, temporary files and exclusively
test-owned virtual endpoints on the available Mac. On 2026-10-23 the owner
corrected the target to **Pi 5**; the earlier Pi Zero references were an incorrect
planning assumption, not a hardware selection. The Pi 5 is occupied by another
project and no target access has been supplied. Its RAM, storage, OS image,
ABI/bitness, libc, Python installation and MIDI endpoint remain unselected.
There is also no Linux/Windows test host available here. Measure a baseline
before agreeing acceptance budgets. Do not claim board qualification from an
arm64 build. The staged [Pi 5 procedure](pi-qualification.md) prepares later
qualification without reserving or modifying the board. No release, hardware
probe, physical tuning/reset transmission, OS service restart,
system installation or workflow trigger is authorized by this procedure.

On 2026-10-21 the owner additionally authorized supervised SysEx/MTS workflow
checks using synthetic replies exclusively between uniquely named test-owned
virtual endpoints, and native recording/playback metric checks. This does not
authorize selecting pre-existing destinations or sending to physical equipment.
Native results remain software-loopback evidence, not hardware qualification.

The tools deliberately have no new runtime dependencies or command parser. They
use the existing pytest runner and standalone Rust examples. Default pytest does
not collect the measurement files. CI remains published-release-only with Python
3.11/3.15; these opt-in native workloads are not added to unattended CI.

## Reproduce a software baseline

Start from a complete checkout. Record its commit, dirty status and the build
profile alongside all output. Run on external power, note CPU model, RAM, free
disk space, storage medium, OS image/build, libc, Rust/C++/CMake/Python versions,
power/thermal settings and background load. On the Pi also record kernel/ALSA
versions, device permissions and thermal/throttling state. Do not update the OS
or tools merely to obtain a result. A missing prerequisite is an open gate.

Use the prerequisites in README.md. Linux builds need ALSA development headers
and pkg-config; Linux execution needs libasound.so.2. MIDI sequencer I/O needs
access to /dev/snd/seq. Build inside an MSVC developer environment on Windows.
All commands below use bash, including Windows Git Bash with the MSVC environment
already configured. On a small board, start with one build job and observe memory
and free space. Do not install a swap file or change system settings implicitly.

```bash
mkdir -p target/qualification
export CARGO_HOME="$PWD/target/cargo-home"
export CARGO_BUILD_JOBS=1
uv sync --frozen --python 3.11
export PYO3_PYTHON="$PWD/.venv/bin/python"
# On Windows use: export PYO3_PYTHON="$PWD/.venv/Scripts/python.exe"
git rev-parse HEAD > target/qualification/revision.txt
git status --short > target/qualification/worktree.txt
rustc -vV > target/qualification/rust.txt
cmake --version > target/qualification/cmake.txt
cargo build --locked --release -p meedee --examples
# On Windows the executable has an .exe suffix.
target/release/examples/qualification > target/qualification/rust.jsonl
uv run --frozen pytest -q -s test/qualification/measure.py -k 'not native' \
  --junitxml=target/qualification/python.xml > target/qualification/python.log 2>&1
```

Repeat the Python measurements with 3.15 in a separate environment, using
`UV_PROJECT_ENVIRONMENT=target/qualification-python315` and
`uv run --frozen --python 3.15 ...`. Record which interpreter/build each output
belongs to. First-run costs are retained; the seven batch samples are not a
claim of statistical significance or comparable warm/cold CPU state. Repeat on
an otherwise idle target before setting a budget. Do not compare debug and
release builds as language overhead.

For release measurements, build the wheel from the sdist with the existing
release workflow procedure, install it plus pytest into a separate environment,
and run the absolute path to measure.py from outside the checkout using that
environment's Python. This avoids editable-build ambiguity. Keep the wheel's
name, size and build log. Run test/release/artifacts.py after the build. Run the
ordinary installed-package suites listed in .github/workflows/ci.yml, too;
performance measurements are not a substitute for those regressions.
Run the source Python suites and linked Cargo tests sequentially: the callback
regression rebuilds Cargo artifacts and concurrent interpreter builds can replace
the PyO3 libraries needed by another run's rustdoc step.

## Workloads and interpretation

| Workload | Fixed input | Measurements / checks |
| --- | --- | --- |
| Fresh Python process | Seven selected public modules, no clients | Import wall/CPU time, process peak RSS on Unix |
| Dense CC | 2,048 complete CCs per batch, seven batches | Valid decoded count, batch durations, operation rate |
| NRPN | 2,048 explicit 14-bit commands per batch, seven batches | Six-message selection/data/null expansion, encoding, operation rate |
| Long SysEx | 1 KiB, 64 KiB, 1 MiB payloads; 1 KiB feed chunks; seven batches | Complete delivery, batch durations, payload bytes/second |
| Tuning dump | 128 MTS bulk dumps per batch, seven batches | Pure encoding/explicit decoding only; never transmitted |
| UMP Clip | 2,048 MIDI 2 note events plus clock framing | Streaming read count, file size, seven read durations |
| Four-stream archive | 2,048 interleaved exact-time events | Seven write/read cycles, verified completion, file size |
| Recording, paced | Four recorders, 100 events each, 10 ms sleep per round | Admission, completion and failure reports; actual elapsed time |
| Recording, burst | Four recorders, 2,048 attempts each, no pacing | Saturation behavior and explicit incomplete reports, not a required zero-loss rate |
| Native MIDI 1 | 256 serial CC send/receive transactions | Open time, host round-trip samples, median/p95/max, recording fidelity for Python |

Rates include the validation performed in each workload. SysEx rates count
payload bytes, CC rates count messages and NRPN rates count compound commands.
These are batch throughput measurements, not per-message latency distributions.
Python validates entire decoded values where Rust checks item kinds/counts, so
these are practical path baselines, not equivalent-work language benchmarks.
The Clip benchmark excludes file creation; the archive benchmark includes it.
File tests exercise filesystem cache and do not fsync; they make no power-loss
or sustained physical-disk throughput claim. The paced workload is sleep-driven,
not a precisely maintained 100 Hz clock. Verify its actual elapsed time.

Unix process peak RSS includes native allocations and interpreter/test-runner
costs and is a lifetime high-water mark, not an incremental allocation estimate.
The fresh import runs separately from pytest. CPU measurements exclude child
processes. Windows RSS is explicitly null; use an OS process profiler rather
than interpreting that as zero. Native idle reports cover only the parent, so
they cannot quantify the cost of isolated workers. The Rust example reports
wall time only. Use OS profiling on the actual target to obtain current/peak
RSS, idle CPU, process/thread/handle counts and child-inclusive totals for each
mode. Those measurements remain open here; do not infer them from payload limits
or subtract two process high-water marks. OS process inspection is denied in
this tool environment.

## Native runs are a separate gate

Only run after the target owner authorizes test-owned virtual endpoints:

```bash
uv run --frozen pytest -q -s test/qualification/measure.py -k native \
  --junitxml=target/qualification/native.xml > target/qualification/native.log 2>&1
```

Each Python mode and the Rust example runs in a separate supervised process with
a 30 second deadline. On POSIX the supervisor kills its own process group on a
stall; Windows uses taskkill for that child's tree. Never launch native.py or
qualification_transport directly when backend liveness is unknown. Build the
Rust example first: a missing executable is a reported skip, not a pass.

The supervisor also runs the existing typed capture/replay/cancellation fixture
(`typed`) and native UMP SysEx7 CI discovery/reply fixture (`ump_ci`), each in its
own process. Select these with `-k 'typed or ump_ci'` to avoid repeating completed
latency measurements. These extended fixtures send NoteOn/NoteOff or CI management
messages only to their own endpoints; they never configure physical devices.

The `typed` fixture now enables recording pressure and playback lateness metrics
in both Python modes. It checks accepted-record/sample counts, capacity rejection,
peak reservations and consistency with the existing maximum-lateness report.
It sets no performance threshold and makes no physical latency claim.

The `sysex` supervisor case runs `test/native/meedee_sysex_loopback.py` under the
same 30-second process deadline. In isolated and in-process mode it transfers and
captures two SysEx messages (including a 4,097-byte payload), verifies exact SYX export,
and exchanges ordinary, banked key-based and banked scale/octave MTS dumps through
the production query host. A controlled peer verifies the one request before
sending synthetic replies and unrelated traffic. Original legacy checksum bytes
remain unchanged on export. Known handoff, timeout and pre-send cancellation are
checked without retries. These replies travel only to the fixture's owned
virtual inputs; no device state, acknowledgement protocol or interoperability is
inferred. One standard-library worker serves the peer, and the outer
supervisor remains the deadline for a stalled native call or worker teardown.

The latest available-Mac matrix uses these standalone commands from the project
root. The second interpreter is the existing isolated Python 3.15 environment;
the Rust example is interpreter-independent and is therefore run only once.
JUnit captures per-case stdout, stderr, process-resource observations and native
child output; artifacts remain under ignored target/qualification/.

```bash
uv run --frozen pytest -q test/qualification/measure.py -k native --junitxml=target/qualification/20261021-native311.xml -o junit_logging=all
uv run --frozen --no-sync target/api-python315/bin/python -m pytest -q test/qualification/measure.py -k 'native and not rust' --junitxml=target/qualification/20261021-native315.xml -o junit_logging=all
```

Names are unique to each run, and discovery must match exactly one test-created
virtual destination. No pre-existing destination is selected. The latency
workloads send only CC 7 values. Python checks captured events and the archive trailer. One outstanding
transaction at a time avoids interpreting queue buildup as one-way latency.
Host perf-counter durations are independent of backend timestamp clock origins;
there is no cross-clock subtraction or physical-wire latency claim.

WinMM does not supply virtual endpoint creation, so the portable virtual test
cannot qualify that backend without an independently arranged loopback route.
Do not silently choose a hardware endpoint or install a virtual driver. Agree a
Windows endpoint and test separately when a Windows host is available. Likewise,
ALSA raw is not qualified by sequencer loopback. The `ump_ci` supervisor case uses
test/native/meedee_ci_loopback.py; MIDI 1 measurements alone do not qualify live
MIDI 2. A successful CoreMIDI UMP loopback does not qualify other backends or
physical MIDI 2 equipment.

The `reopen` supervisor case repeats three virtual open/send/close cycles on
each of the same Input/Output clients, in both Python modes and MIDI protocols.
It also checks exact delivery of a full-width MIDI 2 CC on group/channel 16.
One keeper client and a private notification-loop thread remain in each
MIDI-using process until exit; include both in resource accounting. Per-client
operation threads are unchanged. Virtual creation/disposal waits for the matching
local event with a two-second deadline; SDK calls remain subject to the outer
supervisor. The keeper avoids
last-client disposal during normal operation, not recovery after a server crash.

If CoreMIDI client creation fails, compare with a wrapper-independent probe in
an ordinary Terminal session:

```bash
uv run --frozen pytest -q -s test/qualification/access.py
```

It compiles a small program against the installed Apple SDK, creates/disposes
one client and exits. It creates no endpoints and sends no MIDI. Compilation
and execution have separate 30/10 second limits. The agent execution context
returned 268435459 (0x10000003 / MACH_SEND_INVALID_DEST); the owner's ordinary
Terminal succeeded in both creation and disposal. Diagnose access context before
changing library code or restarting services.

For the repeated-reopen discovery failure, use the separate direct SDK probe:

```bash
uv run --frozen pytest -q -s test/qualification/reopen.py
```

This compiles a standalone C++ executable and starts a producer and consumer in
separate processes. Both register notification clients on their main thread and
run `CFRunLoopRun` continuously. Pipe readiness is handled by run-loop sources;
neither process blocks its owning loop awaiting a command or reply. The producer
is launched through `posix_spawn` into a fresh executable image.

Three cycles create/dispose only test-owned virtual MIDI 1 destinations and
ordinary clients. Each process retains a private keeper until exit. No MIDI is
sent and no hardware connections are opened. Each process waits for the matching
destination-added or destination-removed notification for its current operation;
the producer acknowledges only after observing its own notification. The consumer
accepts either notification/reply arrival order, then enumerates and verifies
that the created ID appears exactly once or that the removed ID is absent.
Every property-query failure fails the result. No discovery errors are suppressed.

Each notification wait has a two-second run-loop timer; the harness has a
20-second outer deadline and kills the entire process group on timeout, including
a native call that blocks a loop. Compilation has a separate 30-second limit.
Per-wait notification storage is bounded at 128 destination changes. Logs identify
the process role, cycle, notification IDs, endpoint handles and checked unique-ID
query results, without logging hardware names. Keep results separate from the
meedee transport qualification.

This replaces the earlier short-run-loop diagnostic modes, whose failures are
preserved in validation.md. It tests continuous event servicing in both processes
independently of the new production notification runtime. Run the whole file;
the earlier `-k notified` selection no longer applies. This successful direct
probe does not replace the production `reopen` supervisor case above.

Successful virtual MIDI does not qualify unplug/replug, device reboot, driver
crash, unresponsive hardware or musical state after uncertain sends. Those need
named equipment, explicit authorization and a test-specific recovery procedure.

## Pi and desktop build gates

Prefer an initial native Linux arm64 build on the actual chosen OS. Until the
image and ABI are selected there is no reproducible Pi cross-build command.
Cross-compiling requires a matching Linux aarch64 C++ toolchain/sysroot, ALSA
headers/libraries/pkg-config configuration and Python target ABI as well as
Rust's aarch64-unknown-linux-gnu target. The build explicitly requires
CMAKE_TOOLCHAIN_FILE across targets. A Rust target flag alone is insufficient;
macOS host libraries must never enter the target link. A native arm64 Linux build
or successful cross-link is still not a Pi runtime or performance result.

For each available desktop, run the source and installed-wheel checks from the
release workflow without publishing a release. Include the vendored device-free
native regression targets and sanitizer settings exactly as configured there.
Record compiler, OS/architecture, Python endpoint versions, test totals, wheel
from-sdist result, license/provenance content, external dynamic dependencies and
all skips. Keep Windows SDK integration, WinMM discovery test qualification and Rust
1.87 validation distinct from compilation with a newer compiler. The available
Mac now passes the Rust 1.87 checks below; Windows/Linux target qualification
remains separate. See issues.md.

### Rust 1.87 on the available Mac

The 2026-10-21 check uses the official Rust 1.87.0 aarch64-apple-darwin
distribution, verified against its published SHA-256 checksum and installed
locally under ignored target/qualification/rust187/. It does not replace the
system compiler or change the declared minimum version. With that installation:

```bash
export PATH="$PWD/target/qualification/rust187/bin:$PATH"
export CARGO_HOME="$PWD/target/cargo-home"
export CARGO_TARGET_DIR="$PWD/target/qualification/rust187-build"
export PYO3_PYTHON="$PWD/.venv/bin/python"
cargo fmt --all --check
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
cargo test --locked --offline --workspace -- --test-threads=1
```

Repeat Clippy and tests sequentially with PYO3_PYTHON pointing to the existing
target/api-python315/bin/python. Offline mode uses the already populated frozen
Cargo cache. Both Python 3.11.7 and 3.15.0 pass: 150 tests per interpreter,
five deliberately ignored tests, no failures, and no Clippy warning exceptions.
The ignored cases comprise two live CoreMIDI endpoint tests and three standalone
interpreter-finalization cases. This run does not execute those cases, rebuild
installed wheels or repeat the full Python suites. See validation.md for evidence.

## Acceptance review and remaining gates

After real Pi baseline measurements, agree application budgets for idle and peak
memory, idle CPU, endpoint/process/thread/handle counts, expected streams and
message rates, maximum payload, recording duration/storage and round-trip tail
latency. Rerun at the agreed normal workload and an explicit overload workload.
Check loss counters and capture completeness; an incomplete burst is evidence
of saturation handling, not successful recording of the offered rate. A larger
queue cannot be assumed to improve sustained throughput or latency.

Publish separate claims for immutable values/codecs, file fidelity, pure state
machines, native transport and specific hardware/OS versions. The feature matrix
and local baseline in validation.md record what is presently demonstrated.
Qualification is partial until the outstanding native/platform/resource gates
have evidence. The local archive v1 implementation review is complete; its
findings and corrective regressions are in archive-format.md and validation.md.
Format freeze, future compatibility guarantees and release/publication remain
explicit owner decisions. No performance acceptance threshold has been invented.
