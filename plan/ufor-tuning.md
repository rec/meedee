# uFor tuning previews and MTS message construction

`meedee.ufor_tuning` is an optional Python integration. uFor resolves pitch
mathematics and Scala-derived tunings; meedee's existing Rust-backed MTS types
perform quantization and encoding. The bridge adds no tuning engine, Rust uFor
interpreter, transport, clock, CLI or audio host.

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

Octave-offset formats, device transfer, capability discovery, tuning
request/reply workflows, an interactive preview and enge audition are deferred.
Those need separate device, timing, recovery and musical choices. This module
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
