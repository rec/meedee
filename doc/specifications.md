# Reviewed specification baselines

These are the document revisions used to implement the existing codecs and
derive the shared regression vectors. They are provenance, not claims about
the newest available standard or physical-device certification. Local PDFs were
kept under ignored `target/specifications/`; they are not redistributed here.
SHA-256 values identify the reviewed document bytes, not authenticity.

## Document revisions and sources

| Family | Reviewed baseline | Source |
| --- | --- | --- |
| MIDI 1.0 | Detailed Specification 4.2.1, February 1996 | [MIDI Association](https://midi.org/midi-1-0-detailed-specification) |
| UMP/MIDI 2.0 | M2-104-UM 1.1.2, draft 2023-10-27, published 2023-11-10 | [UMP specification](https://midi.org/universal-midi-packet-ump-and-midi-2-0-protocol-specification) |
| Bit scaling | M2-115-U 1.0.2, draft 2023-12-18, published 2023-12-19 | [Bit scaling and resolution](https://midi.org/midi-2-0-bit-scaling-and-resolution) |
| MTS | Eight-page Updated Specification compilation, CA-020/CA-021/RP-020 and RP-025/amd1 | [MIDI Tuning](https://midi.org/midi-tuning-updated-specification) |
| GM1 | RP-003 / MMA 0007, copyright 1991/1994 | [GM1](https://midi.org/general-midi-level-1) |
| GM2 | RP-024 1.2a, 2007-02-06 | [GM2](https://midi.org/general-midi-level-2-2) |
| MPE | M1-100-UM 1.1, 2022-04-14 | [MPE](https://midi.org/mpe-midi-polyphonic-expression) |
| Timecode | RP-004/RP-008, 4.1.1 core and 4.2 addendum | [MTC](https://midi.org/midi-time-code) |
| Master tuning | CA-025, 1999-03-02 | [Master tuning](https://midi.org/cmaster-fine-course-tuning) |
| Modulation depth | CA-026, 1999-03-02 | [MIDI 1 addenda](https://midi.org/midi-1-0-addenda) |
| Controller destination, key controls, global parameters, velocity prefix | CA-022, CA-023, CA-024 and CA-031 | [MIDI 1 addenda](https://midi.org/midi-1-0-addenda) |
| Standard MIDI Files | RP-001 1.0, document 96-1-4 | [SMF](https://midi.org/standard-midi-files-specification) |
| MIDI Clip | M2-116-U 1.0, 2023-05-11 | [Clip file specification](https://midi.org/midi-clip-file-specification-smf-midi-2-0) |
| MIDI-CI management | M2-101-UM 1.2.1, draft 2025-12-15, published 2025-12-18 | [MIDI-CI](https://midi.org/midi-ci-specification) |

The GM1 developer guidelines are timing/initialization guidance, not a normative
guarantee for every device. The full MIDI-CI Profiles and Property Exchange
documents were not available for the implemented management scope; their
dependent features remain deferred in the
[backlog](https://github.com/rec/middy/blob/main/plan/features.md).

## Reviewed artifact identities

```text
midi1.pdf                 93cbd078d9f57cd5b20349bbd08fd877f057a74e61cf923a7cec5f1b65bc80c4
ump.pdf                   fc9b043e00600e031028f755c7075541e1e32057c3cbe9197a2a0755f1d69b05
mts.pdf                   a798ef37dea8369fc40c1cdb2fdf002ba24cbc8f7ea7eb40d1508abaf5595fff
gm1.pdf                   ba09e13069ce3ad79989c3abc1d70c8f1e8ce3e3dd1097d9c241d6665e152bac
gm2.pdf                   56af9df9d9da3cbca39e743af0595a8441246124cdaa3f70736071ced164996e
mpe.pdf                   c5f6727af065e454fe9bc4ff80f66414e8b25ffb6c1019ce7795ab4a89da1f1e
timecode.pdf              5ead666921d995391624ac23a2b15af785a85b6a5ab6b4608aeab10a3921cad0
master-tuning.pdf         5d4ecbc9cdc563fc40d66a3b3b23ee4d70b1993330f1f1f95b36ded184e21cc1
modulation-depth.pdf      49be1c7b497827a889374028e789198649ca8d94ae92afc395c66d3929c3ecba
gm1-guidelines.pdf        d65bf7ef6d230cf421637529ba9b248a95b3787fb72c5513e0bb477281a15a75
controller-destination.pdf d8bfacb16e64b4b381767519f5d6d9749e15c051733e445a5104ff23318c8f88
key-controls.pdf          8cd97ed56adb3058526ddbf11d4a80fb37c38f58da705d79a7270e833051c380
global-parameters.pdf     df26b1877c1a6085c274dc43769da6ceca995a85ec584c49622062768863e2f8
velocity-prefix.pdf       3ba9538f5b45240483adc91b4e5e6f99245ef77864ea3df34d12ad32f01853a4
bit-scaling.pdf           839e75ba465ab178486c263ce8ae57d9f0cdad25adf19828cc7d6ddfe32e7ef2
smf.pdf                   fdb65bd58cd92981273468954b46fe7e915b1f8b5697f55eaec0d475895762b8
clip.pdf                  d182685b7d2f09667e112f6bb587ae7f0679c08dda39d163c61cbf181036da4b
ci.pdf                    f3c3c63d768f049a387c79998081da284a1baf8605ca10d246e9a52258056e6f
```

## Regression vector ownership and interpretation

The [shared fixtures](https://github.com/rec/middy/tree/main/crates/middy/tests/fixtures)
contain independently
stated bytes/words for MIDI 1, control commands, tuning, profiles, profile engines,
UMP and MIDI-CI. Rust and Python consume the same vectors. They are focused
layout regressions, not an official certification suite or hardware evidence.
The file codec tests also state compact SMF/Clip vectors independently of the
encoder. The [archive layout](archive-format.md) defines middy's own format.

Important interpretation choices retained by the code:

- The MTS compilation documents inconsistent original-bulk checksums. Decoding
  exposes a legacy mismatch, re-encoding normalizes the checksum, and later
  formats require valid checksums. The pitch unit is 1/16384 semitone despite
  inconsistent examples; the fourteen-bit positive scale limit is 8191 units.
- CA-024 uses MSB-first parameter identifiers but LSB-first parameter values.
  CA-031 velocity prefixes apply once to matching nonzero-velocity note events.
- MPE tracking describes observed note/expression state, not audio release tails.
  Timecode sequence locking and silence detection use explicit host time.
- Unknown complete UMP layouts may be retained opaquely without certifying their
  validity. MDS packets are not a dataset-transfer engine, and text framing does
  not imply semantic date/language/lyric interpretation.
- SMF interpretation ignores extra known-meta bytes while retaining them for
  faithful editing. MIDI Clip is distinct from the separate Container format.
- MIDI-CI currently implements management. Overview pages and base wire tables
  do not substitute for the deferred category specifications.

Recheck document revisions and errata before implementing a new dependent
feature. Fixture provenance does not establish native backend support or an
instrument's capabilities.
