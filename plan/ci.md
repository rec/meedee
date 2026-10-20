# Slice 11: MIDI-CI management

Additional work beyond the prompt

None.

## Selected and deferred scope

The owner approved a pure `meedee.ci` API and caller-driven Rust state machines,
explicit identities/routes/recovery, bounded transactions and immutable values.
The full Profiles 1.1 and Property Exchange 1.2 documents are unavailable; the
owner explicitly deferred their dependent work. Generic profile transactions,
Get/Set/subscriptions/notifications, property resources, JSON headers and property
compression are therefore not implemented or advertised. The approved JSON/zlib
dependencies are not added before they are needed. Process Inquiry engines and
individual instrument-profile semantics also remain deferred.

The acquired MIDI-CI base document is M2-101-UM 1.2.1, draft 2025-12-15,
published 2025-12-18. Management support covers Discovery/Reply, Endpoint
Inquiry/Reply for Product Instance ID, Invalidate MUID and ACK/NAK. The session
advertises zero optional capability-category bits. No old protocol negotiation
is implemented; native UMP protocol configuration is a different mechanism.
Native qualification is separate from management codecs and simulated exchanges.

## Values and wire representation

Rust `ci` contains immutable Address, Header, Identity, DiscoveryInfo, Notice and
Management/Body values. Python exposes matching values and concrete frozen Message
subclasses Discovery, DiscoveryReply, EndpointInquiry, EndpointReply,
InvalidateMuid, Ack and Nak. Constructors are keyword-only and integer fields
reject coercion. Values support equality, hashing and checked replacement.
There are no package-root re-exports.

`Header` contains source_muid, destination_muid, version and address. Sources and
unicast destinations range from 0 through MAX_MUID (0x0FFFFEFF). The revision's
reserved range 0x0FFFFF00–0x0FFFFFFE is rejected. BROADCAST_MUID (0x0FFFFFFF)
is accepted only for Discovery and InvalidateMuid among these implemented messages.
These unchunked broadcast messages are limited to 512 complete wire bytes,
including any future extensions. The library does not choose random identities. Applications must supply a freshly
randomized local identifier initially and after restart; range checks cannot
establish randomness or physical uniqueness.

Address.channel(channel=1..16) converts to wire 0..15. Address.GROUP and
Address.FUNCTION_BLOCK are explicit scopes. Management requests/replies use the
Function Block scope, except ACK/NAK, which can retain channel/group addressing.
Function Block numbers remain protocol identifiers 0..31; None declares no
association and is encoded as 127 in format-2 replies. Group numbers in routes
remain 1..16, and None denotes a MIDI-byte route. Routing must describe the actual
connection and topology; the library does not infer Function Blocks.

Identity uses exactly three seven-bit manufacturer bytes, family/model 0..16383,
and four seven-bit revision bytes. One-byte manufacturer identifiers require two
padding zeroes. It neither allocates nor certifies a manufacturer identifier.
DiscoveryInfo carries the declared category bitmap, receive size (at least 128),
and output_path. output_path is absent in format 1 and required in format 2 or
higher. A format-2 reply echoes the initiating path ID, independently of the
responder's own output numbering. Codecs retain peer declarations without
interpreting them as verified support.

The native codec implements formats 1 and 2 where defined, including the short
format-1 NAK. Endpoint Inquiry/Reply and ACK require format 2. Known layouts reject
truncation and unexpected trailing fields. Compatible future minor versions retain
appended bytes as extensions and may retain unrecognized Function Block values
as raw declarations; those values are not usable known topology. Reserved major-version bits fail. The session can
produce a version-not-supported NAK without inventing the new layout. Non-CI SysEx,
unsupported CI categories and unknown Endpoint selectors return None from
ci.decode; malformed supported layouts raise ValueError. Callers retain the raw
SysEx. Future extensions certify neither the unknown fields nor safe execution.

Notice retains original classification, status, status_data, five detail bytes
and raw seven-bit message_text (maximum 16383 bytes). Text keeps JSON-style Unicode
escapes without decoding or normalizing them. Applications should interpret that
text before display; no property JSON parser is implied. Product Instance IDs
are at most 42 printable ASCII characters. They are peer declarations, not an
identity authentication or an automatic reconnect key.

codecs.encode accepts CI Message values. Raw byte decoding still returns SysEx;
ci.decode is explicit. For UMP, use ump.packetize_sysex7 with an explicit group,
assemble incoming SysEx7, then call ci.decode on its payload. No packet words are
sent through a MIDI-byte API and no implicit protocol conversion is introduced.
Events/archives retain CI wire meaning as primitive SysEx or original UMP packets;
there are no new archive record IDs or implicit semantic promotion.

## Caller-driven session

Rust ci_session and Python ci.Session use the same implementation. A session
holds one explicit input/output Route, optional UMP group, local MUID/identity,
output path and optional Function Block. It opens no ports, owns no transport,
spawns no thread, has no background timer and performs no I/O or user callbacks.

Use one serialized application control flow for a session:

1. discover(now_ns=...) or inquire_endpoint(...) returns an immutable Action with
   a unique monotonically increasing token, route and message.
2. Explicitly send through the selected output. Report backend acceptance or
   failure with report_send. Replies arriving first are staged provisionally
   within count/byte limits. Successful reporting applies those replies; failure,
   timeout or reset discards them without populating peer state. This
   handles callbacks arriving before a send returns without treating unsent
   intentions as confirmed transactions. Inspect report_send updates too.
3. Feed complete CI messages or SysEx with the exact input identity/group and
   caller-supplied monotonic integer nanoseconds. Unrelated routes are errors.
4. Inspect Action and Observation updates. Generated replies are proposed actions,
   never automatic sends. Report every action's send result, including generated
   discovery replies, invalidations and NAKs. Endpoint requests require an explicit
   respond call with supplied Product Instance ID or ACK/NAK.
5. Call poll to observe expiry, and reset(reason=...) after known transport loss,
   disconnect/reconnect or ambiguous framing. Restart explicitly with a new random
   MUID. Retire any previously returned actions rather than sending stale work.

Discovery waits at least three seconds after reported send acceptance and accepts
multiple matching replies during the window. One discovery is pending at a time;
one Endpoint inquiry is pending per peer. Endpoint inquiries require a discovered
format-2 peer. Other timeout durations are explicit. Waiting for send confirmation
also has a bounded deadline; it is reported separately from a device-response
expiration. Regressing caller time, zero deadlines and integer overflow fail.
No timeout identifies an offline or failed device, and no retry is performed.

Management replies have no application transaction nonce. A matching identical
late reply may be indistinguishable from a current reply. Path IDs, addresses and
MUIDs reduce misrouting but cannot solve that ambiguity. Product Instance ID caches
are cleared when peers are invalidated. Conflicting identities, versions or known
Function Block numbers for one MUID produce an explicit invalidation action;
uncertain pending work is discarded so queue exhaustion cannot suppress recovery.
Identical physical devices claiming identical identifiers may remain indistinguishable.

A valid Discovery/Reply claiming our own MUID uses collision-resolution Option B:
propose broadcast invalidation of that identifier, discard peers/transactions and
require explicit restart. Malformed messages do not become collision evidence.
Receiving local invalidation also requires restart. Receiving peer invalidation
clears that peer and its transactions and ends the active discovery window, so
an already in-flight late discovery reply cannot recreate the invalidated peer.
Other endpoints require a new explicit discovery. None of these operations resets notes,
restores protocols, disables profiles or changes physical device state itself.

ACK/NAK preserve their details and notifications. A matching Endpoint NAK ends
that pending inquiry; a format-1 NAK lacks sufficient classification to choose an
unrelated pending inquiry safely. Deferred profile/property ACK flow-control,
waiting and retry instructions are not applied. Unsupported addressed categories
produce an explicit NAK action. No optional category support is claimed.

## Bounds and qualification

Defaults are 16 peer declarations, 16 outstanding actions/transactions and a
1 MiB advertised/accepted complete SysEx size, plus 8 MiB aggregate provisional
reply storage (reply_byte_limit). Provisional Discovery replies are count-limited
by peer_limit; one provisional Endpoint reply/NAK is retained per request. Reply
text has its protocol's
16383-byte limit, identities are fixed size, Product Instance IDs are limited
to 42 bytes and route identities to 1024 bytes each. Admission returns errors or
resource-limit observations instead of evicting peers silently. Completed action
tokens are not reused. Timeout/reset/forget remove retained transaction state.
Caller-held snapshots/results and native/Python object overhead remain outside
these bounds. There is no growing log or unbounded result queue inside a session. Provisional
admission excess is reported and the excess reply is not retained.

The approved 16 KiB property-header, 1 MiB encoded/decompressed body and 8 MiB
aggregate property budgets will apply when that deferred subsystem is implemented;
no unused property configuration fields or compression dependencies are introduced.

Wire fixtures identify normative tables and are shared across Rust and Python.
Tests exercise legacy/future formats, every truncation boundary, reserved values,
explicit routes/groups, deadline/send-result handling, collision recovery at
capacity and known-loss reset. Pure module imports must not start MIDI workers.
The opt-in native fixture uses only its own temporary UMP virtual endpoints,
experimental manufacturer identity and freshly randomized test MUIDs. It sends
only management discovery/reply, with zero optional capabilities. Windows, Pi and
physical UMP qualification remain open; see validation.md for the actual outcome.

## One-scan management browser

Rust `ci_browser::Browser` owns a fresh `ci_session::Session`; Python `ci.Browser`
constructs the same shared core from the existing identity, route and resource
settings. Neither construction nor imports perform I/O. The local identity and
MUID are explicit library inputs. Optional capability-category bits remain zero.
The browser starts once, waits for one Discovery window, then queries each
format-2 peer observed by the end of that window sequentially for its Product
Instance ID. Format-1 peers remain visible without endpoint inquiries. Later
unsolicited discoveries may appear in reports but do not extend the target list.

Defaults are a three-second discovery window after handoff, one-second endpoint
and send-confirmation deadlines, a thirty-second overall scan deadline starting
at `start`, and 128 diagnostic samples. Discovery send confirmation retains the
session's discovery-duration admission deadline. The overall deadline must
exceed discovery duration. Existing peer/exchange/provisional-byte limits apply.
The overall deadline bounds activity even if incoming requests generate replies;
native calls and connection setup/teardown are outside its interruptibility.
A timeout is an observation, never proof that a device is offline.

`start(now_ns)`, `feed(message, input=..., group=..., now_ns=...)`, `poll(now_ns)`
and `report_send(token=..., sent=..., now_ns=...)` return proposed `ci.Action`
values. Use one serialized control flow and one nondecreasing host monotonic
clock. Every action must be handed off at most once and reported. The frozen
`BrowseReport.pending_tokens` list identifies current sendable actions; discard
retired queued tokens before sending. Full-action confirmations count transport
handoffs. An interrupted or failed action may have sent a prefix of UMP packets;
its token is uncertain and none of that action counts as fully confirmed.
No per-packet acceptance or instrument acknowledgement is inferred.

The browser preserves existing generated Discovery replies, NAKs and identity
collision invalidations. A known peer's Endpoint inquiry receives an unsupported
NAK because no local Product Instance ID is defined by this tool. An unknown
requester is diagnosed without inventing peer declarations. Local/peer collisions
retire ordinary pending actions, return only required invalidation actions, and
stop after their send results; no identity restart or rediscovery occurs. Peer
invalidation, resource exhaustion and missing send confirmation stop immediately.
Known input loss/failure and cancellation stop the host. There are no retries,
reconnects, profile/property operations, tuning writes or MIDI-state restoration.

Reports include status (`ready`, `discovering`, `inquiring`, `stopping`,
`completed`, `stopped`), local MUID, the last peer declarations, confirmed action
count, uncertain token, sendable tokens, bounded diagnostics/omission count,
problem count and terminal reason. Diagnostics retain kind, MUID, token and at
most 1024 characters of reason, without retaining raw reply payloads. `stop`
retires sends and retains the first terminal reason, likewise bounded. Stopped
reports preserve the last peer snapshot, which may have become stale or invalid;
no identity, capability or Product Instance ID is authenticated. Caller-owned
copies and transient session updates remain outside retained report budgets.

`ci_live.run(browser, receiver, output, cancel=None, ...)` starts a fresh browser
on existing typed connections. A route without a group means MIDI 1; an explicit
group means UMP SysEx7. Connections must use that protocol and the receiver must
match the route's stream. The host reuses the existing UMP assembler, ignores
other groups and unrelated message families, and stops on input diagnostics or
SysEx7 assembly failure. Loss counters are checked before every packet send.
The caller owns connections. Cancellation is checked between sends and receive
waits of at most 50 ms; an in-flight transport call cannot be forcibly cancelled.
Operational failures propagate after progress is recorded; Ctrl-C returns a
stopped report. Unexpected exceptions also leave the browser inspectable.

`meedee ci-browse` opens one exact, unique input and output, resolves both names
before opening either, and uses process isolation by default. It requires the
protocol, explicit UMP group where applicable, and explicit manufacturer (three
seven-bit hex bytes), family/model (0..16383) and revision (four seven-bit hex
bytes). One-byte manufacturer IDs use two zero padding bytes. The fields identify
the local participant; they are not a remote-device filter. The CLI draws a fresh
random legal MUID for each scan and advertises zero optional capabilities. Input
and output path/Function Block declarations are explicit settings, not inferred
physical topology. Connections close on completion, interruption and failure.
Text/JSON output contains the bounded final report, including declared capability
bits without interpreting deferred categories. Status 0 means a complete scan
without reported problems (including a scan with no peers), 1 means observations
with problems or an incomplete/cancelled scan, and 2 means argument/setup/access,
transport/close errors or uncertain output. Shutdown errors/interruption remain
visible even when a scan has completed. Device-free tests do not establish native
MIDI-CI interoperability or qualify any instrument.
