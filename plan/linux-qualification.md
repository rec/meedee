# Linux qualification procedure

Additional work beyond the prompt

None.

## Scope and decisions before access

meedee targets macOS, Windows and Linux. This is the Linux part of
[platform/backend qualification](qualification.md), usable on an available Linux
host. A Raspberry Pi may serve as that host, but no board model or special Pi
milestone is required. Mac results in validation.md do not qualify Linux.
Preparing this procedure does not authorize host access, installations or
runtime tests.

Before starting, obtain:

1. The available Linux host and permission for read-only inventory, using local
   access or an agreed SSH hostname/user. Use existing credentials through the
   normal SSH setup; do not put credentials in reports.
2. The installed OS and permission to use it for qualification. Prefer a native
   build on that image first. This procedure does not select an OS release,
   reimage storage, cross-build sysroot, upgrade tools or modify unrelated projects.
3. Permission for a separate checkout, build/test storage and missing prerequisites.
   Identify missing tools before proposing installations. Python 3.11/3.15
   availability is a gate, not permission to download interpreters automatically.
4. A test scope: device-free baseline first, then separately authorized test-owned
   ALSA sequencer endpoints. Physical interface/cable tests require exact named
   equipment and authorization; existing instruments are not selected implicitly.
5. The intended application/workload: Rust/Python and isolation mode, stream count,
   message rates, maximum SysEx size and recording duration/storage. Measure a
   baseline before agreeing CPU, memory and latency acceptance limits.

## Stage 1: inventory without changing the system

Run in an agreed complete checkout. Record git revision/dirty status and preserve
unrelated files. Stop if this checkout has unrelated changes.
Use a new evidence directory rather than replacing a previous run:

```bash
mkdir -p target/qualification
qualification_run="$PWD/target/qualification/linux-$(date -u +%Y%m%dT%H%M%SZ)"
mkdir "$qualification_run"
git rev-parse HEAD > "$qualification_run/revision.txt"
git status --short > "$qualification_run/worktree.txt"
uname -a > "$qualification_run/kernel.txt"
cat /etc/os-release > "$qualification_run/os-release.txt"
getconf LONG_BIT > "$qualification_run/userland-bits.txt"
getconf GNU_LIBC_VERSION > "$qualification_run/libc.txt"
lscpu > "$qualification_run/cpu.txt"
free -b > "$qualification_run/memory.txt"
df -h . > "$qualification_run/free-space.txt"
lsblk -o NAME,SIZE,TYPE,FSTYPE,MOUNTPOINTS > "$qualification_run/storage.txt"
```

Record power supply, cooling, ambient conditions and background load separately.
Record thermal/throttling observations before/after workloads where the host
exposes them, for example through /sys/class/thermal. Missing utilities or denied
observations must be reported as unavailable, not zero. The libc command above
applies to glibc; record the actual libc with appropriate host tools otherwise.
Review build and wheel compatibility for the observed OS/userland ABI rather
than inferring it from hardware or kernel architecture alone.

Inventory installed `rustc -vV`, `cargo --version`, `cmake --version`,
`c++ --version`, `uv --version`, `python3 --version`, `pkg-config --modversion alsa`
and `timeout --version`. Check `/dev/snd/seq` permissions without opening an
endpoint. Pure codec/file workloads do not need sequencer access. Do not load
kernel modules, change groups, install swap, change governors or restart services.

Build prerequisites are Rust 1.87+, CMake 3.22+, a C++20 compiler, pkg-config,
ALSA development files and the selected CPython installation. Runtime needs
system libasound.so.2. Native UMP support is a separate compiled-backend/kernel/
ALSA capability check, not a consequence of working MIDI 1. Keep the image's
actual package versions in the evidence; no distribution or architecture is
assumed here.

## Stage 2: approved device-free build and checks

Only after prerequisites, disk space and build activity are approved, use one
build job initially. Do not reuse unrelated projects' environments. These
commands require an already available Python 3.11 and populated build inputs;
missing inputs stop the run for an explicit provisioning decision. `--frozen`
preserves dependency selections but may still fetch missing locked packages.
Use the current installed Rust compiler. Do not install or test an older compiler
locally; minimum-version compatibility is checked by release CI. Record the
compiler used for Linux runtime results without making minimum-version claims.

```bash
set -e
export CARGO_HOME="$PWD/target/cargo-home"
export CARGO_TARGET_DIR="$PWD/target"
export CARGO_BUILD_JOBS=1
export UV_PYTHON_DOWNLOADS=never
export UV_PROJECT_ENVIRONMENT="$qualification_run/python311"
timeout --kill-after=10s 30m uv sync --frozen --python 3.11 --extra cli
export PYO3_PYTHON="$UV_PROJECT_ENVIRONMENT/bin/python"
timeout --kill-after=10s 5m cargo fmt --all --check
timeout --kill-after=10s 30m cargo clippy --locked --workspace --all-targets -- -D warnings
timeout --kill-after=10s 30m cargo test --locked --workspace -- --test-threads=1
timeout --kill-after=10s 30m uv run --frozen --no-sync pytest -q
timeout --kill-after=10s 30m cargo build --locked --release -p meedee --examples
timeout --kill-after=10s 5m target/release/examples/qualification > "$qualification_run/rust.jsonl"
timeout --kill-after=10s 5m uv run --frozen --no-sync pytest -q -s test/qualification/measure.py -k 'not native' --junitxml="$qualification_run/python311.xml" > "$qualification_run/python311.log" 2>&1
```

Capture build/test stdout, stderr and each exit code in the evidence directory.
Run sequentially and stop on the first nonzero exit, timeout or resource problem.
The limits above bound investigation time; hitting a limit is not a performance
failure threshold or permission to change system settings. Record cold build
costs separately from steady workloads. Do not compare a debug Python extension
with optimized Rust as a language-overhead benchmark.

Repeat supported-endpoint checks with an independent Python 3.15 environment
only when that interpreter is available and approved. The optional uFor workflow
requires Python 3.13+; test it on 3.15 with the frozen optional dependencies.
Build wheels from the source distribution and run installed-package checks
outside the checkout using the release workflow, adapting the native Linux
wheel tag to the actual image. A locally linked wheel is not automatically a
portable manylinux artifact. Record dynamic dependencies and all skips.

## Stage 3: separately authorized software MIDI

After sequencer access and creation of test-owned virtual endpoints are approved,
start with the existing supervised MIDI 1 cases:

```bash
timeout --kill-after=10s 5m uv run --frozen --no-sync pytest -q -s test/qualification/measure.py -k 'native and (in_process or isolated or rust or typed or sysex)' --junitxml="$qualification_run/native-midi1.xml" > "$qualification_run/native-midi1.log" 2>&1
```

Each native child already has a 30-second deadline. The Rust example must exist;
a skip is not a pass. The fixtures select exact unique test-owned names and send
only to those destinations. They exercise default Linux MIDI 1 ALSA sequencer
I/O, not ALSA raw or a physical interface. Do not run all native cases blindly:
`ump_ci` and `reopen` also require native UMP and must wait for capability review.
macOS direct-SDK probes do not apply to Linux.

Measure the entire process tree for both Python modes, including isolated
workers. Collect wall/CPU time, sampled parent/child RSS, process/thread counts,
open file descriptors and thermal/throttling state. Label sampling intervals and
short-lived-process blind spots; summed sampled RSS is not unique physical
memory or an exact peak. Existing pytest metrics cover only parent CPU and
lifetime peak RSS, so they cannot close the child-inclusive resource gate.
Select an available OS profiler and agree its scope before measurement rather
than installing one implicitly. Record normal and overload workloads separately.

## Stage 4: hardware and acceptance, still open

Agree named interface, ports, cable route, traffic and duration before physical
tests. Unplug/replug, reboot, driver/service faults and instrument queries need
their own authorization and recovery procedure. They are not part of software
loopback. Do not send tuning/reset or cleanup messages to instruments implicitly.

The final report must identify host/configuration, OS/kernel/libc/ALSA,
toolchains, interpreter, checkout, wheel provenance, backend/mode, workload,
pass/fail/skip/timeout counts, loss/completeness and resource observations.
Agree acceptance budgets from the measured baseline and intended application,
then rerun normal/overload tests against those limits. Keep software, physical,
artifact portability and application acceptance claims separate. No release is
authorized by this procedure.
