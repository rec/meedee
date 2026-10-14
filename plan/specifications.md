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
