# Optional native MIDI process isolation

## Current shared implementation

Rust owns the worker protocol, process launch, operation deadlines, bounded
forwarding, terminal failure retention and child reaping. Python uses this same
transport through its native bindings; the former Python pickle pipe engine and
worker service are removed.

Python Input/Output keep run_in_subprocess=False and timeout=5 seconds by default.
subprocess_worker="python" starts the current interpreter with -m middy._worker;
that module only enters the Rust service. subprocess_worker="rust" launches the
middy-worker executable. subprocess_executable optionally selects the interpreter
or binary, with no implicit fallback. CLI commands expose the corresponding flags.
The existing CLI/discovery subprocess defaults remain unchanged.

Rust Input and Output expose with_run_in_subprocess(bool) and
with_subprocess_options(SubprocessOptions). The typed options contain program,
args and a Duration timeout; defaults are middy-worker on PATH, no arguments and
five seconds. Configuration is validated before discovery or connection. The
standalone binary is part of the Rust crate and needs no Python runtime. Python
wheels need only their ordinary extension and the interpreter bootstrap.

Construction remains lazy. Each client uses its own child and immutable,
generation-bound port snapshots. Opening uses the retained child snapshot rather
than rediscovering endpoints. Closing is terminal in both high-level APIs.
Timeouts, unexpected child exit and terminal native failures invalidate ownership;
there are no retries or automatic reconnects. Error categories, exact owned
messages, nested causes and known compound-send progress survive the pipe.
Rust Capability, Invalid and Resource transport errors now own String values.
A compound operation has one deadline across its entire expanded message list.

Receive traffic preserves signed timestamps, disabled timestamps, MIDI bytes,
UMP words and source sequence numbers. Both forwarding stages have a 128-packet
and configured byte budget, including in-flight delivery. Overflow drops newest
traffic and reports counters even if input then becomes silent. User callbacks
remain in the parent and are not serialized. Callback guards, failure reporting,
retirement and draining use the existing shared native mechanisms.

The parent admits at most 32 live subprocess workers, independently of the native
client budget. Startup reservations count against this limit; failed starts and
confirmed exits release them. Destruction schedules cleanup on the owner thread,
without joining a blocked driver from the application thread. A child seeing EOF
on the parent pipe exits even if its native operation is blocked. Worker deadlines
cannot cancel arbitrary parent callback code or reset physical instrument state.

Validation uses the actual Rust client against a compiled, device-free worker
fixture. Python tests retain callback, buffer, loss, UMP, lifecycle and compound
progress coverage. Rust tests cover the public builders, direct worker binary,
timeouts, crashes, spawn failure, admission and reaping without loading Python or
opening MIDI endpoints. Native hardware and OS-backend qualification remain
separate from these device-free checks.

## Additional work beyond the prompt

None.
