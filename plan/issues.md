# Open issues and failure review

Updated for the libremidi Python transport replacement, 2026-10-14. These are
remaining risks, deliberate API limitations and validation gaps. Resolved midir
binding findings are removed. The historical upstream report remains in
[midir-suggestions.md](midir-suggestions.md); it is not an audit of libremidi.
Evidence and platform qualifications are recorded in [validation.md](validation.md).
Remove resolved findings in the final commit of their fix series.

Additional work beyond the prompt

None.

## L6. User callbacks and in-process native calls can prevent progress

**Confirmed limitation.** Default isolation gives native operations deadlines
and terminates failed children. It cannot cancel arbitrary parent Python code,
a C extension holding the GIL, application locks or a shared OS MIDI service.
In-process discovery/open/send/close has no native-driver deadline. A blocked
native operation or callback can stall explicit close and interpreter shutdown.
All four Python cleanup workers can become occupied by stalled jobs.

Callbacks and error hooks must return promptly and must not wait for their
closer, a lock held by their closer, or a full queue whose consumer has stopped.
The MIDI callback guard rejects blocking MIDI operations, including operations
on unrelated clients; it cannot detect application synchronization. Body and
close exceptions are preserved together, but that does not resolve a deadlock.
Releasing a callback can also run its Python finalizer, which must not block.

Exit-handler ordering matters. Forced termination bypasses graceful cleanup and
can leave physical note/controller state unchanged. Forking with imported/live
runtime state, subinterpreters and free-threaded builds are unsupported.
Reinitializing an embedded main interpreter in the same process remains
unqualified because the process-wide cleanup registry survives finalization.
OS failure to kill/reap a child and parent process startup are outside an
absolute operation-deadline guarantee.

## D1. Connection ownership is not device health

**API limitation and validation gap.** `closed` reports an ownership snapshot,
not whether a device is present or responsive. An idle, unplugged, rebooted or
unresponsive input can produce the same silence. A send may be accepted and
then lost; a send that fails may already have changed the device. No heartbeat,
automatic reconnect, driver recovery or remote acknowledgment is supplied.

An endpoint can disappear between discovery and connect. Replugging or rebooting
can reuse IDs, change metadata or create a new native endpoint. Snapshot identity
prevents using another client's or refreshed discovery's selection; it does not
prove persistent physical identity. Deliberately rediscover with a fresh client
after failures and decide whether uncertain operations are safe to resend.

Native physical unplug/replug, driver/service crashes, reboot and exhaustion
remain unqualified across platforms. The old midir software-loopback evidence
does not qualify the replacement backend.

## B1. Native backend safety and platform qualification remain incomplete

**Confirmed source risks and validation gaps.** Checked C API input teardown
covers the patched CoreMIDI, WinMM and Windows MIDI Services paths. Checked
CoreMIDI output teardown covers endpoint/port disposal. Failed checked cleanup
retains handles, contexts and the owning thread. Direct C++ destruction,
client-level CoreMIDI disposal errors and other output backends are outside
that guarantee. Ignored disposal errors can leave OS resources whose extent is
not measured by the library's known retained-resource count.

Windows capability-query failure paths need work: pinned WinMM observer/name
helpers read MIDIINCAPS/MIDIOUTCAPS without checking every capability query's
return value. Device disappearance during enumeration may leave those structures
unwritten. Windows compilation alone would not resolve this risk.

Windows native qualification remains open. Windows MIDI Services SDK/tooling
integration is unfinished and that backend is disabled; requesting it raises a
capability error. The selected Pi image/sysroot and cross-build remain open.
Real CoreMIDI software I/O is also open: service access was unavailable during
the latest tool-environment attempt. Device-free native tests exercise code
paths and injected faults, not driver or hardware guarantees.

## R2. Resource admission is bounded only in specified places

**Confirmed limits; exhaustion behavior partly unqualified.** The Rust core
admits 32 native clients per process, including retained owner threads. The
Python registry admits 32 connections with four cleanup workers. Each client
has a native owner thread; each isolated client also has a Python child and
communication threads, and each isolated input has forwarding/dispatch threads.
There is no global quota across isolated children. Many idle clients can still
consume substantial memory, threads, handles and scheduling time on a Pi.

Native receive admission bounds owned in-flight payloads by bytes. Each isolated
receive queue has a count limit of 128 and an 8 MiB default payload limit.
Application-retained data, packet/tuple/object overhead, OS buffers and copies
in flight are outside the queue budgets. Raw input avoids the old unbounded
unfinished-SysEx assembler; a future message decoder needs its own independent
size and completion policy. Packet floods, oversized sends, discovery lists and
allocation/process aborts remain resource risks. Rust/native allocation failures
are not universally recoverable Python exceptions.

Known drop counters cover native admission and queue overflow, not driver loss,
user-code exceptions or packets lost with a crashing child. CPU/GIL contention
and slow error hooks can delay unrelated Python work. No hard real-time or
throughput guarantee is made. Blocking disk/network operations in callbacks or
error hooks can become MIDI liveness failures.

The transport owns no recording files or network protocol. Disk exhaustion
primarily affects builds, installation and application logging. OS-provided
network-MIDI endpoints retain their own network failure behavior.

## A1. Raw transport and ownership can surprise Python users

**Deliberate API limits.** Rust and Python share `Input`, `Output`, `Port`,
protocol/backend/mode names and transport methods. Python adapts consuming Rust
ownership: successful `close()` restores the original client and returns None.
Every failed Rust open invalidates it. There are no compatibility aliases for
the removed midir API, native naming arguments, ID lookup or Ignore filters.

Retain the returned connection. Dropping it retires input delivery and schedules
cleanup; immediate client reuse can fail until cleanup completes. Keeping the
client variable alone does not keep its connection open. Connections hold their
clients and expose callback edges to Python's collector, but GC timing is not a
lifetime guarantee. Port snapshots hold metadata without keeping clients alive.

Receive callbacks take one immutable ReceivedPacket containing raw byte chunks
or UMP batches, not complete musical Message objects. A chunk can contain partial
SysEx. Sending takes byte buffers or UmpPacket and validates framing/protocol,
not full musical semantics. Mutable buffers are copied. UMP word tuples preserve
all 32 bits. Invalid callback signatures/deferred functions are rejected before
opening; deferred return values are reported without execution.

Timestamps are exact signed nanoseconds or None. Zero is valid. Clock identity
distinguishes connections and isolated children but does not synchronize them.
Absolute backend time is not UTC, may have a host fallback and has unknown
resolution. `last_message_time` is a separate Python monotonic observation.

Creating/discarding a client does not create or reset hardware or intentionally
send MIDI. It can allocate native resources and produce topology changes.
Dropping a virtual endpoint can disrupt peers connected to that endpoint.
Cleanup does not restore musical state or promise delivery of pending output.
Native driver failures and client-level disposal errors prevent a guarantee of
zero effects on unrelated applications using the same OS MIDI service.
