# Opaque SysEx dumps

Use existing connections. Supplying an output requests a dump; omitting it
captures a dump you start on the instrument. The payload remains opaque.

```python
from middi import file_io, midi, sysex

with midi.Input(port="VL70") as receiver, midi.Output(port="VL70") as output:
    result = sysex.dump(receiver, output)
    print(result.status, result.reason)
    with file_io.Writer("vl70.syx") as writer:
        for m in result.messages:
            writer.write(m)
```

For a manual dump, start capture first and then trigger the instrument's dump:

```python
with midi.Input(port="Subsequent") as receiver:
    result = sysex.dump(receiver, model="moog/subsequent-37")
    with file_io.Writer("subsequent.syx") as writer:
        for m in result.messages:
            writer.write(m)
```

The equivalent Rust API uses an optional output and options with defaults:

```rust,no_run
use middi::{midi, sysex};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let mut receiver = midi::Input::new().with_port("VL70")?;
let mut output = midi::Output::new().with_port("VL70")?;
let result = sysex::dump(
    &mut receiver, Some(&mut output), None, &Default::default(),
)?;
println!("{}: {:?}", result.status, result.reason);
// Manual capture: output None and model Some("moog/subsequent-37").
# Ok(())
# }
```

`sysex.models()` lists the exact model keys. Connections remain open and belong
to the caller. The operation consumes input, including discarding already buffered
events before requesting/capturing. Do not concurrently receive on that input or
use callback-mode input. Requests are serialized and never retried automatically.
No program selection, parameter writes or restores are sent.

## Model detection

Identity inquiry is independently available as `universal.identify(receiver,
output)`. It returns an `IdentityReport` containing all raw reply `messages`,
`status`, `uncertain_send`, `ignored_events` and `reason`. It keeps duplicate and
nonstandard replies without selecting a device. Standard messages can be decoded
with `universal.decode`; this decoder still rejects nonstandard layouts.
Status `completed` means the collection window ended without observed failure,
not that exactly one device replied. No replies yields `timed_out`. Failure,
cancellation and limit reports retain any admitted reply prefix. Connections
remain open and buffered input is discarded before the inquiry.

```python
from middi import universal

identities = universal.identify(receiver, output)
print(identities.status, identities.reason)
for reply in identities.messages:
    print(reply.data.hex())
```

With an output and no `model`, the operation sends one broadcast Universal
Non-Realtime Identity Request (`F0 7E 7F 06 01 F7`). It collects replies for
`identity_timeout` (default one second), within the overall timeout, before
sending any dump request. It requires exactly one reply with a documented
manufacturer/family/member combination. Revision bytes are accepted without
using them to guess another model.

Dump capture reuses the universal inquiry and applies its own model-recipe
matching and ambiguity rules to the collected messages. Standalone inquiry
defaults to device ID 127 (broadcast), one second, 4096 observed events, 8 MiB
of retained wire bytes and 1 MiB per payload. `device_id` can target one address;
timeouts, limits and cancellation are explicit in Python and Rust.

| Detected model | Family bytes | Member bytes | Dump addressing |
| --- | --- | --- | --- |
| Yamaha VL70-m (`43`) | `00 41` | `0C 02` | Reply device ID 0-15. |
| Roland VT-4 (`41`) | `51 03` | `00 00` | Fixed reply/dump device ID `10`. |
| Korg minilogue xd (`42`) | `51 01` | `00 00` | Reply device ID 0-15 becomes the dump channel nibble. |
| Sequential Prophet Rev2 (`01`) | `2F 01` | `00 00` | No dump address field. |

These signatures come from the identity sections of the model manuals linked
below: Yamaha p. 29, Roland p. 2, Korg messages 1-5/2-1, and Sequential Appendix E.
The Rev2's documented single revision byte is accepted as well as the standard
four-byte revision. The general universal decoder remains strict.

No reply, an unknown or malformed identity, or multiple replies produces a
failed report without sending a dump request. Even identical replies count as
multiple, since two devices may share an ID. A later identity reply observed
during capture also aborts the transfer. A finite window cannot prove that no
device will respond later; increase `identity_timeout` for slow devices and
avoid shared routes when selecting a device explicitly.

Use `model="yamaha/vl70-m"` to skip detection, including when detection fails.
Manual capture requires explicit `model`. DeepMind 12, Peak, Digitakt and
Subsequent 37 currently require explicit selection because this implementation
has no verified identity mapping for them. Manufacturer alone never selects a
model. If `device_id` conflicts with the detected address, detection fails;
explicit selection lets the caller choose an address. Korg and Rev2 still need
`scope="current"` or `scope="system"` because their `all` requests are unsupported.

## Results and limits

`result.messages` contains complete `messages.SysEx` values in received order,
including a matching reply that fails envelope validation. Their original bytes
and event metadata survive capture. Unrelated traffic is ignored; repeated
messages in manual capture are retained. An input diagnostic stops capture and
its reason is retained. An unfinished fragment is never made into a message.

The result also exposes `model`, `scope`, `status`, `complete`, `requests_sent`,
`uncertain_send`, `ignored_events`, `wire_bytes` and `reason`.
`model` is absent (`None`) if detection did not select a model. Identity replies
are excluded from `messages` and `wire_bytes`; `requests_sent` counts dump
requests only. `uncertain_send` also covers an uncertain identity request send.

| Status | Meaning |
| --- | --- |
| `complete` | All expected replies for the documented scope passed its required envelope checks, with no observed input loss. |
| `unverified` | A manual capture ended in silence, or a supported request completed without reliable published payload lengths. |
| `timed_out` | No matching response, an incomplete requested sequence, or the overall deadline expired. The reason distinguishes these cases. |
| `cancelled` | Caller cancellation or interruption. Already admitted messages are retained. |
| `limit_reached` | A retention limit was reached, or the initial input queue could not be drained within its event budget. |
| `failed` | Identity detection failure/ambiguity, input failure/loss, a device rejection, envelope validation failure or send failure. |

`complete` verifies the transfer's stated scope, not physical sender identity,
restorability or an entire-machine backup. Some devices omit data from their
dumps or cannot directly reload every reply. A `.syx` file stores messages, not
the completion report; retain the report separately when completeness matters.

Manual mode always reports completion and model identity as unverified. For Peak,
Digitakt and Subsequent 37 it filters by manufacturer ID only, on the input the
caller explicitly selected. Other models have model/command header filters.
Avoid shared MIDI sources carrying other instruments' traffic during a dump.
An in-flight old reply with the same envelope cannot be distinguished from a new
one where the device protocol has no transaction identifier.

Defaults are a 120-second total `timeout`, one-second `silence` and
`identity_timeout`, 4096 retained
messages, 8 MiB total wire bytes and a 1 MiB message payload. Python accepts unit
strings such as `timeout="3 minutes"` and `byte_limit="16 MiB"`; Rust uses
`DumpOptions` and typed durations. Limits apply before retaining each message.
The receiver's own decoder/queue limits also apply independently. Other traffic
does not extend the silence window. Timeout starts when the operation starts,
including its initial queue drain. A cancelled `events.Cancellation` can stop it
early. Discovery uses the same message/payload limits to bound reply collection.
Unsupported models/scopes and invalid options raise an error before any dump
request is sent. With automatic detection, model-specific scope validation
necessarily happens after the identity request.

## Model coverage

The following recipes have source-based synthetic regression fixtures, shared
between Python and Rust. None has been physically qualified by this change.
`all` means the exact collection below, not every aspect of instrument state.
Manual mode accepts only `scope="all"`; choose the actual scope on the instrument.

| Model key | Automatic scopes | Automatic `all` coverage | Wire device ID |
| --- | --- | --- | --- |
| `yamaha/vl70-m` | `all`, `current`, `system` | System block, 6 custom voices, 64 internal voices. Excludes current edit/part/effect state. | Default 0; 0-15. |
| `roland/vt-4` | `all`, `current`, `system` | System, temporary patch/effect blocks and all documented user patch/effect blocks. | Fixed `0x10`. |
| `korg/minilogue-xd` | `current`, `system` | Unavailable pending resolution of contradictory published program-address widths. | Default 0; 0-15, encoded into `3g`. |
| `sequential/prophet-rev2` | `current`, `system` | Unavailable pending resolution of contradictory published bank/program ranges. | No dump address field; leave unset. |
| `behringer/deepmind-12` | `all`, `current`, `system` | Global data and 8 banks of 128 programs. Excludes edit buffer. | Default 0; 0-15. |
| `novation/peak` | Manual only | Front-panel selection determines coverage. | Leave unset. |
| `elektron/digitakt` | Manual only | Project/pattern/Sound data; no samples. | Leave unset. |
| `moog/subsequent-37` | Manual only | Front-panel selection determines coverage. | Leave unset. |

`current` on VL70-m captures the common-misc and element blocks, not part/effect
settings. VT-4 `current` captures its six temporary blocks. Other automatic
`current` scopes capture the program edit buffer. `system` requests each model's
system/global block. These are read-only request messages. No recipe enables
SysEx reception, changes mode or changes routing on the instrument.

## Sources, prerequisites and manual initiation

- **Yamaha:** [VL70-m List Book](https://data.yamaha.com/files/download/other_assets/7/314687/VL70mG2.pdf), printed pp. 29-30, Tables 3, 6, 9-11. Native `57` requests require the matching device number and appropriate Voice-mode/SysEx settings. Reply count, address, length and checksum are checked. For manual capture use the instrument's Dump Out function. The existing uFor VL70-m decoder remains the owner of parameter interpretation; these recipes do not depend on it.
- **Roland/Boss group:** [VT-4 MIDI Implementation 1.01](https://static.roland.com/assets/media/pdf/VT-4_MIDI_Imple_eng01_W.pdf), pp. 1-4. RQ1/DT1 tables and the address map consistently specify model `00 00 00 51`, despite a conflicting prose model ID. Requests use the fixed block sizes; replies require the matching address, length and checksum. This is VT-4 support, not a claim of support for Boss products. Manual mode can record externally initiated VT-4 DT1 traffic; it does not imply a documented front-panel all-dump command.
- **Korg:** [minilogue xd MIDI Implementation 1.01](https://www.korg.com/us/support/download/manual/0/811/4440/), messages 1-6, and [owner's manual](https://cdn.korg.com/us/support/download/files/efbf7ff0140570942060130b28f96ae6.pdf), Global Edit / All Dump. Enable SysEx and use the matching channel/device nibble. Requests match the exact model/command header and recognize negative status `24`. Captures remain unverified: stated payload sizes conflict with parameter tables, and the program-address width conflicts with 500-slot coverage. Manual All Dump includes programs, live-set and global data, not user oscillator/effect binaries.
- **Sequential:** [Prophet Rev2 User's Guide 1.2.4](https://www.sequential.com/wp-content/uploads/2021/02/Prophet-Rev2-Users-Guide-1.2.4.pdf), Appendix E and Exporting Programs and Banks. Enable SysEx and select the intended MIDI/USB cable. Current and global requests/reply lengths are explicit. Stored-program request/reply ranges disagree, so no automatic bank enumeration is implemented. The global reply is documented as not directly loadable. For manual capture select the desired PGM DUMP scope in Globals and initiate transmission.
- **Behringer:** DeepMind 12 User Manual, section 19.2.4, [manufacturer-authored manual mirror](https://dolibarr-new-prod.madridhifi.com/crm/documents/produit/0/5/10110750/photos/pdf/1/manual-instrucciones-behringer-deepmind-12.pdf). The manufacturer download hostname was unavailable during implementation; the source text specifies program/edit-buffer/global requests, communication version `06`, 278 packed program bytes and 56 packed global bytes. Replies are checked against those headers/lengths. Match the configured device ID and SysEx interface settings; manual capture uses SysEx Export. Hardware qualification remains necessary, particularly for packed lengths and firmware versions.
- **Novation:** [Peak guide, Backup page](https://userguides.novationmusic.com/hc/en-gb/articles/25494693870354-Novation-Peak-in-detail). Select Current, bank(s), Settings or ABCD+Set; set Send To to the connected MIDI/USB port; press Go after capture starts. No remote request bytes are inferred from the front-panel feature.
- **Elektron:** [Digitakt OS 1.52A manual](https://www.elektron.se/wp-content/uploads/2025/07/Digitakt-User-Manual_ENG_OS1.52A_250708.pdf), section 15.6. Open SysEx Dump / SysEx Send, select the project/pattern/Sound scope and start transmission after capture starts. Samples are excluded. Digitakt II is a different model and is not aliased to this recipe.
- **Moog:** [Subsequent 37 manual](https://cdn.inmusicbrands.com/Moog/Subsequent37/Subsequent_37_Manual_0.pdf), p. 38. Use MIDI Menu / SysEx and select Current Preset, All Presets or Bulk Dump, then initiate Send after capture starts. Bulk Dump includes presets and MIDI/global settings. No undocumented remote request is sent.

Manufacturer IDs used for the three coarse manual filters are assigned in the
[MIDI Association's SysEx ID table](https://midi.org/SysExIDtable). Fixture payloads
are synthetic opaque bytes, not copied patches or claimed hardware observations.
