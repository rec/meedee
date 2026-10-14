# meedee

A Rust MIDI toolkit with matching Python bindings, using vendored
[libremidi](https://github.com/celtera/libremidi) for native transport. Python
imports are `meedee.midi`, `meedee.events`, `meedee.messages`, `meedee.codecs`
and `meedee.controls`; the package root is empty. There are no Python runtime
dependencies.

The toolkit is under development. Raw MIDI 1.0 bytes and native UMP transport
are implemented. Immutable MIDI 1.0 messages and a bounded incremental byte
codec, fine controls and RPN/NRPN commands are available. Tuning, MIDI 2.0 values
and file support are later slices in [the plan](plan/meedee.md). Encode primitive
messages for raw transport; use `send_command` for compound CC commands.

## Messages and byte codecs

```python
from meedee.codecs import Decoder, encode
from meedee.messages import NoteOn

note = NoteOn(channel=1, note=60, velocity=100)
wire = encode(note)  # b'\x90\x3c\x64'
decoder = Decoder()
assert decoder.feed(wire[:2]) == []
assert decoder.feed(wire[2:]) == [note]
assert decoder.finish() == []
```

The pure codec supports MIDI 1.0 note, pressure, CC, program, signed pitch bend,
system common, real-time and SysEx messages. Values are immutable, keyword-only,
hashable and support checked `replace`. Channels are 1–16, programs are 1–128,
CC identifiers are 0–127, and PitchBend.value is -8192–8191 with center zero.
Channel-mode messages remain primitive ControlChange values; zero-velocity
NoteOn is preserved.

`Decoder.feed()` returns messages and frozen diagnostics in completion/detection
order, handles running status and interleaved real-time bytes, and retains
partial messages between calls. Its configurable `sysex_limit` defaults to
1 MiB of payload. Oversized SysEx is discarded with one diagnostic; real-time
delivery and parsing subsequent messages continue. `finish()` reports truncated
input and clears state. `reset()` silently clears state after known loss.
As a conservative policy, SysEx ended by a new status instead of F7 is also
discarded with a diagnostic, even though MIDI permits implicit termination.
Use one decoder per source; diagnostic offsets count bytes actually fed.

These are live byte-stream rules, not Standard MIDI File or UMP parsing.
Encoding adds full status/framing bytes. No timestamps are invented or attached
to decoded values; transport callbacks remain raw. Input snapshots and returned
lists have memory costs beyond the retained SysEx limit. Details and remaining
protocol boundaries are in [the API contract](plan/api.md).

## Fine controls and parameters

```python
from meedee.codecs import expand
from meedee.controls import Interpreter
from meedee.messages import NrpnSet

command = NrpnSet(channel=1, parameter=1234, value=10000, resolution=14)
primitives = expand(command)
interpreter = Interpreter()  # One per independent input source.
observations = [o for m in primitives for o in interpreter.feed(m)]
# An open MIDI 1.0 output can send the whole command:
# connection.send_command(command)
```

`ControlChange14` emits a coarse/fine CC pair. RPN/NRPN set commands require
explicit 7-/14-bit resolution. Increment/decrement commands represent one
directional operation, with no guessed amount or physical unit. Each parameter
command selects afresh and finishes with RPN-null, without reserving an NRPN
identifier. Raw `send()` and primitive `encode()` remain unchanged; use `expand`
to inspect commands and `send_command` to serialize their complete sequence.
UMP outputs reject these MIDI 1.0 commands rather than translating silently.

`send_command` prevents interleaving on its connection. `CompoundSendError`
reports the known completed-message prefix and total count. A failed message
may already have effects; worker loss or timeout reports unknown progress
(`completed_messages=None`). It never retries or sends more cleanup after an
error, so selection release is not guaranteed after failure. Other applications
can still interleave traffic on the device.

The optional interpreter returns frozen, indexed observations and diagnostics
alongside your retained raw stream. It never fabricates a missing fine value or
an absolute value from a relative step. A new coarse CC forgets old fine state;
repeated fine updates can use that known coarse value. A changed complete RPN/
NRPN selection requires both halves afresh, a deliberate conservative policy
that declines single-half selector edits. Bank Select and Data Entry are not
presented as generic knobs. Reset after known loss/reconnect; SystemReset and
Reset All Controllers invalidate the applicable local state automatically.
There are no timers, guessed device defaults, timestamps or source registry.
Rust exposes the same values and state machine; its common Message marker has
PrimitiveMessage and Command subtraits for encoding and expansion.

## Python transport

```python
from meedee.midi import Output

output = Output()
ports = output.ports()
# Select an endpoint deliberately from this snapshot, for example ports[index].
# with output.connect(ports[index]) as connection:
#     connection.send(bytes([0x90, 60, 100]))
#     connection.send(bytes([0x80, 60, 0]))
```

`Input` and `Output` constructors accept keyword-only settings. Defaults are
`protocol=Protocol.Midi1`, `backend=None`, `isolated=True`, and `timeout=5.0`.
Input additionally accepts `timestamps=TimestampMode.Absolute` and
`receive_byte_limit=8_388_608`. Choose `Protocol.Ump` explicitly for UMP, and
send an immutable `UmpPacket(words=[...])`. There is no automatic protocol
selection, conversion or backend fallback. `compiled_backends()` reports build
support, not service, driver or device health. An unavailable backend raises
`CapabilityError`.

`ports()` returns immutable `Port` snapshots with `name`, `backend`, and
`backend_id`. IDs can duplicate, be zero, or change. A snapshot belongs to one
client and discovery generation; refreshing discovery invalidates older
snapshots. `connect(port, callback)` on input or `connect(port)` on output uses
that selection without looking it up again by ID. Virtual endpoints use
`create_virtual(name, callback)` or `create_virtual(name)`. Native client and
real-connection names are not configurable through the pinned C API. CoreMIDI
virtual names still use the fork's existing ASCII input conversion.

Input callbacks accept one immutable `ReceivedPacket` from `meedee.events`.
Its `packet` is owned `bytes` or an immutable `UmpPacket` whose `words` are a
tuple of unsigned 32-bit integers. Its `timestamp_ns` is an exact signed integer
or `None` when capture is disabled. Zero is a valid captured timestamp. `clock`
records backend, mode, unique connection identity and the exact tick unit
1/1,000,000,000 seconds; resolution is unknown. Reopening and distinct isolated
children use distinct clock identities. Backend Absolute timestamps may have a
host-clock fallback, are not UTC and are not assumed comparable across clocks.
Nanosecond storage does not promise nanosecond accuracy.

Receive packets are raw transport chunks, including partial SysEx or multiple
UMPs. The binding does not assemble unfinished SysEx or apply `Ignore` filters.
Use the pure byte codec explicitly for framing; typed transport/timestamps and
receive filters remain later work. `send()` accepts unsigned-byte buffers,
including bytearray and memoryview, or `UmpPacket`; mutable and strided buffers
are snapshotted. Send
validation checks protocol, size and UMP framing, not full MIDI semantics.
Success means backend acceptance, not physical delivery. UMP batch failures
report an accepted prefix; the failed packet may also have had effects. There
is no automatic resend, reconnection, tuning reset or all-notes-off on close.

## Ownership, isolation and failures

Retain the returned connection. It keeps its client alive and owns its single
open resource. The client is unavailable for discovery/opening while connected.
Successful explicit `close()` restores the original client and returns `None`;
context managers close too. Body and close failures are preserved together in a
`BaseExceptionGroup`. `closed` is a momentary ownership snapshot, not device
health or a reservation against another thread closing it.

Every failed open that enters the Rust transport invalidates the client,
including stale/foreign snapshots. Ordinary Python argument validation happens
first. Native discovery/send/close errors can also require a fresh client.
An interrupted isolated request invalidates its worker because native ownership
or send effects may be uncertain. Failure to allocate the returned connection's
clock after opening also discards that worker.
Input silence does not identify unplugging, rebooting, unresponsiveness or an
idle device. A failed send can already have changed physical state.

By default each client has a separate Python child process owning its native
transport. Input callbacks run in the parent. Native operation deadlines
terminate a failed child; native errors, crashes and timeouts invalidate that
client without automatically reopening it. `worker_failure` reports a known
worker failure, not endpoint health. `isolated=False` uses the same Rust
transport in-process with fewer resources and no native-operation deadline.
Both modes use a dedicated native owner thread per client for creation,
operations and teardown; Windows COM initialization stays on that thread.

Callbacks must be synchronous and return promptly. Deferred results are reported
through `sys.unraisablehook`, without executing them; later delivery continues.
Callback exceptions are reported the same way. Blocking MIDI operations are
forbidden from callbacks and error hooks, including operations on other clients.
No internal Rust lock is held while calling user code. A callback must not wait
for its closer or a lock/queue held by its closer. Arbitrary Python or extension
code cannot be forcibly cancelled; callback draining can wait indefinitely.
`last_message_time` records Python monotonic seconds at callback start, separate
from native timestamps, and survives close.

Native receive admission bounds owned in-flight payloads to 8 MiB per input by
default. Each isolated receive queue additionally holds at most 128 packets and
8 MiB of payload. The same input byte-limit setting applies at each stage.
`dropped_packets` combines known native-admission and forwarding/dispatch queue
drops; a worker crash can lose the final count, and driver loss is not measured.
Application-retained data, Python/queue overhead and OS buffers are outside these
budgets. Floods and allocation/process aborts remain possible.

Destruction and cyclic GC retire delivery and schedule cleanup without waiting
for a driver. Restoration is asynchronous; explicit close is the healthy reuse
path. Failed checked cleanup retains native state and its owner thread until
process exit without retry. The Rust core permits 32 native clients per process,
including retained clients; the Python cleanup registry permits 32 connections
and has four workers. Stalled callbacks or teardown can occupy all cleanup
workers. There is no global quota across isolated processes.

Importing transport initializes cleanup workers without opening a MIDI device.
Importing messages/events alone does not start them. Interpreter shutdown retires
callbacks and drains cleanup; isolated children that exceed their deadline are
terminated and reaped. In-process driver calls and parent callbacks can still
block shutdown. Forced termination bypasses graceful cleanup. Free-threaded
Python, subinterpreters and forking with imported/live MIDI state are unsupported.

## Build and qualification

`crates/meedee` is the Rust toolkit; the root crate binds it using PyO3 in one
private `meedee._core` extension. Rust use requires no Python. Both languages
use the same libremidi implementation; midir has been removed.

Source builds require Rust 1.87+, CMake 3.22+, a C++20 toolchain and CPython 3.11+.
Native libremidi sources are pinned and statically linked with no production
build-time download. macOS requires Command Line Tools and macOS 11+; Windows
requires the MSVC C++ tools. Linux requires ALSA development headers and
pkg-config for builds, and system `libasound.so.2` at runtime. Wheels do not
bundle ALSA. ALSA sequencer I/O also requires an accessible `/dev/snd/seq`.

Native OS backends are selected; optional JACK, PipeWire, network and keyboard
backends are disabled. CoreMIDI/UMP, available ALSA variants and WinMM are built.
Windows MIDI Services remains disabled pending pinned SDK/tooling integration
and native qualification. Windows discovery has a separate capability-query
failure risk recorded in [validation](plan/validation.md). Real Mac I/O,
Windows and Pi qualification remain open; device-free fixtures do not certify
hardware behavior. The latest real CoreMIDI attempt failed because service
access was unavailable in the tool environment.

Release CI remains triggered only by a published release, testing Python 3.11
and 3.15 on Linux, macOS arm64/Intel and Windows. It includes native regression
checks, Rust/Python checks, source/wheel builds and installed-package tests.
Configured jobs are not evidence of a passing release run.

```sh
uv run --frozen pytest
uv run --frozen ruff check --select B,E,F,I meedee test
uv run --frozen ruff format --check meedee test
uv run --frozen ty check meedee test
cargo fmt --all --check
PYO3_PYTHON="$PWD/.venv/bin/python" cargo test --locked --workspace -- --test-threads=1
uv run maturin build --release --locked --sdist --out dist
```

On Windows set `PYO3_PYTHON` to `.venv/Scripts/python.exe`. Native software-port
checks in `test/manual/loopback.py` and `test/manual/windows.py` are explicit
opt-ins and require target-host authorization. No unit test selects hardware.

meedee is MIT licensed. See [third-party notices](THIRD_PARTY_NOTICES.md) and
[vendored provenance](third_party/libremidi/SOURCE.md). Publication is a separate
decision after the remaining qualification gates.
