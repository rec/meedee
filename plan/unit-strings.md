# Python unit-string audit

Additional work beyond the prompt

None.

## Agreed contract

Authored Python quantities accept compatible unit expressions using the existing
reccy parser. Numeric defaults, ranges and units remain unchanged. Integer fields
require an exact integral conversion; native nanosecond durations require exact
nanoseconds. No new dependency or Rust string parser is introduced.

- `Output(bpm=...)` accepts numeric BPM, rates such as `"120 bpm"` and beat
  periods such as `"500 ms / beat"`. Its `bpm` property remains numeric.
- `with_gap()` preserves numeric contextual durations: seconds without output
  tempo, beats with tempo. Explicit time or beat strings retain their units.
- Transposition accepts whole pitch intervals, including cents and octaves,
  while retaining clamping, metadata and no-op behavior for non-note messages.
- `ClockMapping` retains Rational inputs and results. Its rate also accepts an
  exact dimensionless string; offset and uncertainty accept exact seconds.
  Unlike nanosecond timers, those rational fields can retain sub-nanosecond values.
- Clock coordinates accept tick expressions. Seconds are not implicitly converted
  to ticks; that operation requires the clock's explicit scale.

## Parameter inventory and disposition

| Quantity | Public inputs reviewed | Result |
| --- | --- | --- |
| Contextual durations | Note, Rest, Output.send, events.with_gap | Existing note/send parser retained; with_gap typing, validation and documentation completed |
| Elapsed seconds | Input/Output timeout, Input.record/receive/stop, Recorder.stop, Cancellation.wait_for, inspection.run, routing.run, tuning requests/query, CI sessions/browser, timecode.Tracker, Universe timeout, universal.identify, sysex.dump | Existing parsing retained, including optional durations and deadlines |
| Tempo | Output.bpm, ump.tempo, smf.export_event | Output gap closed; all accept rates and beat periods; MIDI tempo helpers retain field quantization checks |
| Nanoseconds | ReceivedPacket timestamps, testing.ManualClock, testing.Input.inject, metrics, CI/tuning session methods, timecode.Tracker, Universe.feed/poll | Universe optional timestamps now use the shared parser; existing methods retained |
| Ticks | Timestamp, Clock resolution, SMF Event, UMP JR/delta-clock values, inspection.compare_files tolerance | Comparison tolerance gap closed; ticks remain explicit clock coordinates |
| Byte limits | Input, archive Recorder, codecs/stream Decoder, file Readers/Writers/timelines, routing, inspection, testing, CI, sysex/universal live drivers | CI browse byte limit gap closed; existing positive-limit validation retained |
| Byte offsets | smf.validate_meta | Compatible whole-byte strings added; zero remains valid |
| Frequency | tuning.Pitch.from_frequency | Existing Hz conversion retained |
| Pitch intervals | tuning.Pitch.from_cents, ScaleOffsets.from_cents, universal.MasterCoarseTuning, named control helpers, Message/SMF transpose | Transposition gap closed across base, channel, MIDI 2, UMP and file-record bindings |
| Timecode coordinates | Position/FullFrame hours/minutes/seconds/frames and Position.advance | Existing per-field conversions retained |
| Exact mapping | ClockMapping rate/offset/uncertainty | String inputs added without changing Rational results or rounding |

## Intentionally numeric or structured

MIDI addresses, keys, device IDs, raw controller/velocity/pitch-bend values, enum
selections, bit depths and wire-format widths remain numeric. No physical units
are inferred for encoded values. SMF division remains its packed 16-bit field.
Event/record/message/port counts are counts, not byte quantities. Rational
numerators/denominators and Clock tick-scale components retain their structured
integer representation. Returns and reports remain canonical numeric snapshots.

Rust keeps its existing typed quantities, Duration, Beats and Rational. This
preserves the documented Python-only unit-string exception rather than introducing
different underlying units or operations.

## Verification

Focused regressions cover native bindings as well as Python wrappers: tempo
normalization, scheduling under explicit/contextual gaps, pitch conversion and
metadata, file-record transposition, comparison tolerance, CI byte budgets,
Universe timestamps, byte offsets and exact clock mapping. Incompatible dimensions,
fractional integral fields, negative quantities and raw numeric selectors are
checked without opening MIDI devices.
