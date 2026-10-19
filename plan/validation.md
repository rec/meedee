# Validation and release record

## Scope

Slices 7 and 8 add native check fixtures, CI, distribution checks, and usage
documentation. Build artifacts remain candidates for validation. Publication
and a release tag are separate decisions.

Additional work beyond the prompt

None.

## Local evidence, 2026-10-08

Host: macOS 14.5 (23F79), arm64; Rust 1.87; midir 0.11.0;
PyO3 0.29.3; maturin 1.15.0; uv 0.10.12.

| Interpreter | Binding unit checks | Native-thread lifecycle checks |
| --- | --- | --- |
| CPython 3.11.7 | 34 passed | 19 passed plus 2 isolated finalization checks |
| CPython 3.12.3 | 34 passed | Included in the Python runner |
| CPython 3.13.5 | 34 passed | Included in the Python runner |
| CPython 3.14.6 | 34 passed | Included in the Python runner |

The larger matrix exposed a test synchronization race: the native-thread stop
signal precedes client restoration. The test now waits for the connection lock
to confirm completed automatic cleanup before asserting client reuse. Production
behavior did not change.

Ruff, formatting, ty, pyupgrade, Cargo formatting, Clippy with warnings denied,
and the scoped diff check pass. After explicit owner authorization, all 25
CoreMIDI virtual-port checks passed on this host with CPython 3.11.7. No hardware
ports were connected.

## Default process isolation, 2026-10-08

The public constructors now default to one fresh child per client with a
five-second configurable native-operation deadline. `isolated=False` retains the
original native path. Controlled subprocess checks exercise hung initialization,
send/close, child crash, native returned errors, fresh-client recovery, stale
connection generations, local callbacks/error hooks, two-stage overflow counts,
GC/destruction, callback draining, and interpreter-exit termination/reaping.
No OS MIDI ports are opened by these unit fixtures. The full local Python suite
passes 59 cases; Ruff, formatting, ty, pyupgrade, Cargo formatting, and Clippy
with warnings denied pass. Partial communication-thread startup failure also
terminates and reaps the already-created child. The rebuilt CPython 3.11 arm64
wheel from the sdist passes both archive checks and 58 installed-package checks
outside the checkout, including controlled worker faults and lifecycle checks.
CI now exercises those isolation checks against installed wheels on every
matrix combination. [Run 37824404624](https://github.com/rec/midirp/actions/runs/37824404624)
passed all 16 jobs for implementation commit `5b5fc29`: Python 3.11–3.14 on
Linux x86_64, Windows x86_64, and macOS arm64/Intel. Each job passed Rust checks,
59 Python checks, two archive checks, and 58 installed-wheel checks outside the
checkout. This establishes portable worker logic/builds, not ALSA/WinMM native I/O
or physical hotplug behavior.

All 52 authorized CoreMIDI software-only cases passed in both modes on CPython
3.11.7. This includes endpoint disappearance and deliberate fresh-client
rediscovery, healthy close/reopen against an endpoint kept alive, exact bytes,
filters, SysEx, bursts, duplicate identities, and shutdown. No hardware was used.
Earlier isolated topology-churn runs produced stale ID `"0"` metadata failures
and occasional native initialization failures/timeouts. Failed clients were
contained, but their backend cause is not established. Graceful healthy-worker
disposal precedes forced termination; the final passing run does not prove
shared-service, hardware hotplug, or exhaustion safety.

Native Rust checks pass 36 ordinary tests plus three isolated finalization cases
run by the Python lifecycle harness. Parent callbacks still have no cancellation
deadline. Native ALSA/WinMM I/O and physical unplug/reboot checks remain unrun.

## ALSA failed-open containment (B1), 2026-10-08

Pinned midir 0.11.0 returns `ConnectErrorKind::Other("could not start ALSA input
handler thread")` from both input opening paths after moving the sequencer into
the spawn closure. The binding now marks that client failed before publishing
its returned state. Opening preserves `ConnectError`; subsequent discovery,
configuration, and opening fail with `RuntimeError` requiring a fresh client.
Ordinary returned errors preserve their existing restoration behavior.

A device-free Rust regression injects the same error and a returned client with
its sequencer removed. It failed before the fix because native reuse was
attempted, and passes after the fix. It checks metadata/configuration gating,
reopening rejection, cleanup with no live handle, and independent fresh-client
reuse. This verifies binding containment, not real ALSA thread exhaustion or
upstream resource rollback; native Linux fault injection remains unrun. No
native MIDI ports, dependency changes, or upstream modifications were needed.
All 59 Python checks pass, including the Rust lifecycle harness with 37 ordinary
Rust cases and three isolated finalization cases. Ruff, formatting, ty,
pyupgrade, Cargo formatting, and Clippy with warnings denied pass. CI is now
release-only, so this push does not schedule a new cross-platform run.

## ALSA failed-open resource disposal (B2), 2026-10-09

The GitHub workflow was verified to contain only `release: types: [published]`.
No push, pull-request, or manual-dispatch trigger remains.

Native ALSA input and output clients now use a discard-on-failed-open policy,
including `isolated=False`. Any native returned opening error disables the client
before disposing it outside the state lock. Opening retains its `ConnectError`;
subsequent operations require a fresh client. CoreMIDI/WinMM ordinary returned
errors keep their existing in-process restoration behavior. Successful close
still restores healthy clients. No dependency or upstream source was changed.

The pinned ALSA wrapper's `Seq::drop` invokes `snd_seq_close`. The
[ALSA cleanup contract](https://www.alsa-project.org/alsa-doc/alsa-lib/group___sequencer.html)
closes the client and releases its resources; it also broadcasts client exit and
disconnects its routes. The wrapper ignores the close return value, so source
review and binding tests cannot establish successful OS cleanup under failure.

A device-free regression injects partially allocated queues/ports and the native
stale-port, invalid-name, port-creation, input-subscription, and output-subscription
errors. It failed under restoration because allocations survived. Disposal now
runs once before error return, releases the fixture allocations, and prevents
reopening. Another test injects a destruction panic and verifies that the original
opening error survives and the client stays disabled. Native ALSA resource counts,
thread exhaustion, and close failures remain unmeasured on this Mac. Queue setup
panics/ignored errors remain listed under B6 rather than being marked fixed.

Local verification passes all 59 Python checks and 39 ordinary Rust cases plus
three isolated finalization cases through the Python lifecycle harness. Cargo
formatting, Clippy with warnings denied, Ruff, Python formatting, ty, pyupgrade,
and the scoped diff check pass. The release-only workflow was also verified
against GitHub's current default-branch file and its sole active workflow.

## CI and artifacts

The [CI workflow](../.github/workflows/ci.yml) has 16 combinations: CPython
3.11–3.14 on Ubuntu 24.04 x86_64, macOS 14 arm64, macOS 15 Intel, and
Windows 2022 x86_64. It checks Rust and Python, builds interpreter-specific
wheels from an sdist, and installs/tests each wheel in a fresh environment
outside the checkout. Actions are pinned to verified commit hashes.

Linux wheels target manylinux_2_28. maturin audits and repairs the wheel;
distribution checks require bundled libasound and its matching source RPM in
the download artifact. The third-party notices include the LGPL license and
describe replacement of the bundled shared library. macOS wheels use separate
architectures and system CoreMIDI frameworks. Windows wheels use WinMM.
Actual tags, native dependency inspection, and CI results must be recorded
before advertising the resulting artifacts as verified.

[Run 37787345883](https://github.com/rec/midirp/actions/runs/37787345883) passed all
16 jobs for code commit `1291242`. Each combination passed Rust checks, 34
Python unit checks including isolated finalization, two archive checks, and
33 installed-wheel contract checks outside the checkout. All 16 artifact
downloads were uploaded successfully. Linux archive checks confirm bundled
libasound and an accompanying source RPM. macOS jobs request deployment target
11.0; the local binary inspection below verifies that target on arm64.

The Windows runner initially selected Git's `link.exe` instead of the Microsoft
linker because its environment was too restricted. It now preserves the normal
compiler environment and overrides only the chosen Python executable and Cargo
cache in a scoped context. Subprocess env arguments stay implicit, avoiding
traceback dumps of inherited values. CI sets UV_PYTHON explicitly to prevent
the checkout's default 3.11 setting from changing interpreters mid-job.

The available GitHub credentials permit status inspection but deny log and
artifact downloads. The initial failure output came from the owner; subsequent
failures were readable through CI annotations. Distribution inspection in the
matrix runs on the runners. Local artifact inspection is recorded below.
CI now runs only on published GitHub releases. Pushes and pull requests do not
trigger the workflow.

The local CPython 3.11 wheel built from the sdist, passed both archive checks,
and passed all 33 installed-package checks in a fresh environment outside the
checkout. Its tag is `cp311-cp311-macosx_11_0_arm64`. Mach-O inspection confirms
minimum macOS 11.0 and links system CoreMIDI/CoreAudio/Foundation frameworks,
without a libpython dependency. CI requests the same macOS deployment baseline.

Opt-in archive checks:

```sh
uv run pytest test/release/artifacts.py -v
```

These verify Cargo/Python version agreement, absence of Python runtime
dependencies, native module placement, unchanged stubs, the typing marker,
third-party notices, and sdist source completeness. The sdist-to-wheel build
also checks that the archive can compile independently of the checkout.

## Callback function validation (A3), 2026-10-09

Both input opening methods reject coroutine, generator, and async generator
functions with `TypeError` before opening native input or starting delivery.
Checks cover ordinary functions, partials, callable objects, and partials of
callable objects in both isolation modes. Rejected callbacks leave the client
available. Synchronous functions, partials, and callable objects still deliver
the controlled fixture's message during opening.

All 73 Python checks pass, including the existing Rust lifecycle harness; Ruff,
formatting, ty, pyupgrade, and the scoped diff check pass. These are device-free
checks. Release-only CI was preserved; this push schedules no matrix run.
Synchronous wrappers returning deferred objects remain an application concern;
callback results are still ignored. Other A3 limitations remain in the inventory.

## Unsupported free-threaded build rejection (L6), 2026-10-09

Native lifecycle initialization now rejects free-threaded Python builds before
starting cleanup workers or registering exit cleanup, even if such a build has
temporarily enabled its GIL. Detection uses
[Python's documented build flag](https://docs.python.org/3/howto/free-threading-python.html#identifying-free-threaded-python),
`sysconfig.get_config_var("Py_GIL_DISABLED")`. This enforces the existing standard
GIL-enabled interpreter contract without new dependencies or build tooling.

A fresh-interpreter regression injects that flag, verifies the rejection before
exit registration and private-module publication, then successfully imports
after restoring the ordinary build flag. This tests the guard on standard
CPython, not a native free-threaded build or concurrency support.
All 74 Python checks, Cargo formatting, Clippy with warnings denied, Ruff,
Python formatting, ty, pyupgrade, and the scoped diff check pass.

## Bounded receive examples (R2), 2026-10-09

Both README receive examples now use a single-slot application queue and
nonblocking insertion. When occupied, they discard the newest message; each
example only needs its first received message. The callback cannot wait for
the consumer to free queue capacity, including during connection close.
This removes the examples' unbounded application buffering without changing
the library API or imposing a policy on applications. The remaining native
SysEx and library resource limits stay in the issue inventory.

Documentation review and the scoped diff check pass. No Python or data file
changed, so the test suite was not rerun for this documentation-only commit.

## Unhashable port typing (A4), 2026-10-09

Both public port stubs now declare `__hash__: ClassVar[None]`, matching their
existing runtime behavior. A focused type-checker probe returned each port as
`collections.abc.Hashable`: ty accepted both before the change and now rejects
both with incompatible `__hash__` diagnostics. The project type check still
passes. This establishes the corrected protocol surface with the existing ty
checker; it does not claim every checker diagnoses every direct `hash()` call.
All 74 Python checks, Ruff, formatting, ty, pyupgrade, and the scoped diff check
pass after the stub change.

## Mutable send buffers (A2), 2026-10-09

Public `send()` accepts bytes, bytearray, and memoryview, copying mutable buffers
and views into bytes before native work or serial waiting. Strided views retain
their logical contents; native and isolated paths receive the same snapshot.
Device-free checks cover all three added forms in both modes, including mutation
of the caller's buffer after the snapshot. All 80 Python checks, Ruff, formatting,
ty, pyupgrade, and the scoped diff check pass.

## Callback signature admission (A3), 2026-10-09

Opening rejects inspectable callbacks that cannot accept two positional
arguments, without invoking the callback or consuming client ownership.
Uninspectable callables remain accepted. Static `__call__` descriptors are
inspected through their actual callable to avoid inspect.signature dropping a
valid argument. Deferred-function checks retain their earlier error precedence.
Device-free checks exercise both opening methods and both modes for incompatible
signatures, plus valid callable objects, partials, and uninspectable callables.
All 83 Python checks, Ruff, formatting, ty, pyupgrade, and the scoped diff check pass.

## Context body and cleanup failures (A2), 2026-10-09

Public input/output context managers preserve simultaneous body and close errors
in an exception group, ordered body then close. Interrupts use BaseExceptionGroup;
ordinary errors use ExceptionGroup. Without a body error, close errors retain
their original exception type. Device-free checks cover both directions, both
modes, ordinary body errors, interrupts, and isolated close timeouts.
All 91 Python checks, Ruff, formatting, ty, pyupgrade, and the scoped diff check pass.

## Receive payload budgets (R2), 2026-10-09

`MidiInput(receive_byte_limit=8_388_608)` sets an 8 MiB default payload budget for
each isolated receive stage, alongside the existing 128-message limit. Both
stages reuse one queue implementation; admission and byte release occur under
its existing mutex. Oversized/full-budget messages are dropped without waiting
and counted. In-process mode adds no queue and does not apply this limit.

Device-free checks cover budget exhaustion, oversized single messages, capacity
recovery after dequeue, invalid settings, and configured child-stage drops.
All 98 Python checks, Ruff, formatting, ty, pyupgrade, and the scoped diff check
pass. Queue budgets exclude interpreter overhead, in-flight messages, native
unfinished SysEx, and application-retained messages; those R2 risks remain open.

## Binding error categories (A2/A4), 2026-10-09

The public API exports StateError, CallbackThreadError, NativePanicError,
ResourceError, WorkerError, and WorkerTimeoutError. They preserve the existing
RuntimeError/TimeoutError bases while distinguishing failures without parsing
messages. Native returned errors retain their four existing exception classes
and backend-specific detail. Worker replies preserve named categories.

Existing fault checks now assert actual state, callback-thread, thread-quota,
worker-exit, timeout, native-panic, and cleanup-capacity categories. Contract
checks cover all six public exception types and their bases. This does not
translate unreported backend events or infer device diagnoses from native text.
All 104 Python checks, Rust formatting, Clippy with warnings denied, Ruff,
Python formatting, ty, pyupgrade, and the scoped diff check pass.

## Worker failure and input activity observations (D1), 2026-10-09

Clients and connections expose worker_failure, retaining the first known worker
failure's type and arguments without its traceback. Fatal replies are recorded
before subsequent EOF can obscure their native error category. In-process mode
has no worker and reports None. No automatic reconnection or device-health
diagnosis was added.

Input last_message_time records Python monotonic seconds when callback delivery
starts, before application code, and survives close. Native and parent delivery
reuse the existing Rust callback gate, preserving the original callable in
sys.unraisablehook. The captured clock reference is GC-visible and released
outside the state lock during retirement. Queued/dropped messages do not advance
the observation. Device-free checks cover quiet input, early delivery, both
modes, callback error context, worker crash/timeout/native errors, and detached
failure tracebacks.
All 108 repository Python checks, Rust formatting, Clippy with warnings denied,
Ruff, Python formatting, ty, pyupgrade, and the scoped diff check pass. The
CPython 3.11 arm64 wheel rebuilt from the sdist passes both archive checks and
107 installed-wheel checks from outside the checkout. Ten focused, previously
authorized CoreMIDI virtual-port checks pass in both modes: successful/context
close, 128/1,024/16,384-byte SysEx including activity observations, and ordered
bursts. No hardware was used. These results do not establish WinMM/ALSA native
fault behavior, an upstream SysEx assembly bound, or device health. Release-only
CI remains unchanged and this push schedules no cross-platform run.

## Native backend checks

`test/manual/loopback.py` explicitly selects unique temporary software endpoints
by name and never opens an enumerated hardware device. It covers both virtual
directions, all eight filter masks and the default, exact note/CC/realtime/SysEx
bytes, timestamp type and monotonic ordering, duplicate names and distinct IDs,
missing/stale lookup, client reuse, context exceptions, and interpreter exit
with queued traffic. The shutdown case runs in a subprocess with a deadline.
Additional cases preserve 128-, 1,024-, and 16,384-byte SysEx messages after
close and check an ordered burst of 1,000 CC messages.

`test/manual/windows.py` checks unsupported virtual creation and preserved client
state without connecting hardware. It still initializes the native backend and
must be explicitly selected. Windows I/O requires a separately authorized,
explicitly chosen physical port or installed loopback driver.

| Backend | Recorded native I/O result |
| --- | --- |
| CoreMIDI | 25 software-loopback checks passed; macOS 14.5 arm64, CPython 3.11.7 |
| ALSA | Not run; requires a host with sequencer access |
| WinMM | Not run; requires a selected device/driver for I/O |

Native checks are selected locally with explicit authorization. Release CI
does not initialize an OS MIDI client. These checks do not
cover unplug/replug, driver failure, hardware timing, or a universal throughput
bound. The local 1,000-message CC burst measured about 165,567 messages/s;
send-to-callback median 0.174 ms, p99 0.692 ms, and maximum 0.753 ms. This is one
software-loopback sample under the host's current load, including GIL scheduling.
It is not a hardware latency measurement, timing guarantee, or loss threshold.

## Upstream close/drop review

Reviewed the pinned midir 0.11.0 backend sources, rather than assuming that
the three implementations have identical shutdown behavior:

- CoreMIDI input close takes the handler-data mutex and extracts its client/data;
  native port objects are disposed as the connection is consumed. The Python
  bridge retires and drains delivery before that native close.
- ALSA input close signals its pipe and joins the input thread before
  unsubscribing, freeing its timestamp queue, and deleting the port. Drop also
  uses this close path. It must not run on its input callback thread.
- WinMM input close takes its native handle lock, resets/stops input, unprepares
  and releases SysEx buffers, then closes the handle. Reset can invoke callbacks;
  the retired bridge refuses Python delivery during teardown. Output close/drop
  uses the backend's native handle cleanup.

The production cleanup worker and GIL release address these ownership and
callback constraints. Source review does not prove driver shutdown safety.

## Release gates

- Public API, stubs, byte ownership, exceptions, state restoration, concurrent
  close, GC, and finalization have device-free coverage.
- Original package name `midirp`: PyPI JSON endpoint returned HTTP 404 on 2026-10-08.
  This is a point-in-time check, not a reservation or guarantee of availability.
- Project license: MIT, explicitly chosen by the owner. LICENSE and the
  third-party notices are included in source and wheel distributions.
- Cross-platform artifact validation and native I/O results remain separate gates.
- No publication credentials, publish workflow, release, or tag were added.

## Port identity mitigation (D2), 2026-10-09

Public ID lookup now enumerates both modes and rejects duplicate matches with
`PortInfoError`; a missing ID still returns `None`. CoreMIDI ID `"0"` is rejected
when reading a handle, looking up an ID, reading metadata, or opening. Isolated
worker discovery and resolution apply the same sentinel guard. Linux/Windows
identifiers are not interpreted as CoreMIDI IDs. Focused device-free regressions
cover both directions, both modes, unique/missing/duplicate lookup, and the
platform-specific sentinel, including rejection before native metadata/open.
The remaining D2 text describes reused IDs, stale equality, and backend metadata
limits that this mitigation cannot remove.

All 120 repository tests pass, including the native lifecycle harness. Ruff,
formatting, ty, pyupgrade, and the scoped diff check pass. No native MIDI ports
were opened. Release-only CI remains unchanged.

## Deferred callback result mitigation (A3), 2026-10-09

The shared Rust callback bridge now reports returned coroutines, generators,
async generators, and other awaitables as `TypeError` through
`sys.unraisablehook`, with the original callable as context. Ordinary results
remain ignored; later deliveries continue. The check runs in the existing
callback gate without a callable wrapper. Its cached type references are tracked
for GC and retired outside the state lock. Only unstarted native coroutines are
closed to avoid duplicate unawaited-coroutine warnings; suspended coroutines
and other deferred objects are not executed or cancelled.

Device-free tests cover all four deferred result forms through public input
connections in both modes, original error context, subsequent delivery, no
deferred execution, and coroutine warning suppression. A further regression
checks that a suspended coroutine is reported without cancellation. Resolved
silent-deferred-result text is removed from A3; callback signature uncertainty,
thread constraints, and application synchronization remain documented.

All 129 repository tests pass, including 39 ordinary Rust tests and three
isolated finalization checks through the Python harness. Ruff, formatting, ty,
pyupgrade, Cargo formatting, Clippy with warnings denied, and the scoped diff
check pass. No native MIDI ports were opened. The workflow still contains only
the published-release trigger; no cross-platform CI run was scheduled.

## CPython 3.15 compatibility, 2026-10-10

Host: macOS 14.5 arm64; Homebrew standard GIL-enabled CPython 3.15.0;
Rust 1.99.0; pinned midir 0.11.0, PyO3 0.29.3, and maturin 1.15.0.
Separate environments preserve the checkout's Python 3.11 default.
`pyproject.toml` now declares the Python 3.15 classifier; `requires-python`
remains `>=3.11`. No dependency versions, lockfile, Python minimum, or release
workflow changed. The release matrix still covers 3.11–3.14 and runs only on
published releases.

All 129 repository tests pass under 3.15, including 39 ordinary Rust tests and
three isolated finalization checks through the Python lifecycle harness. A
release wheel built from the source archive passes all 128 applicable tests
outside the checkout. Wheel/source metadata checks confirm the 3.15 classifier,
Python minimum, lack of runtime dependencies, and current packaged Python
sources/stubs. The artifact is
`midirp-0.1.0-cp315-cp315-macosx_11_0_arm64.whl` (the package name at that time).

Ruff, formatting, ty, pyupgrade with the retained 3.11 minimum, Cargo formatting,
and the scoped diff check pass. Strict Clippy with Rust 1.99 fails on the existing
`AtomicU8::fetch_update` deprecation in `src/lifecycle.rs:104`; the successful
release build reports the same warning. This is a compiler-version finding,
not a Python 3.15 build failure. No warning suppression, Rust-version increase,
or unrelated implementation change was made.

Eight authorized software-only CoreMIDI close/reopen and SysEx checks were
attempted against the installed 3.15 wheel in both execution modes. All failed
at client construction with `InitError`, before any virtual endpoint was
created. The two close/reopen comparison cases also fail at initialization
under the checkout's CPython 3.11.7. This does not establish a 3.15-specific
defect; the native initialization failure's cause remains undiagnosed. Native
MIDI I/O under 3.15, Linux/Windows 3.15 builds, and cross-platform 3.15 fault
behavior remain unvalidated. No hardware was accessed.

## Project rename to meedee, 2026-10-13

The Python distribution, package/import paths, Cargo package, native class and
exception module names, worker entry point, test fixtures, and release artifact
names now use `meedee`. The toolkit plan is `plan/meedee.md`. Existing GitHub
URLs and historical wheel names remain accurate records of the original project.
No dependency versions or backend implementation changed.

All 129 device-free repository tests pass on CPython 3.11.7 and 3.15.0 on macOS
arm64. A renamed 3.15 release wheel built from its source archive passes all
128 applicable Python tests in a separate environment outside the checkout.
Both existing distribution checks pass against the new wheel and source archive,
including metadata, typing, native module, Python sources, and test fixture paths.
Ruff, formatting, ty, pyupgrade with the 3.11 minimum, Cargo formatting, and
`git diff --check` pass. The release build still reports the previously recorded
`AtomicU8::fetch_update` deprecation warning. No native MIDI endpoints were opened.

The default development environment was restored to Python 3.11. Release CI
continues to run only on published releases, testing Python 3.11 and 3.15.
The GitHub repository and local checkout directory have not been renamed.

## Immutable API prototype, slice 1, 2026-10-13

The owner selected native frozen PyO3 values, separate messages/events/transport
modules without top-level re-exports, and keyword-only strict constructors.
The test-only prototype checks `Message`, `NoteOn`, and `SysEx`; production
transport and package dependencies remain unchanged. See [api.md](api.md) for
the API contract and measurements and [specifications.md](specifications.md)
for the official document baselines and acquired-file identities.

All 129 repository tests pass under CPython 3.11.7, including the Rust harness's
two new prototype tests and the existing shutdown/finalization checks. The
focused release prototype also passes under CPython 3.11.7 and 3.15.0. Both
distribution checks pass against a Python 3.15 wheel built from the source
archive, including the new Rust prototype and Python fixture in that archive.
Ruff, formatting, ty, pyupgrade, Cargo formatting and `git diff --check` pass.
Clippy completes with the existing Rust 1.99 `fetch_update` deprecation warning;
strict `-D warnings` fails at that unchanged line. No new Clippy findings appeared.

Native values use less Python-tracked memory than the minimal slotted dataclass
baseline; optimized construction also measured faster on this host. Results do
not measure Rust heap payload allocations, total resident memory, real stream
load or Pi performance. No MIDI clients or endpoints were opened.

Git staging initially failed because the session denied creation of
`.git/index.lock`, independently of GitHub authentication. The permission
was corrected on 2026-10-13 so the pending prototype could be committed.

## CoreMIDI fixes in the libremidi fork, 2026-10-13

Two separate commits were created and pushed to `rec/libremidi`:

- `69e060e61b2d4962f283a9838f3307edb050a9bd`: failed integer-property queries
  return no value; discovery reports an error and omits the affected port.
- `46853bd6b0e53e25d918076fca9752552fe49414`: string properties use checked,
  length-aware UTF-8 conversion with Core Foundation ownership, removing the
  uninitialized fixed buffer and termination assumption.

The first commit's two focused cases passed before it was committed. The final
four cases pass all 30 assertions, including AddressSanitizer and
UndefinedBehaviorSanitizer builds on macOS arm64. Property getter stubs emulate
failure without writing the output. Real Core Foundation string conversion is
checked with long ASCII, multibyte text, embedded NUL, empty values and an invalid
UTF-16 surrogate. The tests create no MIDI clients or endpoints and query no devices.

The fork's selected build sources and focused test sources are imported under
`third_party/libremidi`, pinned to the second commit. Every imported file's raw
Git blob hash matches that revision. The vendored copy independently builds and
passes the focused target. Provenance, retained license texts and omissions are
recorded in `third_party/libremidi/SOURCE.md`. The ASCII input-name conversion
is deliberately unchanged; it was not one of the two requested fixes.

This source import does not yet link libremidi into meedee or remove midir.
Meedee's `.git/index.lock` initially prevented the source-import commit.
The permission was corrected on 2026-10-13; the fork commits were already pushed.

## Slice 2: checked native input teardown, 2026-10-13

Fork commit `debce21b71838cf205749ceafd9a17da397af823` adds checked CoreMIDI endpoint/port disposal and a C API
ownership contract: failed free retains the input; failed open retains it when
rollback fails. The caller must keep callback context alive. Direct C++ destruction
and other backends' internal disposal guarantees are not proven by this patch.

Focused property tests (30 assertions) and teardown tests (40 assertions) pass
normally and under AddressSanitizer/UndefinedBehaviorSanitizer on macOS arm64.
Tests use fake CoreMIDI handles and no MIDI device access. The owner's dirty
`/Users/tom/code/libremidi` checkout was not edited. Sources were imported from
the published fork into the pinned vendor directory; transport replacement remains
unfinished and no native whole-backend safety claim is made.

Follow-up fork commit `56eedef9c6b6d8a5a4cc5bed0d4bd2cdaf08d209` limits explicit pre-delete close to CoreMIDI:
other backends' destructors may close again without idempotency. The same focused
normal and sanitizer tests pass after this correction.

## Slice 2: independent Rust values

`cargo test --locked -p meedee` passes the two boundary/payload tests without
Python. `cargo tree -p meedee` contains only the toolkit itself; it has no external
dependencies. `cargo clippy --locked -p meedee --all-targets -- -D warnings` and
`cargo fmt --all --check` pass. The Python representation contract passes on
Python 3.11 using the extracted Rust values. The existing Python suite passes
all 129 tests after rebuilding. No dependency versions changed in Cargo.lock.
The production transport still uses midir pending the rest of slice 2.

## Slice 2: private extension and public messages

The binary is now `meedee._core`. Public `meedee.messages` and `meedee.midi`
loaders register native definitions; message import does not initialize cleanup,
import transport clients, or create MIDI resources. NoteOn/SysEx wrappers use the
shared Rust values and the existing frozen representation fixture. Python 3.11
passes 131 tests, including public-message and fresh-import checks. The Python
3.15 native representation contract and all 131 Python tests pass. A release wheel built from its sdist
and both artifact checks pass with the new binary and independent Rust crate.
Ruff, formatting, ty, pyupgrade and Cargo formatting pass. The existing Rust
1.99 fetch_update deprecation warning remains; MSRV stays 1.87.

## Slice 2: Windows checked input teardown, awaiting native qualification

Owner-approved fork commit `0932a62c05117a8f9b9d9b86ebac326c059531ba` extends checked C input free to WinMM
and Windows MIDI Services. The shared guard refuses self-close even with nested
or overlapping callbacks and waits for active callbacks after source retirement.
WinMM retains partial resources after timeout; MIDI Services checks raw callback
removal and makes successful teardown idempotent. Failed free retains the handle
and requires the caller to retain its callback context.

The callback-lifetime target (14 assertions / three cases), property target
(30 / four), and CoreMIDI teardown target (40 / three) pass on macOS arm64
normally and with ASan/UBSan. The Windows backend headers have not been compiled
or run on this host. Native Windows validation is outstanding; these results do
not certify Windows driver/API teardown guarantees. Direct C++ destruction is
still outside the checked C API lifetime contract. The production transport
remains midir until integration and qualification are complete.

Fork follow-up `07c7cfb1237be1ddeaa097a81f79cc9456c087ee` adds `winmm_teardown_test`, using fake WinMM
MIDI device calls and real Windows synchronization. It is Windows-only and
unvalidated here. No Windows compiler or runner is currently available to this
thread. The owner confirmed that no Windows machine is currently available; native
Windows validation remains open while Rust/macOS work continues. The existing
release-only GitHub workflow was not changed.

From a separate Windows checkout of the pinned fork, byte-backend qualification
(no hardware or Windows MIDI Services runtime required):

```powershell
cmake -S . -B build -DLIBREMIDI_TESTS=ON -DLIBREMIDI_NO_BOOST=ON -DLIBREMIDI_NO_WINMIDI=ON -DLIBREMIDI_NO_WINUWP=ON -DLIBREMIDI_NO_KDMAPI=ON -DLIBREMIDI_NO_JACK=ON -DLIBREMIDI_NO_PIPEWIRE=ON -DLIBREMIDI_NO_NETWORK=ON -DLIBREMIDI_NO_KEYBOARD=ON
cmake --build build --config Debug --target libremidi winmm_teardown_test callback_lifetime_test
ctest --test-dir build -C Debug -R '^(winmm_teardown|callback_lifetime)_test$' --output-on-failure
```

This does not compile the Windows MIDI Services backend. That separate gate
requires the pinned Services SDK headers and C++/WinRT tooling, with build-time
downloads disabled or pinned. Do not infer native UMP qualification from a
successful WinMM build. Integration of the dedicated client thread, safe native
wrapper, transport replacement and Pi cross-build are still unfinished.

## Slice 2: Rust wrapper and macOS integration, 2026-10-14

The Rust core statically builds the pinned libremidi source through CMake, with
C++ exceptions contained by a private C ABI bridge. Native clients use dedicated
owner threads, a bounded command queue and a 32-client process capacity including
retained resources. Native handles and cloned ports do not cross Rust threads.
Callback panic is caught, callable invocation holds no Rust internal lock, receive
admission is bounded in bytes and lost payloads are counted. Failed input teardown
retains the owner thread as well as resources, avoiding COM apartment exit with
live native callbacks. This is in-process Rust I/O; Python isolation is unchanged.

The device-free macOS fixture substitutes CoreMIDI endpoint APIs only in its own
test executable, while using real Core Foundation and packet helpers. It checks
byte and native UMP traffic in both directions, timestamp disabling, clock identity
on reopen, unchanged connections after unrelated client drop, and callback-state
retention on failed disposal. Its local Cargo package is a development dependency;
no fixture symbols are linked into the real loopback executable or production
build. `nm` shows the real executable imports `_MIDIClientCreate` from Apple.

Owner-approved output fixes are published in the fork as `b6a0d29` (checked
CoreMIDI output endpoint/port disposal and failed-open rollback) and `53a9451`
(initialized WinMM output handle and short-message DWORD). The vendor matches
`53a94516cc280fa63f6de12bfe279ba3c26d02b9`. CoreMIDI teardown now passes 83 assertions
in five cases; the property and callback-lifetime targets retain their 30 and
14 assertions. All three targets pass normally and under ASan/UBSan on this Mac.
The new Windows-only `winmm_output_test` is included in release CI but remains
uncompiled here. Client-level CoreMIDI disposal errors, other output backends'
cleanup and direct C++ destruction are outside the checked teardown guarantee.

The Rust core passes 12 unit tests and six device-free macOS boundary tests.
Workspace tests pass 58 ordinary cases with five ignored cases. Embedded Python
tests must run with `--test-threads=1`, matching their existing pytest harness:
parallel execution crashed in CPython's tracemalloc/thread-state path during
verification. Release CI uses serial Rust test execution. The changed Rust crate
passes strict Clippy; the existing Python crate emits a Rust 1.99 deprecation
warning for `AtomicU8::fetch_update`. CI retains MSRV 1.87, where the replacement
API is unavailable. No dependency versions were changed.

Python 3.11 and 3.15 each pass all 131 checks, including the three isolated Rust
finalization cases. A CPython 3.11 wheel built from the source distribution
passes both archive checks and all 130 installed-package checks outside the
checkout. The archives include the C++ bridge, vendored sources/license and
test-only CoreMIDI fixture sources needed to rebuild the Rust tests. Ruff,
formatting, ty, pyupgrade and Cargo formatting pass.

A supervised real virtual-port attempt failed at client creation with OSStatus
268435459 (0x10000003). Apple's installed SDK identifies this as
MACH_SEND_INVALID_DEST. The existing midir path also fails to initialize MIDI in
this tool environment. This is evidence of unavailable service access here,
not a successful real CoreMIDI byte/UMP validation. No physical device was used.

The existing release-only workflow now includes a separate device-free C++ native
regression matrix for macOS arm64/x86_64, Linux and Windows. Non-Windows jobs use
ASan/UBSan; Windows runs the fake-device WinMM input and output targets. Rust checks
include the workspace. CI has not run: no release was created and workflow
triggers are unchanged. Windows MIDI Services is explicitly disabled until pinned
SDK/tooling integration and native compilation can be qualified. WinMM success
would not establish Windows UMP support.

Windows discovery also needs a capability-query failure audit: the pinned
WinMM observer/name helpers read MIDIINCAPS/MIDIOUTCAPS without checking every
device-capability query's return value. Device disappearance during enumeration
could leave those structures unwritten. The two output fixes above do not resolve
this separate path; Windows compilation alone would not certify it.

Remaining slice-2 work includes Python transport replacement, real macOS I/O,
Windows/Services qualification and the selected Pi cross-build. The completion
reminder to submit fork patches upstream remains in plan/meedee.md.

## Python transport replacement, 2026-10-14

The Python binding now uses the Rust toolkit and vendored libremidi in both
isolated and in-process modes. The midir dependency and its old transport API
are removed. Public Input/Output clients select immutable discovery snapshots;
callbacks receive immutable byte/UMP events with exact optional signed
nanoseconds and connection-specific clock identity. This completes the Python
replacement portion of slice 2, not its platform qualification gate.

Python 3.11.7 and 3.15.0 each pass 137 tests. Coverage includes raw UMP/byte
values, strict buffer ownership, zero/negative/disabled timestamps, stale and
foreign snapshots, consumed failed opens, worker crashes/timeouts, queue
budgets and live native drop counters, interrupted operations, allocation
failures and shutdown draining without a live client. Controlled worker faults
exercise the production parent/child protocol; they do not qualify OS drivers.

Serial Rust workspace tests pass 55 ordinary cases: 12 core unit tests, six
device-free CoreMIDI boundary cases and 37 binding/lifecycle tests. The latter
include a new Python-adapter-to-C++-backend regression using the existing fake
CoreMIDI fixture, covering byte and UMP I/O, clock metadata and client reuse.
Five tests remain ignored in the ordinary Rust run: two real CoreMIDI checks
and three finalization cases run separately by pytest's supervised harness.
No production extension links the fake CoreMIDI fixture.

Ruff, formatting, ty, pyupgrade and Cargo formatting pass. Workspace Clippy
passes with warnings denied except the previously recorded Rust 1.99
AtomicU8::fetch_update deprecation; CI retains Rust 1.87. No external dependency
versions changed. Removed dependency notices track the pruned Cargo lockfile.

A CPython 3.11 wheel rebuilt from the source distribution passes both archive
checks. The new public loaders, private wire protocol, native Rust sources and
existing test-only fixture are included. All 136 installed-package checks pass
outside the checkout. Linux packaging now requires system libasound.so.2 rather than
bundling ALSA, matching libremidi's dynamic loading behavior.

The authorized, supervised real macOS virtual-port check again returned
OSStatus 268435459 at client creation, before creating an endpoint. It returned
promptly; no hardware was selected. Real macOS byte/UMP I/O remains unqualified.
Windows/Services and the selected Pi cross-build also remain open. The
release-only trigger, Python 3.11/3.15 matrix and workflow shell syntax were
verified locally; no release was created and CI has not run.

## Slice 3 MIDI 1.0 values and byte codec, 2026-10-14

The shared Rust core and Python binding now expose all 18 primitive message
families in the documented scope. The pure byte codec preserves public
channel/program numbering and signed pitch bend, supports arbitrary feed
boundaries, running status and real-time interleaving, and reports ordered
diagnostics without calling user code. Retained SysEx payload defaults to a
configurable 1 MiB cap. Non-F7 SysEx termination is deliberately discarded with
a diagnostic, despite the standard allowing implicit termination. Timestamps,
transport integration, receive filters, MTC interpretation and later protocol
families are outside this slice.

Python 3.11.7 and 3.15.0 each pass 164 tests. The Rust workspace passes 60
ordinary tests with five ignored cases, including the unchanged device-free
native regressions; pytest separately runs the three finalization cases. Five
new Rust codec tests cover shared independent wire vectors, every two-part feed
split of those vectors, all 2,048 feed partitions of a representative mixed
stream, all channel statuses with every first data value and second-byte
boundaries, all 65,536 arbitrary byte pairs in a stateful malformed stream, and
SysEx overflow/recovery. Python adds native value immutability, strict arguments,
checked replacement, pattern matching, raw buffer ownership, recovery diagnostics,
reset/finish and pure-import checks. Test-vector provenance is recorded in
[specifications.md](specifications.md).

Cargo formatting, workspace Clippy and Python Ruff, formatting, ty and pyupgrade
pass. Clippy uses the same local exception for the existing Rust 1.99 atomic
deprecation; the release workflow retains Rust 1.87. No dependency versions
or native backend sources changed. A CPython 3.11 wheel builds from the source
distribution, all 163 installed-package tests pass outside the checkout, and
both distribution-content checks pass. The source archive
contains the shared vectors and Rust codec tests. Release CI now includes the
installed codec tests; its published-release-only trigger, Python 3.11/3.15
matrix and shell syntax pass local checks. CI has not run.

This slice needs no hardware access and no new native runtime flow was run.
The real macOS I/O, Windows/Services and selected Pi qualification gaps from
slice 2 remain open. Passing pure codec checks does not close those gates.

## Slice 4 fine controls and RPN/NRPN, 2026-10-14

Seven frozen command families, primitive expansion, per-connection serialized
compound output and the pure per-source interpreter are implemented. The
approved conservative policies are recorded in [api.md](api.md): explicit
resolution, fresh selectors on output and after edited selections on input,
RPN-null release, wire-order CC pairing, indexed observations and explicit loss
reset. Raw transport, primitive codecs and timestamps retain their existing
surface. Rust's approved common Message marker now has PrimitiveMessage and
Command subtraits; free primitive encoding retains its behavior.

Python 3.11.7 and 3.15.0 each pass 183 tests. The Rust workspace passes 69
ordinary tests with five ignored cases; pytest separately runs the three
finalization cases. Six new pure Rust control tests and Python tests consume
independent command vectors and cover coarse/fine ordering, repeated LSBs,
partial/family-changing selectors, RPN-null versus NRPN 16383, direction-only
steps, stale absolute context, reset/loss and source/channel separation.
Python also covers frozen command/observation contracts, strict arguments,
replacement and pure imports without transport initialization.

Two added device-free native boundary tests inject failure at every boundary
of a six-message command, verifying the exact accepted prefix and absence of
retry/extra cleanup, and race 50 commands against 50 raw sends to verify no
interleaving within a command. The Python adapter fixture checks both successful
compound sending and conversion of a native partial failure into structured
progress, subsequent send rejection and eventual teardown. Its Python exception
helper is exercised with the uv-built binding package. Error construction is
outside the connection mutex. Native ownership remains distinct from failed
state: in-process closed becomes true only after teardown finishes.

Controlled worker faults preserve all six known failure prefixes across the
pipe and classify crash/timeout progress as unknown. An acknowledgment/failure
race preserves the fully accepted count if the complete reply arrived first.
Known failure diagnostics remain traceback-free on worker_failure. The original
raw transport tests also pass with the structured exception-argument protocol.

Cargo formatting, workspace Clippy, Ruff, formatting, ty and pyupgrade pass.
The existing Rust 1.99 atomic deprecation remains the only local Clippy warning
exception; CI retains Rust 1.87. No dependency versions or vendored backend
sources changed. A CPython 3.11 wheel rebuilt from the source distribution
passes all 182 installed-package tests outside the checkout and both archive
checks. Sources include the control modules, exception helper, native boundary
tests and shared command vectors. The release-only trigger, endpoint Python
matrix and workflow shell syntax were verified; no release/CI run was created.

No new hardware/native-service flow was run. Real macOS I/O, Windows/Services,
physical failure scenarios and selected Pi qualification remain open. Per-client
serialization and software fixtures do not prove atomicity against other
applications, physical delivery, parameter-specific units or native-driver
liveness. Selection release and exact progress cannot be guaranteed after a
failed/unknown operation; no recovery send is attempted automatically.

## Slice 5 MIDI Tuning Standard, 2026-10-15

The pure Rust/Python tuning module implements exact pitches, explicit NoChange,
128-note tables, signed scale/octave offsets, original/banked dump requests and
replies, original real-time single-note changes, banked real/non-real-time note
changes and both real/non-real-time scale/octave resolutions. MTS interpretation
is explicit after the ordinary byte decoder produces SysEx. Exact constructors
never quantize; Hz/cents conversions report the signed error from nearest,
ties-to-even wire quantization. No clamp or implicit device broadcast is added.
The approved API and legacy-checksum policy are recorded in [api.md](api.md).

Python 3.11.7 and 3.15.0 each pass 189 tests. The Rust workspace passes 74
ordinary tests with five ignored cases; pytest separately runs the three
finalization cases. Five new pure Rust tests and six Python tests exercise
independent shared wire vectors covering every implemented family and real-time
alternative. They cover all recognized truncated lengths, extra data, legacy
checksum status versus strict modern checksums, pitch and scale endpoints,
no-change preservation, exact frequency round trips, rounding ties/error,
nonfinite/out-of-range rejection, immutable snapshots, checked replacement,
channel bitmap order/reserved bits and explicit decoding. The import fixture
forbids cleanup initialization while importing and using tuning values/codecs.

Caller-driven reply tests cover device/bank/program matching, both original and
banked request types, unicast-only tracking, unrelated replies, exact-deadline
expiry, terminal state, cancellation, backwards time and deadline overflow.
No transaction-ID or acknowledgment semantics are invented. Identical late or
unsolicited dumps remain indistinguishable; callers retain their traffic and
choose how to handle legacy-checksum status.

Cargo formatting, workspace Clippy, Ruff, formatting, ty and pyupgrade pass.
Clippy retains the existing local Rust 1.99 atomic-deprecation exception; the
release workflow retains Rust 1.87. No dependencies or native backend sources
changed. A CPython 3.11 wheel built from the source distribution passes all
188 installed-package tests outside the checkout and both archive checks. The
archives include the new modules, typing and shared fixture/test sources.
Release CI includes the installed tuning tests; its trigger remains published
releases only and its Python endpoints remain 3.11/3.15. No CI run was triggered.

No instrument operation or new native-service flow was run. Windows/Services,
real macOS I/O, physical failure scenarios and selected Pi qualification remain
open. Codec/state-machine evidence does not qualify device tuning behavior or
change the transport's delivery, native-driver or liveness guarantees.

## Slice 6 General MIDI and bounded profile helpers, 2026-10-15

Rust owns the GM1/GM2 sound and percussion maps and all new codec logic. Python
exposes frozen native entries/messages in gm, universal, mpe and timecode, plus
named CC/channel-mode/RPN helpers in controls. GM1 banks remain unspecified.
Compound expansion now returns primitive MIDI 1.0 messages, allowing explicit
bank/program selection through the existing serialization/progress path.
No backend, dependency version or runtime initialization policy changed.

Python 3.11.7 and 3.15.0 each pass 198 tests. The Rust workspace passes 78
ordinary tests with five ignored cases; pytest separately runs the three
finalization cases. Four new pure Rust tests and six Python tests cover shared
independent wire vectors, strict immutable values/replacement, identity variants,
malformed/truncated messages, rate bounds, map counts/addresses, drum inheritance
and silence, named controls and explicit typed GM interpreter resets. The pure
import fixture exercises every new namespace without transport initialization.

Device-free native boundary tests now check every known failure prefix of both
the six-message NRPN command and three-message bank/program command, preserving
no-retry behavior. Isolated-worker tests verify exact bank/program and MPE bytes
and known partial progress across the pipe. These checks establish software
ordering and backend acceptance reporting, not atomic physical instrument changes.

Cargo formatting and workspace Clippy pass, retaining only the existing local
Rust 1.99 atomic-deprecation exception; CI still targets Rust 1.87. Ruff,
formatting, ty, pyupgrade and whitespace checks pass. The release workflow adds
the installed profile tests while retaining published-release-only execution
and Python endpoints 3.11/3.15. No release or CI run was triggered.

The approved deferrals are recorded in meedee.md and api.md. No hardware or new
native-service flow was run. Windows/Services, physical device behavior, real
macOS I/O and selected Pi qualification remain open. GM2 message completeness,
MPE voice/expression engines, quarter-frame locking and instrument timing or
capability guarantees are not claimed.

A CPython 3.11 wheel rebuilt from the source distribution passes all 197
installed-package tests outside the checkout. Both distribution archive checks
pass, including the four new module/stub pairs, Rust tables and profile fixtures.

## Slice 6.5 profile codecs and state engines, 2026-10-15

Implemented the approved bounded generic GM2-related SysEx codecs, explicit
no-stealing MPE allocation, overlap-aware per-zone tracking, CA-031 velocity
interpretation and caller-driven MTC sequence assembly. Rust owns the behavior;
Python exposes frozen native values/observations. Source/zone/reset ownership,
resource limits, sustained allocations and sequence-lock policies are documented
in api.md. No native transport, dependency or backend source changed.

Python 3.11.7 and 3.15.0 each pass 204 tests. The Rust workspace passes 82
ordinary tests with five ignored cases; pytest separately runs the three
finalization cases. Four added Rust tests and six Python tests cover independent
SysEx vectors, mixed byte orders, unknown IDs, malformed lengths, resource limits,
checked replacement, sustain/exhaustion, repeated-note ambiguity, resets,
channel-scoped one-shot prefixes, forward/reverse sequence continuity, deadlines,
relocation, malformed nibbles, skipped drop-frame labels and day rollover.
The pure-import fixture also constructs the new engines while cleanup/runtime
initialization is forbidden. Ruff, formatting, ty, pyupgrade, cargo formatting,
workspace Clippy and whitespace checks pass, retaining the existing local atomic
deprecation exception. Installed profile-engine tests are added to release CI;
the published-release-only trigger and endpoint versions are unchanged.

These are unit/software checks. No hardware, native-service flow or CI release
was run. MPE synthesis/zone interoperability, physical synchronization and native
platform qualification remain open. A full MPE transmitter's automatic channel
sharing and release-envelope behavior are not claimed by the selected allocator.

Final slice 6.5 review also closes construction holes in Rust Voice and Position:
private validated fields expose getters, preventing unchecked states from reaching
encoding. MPE tracker expression and sustain start unknown (`None`), and reset
clears that knowledge. Member snapshots report observed manager sustain; counts
cover only observed notes. The allocator keeps its explicit local output state.
Focused Rust and Python profile-engine tests cover these contracts.

## Slice 7: UMP and MIDI 2.0 core, 2026-10-15

The pure Rust/Python UMP module implements the packet/value families, explicit
translation and bounded source-local engines described in api.md. The baseline
and independent word fixtures are recorded in specifications.md. No dependency,
native backend or transport runtime changed. No additional work beyond the prompt.

Final Python 3.11.7 and 3.15.0 suites each pass 210 tests. The Rust workspace
passes 88 ordinary tests with five ignored cases; pytest separately exercises
the three interpreter-finalization cases. Six added Rust and six Python tests
cover 31 shared vectors, immutable native values, reserved/malformed fields,
explicit byte order, incremental partitions, opaque preservation, SysEx7/8
interleaving/abort, assembly resource limits, UTF-8 across packet boundaries,
protocol text limits, capability declarations and translation reports. Rust
checks every seven-bit velocity and fourteen-bit pitch-bend round trip, plus
4,096 deterministic packet layouts and bounded assembly invariants. MPE tests
include the final approved unknown-observation/reset policy.

Ruff, formatting, ty, pyupgrade, cargo formatting, workspace Clippy and whitespace
checks pass. Clippy retains the pre-existing local atomic-deprecation exception.
A macOS arm64 CPython 3.11 wheel was built from the newly generated source
distribution. Both archive checks pass, including new Rust sources, shared
fixtures, Python modules and stubs. Reinstalling that wheel into the isolated
wheel environment and testing outside the checkout passes all 209 applicable
installed-package tests. The source-only cargo-build callback test is exercised
in the full checkout suites. Release CI adds the installed UMP test module; it
still runs only for published releases, testing Python 3.11 and 3.15. No release
or CI run was triggered.

These are software/unit and packaging checks, not live MIDI 2.0 qualification.
Windows native validation, the selected Pi build/sysroot and physical UMP device
behavior remain open. MDS support is packet-level; translation deliberately
requires missing source/profile context rather than pretending to implement a
complete stateful default translator. Typed/timed transport integration and
MIDI-CI remain later slices.

## Standard-file codec foundation for slices 8–10, 2026-10-15

The streaming SMF/SYX/Clip Rust codecs and Python stream adapters pass five new
Rust tests and four Python tests. The full Python 3.11 suite passes 214 tests;
the Rust workspace passes 93 ordinary tests with five ignored cases. Ruff,
formatting, ty, pyupgrade, cargo formatting, workspace Clippy (the existing local
atomic-deprecation exception) and whitespace checks pass. Coverage includes
independent byte vectors, running status, extended/unknown records, SMPTE
headers, truncation at every byte boundary, payload limits, F0/F7 fragments,
independent Type 2 tracks, short writes and injected disk-full errors. No native
MIDI flow was run. Exact clock/export integration, archive support and live
integration follow in this same authorized effort; this foundation alone does
not mark all slice 8 export gates complete.

## Slice 9 archive and shared exact event values, 2026-10-15

The v1 schema is documented in archive-format.md. Its Rust implementation and
native Python values preserve rational clock units, optional signed ticks,
source spans, stream order, primitive MIDI/UMP and all nine compound commands.
Normal reading checks CRCs and completion; explicit recovery exposes only a
verified prefix with an incomplete report. Readers bound record allocation and
descriptor retention. File operations do not start MIDI workers.

Three Rust tests and three Python tests cover independent CRC/header/trailer
values, negative boundary ticks, absent timestamps, all command IDs, opaque
records, descriptor references, disk/completion failure states and truncation at
every frame byte. The full Python 3.11 suite passes 217 tests, and Rust passes
96 ordinary tests with five ignored cases. Focused native packet tests also
confirm the existing raw clock contract. Final cross-version, installed-wheel
and native virtual integration checks follow after slice 10.

## Slices 8–10 file timing, typed live integration and capture/replay, 2026-10-15

Implemented the approved Rust file codecs, `.meedee` archive and typed live
adapters, exposed through focused Python modules. Exact rational file clocks,
SMF track merging and tempo/SMPTE conversion, Clip timing and explicit export
reports complete the selected slice 8 scope. Stream-clock links and optionally
timed metadata finalize the unpublished archive v1 schema. Typed packet framing,
source-scoped semantic views, bounded recording and caller-thread playback use
the existing transport and canonical Rust codecs/engines.

The final full Python suites pass **236 tests on CPython 3.11 and 3.15**.
The Rust workspace passes **107 ordinary tests**, with five opt-in/native cases
ignored. Checks include completion/source spans across packets, independent
split-SysEx assembly when SMF tracks interleave, exact tempo changes and leading
silence, independent Type 2 selection, explicit quantization and UMP group-routing
refusal. Control/velocity/UMP observations retain contributor times across long
gaps; MPE retains unknown state; known loss and clock changes clear views. Missing
contributor time remains absent rather than being replaced by completion time.

Fault checks cover malformed/truncated files, CRCs, verified-prefix recovery,
unknown records, descriptor references, short writes, disk-full failures,
recording count/byte exhaustion, a blocked writer and stop deadline, queue overflow,
callback guards, receive close/cancellation and playback failure/cancellation.
A controlled typed input also verifies final native-drop accounting when there
is no following packet to reveal a sequence gap. Fake-clock checks verify absolute
deadlines, equal-time order, lateness and completed-prefix reporting. Native driver
acceptance is not physical delivery; cancelled playback sends no cleanup messages.

Ruff, formatting, ty, pyupgrade, cargo formatting, workspace Clippy and whitespace
checks pass. Clippy retains the existing local atomic-deprecation exception;
Rust 1.87 qualification remains with release CI because rustup is unavailable
here. A macOS arm64 CPython 3.11 wheel was built from the generated source
distribution. Both distribution checks pass, including all new modules, stubs,
Rust sources and regression tests. Reinstalling that wheel into a separate
environment and running outside the checkout passes **235 installed-package
tests**. The remaining cargo-build callback test passes in both checkout suites.
The release workflow adds installed file/archive/live/view checks and still runs
only on published releases, with Python 3.11 and 3.15. No workflow was triggered.

The authorized supervised virtual-endpoint flow attempted default isolated typed
capture/replay on macOS, but CoreMIDI client creation failed with **268435459**.
Checked cleanup reported retention rather than releasing potentially active
resources. No hardware endpoint was selected, and no tuning/reset message was
sent. Successful native capture/replay, in-process loopback, physical behavior,
Windows, the selected Pi image/sysroot and live UMP qualification remain open.
The opt-in fixture is test/native/meedee_typed_loopback.py. Software/fault and
packaging results do not substitute for those native/platform gates.

## Slice 11 MIDI-CI management, 2026-10-15

The owner selected the pure caller-driven API, explicit local identities/routes,
bounded transactions and conservative recovery, then deferred Profiles and
Property Exchange because their full normative documents are unavailable. The
accessible MIDI-CI 1.2.1 base supports management only: Discovery/Reply, Endpoint
Inquiry/Reply, Invalidate MUID and ACK/NAK. Rust ci/ci_session and Python meedee.ci
share the implementation. Session declarations advertise zero optional-category
bits. No JSON/compression dependencies or unimplemented property/profile stubs
are added. See ci.md and specifications.md for the contract and document hash.

The full suites pass **247 tests on Python 3.11 and 3.15**. Rust passes **113
ordinary tests**, with five opt-in/native tests ignored. Eleven Python cases and
six Rust tests exercise independent shared vectors, legacy/current/future
formats, truncation at each byte boundary, reserved MUIDs/addresses, extension
preservation and broadcast size limits. Sessions cover explicit route/group and
time checks, output-path echo, send uncertainty, deadlines, admission limits,
known-loss restart and collision invalidation even at capacity. Early replies
are retained provisionally within count/byte limits, applied only after reported
send acceptance, and discarded on failure or invalidation. Regressions cover
malformed self-addressed messages and late replies recreating invalidated peers.
Pure-import checks confirm that importing CI does not initialize MIDI workers.

Ruff, formatting, ty, pyupgrade, cargo formatting, workspace Clippy and whitespace
checks pass. Clippy uses the existing local atomic-deprecation exception. The
release workflow adds installed CI tests and retains its published-release-only
trigger and Python 3.11/3.15 matrix. No release or workflow was triggered.

The supervised opt-in fixture test/native/meedee_ci_loopback.py attempted a
native UMP SysEx7 discovery/reply exchange between two exclusively test-owned
virtual endpoints, using randomized test MUIDs and zero optional capabilities.
CoreMIDI rejected creation of the first UMP client with **268435459**. Checked
cleanup reported retained native resources until process exit; the supervisor
returned without a hang. No hardware endpoint was selected or sent messages.
Successful native UMP exchange, Windows/Pi qualification, physical device behavior
and full MIDI-CI profile/property conformance remain open. This is management
software coverage, not a claim of complete slice 11 or live MIDI 2.0 qualification.

Packaging builds a macOS arm64 CPython 3.11 wheel from the generated source
distribution. Both archive-content checks pass, including the CI loader/stub,
Rust modules, shared vectors and native fixture. Reinstalling the wheel into a
separate environment and testing outside the checkout passes **246 installed
package tests**. The source-only cargo-build callback test is covered by the
checkout suites. The new management code adds no runtime/build dependencies.

## Slice 12 qualification, 2026-10-15

Delivered opt-in qualification tools and a Mac software baseline. This is partial
qualification, not certification of all targets. The owner has no Pi available,
selected baseline-first budgeting and authorized bounded synthetic/test-owned
virtual workloads on this Mac. The reproducible procedure, workload sizes,
interpretation limits and Linux/Windows/Pi handoff are in qualification.md.
No toolchain/dependency versions, CI triggers or public APIs were changed.

| Capability | Software evidence | Native/platform qualification |
| --- | --- | --- |
| Immutable MIDI 1, controls/NRPN, MTS, selected GM/universal/MPE/time-code | Shared Rust/Python codecs and state-machine regressions; bounded synthetic workloads | Physical instrument behavior and fault recovery open |
| UMP/MIDI 2 messages, explicit conversion and assembly | Pure codecs, stream/recovery tests and MIDI Clip file workload | CoreMIDI UMP exchange blocked; Linux UMP unrun; Windows MIDI Services disabled |
| SMF/SYX/Clip and exact-time archive | Streaming/fault/clock tests; from-sdist wheel and installed-package checks | Archive schema remains unpublished; durability is not established by cached I/O |
| Typed capture/playback | Fake transport, cancellation/fault tests; four-recording software workload | Real libremidi loopback still unqualified |
| MIDI-CI | Implemented management subset, bounded sessions and shared vectors | Profiles/Property Exchange deferred; Process Inquiry engines deferred; no claim of complete CI |
| macOS arm64 | Source/package tests and sanitized device-free C++ regressions | CoreMIDI client creation fails with 268435459 in this tool environment |
| Linux x86/arm64, Pi Zero 2 W | Procedures and release-only CI configuration | No target run here; Pi OS/image/sysroot and endpoint selection still open |
| Windows and macOS Intel | Existing release-only CI configuration and target-specific fixtures | No target run here; Windows capability-query risk and SDK integration remain open |

The Rust release example and opt-in Python suite cover dense CC, NRPN expansion,
1 KiB/64 KiB/1 MiB SysEx, MTS bulk interpretation, UMP Clip reads, four-stream
archive write/read and four concurrent bounded recorders. Seven batch samples
are retained per throughput workload. Correct values/counts/completion are
checked; the benchmark results do not establish acceptance budgets. Python and
Rust checks perform different amounts of validation, so their operation rates
are not a controlled comparison of language overhead.

All three supervised native MIDI 1 attempts (Rust, Python in-process, Python
isolated) failed before endpoint creation with **268435459**. All exited promptly;
none timed out or sent to hardware. Failed cleanup reported conservative retained
resources until process exit. Native latency, successful idle-worker resource
use and child-inclusive memory/CPU/thread/handle totals therefore remain open.
The supervisor deliberately reports these as failed qualification tests rather
than converting unavailable native evidence to a pass.

Unix RSS measurement uses process lifetime high-water marks and includes native
allocations, but excludes children. Fresh import is measured in a separate
interpreter. Windows RSS is absent, not zero. OS process inspection is denied
here, so there are no measured process/thread/handle totals or whole-tree idle
budgets. Source-build size and performance must not be extrapolated to the Pi.
The synthetic recorder burst explicitly exercises queue exhaustion and marks
captures incomplete; that offered burst rate is not a supported capture rate.

The ordinary source suites pass **247 tests on each of CPython 3.11 and 3.15**.
Linked Rust tests pass **113 ordinary tests**, with five opt-in/native tests
ignored. All **three** selected C++ regression executables pass with address and
undefined-behavior sanitizers. Cargo formatting, workspace Clippy (the existing
local Rust 1.99 atomic-deprecation exception), Ruff, ty and pyupgrade checks pass.
An initial concurrent source Python/Cargo run replaced a PyO3 artifact before
rustdoc consumed it; the sequential rerun passes including doctests. Qualification
instructions require sequential source/build tests. Rust 1.87 remains unverified
locally, and configured release CI is not a substitute for an observed run.

### Recorded Mac baseline

Measured on macOS 14.5 arm64, Rust 1.99.0, Apple Clang 15.0.0 and CMake 4.4.4.
The measured release wheel uses CPython 3.11.7 and was built from the source
distribution. The source base is 5665214 plus the accompanying slice 12
qualification tools; production library code is unchanged. Runs are short and
host CPU model, power/thermal state and background load were not controlled.
These are observations, not throughput promises or Pi budgets. Raw batch samples
are preserved in qualification-macos-python311.jsonl and
qualification-macos-rust.jsonl. Cold first batches are included.

| Observation | Result |
| --- | --- |
| Selected public-module import, fresh Python process | 35.85 ms wall, 33.99 ms CPU; 27.3 MiB process peak RSS |
| Python CC decode | 11.84 million messages/second across seven 2,048-message batches |
| Rust CC decode | 47.85 million messages/second across seven 2,048-message batches |
| Python / Rust NRPN expansion and encoding | 0.700 / 3.129 million compound commands/second |
| Python 1 MiB SysEx, 1 KiB chunks | 3.17 ms median batch; 328.3 million payload bytes/second |
| Rust 1 MiB SysEx, 1 KiB chunks | 360.9 million payload bytes/second |
| Python MTS bulk interpretation | 382,259 dumps/second; seven 128-dump batches |
| Python MIDI Clip read | 316,478 note events/second; 24,632-byte file |
| Python four-stream archive write/read | 113,670 events/second; 94,488-byte file |
| Four paced recorders | All 400 events admitted and verified written; four complete captures; 1.28 seconds elapsed |
| Four unpaced recorders | 570 of 8,192 attempts admitted and verified written; four incomplete captures reporting queue/payload exhaustion |
| Parent pytest process lifetime peak RSS | 63.1 MiB including runner and workloads; not library-only memory |
| Wheel / unpacked extension / Rust measurement executable | 1,514,101 / 5,090,304 / 500,016 bytes |

The fresh wheel passes **246 installed-package tests** outside the checkout;
the source-only Cargo callback test is covered by the source suites. Both
artifact-content checks pass, including distribution of the qualification tools.
All **four** pure measurement tests pass against that installed wheel. The same
four opt-in tests pass with the CPython 3.15 source environment, including verified
paced capture and explicit burst incompleteness. Those additional figures remain
in target/qualification-python315.log and are not mixed into the release-wheel
baseline above. The installed macOS extension links system C++/CoreMIDI/CoreAudio/
CoreFoundation/iconv/libSystem libraries, with no separate libremidi dylib.

No issue was closed merely by measuring or documenting it. Physical recovery,
backend safety gaps, full resource profiling, Linux/Windows/Pi runs, native UMP
and successful CoreMIDI transport remain open in issues.md. No release or CI run
was created, and the workflow remains unchanged: published releases only,
Python 3.11 and 3.15.

## Native access and WinMM discovery follow-up, 2026-10-15

A standalone C++ program calls Apple's MIDIClientCreate directly, bypassing both
meedee and libremidi. From the agent execution context it returns **268435459**
(**0x10000003**, MACH_SEND_INVALID_DEST in the installed Mach headers) and leaves
the zero-initialized client reference unchanged. This reproduces the first-call
failure without either wrapper and does not identify the underlying service or
access-policy cause. The opt-in access.py test compiles/runs it with separate
30/10 second limits; it creates no endpoints and sends no MIDI.

The owner ran the identical test in ordinary Terminal: **one passed**, with
MIDIClientCreate and MIDIClientDispose both returning zero. The tool execution
context therefore cannot stand in for the user's normal CoreMIDI access. No OS
service was restarted and no system configuration was changed.

The owner then ran the three supervised MIDI 1 virtual loopbacks from Terminal:
**three passed**. Rust, Python in-process and Python isolated each delivered 256
CC messages through a uniquely test-owned destination. Both Python modes wrote
and verified complete archives of their 256 events. The preserved output is
qualification-macos-native.jsonl; target/mac-terminal-native.log is the original
local transcript. This supersedes the earlier absence of successful native
MIDI 1 loopback evidence, while retaining the agent-context failure as a distinct
access limitation. Native round-trip measurements include host scheduling and
return to the receiver, and Python also includes typed delivery/record admission.

| MIDI 1 loopback | Median round trip | p95 | Maximum |
| --- | --- | --- | --- |
| Rust native | 24.396 microseconds | Not calculated | 85 microseconds |
| Python in-process | 59.771 microseconds | 82.671 microseconds | 413.708 microseconds |
| Python isolated | 91.230 microseconds | 144.156 microseconds | 2.619 milliseconds |

These are short serial virtual round trips, not physical-wire latency,
sustained-load limits or synchronization measurements. Open times vary with
service warmup. Parent-only idle CPU samples do not measure whole isolated
process trees; full resource profiling remains open.

The fork patch b6c7dfe checks every WinMM capability-query result before reading
its output, reports the failed index/direction/status and omits that port.
Duplicate suffixes use only successful snapshots from the same enumeration,
removing additional native queries and their disappearance race. The error hook
has a nonblocking atomic reentrancy/concurrency guard; nested/concurrent error
notifications may be suppressed while a hook is active, but failed records remain
omitted. meedee's existing sticky native-error diagnostics can reject the whole
enumeration rather than returning libremidi's successful subset.

The original sibling checkout was clean but had two unpushed commits diverging
from origin/master. It was preserved. The patch was committed/pushed from an
isolated fork worktree on codex/checked-winmm-discovery, based on the exact
previous vendor pin, and the committed sources were imported byte-for-byte.
SOURCE.md records the new revision. ANSI/Unicode Windows-only regressions cover
untouched outputs on failure, successful metadata, duplicate names, recovery,
reentrant hooks and concurrent hook suppression. Neither new executable has
been compiled or run here because no Windows toolchain/host is available. All
three existing Mac device-free regression targets pass with address/undefined-
behavior sanitizers against the fork. Release CI adds the two Windows targets;
its published-release-only trigger and Python 3.11/3.15 endpoints are unchanged.
No release or workflow was triggered.

The audit also identified Apple's documented client-disposal lifetime warning:
disposing the last client can prevent later creation by that application if the
server exits. That is distinct from the first-call agent-context failure. It
needs an explicit ownership/resource design, including observer callbacks,
before implementation; no process-global client or additional retained resources
were silently introduced. The concern remains in issues.md B1.

The owner also ran the extended supervised fixtures in ordinary Terminal:
**two passed**. Typed capture, archive verification, clock-aware replay and
receive cancellation pass with isolation enabled and disabled. The native UMP
fixture packetizes/reassembles SysEx7 and completes a MIDI-CI discovery/reply
exchange between two test-owned peers. Its declarations advertise zero optional
CI categories, and no physical device or tuning/reset/configuration command is
used. Their output is appended to qualification-macos-native.jsonl; the local
transcript is target/mac-terminal-extended.log. This closes the basic Mac virtual
capture/replay/cancellation and CoreMIDI UMP SysEx7 exchange gaps. It does not
qualify MIDI 2 channel-voice precision on physical equipment, other backends,
Profiles/Property Exchange or recovery from driver/hardware failure.

For this import, the full source suites pass 247 tests on each Python endpoint,
and the linked Rust suite passes 113 ordinary tests with five opt-in cases
ignored. Ruff, formatting, ty, pyupgrade, Cargo formatting and workspace Clippy
pass (the existing local atomic-deprecation exception remains). A fresh macOS
arm64 CPython 3.11 wheel builds from the sdist; both artifact checks pass,
including the newly shipped access probe and WinMM discovery test source.
Reinstalling that finished wheel and testing outside the checkout passes 246
installed-package tests. Windows native compilation remains an explicit open
gate; Mac sanitizer and packaging checks do not replace it.

## Bounded CoreMIDI client keeper

The owner approved one private callback-free CoreMIDI client per MIDI-using
process, retained until process exit. Fork commit 189334f initializes it under
a mutex before owned input/output or notification-client creation. Pure codecs
and file operations do not initialize it; borrowed contexts remain caller-owned.
Normal ports and ordinary clients are still disposed. A failed keeper creation
returns its OS error without using the unwritten output; a later request can
retry. A failed ordinary creation does not discard the keeper. Failed creation
no longer disposes a zero client. No application callback or observer pointer
is attached to the keeper. A bounded intentionally retained state object avoids
static-destruction races with still-running owner threads.

This avoids normal last-client disposal; it is not MIDI-server health detection
or recovery after a service crash/restart. Initial keeper creation is serialized
within a process and can block like other native calls. Default isolated clients
remain in separate processes. Account for one additional native client and small
synchronization state per MIDI-using process, outside application connection
counts. Observer disposal failures remain a separate open lifetime risk.

All four selected C++ regression executables pass with address/undefined-behavior
sanitizers. The new fake-native keeper test models permanent creation failure
after the last client is disposed. It checks a failed first keeper attempt,
concurrent MIDI 1/UMP creation/destruction, subsequent reopen, ordinary creation
failure, notification-client cleanup, caller-owned contexts and absence of
keeper callbacks/disposal. Its single case uses eight threads and preserves
exactly one live private client after the ordinary clients close. No OS client
is created by these injected tests.

The opt-in supervised reopen fixture repeats three open/send/close cycles on the
same clients for each protocol and Python mode, verifying native timestamps and
exact MIDI 2 CC value 0xFEDCBA98 on group/channel 16. The prior successful native
runs predate this keeper import; a run of the new build is a separate gate.

For the keeper import, both full Python suites pass 247 tests, and the Rust
workspace passes 113 ordinary tests with five opt-in cases ignored. Static checks,
Cargo formatting and workspace Clippy pass with the existing local deprecation
exception. A fresh wheel builds from the sdist, both artifact checks pass, and
246 installed-package tests pass outside the checkout. The release workflow
includes the keeper regression target without changing its trigger or Python
matrix. Native repeated-reopen validation remains explicitly pending.

## Per-parent isolated-worker admission

The owner approved a cap of 32 live isolated MIDI workers per parent Python
process, including discovery-only clients and children still retiring. A fixed
32-slot table retains process handles; a parallel fixed reservation table covers
starts in progress. Reservations are made under the existing registry lock, but
Popen executes outside it. Other admissions and shutdown do not wait behind a
blocked process creation while holding that registry lock. Publishing a child
uses an already allocated slot, so it requires no set entry/list growth after
process creation. Failed Popen calls release the reservation in finally.

A slot is reusable only when poll confirms its previous child has exited; neither
weak-reference disappearance, connection close nor a kill request releases it.
A failed communication-thread setup still counts its created child until exit
is confirmed. Overflow raises the existing ResourceError before another spawn.
The limit is independent of native-client/connection counts and of other parent
processes; it is not a total-memory, CPU or safe-Pi-workload guarantee. CoreMIDI's
private process-lifetime keeper remains an additional resource in each child.
Retaining closed connections can retain their reusable clients/workers, so
applications must release those references when finished.

Five focused cases pass: discovery-only clients fill capacity, retirement keeps
its slot until exit confirmation, a confirmed exit admits a replacement, startup
OSError/MemoryError/KeyboardInterrupt release reservations, and an in-progress
spawn reserves capacity without blocking another admission on the registry lock.
These use the existing controlled worker fixture and no OS MIDI. Both final
source suites pass **252 tests** on CPython 3.11 and 3.15.

The final worker-budget wheel builds from the sdist; both artifact checks pass.
After installation into a separate environment, **251 installed-package tests**
pass outside the checkout, including the five new admission cases. Ruff, formatting,
ty and pyupgrade pass. The Rust/C++ implementation is unchanged from the keeper
commit's successful 113-test Rust and four-target sanitized native checks.
CI remains published-release-only with Python 3.11 and 3.15; no workflow was run.

## Final native keeper/worker-cap follow-up, 2026-10-15

The owner ran the six supervised native cases in ordinary Terminal against the
keeper and worker-cap build. The result was **five passed, one failed, four
deselected in 4.18 seconds**. Raw per-case output, including the failure and
parent resource samples, is preserved in `qualification-macos-final.jsonl`
(12 JSONL records), separately from the earlier baseline.

The passing cases cover Rust MIDI 1 loopback, Python MIDI 1 loopback in both
process modes, typed capture/replay/receive cancellation in both modes, and
native UMP SysEx7 MIDI-CI discovery/reply between test-owned peers. Each of the
three basic loopbacks delivered 256 messages. These results validate those paths
with the keeper present; they do not exercise admission at the 32-worker cap.

The repeated-reopen case failed in `sender.ports()` with
`PortInfoError: could not read CoreMIDI port unique ID`. No completed
protocol/mode combination was printed. The log does not identify the endpoint,
cycle or OS status. The checked property helper rejects the failed read, so
this result does not demonstrate use of an uninitialized ID. It also does not
establish whether a disappearing endpoint, the test lifecycle or another
CoreMIDI condition caused the failure. Repeated reopen and this fixture's exact
full-width MIDI 2 CC delivery remain open; the failure must not be converted to
a passing result by suppressing discovery errors or adding an unexplained retry.

No implementation changed in this follow-up. JSON records were parsed and
their case statuses checked; documentation passed `git diff --check`. Python
suites were not rerun for this documentation/evidence-only update. Windows,
Linux/Pi, physical fault recovery and workload budgets remain open.

### Standalone reopen follow-up

The owner reran only `native and reopen`, with unbuffered Python output. It
failed again with the same unique-ID discovery error: **one failed, nine
deselected in 1.14 seconds**. The two raw JSON records are preserved in
`qualification-macos-reopen.jsonl`. This shows the other five native cases are
not necessary to trigger the failure. It does not rule out system state left
by previous processes, and still does not identify the failing cycle or endpoint.

The reopen fixture now prints flushed progress markers before virtual creation,
discovery and connection, after delivery, and after close. Each marker identifies
the protocol, isolation mode, cycle and test-owned endpoint name. Discovery
errors still propagate without retries or suppression; sends and endpoint
selection are unchanged. Native execution of this diagnostic version remains
pending in ordinary Terminal.

The ordinary Python suite passes all 252 tests after the diagnostic-only fixture
change. Ruff checking/formatting, ty and pyupgrade pass; the preserved JSON
records parse and `git diff --check` passes. No production code changed.

### Reopen diagnostic result and native error details

The instrumented fixture failed again: **one failed, nine deselected in 1.03
seconds**. Its two JSON records are preserved in
`qualification-macos-reopen-diagnostic.jsonl`. The markers establish that MIDI 1
with isolation enabled completed cycle 1, including exact delivery and connection
close. Cycle 2 created its virtual input, then failed while discovering output
ports. This corrects the earlier uncertainty about whether any message was sent;
the first cycle did send and receive successfully. No later mode was exercised.

The failure occurs after endpoint replacement, but the log does not establish
which endpoint's property query failed or why. The libremidi fork now includes
the failing object handle and exact OSStatus in this diagnostic. The existing
checked integer-property helper carries the original status to the observer;
failed reads still produce no ID. The existing fake-native regression checks
successful status reporting, unchanged-output failure, and the object/status
reported for both port directions. All three affected CoreMIDI regression
targets pass with address/undefined-behavior sanitizers. The fork commit is
`faf5b3bf724b2de162a8bdacf2f57b2687d9a790`, imported without local differences.

This diagnostic change introduces no retries, error suppression or change to
discovery policy. Native execution of the more informative build is pending;
reopen qualification remains open.

The rebuilt editable Python package passes all 252 ordinary tests in 25.17
seconds. The preserved JSON records parse, the imported files match the fork,
and `git diff --check` passes. No Python source or dependency versions changed.

### Native status and wrapper-independent reproduction

The owner reran the diagnostic build. The failure again followed successful
delivery/close in the first isolated MIDI 1 cycle, during discovery in cycle 2.
The exact diagnostic was `object=158564417, OSStatus=-50`. The result was **one
failed, nine deselected in 1.27 seconds**; both raw JSON records are preserved in
`qualification-macos-reopen-status.jsonl`. The installed Apple SDK defines
`paramErr = -50` in CarbonCore's `MacErrors.h`. This is not proof of a particular
lifetime or run-loop failure.

`test/native/coremidi_reopen.cpp` and `test/qualification/reopen.py` now provide
a direct SDK comparison, independent of Rust, libremidi and Python transport.
Three fresh producer/consumer process pairs compare lazy client creation, early
client creation and positive-duration run-loop servicing. Each replaces only
its own virtual endpoints and ordinary clients while retaining private keepers.
No MIDI is sent. Handles and checked unique-ID query results are logged for all
three replacement cycles, including failures, without logging hardware names.
The harness bounds each pair and kills its process group on timeout. The native
probe compiles with `-Wall -Wextra -Werror`; runtime evidence remains pending in
ordinary Terminal. No production behavior was changed in this follow-up.

The producer is launched through `posix_spawn` into a fresh executable image,
avoiding a forked copy of initialized Apple runtime state. The ordinary suite
passes all 252 tests in 25.18 seconds. Ruff checking/formatting, ty and pyupgrade
pass for the new Python harness. The preserved JSON parses and
`git diff --check` passes. No dependencies or CI triggers changed.

### Direct SDK results: stale disposed endpoints

The owner ran all three initial direct SDK modes: **three failed in 1.73
seconds**. Extracted native output is preserved in
`qualification-macos-direct-reopen.txt`, without pytest tracebacks. All producer
create/dispose calls reported success. Each first cycle discovered its test-owned
endpoint with a valid unique ID. After replacement, however, the consumer could
still enumerate the disposed first endpoint and its ID query returned `-50`.

In `late`, the producer created endpoints 204484673, 204484679 and 204484683,
but the consumer enumerated 204484673 on all three cycles. `early` behaved the
same way with first endpoint 204484691 despite keeper creation before discovery.
`serviced` still returned disposed endpoint 204484705 in cycle 2 after producer
creation of 204484711; cycle 3 finally discovered the new endpoint 204484715.
Neither early creation alone nor the tested 50 ms run-loop wait gives a reliable
fix. This establishes stale enumeration with direct SDK calls under the tested
callback-free client usage, independently of either wrapper. It does not prove
the underlying cause or establish a safe production workaround.

The probe now has two additional diagnostic modes. Both register a static
notification callback before discovery, without an application context. One
retains zero-duration run-loop servicing; the other uses 50 ms. Notification IDs
are logged, and all existing query/match failures still fail the case. These
modes compile cleanly with warnings treated as errors; their runtime results
remain pending. Production keeper behavior, discovery policy and CI are unchanged.

The ordinary suite passes all 252 tests in 18.75 seconds. Ruff checking/formatting,
ty and pyupgrade pass for the changed Python harness; `git diff --check` passes.

## Continuous two-process event-loop probe, 2026-10-16

The previously requested notification comparison is now read and preserved in
`qualification-macos-notified-reopen.txt`. Both `notified` and
`notified_serviced` failed: **two failed, three deselected in 1.75 seconds**.
Both enumerated the disposed cycle-1 endpoint in subsequent cycles; neither
logged any notification. Registering a callback alone, or combining it with the
tested 50 ms consumer-loop wait, therefore gives no verified remedy. Those probes
left the producer blocked on pipe input, so they did not test continuous event
servicing in both processes.

The owner authorized replacing the short-run-loop comparisons with an
event-driven probe. Both processes now create their notification clients on
their main thread and run `CFRunLoopRun` continuously. `CFFileDescriptor` sources
handle pipe readiness on those same loops. The producer creates/disposes its
test-owned destination and ordinary client, then acknowledges only after its
matching added/removed notification. The consumer retains notifications that
arrive before the reply and waits for its own matching event before enumeration.
It verifies the new unique ID appears once and the removed ID disappears across
three cycles. SDK results, notification IDs, roles and handles are logged. No MIDI
is sent and no hardware connections or names are used.

Each notification wait uses a two-second run-loop timer, with 128 destination
changes of bounded per-wait storage. A 20-second supervisor bounds the pair and
kills its whole process group on timeout. This outer deadline also covers a
native call blocking an owning loop. The producer is still a fresh executable
launched with `posix_spawn`. Successful completion closes its command pipe and
stops the producer through its EOF readiness event. Only private keepers remain
until process exit. The production keeper and transport are unchanged.

The new probe compiles against the installed SDK with
`-std=c++20 -Wall -Wextra -Werror`. Its supervised agent-context attempt failed
at `MIDIClientCreate` in both processes with **268435459**, before endpoints or
notification waits. Extracted output is preserved in
`qualification-macos-event-driven-agent.txt`. This is the previously observed
agent access limitation; it supplies no evidence about discovery under continuous
event servicing. Ordinary Terminal execution is still required.

The ordinary suite passes **252 tests in 18.39 seconds**. Ruff checking/formatting,
ty and pyupgrade pass for the changed harness; `git diff --check` passes. No
dependencies or CI triggers changed. The historical short-run-loop cases remain
in Git/evidence; the current command runs only the event-driven probe.

### Ordinary Terminal event-driven result

The owner ran the new probe successfully: **one passed in 2.74 seconds**. Raw
native output is preserved in `qualification-macos-event-driven.txt`. Both
notification clients were created successfully. Each process matched all six
expected destination add/remove events across three cycles. The consumer's three
post-add enumerations found the created ID exactly once; its three post-remove
enumerations found that ID absent. All enumerated property reads succeeded.

The created endpoints were 396460099, 396460103 and 396460107, with distinct
unique IDs 515369766, 1962367418 and 149930890. No stale disposed endpoint was
reported after the matching removal notification. This demonstrates correct
discovery for the tested combination of continuous run-loop servicing in both
processes, notification registration and matching-event barriers. It does not
isolate which part is necessary or establish physical-device/OS-fault behavior.

The production transport has not changed and still needs an integration fix
and its own native validation. The successful SDK probe does not qualify the
wrapper's repeated-reopen or full-width MIDI 2 send path. The open issue is
retained with this narrower, current status. No further runtime tests were run
for this documentation/evidence-only follow-up; counts and checks were reviewed
against the log and `git diff --check` passes.

## Production CoreMIDI notification integration, 2026-10-16

The owner approved one persistent notification-loop thread per MIDI-using process
while preserving per-client operation threads, fixed two-second notification
deadlines, and recoverable CoreMIDI discovery-call errors. The fork implements
the native changes in `a3d7d168615675f1257b5f5bc480cae2e75b5510`; the vendor pin and
imported source match that commit. Windows/Linux operation ownership is unchanged.

The private client is now created on a process-lifetime CoreMIDI loop, including
discovery-only client use. It retains only internal state, not application
callbacks or observer objects. A fixed 256-entry sequence history records endpoint
add/remove notifications without allocating in the callback. Each virtual open
or disposal marks the history before entering the SDK, then waits for its own
matching event on a steady-clock two-second deadline. Overflow fails explicitly;
an earlier event cannot acknowledge a later operation. SDK disposal success is
recorded before waiting, preserving failed cleanup state without disposing the
same endpoint again after a delayed acknowledgement. Direct C++ destruction and
other existing native cleanup gaps remain outside the checked C API guarantee.

Rust now separates observer diagnostics from connection diagnostics. A CoreMIDI
metadata-read error returns no partial snapshot and clears only that observer
call's diagnostic, leaving explicit rediscovery possible. Connection diagnostics
remain sticky. Other backends retain their prior sticky discovery behavior. The
isolated Python worker also keeps CoreMIDI PortInfoError replies nonfatal;
otherwise it would terminate a recoverable Rust client. No automatic retry,
skipped error, public timeout option or musical cleanup send was added.

All three affected C++ targets pass with address/undefined-behavior sanitizers
in 5.51 seconds. Their fakes deliver notifications through the actual private
run loop. Coverage includes startup failure/retry, concurrent clients, pre-operation
events, two-second timeout, history overflow and late removal acknowledgement
without a second SDK disposal. The final Rust workspace passes **114 ordinary
tests, with five opt-in cases ignored**. A new boundary case verifies explicit
rediscovery and subsequent connection after a failed metadata read for both
MIDI 1 and UMP. It uses fake CoreMIDI resources, not the OS service.

Both source Python suites pass **254 tests**: 25.39 seconds on CPython 3.11 and
34.86 seconds on 3.15. Two new controlled-worker cases cover input/output
rediscovery after an error and preserve the old non-macOS fatal policy. Ruff,
formatting, ty, pyupgrade, Cargo formatting and workspace Clippy pass, using the
existing local Rust 1.99 deprecation exception. Release Rust examples rebuild.
A fresh CPython 3.11 wheel builds from the sdist; both artifact checks pass,
including the required new notification header. After explicit reinstallation,
**253 installed-package tests** pass outside the checkout in 17.19 seconds.

The agent attempted the production `native and reopen` supervisor case. It
failed at initial client construction with **268435459**, before creating a
virtual endpoint: one failed, nine deselected in 0.17 seconds. Both JSON records
are preserved in `qualification-macos-production-reopen-agent.jsonl`. This is
the known agent-context service-access failure; it does not establish the new
runtime's native discovery behavior.

The owner then ran the same production case in ordinary Terminal. The result
is **one passed, nine deselected in 7.49 seconds**, with supervisor return code
zero and no child stderr. All four MIDI 1/UMP and isolated/in-process combinations
complete three open/discover/send/close cycles each, for **12 successful cycles**.
Every cycle checks exact typed-message equality, a present native timestamp and
closed connections. The MIDI 2 case preserves group 16, channel 16, controller 7
and the full-width value `0xFEDCBA98`; MIDI 1 preserves channel 16, controller 7
and value 127. Both JSON records from `target/mac-terminal-production-reopen.log`
are preserved in `qualification-macos-production-reopen.jsonl`. This closes the
production virtual-reopen discovery gate on this Mac; the resolved issue text
has been removed from `issues.md`.

Account for one additional thread/client and the bounded history in every
MIDI-using process, including isolated children. These changes do not qualify
Pi budgets, physical faults, other MIDI libraries' initialization choices, or
recovery from MIDI-server restart. CI stays published-release-only with Python
3.11 and 3.15; no Actions run or release was initiated. No dependencies changed.


## Isolated receive-loss reporting while idle, 2026-10-19

The Python arpeg host migration exposed a gap in the private worker protocol:
known native drops were carried by subsequent MIDI packets or command replies,
so dropping a final packet followed by silence left the parent counter stale.
The forwarding thread now uses its existing 100 ms empty-queue wake-up to report
changed native/forwarding drop totals in a status-only message. It starts no
additional thread and sends no MIDI traffic. Packet messages still carry their
counts. Unchanged idle counts produce no status message.

The parent consumes these notifications outside the command-reply queue and
accepts only the active connection generation. Existing monotonic count merging
and close-time snapshots remain in use. Scheduling, pipe backpressure and worker
failure still prevent an absolute reporting deadline or a complete loss account.

A controlled child regression changes its native drop count after open and sends
no MIDI callback. The parent observes seven drops without another command or
packet, remains usable, and retains the count after explicit close. The existing
packet-carried accounting regression also passes. This uses controlled child
faults, not CoreMIDI or a physical device. The Python 3.11 full suite passes
255 tests in 24.84 seconds; Python 3.15 passes the same 255 tests in 34.17
seconds. Ruff, Python formatting, ty, pyupgrade and diff checks pass. The native
arpeg integration will be rerun against this Git revision independently.
