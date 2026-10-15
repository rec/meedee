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

- SMF, high-resolution velocity prefix, and further selected universal
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

## Slice 6 profile evidence, 2026-10-15

Additional acquired documents, read before implementing their selected layouts:

| Family | Reviewed baseline | Primary source |
| --- | --- | --- |
| GM1 | RP-003 / MMA 0007, copyright 1991/1994; distributed in the 96-1-4 collection | [GM1](https://midi.org/general-midi-level-1) |
| GM2 | RP-024, revision 1.2a, February 6, 2007 | [GM2](https://midi.org/general-midi-level-2-2) |
| MPE | M1-100-UM 1.1, April 14, 2022 | [MPE](https://midi.org/mpe-midi-polyphonic-expression) |
| MIDI Time Code | RP-004/RP-008, 4.1.1 core plus 4.2 addendum, distributed in the 4.2.1 collection | [MTC](https://midi.org/midi-time-code) |
| Master tuning | CA-025, March 2, 1999 | [Master fine/coarse tuning](https://midi.org/cmaster-fine-course-tuning) |
| Modulation depth | CA-026, March 2, 1999 | [MIDI 1.0 addenda](https://midi.org/midi-1-0-addenda) |
| GM1 initialization guidance | Developer Guidelines, 1995 commission/1996 distribution; guidance rather than a normative timing guarantee | [Guidelines](https://midi.org/general-midi-level-1-developer-guidelines) |

```text
gm1.pdf              ba09e13069ce3ad79989c3abc1d70c8f1e8ce3e3dd1097d9c241d6665e152bac
gm2.pdf              56af9df9d9da3cbca39e743af0595a8441246124cdaa3f70736071ced164996e
mpe.pdf              c5f6727af065e454fe9bc4ff80f66414e8b25ffb6c1019ce7795ab4a89da1f1e
timecode.pdf         5ead666921d995391624ac23a2b15af785a85b6a5ab6b4608aeab10a3921cad0
master-tuning.pdf    5d4ecbc9cdc563fc40d66a3b3b23ee4d70b1993330f1f1f95b36ded184e21cc1
modulation-depth.pdf 49be1c7b497827a889374028e789198649ca8d94ae92afc395c66d3929c3ecba
gm1-guidelines.pdf   d65bf7ef6d230cf421637529ba9b248a95b3787fb72c5513e0bb477281a15a75
```

GM1 supplies program numbers and percussion assignments, without bank numbers.
GM2's sound/percussion tables supply 256 melodic addresses, nine drum kits and
per-key overrides, including explicit silent keys. The Rust tables contain
functional names and addresses, not copied page layouts, synthesis requirements,
key ranges, panning or exclusion-group prose. Python uses those same Rust tables.
MIDI 1.0's device-inquiry/master-volume layouts supply the identity codecs;
CA-025 supplies master tuning. GM2 supplies named RPN semantics, while CA-026
identifies modulation-depth RPN 5 without guaranteeing a universal physical
response. Bend-sensitivity cents remain seven-bit values; modulation-depth fine
units instead divide a semitone by 128.

`crates/meedee/tests/fixtures/profiles.txt` contains independent compact vectors
for bank/program ordering, MPE RPN 6, GM modes, identity request, master controls
and full-frame MTC. Rust and Python consume the same expected bytes, stated
independently of the encoder. Additional tests cover both identity manufacturer
forms, malformed lengths, table addresses/counts, inherited/silent drum entries,
frame-rate bounds and explicit GM interpreter resets. These are regression
vectors, not certification or physical instrument validation.

Full-frame MTC follows the acquired broadcast layout and requires device ID 127;
its rate/frame fields do not implement timeline conversion or quarter-frame
locking. MPE supports zone configuration only. GM2 controller-destination,
key-based instrument-control and global effect-parameter SysEx are explicitly
deferred. Full GM2 message coverage is not claimed. GM2 On's 100 ms settling
requirement is exposed as metadata; no automatic sleep/reset is performed.
GM1 developer guidance does not establish a universally safe initialization delay.

## Slice 6.5 profile engines, 2026-10-15

Reviewed the standalone CA-022 (1999-02-28), CA-023 (1999-02-28), CA-024
(1999-03-05) and CA-031 addenda from the Association's
[MIDI 1.0 addenda downloads](https://midi.org/midi-1-0-addenda), in addition to
the already recorded GM2, MPE 1.1 and MTC baselines. The acquired CA-031 PDF
identifies the approval number but advertises no numbered revision/date.

```text
controller-destination.pdf d8bfacb16e64b4b381767519f5d6d9749e15c051733e445a5104ff23318c8f88
key-controls.pdf           8cd97ed56adb3058526ddbf11d4a80fb37c38f58da705d79a7270e833051c380
global-parameters.pdf      df26b1877c1a6085c274dc43769da6ceca995a85ec584c49622062768863e2f8
velocity-prefix.pdf        3ba9538f5b45240483adc91b4e5e6f99245ef77864ea3df34d12ad32f01853a4
```

CA-024 distinguishes MSB-first parameter identifiers from LSB-first parameter
values. CA-023 excludes bank/data-entry/parameter selectors and channel modes,
while redefining 120/121 for key tuning. CA-031 consumes one prefix per matching
channel note event, permits intervening messages and excludes zero-velocity
NoteOn from fine-velocity interpretation. Compact independent vectors in
`crates/meedee/tests/fixtures/profile-engines.txt` are checked in both languages.
MPE sections 2.2/2.3 and appendix A distinguish member expression on Active Notes
from manager expression on Sounding Notes; a pure tracker cannot infer release
envelopes. The allocator's explicit no-sharing/no-stealing policy is narrower
than a complete Mode 3 sender. MTC sequence evidence is checked against its
quarter-frame implementation section; the approved two-cycle acquisition rule
is deliberately stricter than the minimum complete eight-message sequence.
