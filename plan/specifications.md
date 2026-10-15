# Normative specification baselines

Acquisition review, 2026-10-13, for slice 1. Obtain documents from the MIDI
Association's published download links. PDFs are stored locally under the
ignored `target/specifications/` directory, not redistributed in this repository.
Record SHA-256 values to distinguish document bytes from an unchanged web title.
These hashes identify reviewed artifacts; they are not signatures or claims
that this is the newest revision of every standard.

Additional work beyond the prompt

None.

## Acquired baselines

| Family | Baseline read from the document | Primary source |
| --- | --- | --- |
| MIDI 1.0 | Detailed Specification 4.2.1, revised February 1996; incorporates the 4.1.1 core and 4.2 addendum, excludes the MIDI Time Code specification | [MIDI Association](https://midi.org/midi-1-0-detailed-specification) |
| UMP/MIDI 2.0 | M2-104-UM 1.1.2, draft 2023-10-27, published 2023-11-10; includes MIDI 1.0 in UMP | [MIDI Association](https://midi.org/universal-midi-packet-ump-and-midi-2-0-protocol-specification) |
| MTS | Eight-page Updated Specification compilation, including CA-020/CA-021/RP-020 and RP-025/amd1; extension approvals 1999, amendment 2000, copyright through 2004; no single numbered revision advertised | [MIDI Association](https://midi.org/midi-tuning-updated-specification) |

Artifact identities:

```text
midi1.pdf  93cbd078d9f57cd5b20349bbd08fd877f057a74e61cf923a7cec5f1b65bc80c4
ump.pdf    fc9b043e00600e031028f755c7075541e1e32057c3cbe9197a2a0755f1d69b05
mts.pdf    a798ef37dea8369fc40c1cdb2fdf002ba24cbc8f7ea7eb40d1508abaf5595fff
```

The prototype uses MIDI 1.0's channel and seven-bit data ranges. It performs no
UMP, MTS or SysEx packet encoding. Future wire fixtures must cite the relevant
document section/table and use independently stated expected bytes rather
than deriving expectations from our own encoder. Keep test vectors narrowly
scoped and do not copy full copyrighted tables or specification text into code.

## Documents required before dependent slices

- General MIDI 1: acquire RP-003, the 1996 specification identified on the
  [GM1 page](https://midi.org/general-midi-level-1), before committing sound maps.
- General MIDI 2: acquire the 1.2a document identified on the
  [GM2 page](https://midi.org/general-midi-level-2-2), before implementing its
  banks, drum kits and additional controller/mode semantics.
- MPE: acquire M1-100-UM 1.1 from the
  [MPE page](https://midi.org/mpe-midi-polyphonic-expression); do not use the
  superseded RP-053 document without explaining a version difference.
- SMF, MIDI Time Code, high-resolution velocity prefix, and selected universal
  SysEx addenda: acquire and record the particular revisions before their codecs.
- MIDI-CI base, profiles and property exchange: acquire the selected current
  normative revisions before defining transaction formats or negotiation.
- Standard UMP file/container formats: acquire their own specifications;
  M2-104-UM packet support alone is not a file-format specification.

These are acquisition gates for later features, not implemented standards.
Recheck for approved errata or superseding revisions when each dependent slice
starts. Full document access, precise revision identity and test-vector provenance
must be established before encoding any of those families.

## Slice 3 wire evidence, 2026-10-14

The acquired MIDI 1.0 PDF still matches the recorded SHA-256. The Association's
[specification page](https://midi.org/midi-1-0-detailed-specification) continues
to identify the 1996 4.2.1 revision. The byte codec uses its Channel Voice,
System Common, System Real-Time and System Exclusive sections and appendix A's
Running Status and Undefined Status rules. The public
[message summary, table 1](https://midi.org/summary-of-midi-1-0-messages) also
states the F1 `0nnndddd` bit layout. QuarterFrame implements only that raw
layout, not the separately specified MTC sequence or full-frame interpretation.

Independent compact fixtures are in
`crates/meedee/tests/fixtures/midi1.txt`, consumed by Rust and Python tests.
They state expected wire bytes directly, including channel/program boundaries,
signed bend conversion and preserved zero-velocity NoteOn. Parser tests cover
running status, reserved statuses, real-time interleaving and malformed streams.
These are meedee regression vectors derived from the cited layouts, not an
official MIDI certification suite or copied full specification tables.

## Slice 4 control and parameter evidence, 2026-10-14

The acquired 4.2.1 baseline's Control Change section describes paired controllers
0–31/32–63, repeated fine updates and clearing the previous fine component when
a new coarse value arrives. Its Registered and Non-Registered Parameter Numbers
section describes two selector halves and Data Entry/Increment/Decrement.
The Association's current [controller/RPN table](https://midi.org/midi-1-0-control-change-messages)
provides selector identifiers 98–101, Data Entry 6/38, direction-only Increment/
Decrement 96/97 (data N/A), and the RPN 127/127 release affecting both families.
The implementation uses those raw protocol assignments. It does not implement
RP-018's parameter-specific receiver response/units or profile-specific scaling.

The shared independent vectors in `crates/meedee/tests/fixtures/commands.txt`
state complete expected CC bytes, including meedee's selected release policy.
Tests distinguish valid NRPN 16383 from RPN-null and preserve seven-bit coarse
observations instead of inventing an observed fine zero. The fresh-selector-half
receive rule is an explicitly approved conservative deviation: devices allowed
to edit only one selector half will not receive inferred parameter observations.
Physical instrument responses and cross-platform native qualification remain
separate from wire layout/state-machine validation.

## Slice 5 MTS evidence, 2026-10-15

The acquired eight-page Updated Specification remains the baseline listed above.
Pages 1–3 define absolute pitches, no-change, original bulk dumps/requests and
real-time single-note changes. Pages 4–6 define banked requests, key-based dumps,
banked real-time/non-real-time note changes and scale/octave dumps. Pages 6–8
define both scale/octave resolutions and channel bitmap order/reserved bits.
These are MIDI 1.0 SysEx encodings, not MIDI 2.0 per-note-pitch semantics.

`crates/meedee/tests/fixtures/tuning.txt` states independent compact vectors for
all implemented families and real-time alternatives, consumed by Rust/Python.
The fixed checksum bytes are independently stated rather than computed with the
implementation under test. Tests cover exact pitches, sentinels, signed scale
limits, rounding/error, every recognized truncated length, extra data, checksum
policy and request matching/expiry/cancellation. Additional boundary checks
preserve channel masks, names, repeated-note order and no-op forms. These are
regression vectors derived from layouts, not manufacturer interoperability or
MIDI certification results.

Page 6 explicitly documents inconsistent original-bulk checksum implementations
and permits receivers to ignore that checksum. The approved API exposes a legacy
mismatch instead of discarding the dump; encoding uses the all-payload XOR rule,
and later dump checksums are required. The specification includes typographical
inconsistencies: its 00 00 01 frequency example does not agree with the defined
1/16384-semitone units, a banked-request diagram omits the device byte despite
its field description, and fourteen-bit scale positive endpoints are rounded to
100 cents in prose. The implementation follows the stated field layouts and
resolution, including the actual +8191 * 100/8192-cent upper bound. Zero count/
zero channel masks are preserved as no-ops; no additional reserved-value rule
is invented. RPN tuning selection does not alter the approved 0–127 tuning
bank/program identifiers in the SysEx API.
