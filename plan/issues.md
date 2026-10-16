# Open issues and failure review

Updated through the native qualification and discovery follow-up, 2026-10-16. These are
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

Windows native qualification remains open. Windows MIDI Services SDK/tooling
integration is unfinished and that backend is disabled; requesting it raises a
capability error. The new WinMM discovery failure/recovery and error-hook tests
still need Windows compilation and execution in ANSI and Unicode builds.
The selected Pi image/sysroot and cross-build remain open.
Device-free native tests and virtual loopbacks do not establish driver-fault
recovery or physical hardware guarantees.

The process-lifetime CoreMIDI keeper passes deterministic concurrency/reopen
tests and five native virtual-only qualification cases. Native repeated reopen
remains unqualified: the final run failed during output-port discovery with
`PortInfoError: could not read CoreMIDI port unique ID`, before completing any
protocol/mode combination. The failing endpoint, cycle and OS status were not
recorded in the original run. A subsequent instrumented standalone run delivered
and closed the first MIDI 1 isolated cycle, then failed at discovery in cycle 2
after creating its new virtual input. The other five cases are not required to
trigger it. The next run reported object `158564417`, OSStatus `-50` (`paramErr`)
at the same stage. That code does not identify whether the endpoint was stale,
which lifecycle operation invalidated it, or why discovery returned it. A direct
SDK probe reproduced stale endpoint enumeration without either wrapper: in
cycles 2 and 3 the consumer could still enumerate the disposed cycle-1 endpoint.
Early client creation did not fix it; 50 ms run-loop servicing was inconsistent.
Notification registration, with or without a 50 ms consumer-loop wait, also
failed; neither run logged notifications. The probe now runs both owning loops
continuously and waits for matching add/remove events. Its agent-context attempt
failed at client creation before any endpoints, with `268435459`; ordinary
Terminal evidence is pending. No discovery errors are suppressed and reopen
remains unqualified.
Full-width MIDI 2 CC delivery in that fixture also remains unverified.
See `qualification-macos-final.jsonl` and
`validation.md` for the successful cases and the preserved failure.

## R2. Resource admission is bounded only in specified places

**Confirmed limits; exhaustion behavior partly unqualified.** The Rust core
admits 32 native clients per process, including retained owner threads. The
Python registry admits 32 connections with four cleanup workers. Each client
has a native owner thread; each isolated client also has a Python child and
communication threads, and each isolated input has forwarding/dispatch threads.
CoreMIDI also retains one callback-free keeper client per MIDI-using process
until process exit, including isolated children; this is outside connection counts.
Per-parent count limits do not impose a machine-wide memory budget across
independent applications. Many idle clients can still consume substantial memory,
threads, handles and scheduling time on a Pi.

Native receive admission bounds owned in-flight payloads by bytes. Each isolated
receive queue has a count limit of 128 and an 8 MiB default payload limit.
Application-retained data, packet/tuple/object overhead, OS buffers and copies
in flight are outside the queue budgets. Raw input avoids the old unbounded
unfinished-SysEx assembler. The pure byte decoder now defaults to a separate
1 MiB retained SysEx payload cap with overflow diagnostics. Feed snapshots and
returned message/diagnostic lists remain proportional to caller-supplied input;
the limit is not a total-memory or processing-time budget. Packet floods,
oversized sends, discovery lists and
allocation/process aborts remain resource risks. Rust/native allocation failures
are not universally recoverable Python exceptions.
The optional control interpreter has fixed state for 16 channels of one source;
many application-created interpreters and retained observation lists still cost
memory. Built-in commands expand to at most six messages, but their native
duration remains driver-dependent.

MTS values have fixed limits (128 table entries, 127 single-note updates, 12
scale offsets) and snapshot bounded built-in list/tuple inputs. Application-held
values and many independent trackers still consume memory. Request trackers
hold no growing reply registry and perform no background work.

Known drop counters cover native admission and queue overflow, not driver loss,
user-code exceptions or packets lost with a crashing child. CPU/GIL contention
and slow error hooks can delay unrelated Python work. No hard real-time or
throughput guarantee is made. Blocking disk/network operations in callbacks or
error hooks can become MIDI liveness failures.

File codecs and recording now own files. Payload, descriptor and merge-lookahead
limits bound specified working data, not total files, application-held objects,
filesystem buffers or CPU time. A blocked filesystem can retain a recording thread
and handle after a bounded stop times out; there is no safe forced thread kill.
Disk-full/short-write/flush failures report incomplete output, but successful flush
is not fsync or power-loss durability. Output paths use ordinary truncating opens.
Capture budgets include queued/in-flight records; many recordings have no global
thread/disk quota. No library network protocol is introduced. OS network-MIDI
endpoints retain their own failure behavior.

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
Even a closed connection retains its reusable client. Closing that connection
does not free the client's isolated-worker slot; retaining many such objects
can exhaust the per-parent cap until those references and workers retire.

Receive callbacks take one immutable ReceivedPacket containing raw byte chunks
or UMP batches, not complete musical Message objects. A chunk can contain partial
SysEx. Sending takes byte buffers or UmpPacket and validates framing/protocol,
not full musical semantics. Mutable buffers are copied. UMP word tuples preserve
all 32 bits. Invalid callback signatures/deferred functions are rejected before
opening; deferred return values are reported without execution.

The optional pure byte decoder has no timestamps or automatic transport-loss
notification. Use a separate decoder per source, reset after observed loss,
and finish at end of stream. Its strict SysEx policy discards implicit non-F7
termination with a diagnostic, despite the standard permitting it. Message
encoding includes explicit statuses and is not Standard MIDI File encoding.

Compound commands require expand/send_command, not primitive encode or raw send.
Their serialization covers one connection, not other clients/applications using
the device. A completed prefix is backend acceptance, not physical confirmation;
the failed step may have effects, and process loss/timeouts leave progress unknown.
There is no retry or post-failure cleanup send, so parameter selection can remain
active or partially changed. In-process failed connections retain ownership until
teardown, even while rejecting sends. The optional interpreter cannot repair
unknown loss and deliberately declines single-half edits to a completed parameter
selection. Callers retain raw messages and reset source state after known loss.

MTS interpretation is explicit; the byte decoder continues to return SysEx.
Original bulk-dump checksum mismatches are accepted with status because the
standard documents differing manufacturer implementations. Later dump formats
require valid checksums. Re-encoding an original dump normalizes its checksum;
retain raw SysEx when exact received bytes matter. Frequency/cents conveniences
quantize to the selected wire resolution and report signed error.

Tuning reply trackers require caller-driven monotonic time and polling/observation
to notice expiry. MTS has no transaction ID, so identical late or unsolicited
dumps cannot be distinguished from a reply. Correlation does not establish
physical instrument support or successful retuning; those operations remain
unqualified. No automatic request, resend, bank selection or reset is performed.

Raw transport timestamps are exact signed nanoseconds or None. Zero is valid. Clock identity
distinguishes connections and isolated children but does not synchronize them.
Absolute backend time is not UTC, may have a host fallback and has unknown
resolution. `last_message_time` is a separate Python monotonic observation.

Creating/discarding a client does not create or reset hardware or intentionally
send MIDI. It can allocate native resources and produce topology changes.
Dropping a virtual endpoint can disrupt peers connected to that endpoint.
Cleanup does not restore musical state or promise delivery of pending output.
Native driver failures and client-level disposal errors prevent a guarantee of
zero effects on unrelated applications using the same OS MIDI service.

Profile helpers do not establish instrument capabilities. GM1 entries deliberately
have no bank numbers; GM2 table addresses can be unsupported by a particular
instrument. Bank/program commands can leave a changed bank without a completed
ProgramChange after partial failure. GM mode messages can mute/reset an instrument;
callers choose targets and settling delays. Only explicitly supplied typed GM
mode messages invalidate control-interpreter context, not arbitrary raw SysEx.
MPE configuration does not allocate voices or prevent competing zone changes.
Full-frame time code represents a position, not a running or synchronized clock.
Profile codecs and state engines cannot infer device capabilities, release envelopes
or physical synchronization. MPE zone reconfiguration and known transport loss must
be fed explicitly. Quarter-frame sequence lock requires caller polling to detect
silence; neither it nor prefix tracking can detect unknown dropped input.

UMP values/codecs do not establish live MIDI 2.0 support or negotiate protocols.
Opaque preservation also retains rejected complete layouts, without certifying
validity or safe transmission. Declared capabilities can leave support unknown.
Reset payload assembly after known framing/transport loss; a missing fragment
cannot always be detected from otherwise valid continuation packets. Default
1 MiB message/8 MiB aggregate/64-stream limits bound retained payload, not Python
inputs, returned lists, allocator capacity or application-held objects. Native
allocator exhaustion and CPU contention remain possible.

Stateless translation declines missing source/profile context. Quantized output
requires explicit permission; it does not permit semantic attribute stripping.
Translated primitive lists are not atomic sends, do not report physical delivery,
and parameter sequences can leave selection active. JR ticks, stream offsets
and host timestamps remain different clock domains. MDS transfer and semantic
text interpretation are outside the packet codec. Typed/timed live integration
and MIDI-CI management are implemented; Profiles, Property Exchange and native
platform validation remain open.


## F1. File completion, clocks and performance views require explicit policy

**Deliberate boundaries and remaining qualification.** A streaming archive reader
can yield valid records before discovering a corrupt/incomplete trailer. Playback
from such a reader can therefore send an earlier prefix before failing. Preverify
when that is unacceptable. CRC-32 detects accidental corruption, not authenticity;
archives never execute Python code. Recovery returns only a verified prefix and
does not prove a complete musical capture. Best-effort files can be structurally
complete while containing gap records; inspect gap_records and the actual records.

File semantic round trips do not reproduce original running-status bytes. SMF/Clip
TimedReader is a performance view, omitting metadata/configuration as documented;
raw readers are required for faithful file editing. Exact rational clock arithmetic
is bounded to checked integer widths, and long files may exceed the signed typed
timestamp range while raw file ticks remain readable. Merged SMF input must be
seekable, and Type 2 requires an explicit independent sequence. Split-SysEx state is
independent per selected track, so its payload budget multiplies by active track
count; lower the payload/track limits on constrained targets. MIDI Clip profile
negotiation/semantic qualification remains deferred pending specification access.

No clock mapping establishes physical synchronization. Timestamps can be absent,
driver fallback time, inaccurate, discontinuous or only completion time. Packet
spans cannot reconstruct individual-byte timing. Quarter-frame time-code event
views lack precise first-quarter contributor timing and expose None.
Fake-device/clock tests and short virtual loopbacks do not qualify scheduling
under sustained load, physical instruments, OS service crashes or target Pi
resource behavior.


## C1. MIDI-CI management cannot establish device health or trusted identity

**Deliberate limitations.** Only management is implemented. Profiles/Property
Exchange are deferred by the owner pending normative documents; optional-category
support is not advertised. Sessions need explicit routing, caller-randomized MUIDs,
caller-driven time/send-result/input handling and polling. Early replies use
bounded provisional storage; overflow is reported rather than silently consuming
unlimited memory. Applications must inspect all updates, including send results.

MUID/identity/path checks are not authentication. Identical peers can make collisions
undetectable, and matching late replies cannot always be distinguished from current
ones. Product Instance IDs do not authorize automatic reconnect or restoration.
Timeouts establish unanswered requests, not offline devices. Local collision/loss
requires a fresh randomized MUID and explicit restart; no notes, profiles or physical
state are restored. Previously returned actions must be retired after reset. Native
UMP qualification on Linux/Windows and physical CI interoperability remain
open independently of codec/session tests and the CoreMIDI virtual exchange.
