# meedee binary archive version 1

Approved binary format for slices 9–10. Extension: `.meedee`.

Additional work beyond the prompt

None.

## Encoding

All integers and UMP words are big endian. Integers have the widths below;
there are no host-layout structs, floats, pickles or implicit compression.
The 12-byte header is ASCII `MEEDEE`, CR LF, version u16 `1`, reserved u16 `0`.
Other headers/versions are rejected. A frame consists of payload length u32,
that many payload bytes, then CRC u32. CRC is CRC-32/ISO-HDLC over the payload
only: polynomial 0x04C11DB7 (reflected 0xEDB88320), initial/final XOR 0xFFFFFFFF.
The check value for ASCII `123456789` is 0xCBF43926. CRC detects accidental
corruption; it provides no authentication.

A payload starts with record kind u16 followed by its fields. `blob` means a
u32 byte length plus bytes; `string` is a UTF-8 blob. A boolean/optional presence
flag is one byte, exactly 0 or 1. Optional fields include their value only when
present. Lengths are checked against the configurable payload limit (default
1 MiB) before allocation. Nested fields cannot exceed the containing frame.

| ID | Fields after kind |
| --- | --- |
| 1 Clock | identity string; tick numerator u64; tick denominator u64; origin string; optional resolution in ticks u64; optional backend i32; mode byte: 0 unspecified, 1 disabled, 2 backend absolute |
| 2 Stream | identity string; descriptive label string |
| 3 Event | stream identity string; optional Timestamp; optional SourceSpan; message representation byte; remaining bytes are its payload |
| 4 Gap | stream string; observed lost count u64; reason string |
| 5 Metadata | key string; data blob |
| 6 Discontinuity | stream string; reason string |
| 65535 Completion | preceding frame count u64; complete boolean; reason string |

Timestamp is ticks i64 followed by clock identity string. SourceSpan is first
packet u64, last packet u64, optional first Timestamp, optional last Timestamp.
It describes contributing packet boundaries, not intra-packet accuracy. A span
cannot reverse packet indexes or combine clock generations. Timestamp absence
has its own presence flag and is distinct from zero. Equal timestamps and
negative ticks are preserved. File order is authoritative; clocks do not imply
a global physical ordering.

A clock requires nonempty identity/origin and positive rational tick unit;
known resolution is a positive integer tick count. Native backend and mode
are both supplied or both absent. Saved native backend IDs match the pinned
transport IDs; descriptors are archival metadata, never instructions to open
ports. Clock and stream descriptors precede referencing records. Duplicate IDs
are rejected. The initial implementation admits at most 128 descriptors and
8 MiB total descriptor string bytes, independent of the per-record limit.
A long-running capture that exhausts these bounds must start another archive.

## Message representations

1. One complete primitive MIDI 1 message, including status and SysEx F0/F7
   framing. It is not a running-status stream or a sequence of commands.
2. Exactly one UMP (one to four words), serialized big endian. Opaque UMPs can
   be preserved when explicitly requested, without asserting semantic validity.
3. Semantic command ID u16 followed by its fixed-order signed i64 fields:

| Command ID | Value | Fields |
| --- | --- | --- |
| 1 | ControlChange14 | channel, controller, value |
| 2 | RpnSet | channel, parameter, value, resolution |
| 3 | NrpnSet | channel, parameter, value, resolution |
| 4 | RpnIncrement | channel, parameter |
| 5 | RpnDecrement | channel, parameter |
| 6 | NrpnIncrement | channel, parameter |
| 7 | NrpnDecrement | channel, parameter |
| 8 | BankProgramSelect | channel, bank_msb, bank_lsb, program |
| 9 | ZoneConfiguration | manager_channel, members |

Command fields use public musical numbering and their normal checked ranges;
commands are not expanded during storage. Profile/tuning SysEx interpretation
is explicit, as in the byte codec: archive a primitive SysEx when retaining its
wire meaning, then use the relevant module's decoder when reading it. No class
names or unstable Rust enum discriminants are persisted.

## Completion, recovery and resources

Exactly one Completion frame is last, with no trailing bytes. Its frame count
excludes itself. Normal reading rejects missing/incomplete trailers, bad CRCs,
malformed records, undeclared references and unknown kinds. `preserve_unknown`
retains unknown kinds or unsupported message representations as Opaque records,
including the original kind and bytes. Such records are storage, not executable
messages. This is not permission to ignore malformed known layouts.

`recover=True` stops at the first failure and exposes verified complete prefix
records, `complete=False` and a problem string with record/byte location. It
never skips corruption to invent a later valid sequence. Header failure still
raises: there is no established archive to recover. Records yielded before a
normal reader reaches the trailer are provisional; merely iterating a prefix
does not establish complete capture. To preserve forward records verbatim,
write their Opaque values without interpreting them.

Explicit writer finish emits the trailer and flushes. Abandoning a writer leaves
an incomplete file. Write/flush failures propagate; a failed writer cannot be
resumed or claim completion. A successful flush is not a power-loss durability
promise: fsync, atomic replacement and media guarantees are not supplied.
Streaming bounds working record/descriptor storage, not returned records,
caller-held objects, allocator overhead, OS buffers or CPU use. File errors
must be handled even when the source is local. Blocking filesystems remain a
liveness risk outside native MIDI isolation.
