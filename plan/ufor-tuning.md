# uFor note conversion and MTS message construction

`meedee.tuning.convert(source, note)` converts one note from an existing uFor
`Tuning` into the native `NoteTuning` value containing its MIDI key and MTS pitch.
uFor evaluates the tuning; meedee quantizes and encodes the resulting frequency.
There is no intermediate report or batch conversion API.

## Installation

The optional bridge uses the `ufor` extra on Python 3.13:

```bash
uv sync --extra ufor --python 3.13
```

Only `convert()` imports uFor. Core MTS values and message builders remain usable
without it. Ordinary MIDI workflows do not load uFor, and the root package does
not re-export these symbols.

## Convert one note

```python
from ufor.tuning import RatioTable, Tuning
from meedee import tuning

source = Tuning(source=RatioTable(values=["1", "3/2", "2"]), root_note=69)
note = tuning.convert(source, 70)
print(note)
```

The result is a `NoteTuning` with `note=70` and a `Pitch` for 660 Hz. The key must
be a built-in integer in 0..127. Invalid key types raise `TypeError`; keys outside
that range raise `ValueError`.

A note outside a finite source's coverage raises `KeyError`. Frequency tables
use `first_note`, ratio tables use the tuning's `root_note`, and finite interval
patterns include degree zero and their final cumulative interval. Computed and
repeating sources cover every MIDI key.

Evaluation errors propagate. Non-positive, non-finite or out-of-MTS-range
frequencies raise `ValueError`. A failed conversion returns no result and never
silently substitutes a pitch or `NoChange`. Reference frequency and detuning
come from the uFor source. Each call evaluates its source at that time; changing
the source does not change previously returned immutable note values.

MTS stores pitch in 1/16384-semitone units. Exact frequencies are checked against
the codec's bounds before conversion to float, so an out-of-range rational cannot
round onto a valid endpoint. Conversion uses the existing `Pitch.from_frequency`
rounding. Callers needing the original frequency can evaluate `source(note)`;
`Pitch.from_frequency(frequency_hz=float(source(note)))` also exposes quantization
error through `PitchConversion.error_cents`.

## Construct messages

Collect converted notes only when a message needs several keys:

```python
notes = [tuning.convert(source, n) for n in [69, 70, 71]]
dump = tuning.bulk_dump(
    notes, device_id=1, program=0, name="Just three", preserve_unmapped=True
)
print(dump)
```

`bulk_dump(notes, *, device_id, program, name, bank=None,
preserve_unmapped=False)` creates an ordinary `BulkDump` or banked `KeyBasedDump`.
It requires all 128 distinct MIDI keys unless preservation is explicitly enabled.
With preservation enabled, omitted keys use `NoChange`. Callers may also supply
`NoteTuning(note=..., pitch=NoChange())`, which requires the same opt-in.
NoChange leaves the receiver's value unchanged; it does not establish that value.
Names must contain at most 16 ASCII characters.

`note_changes(notes, *, device_id, program, realtime, bank=None,
preserve_unmapped=False)` builds `SingleNoteChange` or `BankedNoteChange` messages.
It preserves caller order and splits 128 keys into messages of 127 and one key.
The unbanked format requires `realtime=True`. A subset needs no preservation
flag unless it contains explicit `NoChange` values. These messages do not form
an atomic device transaction.

Both builders accept lists of 1..128 distinct `NoteTuning` values and retain
native message validation. Device, bank and program identifiers use MTS values
0..127. Library callers may explicitly select broadcast device ID 127.
Construction opens no ports and sends nothing.

Use `bulk_dump` directly to build the typed dump. Its encoded wire bytes can be
wrapped as a SysEx message for `sysex.write` or `Output.send`; use
`events.with_gap` for explicit waits. No transfer plan or
separate execution object is needed. The former bulk_plan helper is removed.

## Offline file export

With `meedee[cli,ufor]` installed:

```bash
meedee ufor-export tuning.json dump.syx --source-format tuning-json --device-id 1 --program 2 --name "My tuning"
meedee ufor-export scale.scl scale.syx --source-format scala --root-note 69 --root-frequency 440 --device-id 1 --program 2 --name "My scale"
```

The command converts each selected note and writes a bulk dump. `tuning-json`
loads the existing uFor model and its reference settings; CLI reference overrides
are rejected. `scala` uses uFor's parser and requires explicit root note and
frequency. It does not add keyboard maps or a separate tuning engine.

The default selection is all 128 keys. `--first-note` and `--last-note` select an
inclusive ascending range. `--preserve-unmapped` permits missing source keys and
unselected keys to become `NoChange`; evaluation and range failures always abort.
The summary reports selected, converted and preserved key counts, without
per-note status records.

Export requires device ID 0..126, program 0..127 and a name. Optional `--bank`
selects the banked format. Conversion and validation finish before the destination
is opened. Exclusive creation preserves existing files; a write failure can leave
a partial new file. Success returns 0; argument, conversion or file errors return
2. The output is readable by existing inspection and transfer tools.

Export does not send MIDI, select a receiver's tuning program or reset an
instrument. Sending remains a separate explicit operation with its own pacing.

## Verification

Device-free tests cover all four uFor source families, Scala, reference origins,
detuning, exact endpoint bounds, finite coverage, evaluation failures, strict key
validation, duplicate keys, explicit NoChange, both bulk formats, both note-change
formats, 128-key splitting and codec round trips. CLI tests cover source choices,
partial coverage, invalid pitches and exclusive file creation. No physical MIDI
interoperability is implied.

## Additional work beyond the prompt

None.
