# uFor tuning previews and MTS message construction

`meedee.ufor_tuning` is an optional Python integration. uFor resolves pitch
mathematics and Scala-derived tunings; meedee's existing Rust-backed MTS types
perform quantization and encoding. The bridge adds no tuning engine, Rust uFor
interpreter, transport, clock or audio host. Optional offline CLI preview/export
composes these existing models and codecs as described below.

## Installation and compatibility

The bridge requires Python 3.13+ and the `ufor` extra. In this checkout:

```bash
uv sync --extra ufor --python 3.13
```

The uFor and reccy source declarations track HTTPS/main; frozen uv commands use
the recorded lockfile revisions. The ordinary meedee installation still supports
Python 3.11+ without runtime dependencies. The extra's dependency marker applies
only on Python 3.13+; on 3.11/3.12 importing the bridge raises a clear ImportError.
Installing the extra on an older interpreter does not make uFor available.

No optional module is imported or re-exported by `meedee.__init__`. Existing
message, MTS and transport APIs retain their current dependencies and behavior.

## Resolve and inspect

```python
from fractions import Fraction
from ufor import scala
from ufor.tuning import Tuning
from meedee import ufor_tuning

scale = scala.parse_scala("Three steps\n3\n9/8\n3/2\n2/1\n", name="three")
source = Tuning(source=scale, root_note=69, root_frequency="440")
report = ufor_tuning.preview(source, notes=[69, 70, 71, 72])
assert report.entries[1].frequency == Fraction(495)
assert report.entries[1].pitch is not None
```

Selection requires a list of 1–128 distinct built-in integers in 0–127. Boolean,
floating-point and string keys are rejected. The preview retains caller order,
copies the selection and snapshots the source definition before resolving it.
The snapshot is a deep copy of the uFor model, not a new normalized tuning format.

`Preview` and `KeyPreview` are frozen uFor/Pydantic models. Their collection
fields are lists, which Python callers must not edit while inspecting or building
messages. Message builders snapshot those lists and recheck key uniqueness and
bounds, so duplicate keys cannot silently overwrite a table slot. Reports retain
native immutable `tuning.Pitch` objects; JSON/archive serialization is not part
of this bridge's contract.

Each key has a `PitchStatus`:

| Status | Meaning |
| --- | --- |
| `CONVERTED` | uFor resolved a frequency within the MTS range; the preview includes its encoded pitch, encoded Hz and signed error in cents |
| `UNMAPPED` | The key lies outside the explicitly finite source coverage |
| `OUT_OF_RANGE` | A resolved finite positive frequency lies outside meedee's MTS range |
| `EVALUATION_ERROR` | uFor evaluation raised an arithmetic/value error or produced an invalid non-positive/non-finite result |

`frequency` retains the original resolved `int`, `Fraction` or `float` where a
valid finite result exists, including out-of-range results. Evaluation failures
have a reason and may have no frequency. No fallback equal temperament, clamping
or silent omission is applied.

Coverage follows each existing uFor source: a `FrequencyTable` uses its
`first_note`; finite `RatioTable` and non-repeating `IntervalPattern` values are
relative to `root_note`. Interval patterns include degree zero and their last
cumulative interval. Computed and repeating sources cover every selected key.
Covered keys are resolved by calling uFor's `Tuning`, preserving its reference
frequency and detuning behavior. The bridge does not copy Scala parsing or
ratio/equal-temperament calculations. Unexpected programming exceptions propagate
rather than being labeled as an unmapped key.

MTS stores pitches in 1/16384-semitone units. Original exact frequencies are
compared with the existing codec's finite Hz bounds before conversion to double,
preventing an out-of-range rational from rounding onto a valid endpoint. In-range
conversion calls `Pitch.from_frequency`, using its existing nearest/ties-to-even
rounding. `error_cents` is positive when the encoded pitch is sharper and negative
when flatter, relative to the double-precision frequency passed to that factory.
It does not claim to measure every earlier floating-point error in an irrational
uFor expression or in conversion from an exact frequency. The original resolved
value remains available for comparison. The reserved no-change pitch triplet is
never interpreted as a frequency.

Output is bounded to 128 records. Resolving still incurs the existing uFor
source's evaluation cost; the source snapshot and expression/table storage are
not bounded by the MIDI key count. No hard CPU/time or memory guarantee is made
for arbitrary user-authored tuning definitions.

## Construct messages explicitly

```python
from meedee import codecs

outgoing = ufor_tuning.note_changes(
    report,
    device_id=1,
    program=0,
    bank=0,
    realtime=False,
)
encoded = [codecs.encode(m) for m in outgoing]
```

This constructs bytes only. There is no implicit port selection or sending.
Device ID and program are required; bank is explicitly supplied when choosing a
banked format. MTS device/bank/program identifiers retain their protocol values
0–127, independently of General MIDI's program numbering. Device ID 127 remains
an intentional broadcast choice, never a default.

`note_changes(report, *, device_id, program, realtime, bank=None,
preserve_unmapped=False)` returns ordinary `SingleNoteChange` messages when bank
is absent, or `BankedNoteChange` messages when present. Real-time intent is always
required. The ordinary unbanked format is intrinsically real-time, so
`realtime=False` requires a bank. Selected key order is preserved, with at most
127 changes per message. A full 128-key selection therefore produces two messages
of 127 and one change. These are not an atomic device transaction or a transfer
queue; a future host must handle partial delivery and uncertainty explicitly.

`bulk_dump(report, *, device_id, program, name, bank=None,
preserve_unmapped=False)` creates `BulkDump` or `KeyBasedDump`. Bulk formats are
non-real-time and require all 128 table slots. The default requires a complete
128-key converted preview. Otherwise the caller must opt into preservation,
which fills unselected or explicitly unmapped keys with `NoChange`. A successful
subset does not silently manufacture pitches for the rest of the keyboard.
Names retain the native requirement of at most 16 ASCII characters, with native
padding and checksum generation.

For either builder, a selected `UNMAPPED` key requires
`preserve_unmapped=True`. NoChange means leave the receiver's existing value
unchanged; it does not establish what that value is. `OUT_OF_RANGE` and
`EVALUATION_ERROR` always block generation, even with preservation enabled.
Resolve the source or deliberately make a new selection before building output.
Native message constructors retain their type/range checks. No error yields a
partial returned message list.

Octave-offset generation, capability discovery, interactive views and enge audition
remain deferred here. Existing transfer and MTS-query tools handle their separate
explicit workflows; this bridge does not add another device host. This module
neither changes a synth's tuning nor selects a tuning program in an instrument.

## Verification

Tests cover the four uFor source families, Scala-derived ratios, reference origins,
detuning, exact-value preservation, rounding error, endpoint precision, finite
coverage, evaluation failures, guarded NoChange, both bulk formats, both note-change
formats, 128-key splitting and native byte-codec round trips. No ports, MIDI sends
or audio playback are involved.

Python 3.11 release checks exercise the dependency-free core and the unsupported-
version explanation. Python 3.15 checks additionally install the optional extra,
run the bridge tests and verify the installed wheel. The workflow remains
release-only and retains the oldest/newest Python matrix.

## Additional work beyond the prompt

None.

## Bounded transfer plans

`bulk_plan(report, *, device_id, program, name, gap_ns, bank=None,
preserve_unmapped=False)` composes the existing bulk builder and byte codec with
a `transfer.Plan` containing exactly one SysEx message. Count and byte budgets
are limited to that generated message. Explicit gap_ns has the existing transfer
meaning; it does not create a timer or send. The same coverage/error policy and
ordinary/banked choice apply. Library device ID 127 remains available as an
explicit broadcast choice. No per-key transfer-plan factory is added here.

Callers can inspect the message, create a `transfer.Session`, and explicitly run
it through the existing output adapter. Plan construction opens no port and
makes no capability, delivery or tuning-program-selection claim.

## Offline file workflow

On Python 3.13+, install the existing extras with `meedee[cli,ufor]`. No new
mandatory dependency is introduced. Other commands continue to work on Python
3.11+ without importing uFor. Selecting these commands on an older interpreter
returns an explicit Python requirement error before reading the source.

```bash
meedee ufor-preview tuning.json --source-format tuning-json
meedee ufor-preview scale.scl --source-format scala --root-note 69 --root-frequency 440
meedee ufor-export scale.scl dump.syx --source-format scala --root-note 69 --root-frequency 440 --device-id 1 --program 2 --name "My tuning"
meedee transfer-preview dump.syx --gap-ms 20
meedee transfer-send dump.syx --port "Synth In" --gap-ms 20
```

The first three commands are offline. The final command is a separate explicit
live action using the existing transfer executor and its failure semantics; no
instrument was used during implementation. Preview/export neither select a
receiver's tuning program nor send a reset, selection or tuning message.

Source format is required, with no suffix inference or fallback. `tuning-json`
loads the existing uFor Tuning model using its JSON validator, including its
stored reference/detuning settings; CLI reference overrides are rejected.
`scala` uses uFor's existing parser and requires explicit root note (0..127) and
root frequency, validated by the Tuning model. It uses the model's zero-detune
default. Scala keyboard maps, additional source schemas and new scale evaluation
are not introduced. Text decoding uses the host's ordinary Path.read_text defaults.

Both commands select all 128 keys by default. `--first-note` and `--last-note`
select one inclusive ascending range within 0..127. The existing library preview
continues to accept arbitrary explicit key lists. Preview shows every selected
key's status, original resolved frequency as a string (preserving rational text),
encoded pitch/Hz, signed quantization error and optional reason. This presentation
is not a new loadable tuning-file format. It returns 0 when every selected key
converted, 1 for omissions or evaluation/range problems and 2 for argument/source
errors. At most 128 key rows are emitted.

Export requires explicit device ID 0..126, program 0..127 and a native-compatible
name of at most 16 ASCII characters. Optional `--bank` (0..127) chooses KeyBasedDump;
otherwise it creates BulkDump. There is no CLI broadcast target, per-key note-change
or real-time mode. All source resolution and plan validation complete before
opening the destination. Full converted coverage is required by default.
`--preserve-unmapped` deliberately uses NoChange for unselected/unmapped keys;
out-of-range or evaluation failures still block generation. The summary reports
converted keys and preserved_keys across the full 128-slot dump. A successful
explicit-preservation export returns 0 and retains per-key statuses in its summary.

Destinations use exclusive creation, preserving existing files and refusing
symlinks or files appearing before the open. Writes use the existing SYX writer
and flush without fsync; a disk/flush failure can leave a partial new file and
returns 2. Validation failures create no destination. No partial generated plan
is returned. The exported SYX is readable by existing inspection/transfer tools.
The offline plan uses gap_ns=0; SYX stores no pacing metadata. Sending requires
its own explicit gap through the existing transfer command, and handoff never
proves device acceptance. No second live sending command or audition engine is
introduced.
