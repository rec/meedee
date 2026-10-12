# midirp: Rust MIDI toolkit and matching Python API

## 1. Purpose and status

Build a cross-platform Rust MIDI toolkit with the public features and names of
Mido, and expose the same toolkit to Python. Applications should use one musical
API in either language. Python applications must not need Mido as a runtime
companion, and Rust applications must not need Python.

The first design baseline is **Mido-compatible names, not redesigned names**.
Changes or additions belong in the separate extension section of this plan.
Do not rename `Message`, `MidiFile`, `send`, `receive`, or other established
names merely to express a preference. Identify a concrete language constraint
or behavioral problem before proposing a departure for the owner to review.

This is a proposal for review, dated 2026-10-12. It authorizes writing this plan,
not the workspace restructuring, implementation, dependency changes, native
runtime tests, network services, or publication described below. The existing
raw MIDI binding remains unchanged until implementation is authorized.

Additional work beyond the prompt

None.

### Baseline sources

Use released **Mido 1.3.3** as the compatibility reference. Freeze its source and
record the reference version with conformance fixtures; do not silently follow
changing development documentation. Relevant primary sources are:

- [Top-level API](https://github.com/mido/mido/blob/1.3.3/mido/__init__.py).
- [Messages](https://github.com/mido/mido/blob/1.3.3/mido/messages/messages.py),
  [message specifications](https://github.com/mido/mido/blob/1.3.3/mido/messages/specs.py),
  [text representation](https://github.com/mido/mido/blob/1.3.3/mido/messages/strings.py),
  and [frozen messages](https://github.com/mido/mido/blob/1.3.3/mido/frozen.py).
- [Parser](https://github.com/mido/mido/blob/1.3.3/mido/parser.py) and
  [tokenizer](https://github.com/mido/mido/blob/1.3.3/mido/tokenizer.py).
- [MIDI files](https://github.com/mido/mido/blob/1.3.3/mido/midifiles/midifiles.py),
  [tracks](https://github.com/mido/mido/blob/1.3.3/mido/midifiles/tracks.py),
  [meta messages](https://github.com/mido/mido/blob/1.3.3/mido/midifiles/meta.py),
  [units](https://github.com/mido/mido/blob/1.3.3/mido/midifiles/units.py), and
  [SYX files](https://github.com/mido/mido/blob/1.3.3/mido/syx.py).
- [Ports and aggregation](https://github.com/mido/mido/blob/1.3.3/mido/ports.py),
  [backend selection](https://github.com/mido/mido/blob/1.3.3/mido/backends/backend.py),
  and [socket ports](https://github.com/mido/mido/blob/1.3.3/mido/sockets.py).

The existing transport pins midir 0.11.0 and PyO3 0.29.3. Preserve those until a
specific implementation requirement justifies a reviewed dependency change.
Reuse the recorded [validation](validation.md), [remaining issues](issues.md),
and [upstream suggestions](midir-suggestions.md); moving behavior into Rust
must not erase those limitations.

## 2. Three layers, with one implementation of MIDI semantics

The three components are libraries/crates, not three executable binaries:

| Layer | Responsibility | What it must not require |
| --- | --- | --- |
| Third-party `midir` | Native MIDI discovery and transport through CoreMIDI, ALSA, and WinMM | Our message/file model or Python |
| Our Rust `midirp` toolkit | Messages, parsing, encoding, files, timing, port abstractions, and transport integration | PyO3, a Python interpreter, or Mido |
| Our Python binding | Python objects, arguments, exceptions, iteration, file-like objects, and existing process isolation over the Rust toolkit | Mido as a runtime implementation |

Dependencies point downward. The toolkit depends on midir for device I/O. The
binding depends on the toolkit and PyO3. The toolkit never imports or calls the
binding. Pure message, parser, and file tests should need no OS MIDI client.

MIDI definitions, codecs, parser state machines, file interpretation, conversion
math, merge ordering, and playback timing are implemented once in Rust. Python
adapts representations and language protocols; it does not implement a second
musical engine. Differential tests may use Mido as a development oracle, not a
production dependency.

The present Cargo package named `midirp` builds the PyO3 library named `midi`.
The proposed workspace separates that package into a Rust toolkit package named
`midirp` and a binding package, provisionally `midirp-python`, whose extension
still has the Python identity `midirp.midi`. Package naming and shared version
ownership must be settled before moving source files.

A proposed layout, subject to that review:

```text
Cargo.toml                 # Workspace and authoritative shared version
Cargo.lock
crates/
    midirp/
        Cargo.toml         # Rust toolkit, no PyO3
        src/
            lib.rs
            messages.rs
            parser.rs
            tokenizer.rs
            midifiles/
            ports/
            sockets.rs
    midirp-python/
        Cargo.toml         # PyO3 binding; extension library named midi
        src/
            lib.rs
            messages.rs
            parser.rs
            midifiles.rs
            ports.rs
midirp/                    # Existing Python package, remains at repository root
    __init__.py            # Remains empty under current repository instructions
    midi.pyi
    py.typed
    _clients.py
    _transport.py
    _worker.py
test/
    compatibility/
    native/
    manual/
    release/
plan/
    midirp.md
```

This is a responsibility sketch, not permission to create unused modules. Add
modules as their slices require them. Retain one active implementation of each
feature when extracting current Rust ownership/lifecycle code.

## 3. What compatibility means

### Names and modules

Python class, function, method, property, parameter, and message-type names
match Mido's reference API. Preserve defaults, positional/keyword behavior,
return shapes, field validation, mutation, and observable ordering where these
are part of the public contract. Examples may substitute the package namespace;
users are not expected to import the third-party `mido` package.

The module families to account for are messages, parser, tokenizer, frozen,
midifiles, ports, sockets, and backends, plus the root public exports. Public
extension points are included; underscore-prefixed implementation helpers and
backend-private native handles are not a compatibility promise.

**Python export decision still required:** Mido exposes `mido.Message` and other
root symbols. Current instructions keep `midirp/__init__.py` empty and prohibit
re-exporting imported symbols. Under those instructions, names can be exposed
as `midirp.midi.Message` and matching submodules, but `import midirp` alone will
not reproduce Mido's root namespace. Do not silently waive that rule. The owner
must choose whether exact root-module compatibility warrants an exception or
whether an explicit import of `midirp.midi` is the sole namespace difference.
Do not implement both alternatives without approval.

### Rust spelling and language mechanics

Expose the same public nouns and method/function names wherever Rust permits
it. A Rust `Message` corresponds to a Python `Message`; `copy`, `bytes`,
`from_bytes`, `feed_byte`, `merge_tracks`, and `open_output` retain their names.
Do not substitute `MidiMessage`, `decode`, or `write_message` without review.

Rust has no Python-style class calls, keyword arguments, default arguments,
properties, or dynamic module import. Mechanical adaptations must be listed,
not passed off as identical signatures:

- Constructor call syntax maps to associated `new` functions on the same types.
- Named/default arguments map to one options representation per operation;
  its fields retain Mido's parameter names. Avoid multiple redundant builders.
- Rust uses `r#type` where `type` is a reserved identifier; Python keeps `type`.
- Fallible functions return `Result`; optional results use `Option`. Python
  converts them to the corresponding exceptions and `None`.
- Iterators and ordinary Rust collection/ownership operations replace Python
  special-method syntax. Keep the underlying operation and ordering the same.
- Checked access to variant-specific message fields must preserve field names.
  The exact Rust representation/access syntax is a review gate, not grounds to
  change Python's mutable named-attribute API.
- Python's `bytes()` result is a list of integers and `bin()` is a bytearray.
  Rust can use owned byte vectors for both while retaining both method names.
  Document that Rust does not have those two Python container distinctions.

Name compatibility is achievable; literal cross-language signature and object
identity compatibility are not. Proposed unavoidable differences require an
explicit mapping table before implementing the affected surface.

### Behavioral reference and intentional departures

Distinguish documented Mido semantics, reference implementation behavior,
backend-specific behavior, and apparent bugs. Tests establish the baseline
before departures are considered. Identical names must not hide incompatible
meaning, units, loss policy, or lifetime behavior.

Do not reproduce a memory-safety defect or a deadlock to imitate a backend.
Record such a departure, its rationale, and a regression test. Do not silently
upgrade other surprising behavior to what a MIDI specification suggests.

## 4. Main API: messages and serialization

### Message surface

| Reference entry | Required surface |
| --- | --- |
| `Message(type, skip_checks=False, **args)` | Same message strings, fields, defaults, and checking switch |
| `copy(skip_checks=False, **overrides)` | Independent copy; same-type override rules |
| `bytes()`, `bin()`, `hex(sep=' ')` | Encoding and exact Python result/container forms |
| `dict()`, `from_dict(data)` | Field dictionary conversion, including SysEx data as a list |
| `from_bytes(data, time=0)` | Exactly one complete message; bytes and integer iterables |
| `from_hex(text, time=0, sep=None)`, `from_str(text)` | Corresponding textual constructors |
| `is_cc(control=None)`, `is_meta`, `is_realtime` | Same predicates, including reference edge cases |
| Attribute access/mutation, equality, `len`, `str`, `repr` | Public Python behavior and time-aware comparison |

Include all reference message types and exact field names:

| Types | Fields besides `type` and `time` |
| --- | --- |
| `note_on`, `note_off` | `channel`, `note`, `velocity` |
| `polytouch` | `channel`, `note`, `value` |
| `control_change` | `channel`, `control`, `value` |
| `program_change` | `channel`, `program` |
| `aftertouch` | `channel`, `value` |
| `pitchwheel` | `channel`, `pitch` |
| `sysex` | `data` |
| `quarter_frame` | `frame_type`, `frame_value` |
| `songpos` | `pos` |
| `song_select` | `song` |
| `tune_request`, `clock`, `start`, `continue`, `stop`, `active_sensing`, `reset` | No payload fields |

Preserve velocity's default of 64, other scalar defaults of zero, zero-based
channels, pitchwheel's signed range, and song-position range. Preserve constants
`MIN_PITCHWHEEL`, `MAX_PITCHWHEEL`, `MIN_SONGPOS`, and `MAX_SONGPOS`.

SysEx `data` contains payload only; encoding adds `F0` and `F7`. Python's public
data sequence and `+=` behavior must match the reference. Do not replace its
immutable tuple-like representation with a list merely for convenience.
Ordinary messages remain mutable and unhashable. Invalid field access, unknown
message names, wrong types, and out-of-range values need exception fixtures.

`skip_checks` skips the promised domain validation, not memory safety. A Rust
representation that cannot hold an unchecked value may conflict with that
public API; resolve this explicitly rather than silently dropping the parameter
or allowing unchecked indexing/conversion to panic.

### Text and frozen messages

Implement `parse_string(text)`, `parse_string_stream(stream)`, and
`format_as_string(msg, include_time=True)`. Preserve error/result forms for
stream parsing, whitespace, comments, numbers, and serialized field order.

Under the matching `frozen` module, implement `Frozen`, `FrozenMessage`,
`FrozenMetaMessage`, `FrozenUnknownMetaMessage`, `freeze_message(msg)`,
`thaw_message(msg)`, and `is_frozen(msg)`. Preserve immutability, hashing,
copy/thaw behavior, and handling of `None`; do not add renamed alternatives.

## 5. Main API: incremental parsing and tokenizing

Provide `Parser(data=None)` with `feed(data)`, `feed_byte(byte)`,
`get_message()`, `pending()`, iteration, and length. Preserve `Parser.messages`
as the publicly exposed deque in Python, including the effects of direct deque
mutation. Do not substitute a private queue or a copied list without approval.
Provide `parse(data)` and `parse_all(data)` with the same first-result/all-results
behavior. `parse` may return `None`; it is not the same strict operation as
`Message.from_bytes`.

Provide `Tokenizer(data=None)` with `feed`, `feed_byte`, length, and consuming
iteration over token byte sequences. State is incremental across feeds. Test
every split point of short messages and SysEx, empty feeds, undefined statuses,
stray data, interrupted messages, and realtime interleaving.

**Important reference constraint:** Mido 1.3.3's live tokenizer resets its status
after a complete short message; it does not retain normal channel running
status. Its realtime handling outside SysEx can discard an incomplete short
message. Preserve the measured baseline for the main parser rather than quietly
claiming a fully general MIDI stream decoder. Standard MIDI File running status
is handled by the file reader separately. More complete wire parsing is an
extension candidate, not an unannounced replacement.

Keep parser state/decoding in Rust. Python-visible queues and conversion of
arbitrary iterables are binding responsibilities. Python feeding must preserve
the observable treatment of an iterable that fails partway through.

## 6. Main API: meta messages, files, tracks, and time

### Meta messages

Provide `MetaMessage(type, skip_checks=False, **kwargs)` and
`UnknownMetaMessage(type_byte, data=None, time=0, type='unknown_meta', **kwargs)`.
Cover their own copy/encoding/from-bytes behavior and shared message properties;
do not assume their signatures equal `Message`'s. Preserve unknown meta payloads
so reading and writing does not lose unrecognized events.

Include the built-in meta names: `sequence_number`, `text`, `copyright`,
`track_name`, `instrument_name`, `lyrics`, `marker`, `cue_marker`, `device_name`,
`channel_prefix`, `midi_port`, `end_of_track`, `set_tempo`, `smpte_offset`,
`time_signature`, `key_signature`, and `sequencer_specific`. Inventory every
field, default, byte representation, and validation rule from the reference
specifications before implementation. Include `KeySignatureError`.

Account for the public custom-meta extension surface: `MetaSpec`,
`add_meta_spec(klass)`, and the codec context used by `meta_charset`. Python
subclass registration cannot be copied literally into Rust; define one Rust
codec registration mechanism and adapt the Python hooks to it. Built-in codecs
stay in Rust. User-provided Python codecs are user code, not a second built-in
implementation. Registry mutation/concurrency and callback ownership need tests.

### File and track surface

Preserve the reference constructor:

```text
MidiFile(filename=None, file=None, type=1, ticks_per_beat=480,
         charset='latin1', debug=False, clip=False, tracks=None)
```

Provide `save(filename=None, file=None)`, `add_track(name=None)`,
`print_tracks(meta_only=False)`, iteration, context management, `length`, and
`merged_track`. Match mutable `filename`, `type`, `ticks_per_beat`, `charset`,
`debug`, `clip`, and `tracks`, plus deletion of the merged-track cache.
Caller-supplied file-like objects must not be closed as if the toolkit owned
them. Preserve text charset and clipping/debug options; identify any required
codec dependency rather than silently supporting only UTF-8.

Implement `MidiTrack`'s list behavior in Python, including `.name`, `.copy()`,
slicing, concatenation, repetition, insertion/removal, and mutation. Rust's
track container uses ordinary Rust collections but carries the same messages,
ordering, naming behavior, and copy semantics.

Implement Standard MIDI File types 0, 1, and 2: headers/chunks, variable-length
integers, delta ticks, file running status, SysEx events, metadata, and
end-of-track handling. Type-2 limitations on merging/playback length must match
the reference. Test truncated files and malformed lengths as well as valid
round trips; never allocate from an untrusted length without checked bounds.

Provide `merge_tracks(tracks, skip_checks=False)` and the public
`fix_end_of_track(messages, skip_checks=False)` track helper. Preserve stable
ordering of equal-time events, delta/absolute conversion, and synthesized end
markers. Do not silently fix Mido's manually invalidated merged-track cache:
first capture its behavior and propose automatic invalidation separately.

### Time and playback

Provide `tick2second(tick, ticks_per_beat, tempo)`,
`second2tick(second, ticks_per_beat, tempo)`,
`bpm2tempo(bpm, time_signature=(4, 4))`, and
`tempo2bpm(tempo, time_signature=(4, 4))`. Capture rounding and validation at
boundaries rather than choosing new numeric rules. Rust must distinguish
integer tick values from floating-point results without changing Python's
public accepted/result types.

`MidiFile` iteration yields playback-ordered messages whose `time` is elapsed
seconds; messages stored in tracks use delta ticks. Implement
`play(meta_messages=False, now=time.time)` with reference yield/filter/timing
behavior and an injectable Rust clock counterpart. A clock taking a different
form in Rust is a language adaptation, not a renamed playback API. Do not add
scheduled device send semantics to ordinary `send`.

### SYX files

Provide `read_syx_file(filename)` and
`write_syx_file(filename, messages, plaintext=False)` with binary and plaintext
forms. Preserve the reference handling of non-SysEx input and return objects.
Use the same Rust message/parser/encoding engine, not a separate SYX codec.

### Representation boundary

Python list/deque compatibility is a real constraint, not an incidental detail.
A Python `MidiTrack` can own a list of Rust-backed message wrappers while Rust
file operations take a coherent snapshot at the binding boundary. Do not keep
an independently mutable Python list and a permanently mirrored Rust vector.
Likewise, direct `Parser.messages` mutation must affect subsequent consumption.
Prototype these boundaries before committing to a container architecture.

## 7. Main API: ports, callbacks, and backend selection

### Opening and discovery

Keep these names and signatures:

```text
open_input(name=None, virtual=False, callback=None, **kwargs)
open_output(name=None, virtual=False, autoreset=False, **kwargs)
open_ioport(name=None, virtual=False, callback=None, autoreset=False, **kwargs)
get_input_names(**kwargs)
get_output_names(**kwargs)
get_ioport_names(**kwargs)
```

Return Mido-shaped port objects rather than exposing client ownership as the
ordinary user's musical API. Each port internally owns the necessary midir
client and connection. Discovery remains available independently of open ports;
multiple ports use separate native clients as required by midir's ownership.

Respect the corresponding default-port environment variables and backend
selection conventions. Document the built-in midir backend's default selection
and filters: Mido's backend-dependent defaults cannot be treated as one universal
hardware contract. Duplicate names and separate input/output naming need explicit
reference fixtures and a review decision; do not invent silently decorated
names or silently use the existing ID-only API in the main surface.

### Port methods and classes

Common port attributes/methods: `name`, `closed`, `is_input`, `is_output`,
`close()`, context management, and representation. Output: `send(message)`,
`reset()`, `panic()`, and `autoreset`. Input: `receive(block=True)`, `poll()`,
`iter_pending()`, and consuming iteration. Support callback selection and
replacement through `.callback` where supported.

Retain the module-level class names `BasePort`, `BaseInput`, `BaseOutput`,
`BaseIOPort`, `IOPort(input, output)`, `EchoPort`, and
`MultiPort(ports, yield_ports=False)`. Python subclass hooks `_open`, `_close`,
`_send`, and `_receive` are public extension contracts even though their names
start with underscores. The Rust counterpart needs a corresponding trait-based
port interface, with an explicit mapping rather than arbitrary Python imports.

Provide `multi_receive(ports, yield_ports=False, block=True)`,
`multi_iter_pending(ports, yield_ports=False)`, `multi_send(ports, msg)`,
`reset_messages()`, `panic_messages()`, `set_sleep_time`, `get_sleep_time`, and
`sleep` with reference defaults/order/fairness behavior. Mido's reset helpers
send musical controller messages; they are separate from native driver reset.
No close operation should acquire musical side effects except the documented
`autoreset` path. `send` must snapshot mutable messages as the reference promises.

### Receiving and callback lifecycle

Polling returns `None` when empty; blocking receive waits for a message. Capture
the main API's behavior on close, including pending messages, initially closed
ports, and a port closed while receiving. The reference documentation and
specific RtMidi overrides differ here; choose and document the public baseline
instead of inadvertently reproducing an override's indefinite queue wait.

Callbacks receive one `Message`, not the existing raw `(timestamp, bytes)` pair.
They remain mutually exclusive with ordinary receive/iteration as specified.
Callbacks may forward to an output port. The current midirp callback-thread
blocking guard therefore cannot be carried into this public path unchanged.
The low-level native callback should admit bytes into bounded transport storage;
user callback delivery belongs on a controlled application dispatch path.

Design and test callback replacement, errors, send-from-callback, self-close,
closing a different port, slow callbacks, and close during delivery. Never join
or drain the currently executing dispatcher from itself. Preserve the existing
safe lifetime handling while avoiding an application-lock cycle introduced by
new layers. Define user-callback exception handling explicitly; native error
and user-code exception handling are different paths.

### Backend interface

Provide `Backend(name=None, api=None, load=False, use_environ=True)`,
`set_backend(name=None, load=False)`, `backend`, and the backend wrapper's
opening/discovery methods, `load`, `loaded`, and Python `module` property.
Use the built-in midir implementation as the default; the default module string
necessarily differs from Mido's RtMidi implementation. Preserve extensibility
and lazy native initialization.

API compatibility does not imply implementing PortMidi, Pygame, or RtMidi in
addition to midir. Do not add those parallel transports. Python custom backend
modules and Rust registered port factories are language-specific implementations
of the same extension contract. Keep environment keys `MIDO_BACKEND`,
`MIDO_DEFAULT_INPUT`, `MIDO_DEFAULT_OUTPUT`, and `MIDO_DEFAULT_IOPORT` in the
baseline; renaming them is a proposed extension/departure, not assumed work.

Translate ordinary operation errors into compatible Python categories, usually
`OSError` for transport failures and appropriate argument exceptions. Keep
richer binding failure detail available separately. Document the native failure
behavior, exception grouping, deadlines, and ownership differences that the
existing safety layer introduces; do not claim byte-for-byte exception-text
compatibility or clone unsafe backend cleanup.

## 8. Main API: socket ports and version information

Match `sockets.SocketPort(host, portno, conn=None)`,
`PortServer(host, portno, backlog=1)`, `connect(host, portno)`,
`parse_address(address)`, `format_address(host, portno)`, and
`PortServer.accept(block=True)`. Socket ports implement the common port API.
The protocol is MIDI bytes over TCP, not RTP-MIDI. No discovery, authentication,
encryption, or automatic reconnect is implied. Use synchronous socket I/O;
async/await is not required for this project.

Socket implementation is part of the requested feature plan but network I/O
and server execution need explicit approval under repository instructions when
implementation reaches this slice. Deterministic byte-stream tests can run
before any network server is launched. No service starts on import.

Provide `version_info` and module version reporting. Mido's `version_info` is a
`packaging.version.Version`; matching that exact Python object requires a
runtime dependency that midirp does not currently have. Decide whether the
exact object type is required or whether equivalent documented version behavior
is an acceptable exception. Do not introduce that dependency silently.

Mido's command-line utilities are not the library API. Their names/features
can be inventoried for a later explicit CLI request; this plan does not add
production executables simply because Mido distributes scripts.

## 9. Cross-platform implementation and Pi Zero suitability

### Supported environments

Retain Windows/WinMM, macOS/CoreMIDI, and Linux/ALSA. The Pi Zero is a Linux
deployment target, not a reason to make the toolkit Linux-only. Standard Rust
and OS threads are allowed; `no_std`, embedded firmware, and microcontroller
transports are outside this design.

Plan conservatively for the original Pi Zero until the owner identifies the
exact board, OS image, bitness, and Python installation. Do not substitute a
Pi Zero 2 result for original Pi Zero evidence. Record CPU architecture, target
triple, floating-point ABI, libc, Rust toolchain, ALSA development/runtime
availability, and CPython ABI before choosing cross-build commands. Distinguish
cross-compilation success, running on hardware, and acceptable performance.

An ARM Linux wheel is a separate release artifact from desktop x86_64/arm64
wheels. A supported Python version does not establish wheel availability on the
Pi image. A pure Rust application links the toolkit directly and has no Python
startup/process dependency.

### Resource design

Short messages should use fixed-size payload storage internally where it reduces
allocation; SysEx/file data uses checked owned storage. Avoid unnecessary
per-message heap objects in the Rust path. Keep Python representations compatible
at the language boundary, where conversion and ownership require allocations.

Do not create a thread, process, or Rust timer per message. Thread/process costs
should scale with configured ports or dispatchers and be measured. Pure parsing
and file operations must not start native MIDI cleanup or communication workers.
Extraction should remove Python dependence from Rust-only lifecycle handling,
not leave a dormant embedded interpreter or Python worker fallback.

Python retains configurable isolated operation and an explicit in-process mode.
Do not reverse its approved conservative default based solely on the Pi target.
Rust device I/O initially runs natively; optional Rust process isolation would
need a helper executable and is separately proposed below.

Mido-compatible file loading is eager and can consume memory proportional to
input size. Do not silently turn `MidiFile.tracks` into a lazy stream. Alternative
streaming and bounded parsers belong in extensions. Transport queues and native
unfinished SysEx need separate accounting; a queue limit after midir assembly
cannot cap memory already allocated inside midir.

Measure idle RSS, peak memory, executable/wheel size, import/startup time, idle
CPU, thread/process/handle count, sustained callback throughput, dropped messages,
and latency distributions for direct Rust, in-process Python, and isolated
Python. Record the workload and message sizes. Define release budgets from
measured representative Pi workloads with the owner; do not invent a
hard-real-time guarantee or claim Rust automatically makes the Python path fast.

## 10. Possible additions beyond Mido: separate, optional surface

None of these change the main compatibility API without review. They are
candidates, not approved implementation work or reasons to delay baseline
conformance. Names listed here are existing names where possible; new names
remain provisional until selected.

| Addition | Purpose and review considerations |
| --- | --- |
| Native timestamped receive | Preserve backend microsecond time and clock identity separately from `Message.time`, whose file meanings stay unchanged |
| Monotonic activity and worker failure | Retain `last_message_time`, `worker_failure`, and structured failure categories without presenting them as device health |
| Raw byte transport | Retain access to bytes/bytearray/memoryview without forcing a message-object round trip; settle the existing `midirp.midi` raw API's placement before replacing it |
| Bounded queue/parser/file policies | Configurable byte/count/assembly limits and observable loss; proposed exceptions to Mido's unbounded behavior need explicit policy/default decisions |
| Complete running-status stream parsing | A clearly selected mode with proper realtime interleaving, not a silent semantic change to baseline `Parser` |
| Streaming MIDI file reading/writing | Process large files without materializing every track; keep baseline `MidiFile` eager and list-compatible |
| Safe track-cache invalidation | Invalidate derived merges after edits, with defined mutation tracking and a measured cost |
| Cancellation and receive deadlines | Interrupt playback/waits and return a defined outcome without pretending to cancel an arbitrary driver or user callback |
| Scheduled output and clock domains | Absolute/relative scheduling with clock identity; keep ordinary `send` immediate |
| Batch operations | Amortize language-boundary/copy overhead, with ordering and partial-failure semantics |
| Stable discovery descriptors | Names plus opaque IDs, directions, and backend information; distinguish uniqueness from persistent physical identity |
| Device-specific health/recovery | Protocol-aware request/reply deadlines; no generic claim that silence means disconnect or that a send is acknowledged |
| Rust process isolation | Optional supervised native helper for hangs/crashes; an additional executable is a separate architecture decision |
| Higher-level music utilities | Note naming, scales/chords, clock generation, or transport helpers only if separately requested; not part of the Mido baseline |
| MIDI 2.0/UMP or additional transports | Separate future protocol/transport work; raw MIDI 1.0 compatibility does not implement these |

Native safety remains mandatory even in the compatible path: checked arithmetic,
valid buffer lifetimes, coherent close, and no deliberate indefinite error spins.
Changes to successful results, default buffering/loss, or exposed semantics must
still be identified as compatibility decisions. The upstream B3–B6 handoff is
not a solved dependency; implementation should retain containment and evidence
boundaries from the existing review.

## 11. Verification strategy

### API and behavior manifest

Before the first feature slice, inventory each public entry, module placement,
constructor/defaults, accepted arguments, return form, exceptions, mutation,
ordering, and serialization. This plan supplies the surface checklist; the
manifest closes signature omissions and establishes explicit conformance status.
Use implemented, deferred, intentional departure, and unvalidated as distinct
statuses. A stub is not implemented support.

Use a pinned development-only Mido installation to produce reference fixtures.
Prefer fixture generation over maintaining a second oracle implementation in
our code. If source is reused under Mido's MIT license, preserve attribution and
license notices; do not assume name compatibility requires copying its code.
No Mido dependency is installed by the production Rust or Python library.

### Shared conformance cases

Rust tests exercise the core; Python tests exercise the same cases through the
binding. Include exact encoding, text, dict forms, mutation, equality, defaults,
invalid values, unknown metadata, all feed split points, file round trips,
merge tie ordering, tempo changes, and time conversion/rounding. Distinguish
semantic round trips from byte-identical rewriting; running-status and chunk
encoding can vary when the reference itself normalizes them.

Use fake port transports and a controlled clock to test ownership, queueing,
close/error wakeup, callback swapping, self-close, aggregation, playback timing,
and partial send failures without devices or sleeping timing assertions.
Callbacks must not acquire application locks under internal state locks.

Python-specific tests cover iterables, list/deque behavior, file-like ownership,
GC cycles, mutation while copying/sending, exception translation, and real
interpreter shutdown. Do not silently weaken the current lifecycle coverage
when extracting the toolkit.

### Native and platform gates

Run portable core tests on all desktop OSes. Keep Python release CI limited to
the oldest/newest supported versions, currently 3.11 and 3.15, and published
releases only. Verify wheel imports/installations outside the checkout.
Add Pi build/run evidence separately with a real target image and ALSA access.

Native virtual-port checks need the existing applicable authorization; new
hardware, driver fault, unplug/replug, and network runtime tests need explicit
approval. Use supervised subprocess deadlines for native hang tests. Record
actual ports/devices and failures. Rust-only and Python-path checks must not be
mistaken for proof that a shared driver or OS MIDI service is healthy.

The last recorded Python 3.15 native check failed at CoreMIDI initialization,
also reproduced on 3.11; that remains a validation gap. The Rust 1.99 strict
Clippy deprecation finding also remains separate from this plan. Do not change
toolchains or claim those gates passed as a side effect of documentation work.

## 12. Proposed implementation slices and completion gates

Each slice is a reviewable feature group and can take multiple commits. This
ordering is provisional until the decisions below are answered. Run meaningful
checks before each implementation commit; remove resolved issue text in the
last commit of its fix series and record the remaining limitations.

1. **Compatibility manifest and design decisions.** Freeze the reference;
   settle Python exports, Rust constructor/field access, ownership containers,
   exact Pi target, and the old raw surface. Gate: every public family accounted
   for, with deviations visible and no renamed substitute API.
2. **Extract the Rust toolkit and thin binding.** Create the workspace, move
   shared transport/lifecycle responsibilities, keep existing raw behavior
   working, and verify Rust-only consumers need no Python. Gate: existing tests
   and wheels pass; platform ownership/error coverage is retained.
3. **Messages, text, and frozen values.** Implement all channel/system messages,
   validation, copying/mutation, encoders, text/dict conversions, and freezing.
   Gate: shared Rust/Python fixtures and public signatures pass.
4. **Tokenizer and Parser.** Implement incremental reference behavior and
   Python deque/iterator contracts. Gate: all feed partitions and malformed
   cases agree with the pinned baseline; no hidden parser improvements.
5. **Meta messages and timing units.** Implement built-in/unknown codecs,
   charset handling, extension registration, and conversion math. Gate: every
   declared type/default/range has evidence in both language interfaces.
6. **Tracks, MIDI files, and SYX.** Implement list boundaries, SMF/SYX read/write,
   merging, caches, type distinctions, and file-like objects. Gate: fixtures,
   malformed-input checks, resource measurements, and caller-file ownership pass.
7. **Live port API and backend contract.** Add Mido-named opening, discovery,
   sends, receive/poll/iteration, callbacks, close, and reset helpers over midir.
   Gate: deterministic send-from-callback, self-close, failure wakeup, duplicate
   names, and lifecycle behavior pass before native execution.
8. **I/O wrappers and aggregation.** Implement `IOPort`, `EchoPort`, `MultiPort`,
   and module helpers. Gate: ordering, fairness, yield-port identities, and
   ownership agree with baseline; Rust and Python share the implementation.
9. **Playback.** Implement iteration/play timing and clock injection using the
   shared core. Gate: fake-clock tests and tempo/meta filtering agree; measured
   Pi playback does not claim hard-real-time behavior.
10. **Socket API.** After explicit network-work approval, implement compatible
    synchronous socket ports and server helpers. Gate: stream tests first,
    then authorized loopback tests; no import-time listeners or auto-reconnect.
11. **Cross-platform/Pi release qualification.** Validate desktop artifacts and
    the selected Pi target, measure resource/performance budgets, finish the
    signature/conformance report, and publish only with explicit authorization.
    Gate: all advertised main features implemented or their release explicitly
    labeled partial; native validation gaps and upstream risks are documented.

Only after main-surface review select any extension slice from section 10. Do
not expand a message/file slice into scheduling, reconnection, batch transport,
or a new CLI without the owner's approval.

## 13. Decisions for the owner's review before implementation

1. **Python namespace:** approve Mido-style root exports as an exception to the
   empty-`__init__.py`/no-re-export rules, or retain `midirp.midi` as the documented
   namespace difference. Names inside the selected namespace still match Mido.
2. **Rust construction and fields:** select the one options/typed-value mapping
   for Python keyword arguments and variant fields, with unchanged public names.
3. **Exact compatibility scope:** confirm 1.3.3 as the reference and decide which
   observed quirks to preserve or formally depart from. In particular: tokenizer
   running status/realtime handling, predicate classifications, manual merge-cache
   invalidation, and errors during callback/receive/close.
4. **Existing raw API:** retain it in a clearly separate low-level surface or
   approve replacement at a breaking release. Do not add duplicate aliases or
   silently reinterpret existing `send(bytes)` as `send(Message)`.
5. **Pi target:** identify original Pi Zero versus Zero 2, OS/ABI, supported
   Python installation, selected MIDI hardware, and acceptable workloads.
6. **Safety policy differences:** decide bounded/unbounded public parser and
   receive defaults, loss/error notification, and how Python isolation options
   fit the Mido-compatible keyword extension mechanism. Keep known native
   safety requirements; make resource-policy differences explicit.
7. **Backend/codec extensibility and version object:** approve the minimal
   language mappings and any indispensable dependency needed for exact Python
   object compatibility. Do not recreate other native backends merely to match
   Mido's internal module strings.
8. **Workspace/package identity:** settle crate names and shared release version
   without publishing the toolkit or changing dependencies during planning.

These are review items, not a request to approve all implementation immediately.
The main API remains name-compatible unless the owner accepts a specifically
identified exception after reviewing this plan.
