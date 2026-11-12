# Basic SysEx dump requests and opaque capture

## Implementation status

Implemented in `sysex` in Python and Rust. The simple entry point is
`sysex.dump(receiver, output)`; omit output for manual capture with explicit model.
Optional `model` skips identity discovery. Without it, one universal identity
request must produce exactly one supported reply within the bounded collection
window. Missing, unsupported or multiple identities require explicit selection.
The existing recorder retains matched opaque messages. Current model/scope
coverage, source inconsistencies and qualification limits are recorded in
[doc/sysex-dumps.md](../doc/sysex-dumps.md).

Three models support automatic stored-data collections; minilogue xd and Rev2
support current/system requests only because their published program addressing
is inconsistent. Peak, Digitakt and Subsequent 37 support explicitly unverified
manual manufacturer-filtered capture, as approved for the basic implementation.
Roland support is VT-4 only, not a claim of Boss support. Hardware was not used.

## Objective and agreed scope

Request documented instrument dumps and capture the replies without decoding or
editing their contents. Also capture dumps initiated manually on the instrument.
Support is explicit per model, protocol revision and dump scope, never inferred
from a manufacturer name alone.

The initial manufacturer research list is Yamaha (starting with VL70-m),
Roland/Boss, Korg, Sequential, Novation, Elektron, Moog and Behringer. This is a
coverage programme, not a claim that every model from these manufacturers has a
remote dump command or can export its whole state.

Decisions agreed with the user:

- Include remote requests and user-triggered manual capture.
- Make model optional for remote requests. Use a universal identity request and
  select only a single documented identity; never guess among multiple replies.
  Keep explicit selection for manual capture and failed discovery.
- Keep the received SysEx payload completely opaque. Inspect only the envelope
  and protocol fields required to match replies and manage a transfer.
- Allow header/length matching and a silence timeout where no documented end
  condition exists. Retain the capture and mark completion unverified.
- Implement checksums or handshakes only when the supported transfer protocol
  requires them. Do not build a general patch decoder or validation framework.

Hardware qualification remains a separate explicitly authorized activity.

## Existing foundations and ownership

- `messages.SysEx` retains MIDI 1 payload bytes; `codecs` handles framing.
- `Input.start()` / `Input.stop()` and `archive.Recorder` provide the shared
  recording lifecycle. `Input.record(duration)` uses that same recorder.
- `Output.send`, generic `events.with_gap` and `file_io.Reader`/`Writer` already
  provide transport and concatenated `.syx` storage.
- The tuning query implementation already demonstrates reply matching,
  caller-driven deadlines and uncertain-send reporting. Reuse suitable existing
  logic and patterns; do not generalize that implementation speculatively.
- `plan/features.md` assigns meaningful instrument descriptions and parameter
  mappings to integration with uFor. Keep dump recipes limited to transfer
  knowledge. Before introducing persistent model metadata, inspect the existing
  uFor contracts and reuse them where appropriate; do not invent a competing
  instrument-description schema or add a dependency merely for this feature.

Implement the shared behavior in Rust and expose equivalent Python APIs.
Preserve existing names and behavior outside the dump feature. Do not introduce
another recording engine, callback framework, asynchronous runtime, database,
plugin system or executable recipe language.

## Minimal supported operation

1. The caller selects the supported dump scope, input endpoint and, for a remote
   request, output endpoint. Without an explicit model, issue a universal
   identity request and require exactly one supported identity through a bounded
   collection window. Manufacturer identity alone is insufficient. Manual
   capture requires explicit model selection. Do not guess endpoint pairings.
2. Validate the selected recipe and address values before sending. Prepare the
   input and recording boundary before sending the request so immediate replies
   cannot be missed. Exclude buffered traffic preceding that boundary, even
   though ordinary memory recording can include previously buffered events.
3. Send the documented request bytes, substituting only required device IDs,
   bank/program numbers or block addresses. Manual mode sends no request and
   tells the caller which front-panel action starts the selected dump.
4. Retain matching complete SysEx messages byte-for-byte in arrival order.
   Preserve repeated messages unless the particular protocol defines duplicates.
   Ignore unrelated traffic for dump matching and timeout progress. Do not echo
   replies, restore data, send program changes or change instrument settings.
5. Perform only protocol-required follow-up requests or acknowledgements.
   Request sequences must have a documented finite scope. Do not retry uncertain
   sends or probe undocumented commands.
6. Stop on the recipe's documented completion condition, silence after matching
   traffic, overall deadline, cancellation, resource limit or input failure.
   Distinguish no first response from silence after a partial response.
7. Finalize through the existing recorder lifecycle. Retain complete messages
   already captured on failure and expose loss/truncation diagnostics. Do not
   turn an unfinished SysEx fragment into a complete message.

Use the same operation and capture path for remote and manual modes. Manual
capture needs no output connection. Keep request orchestration separate from
caller-owned connection lifetime; finish the dump without closing connections
that the caller supplied. Use monotonic time and the existing duration/unit
conventions. Keep explicit per-message, total-byte, message-count and elapsed-time
limits so the operation cannot retain unbounded data or wait indefinitely.

The library surface belongs beside the existing `sysex` operations. Finalize its
small Python/Rust signatures against the first working model rather than adding
an abstract session hierarchy. Reuse current CLI receive/export facilities where
possible; a new CLI or GUI is not part of the initial implementation scope.

## What a model recipe contains

Include only information needed by a supported transfer:

- Model/protocol identification, source document and known firmware restrictions.
- Exact dump coverage and prerequisites, including required instrument settings.
- Request bytes and their required substitutions, or manual initiation steps.
- Reply header matching, documented lengths and block/bank/program identifiers
  needed for correlation. A manufacturer prefix alone is usually insufficient.
- Completion and timeout rules; required request ordering, pacing or handshakes.
- Any protocol-required checksum calculation or integrity check.

Do not unpack parameter values, interpret patch names, relocate memory slots or
normalize the returned data. Start with straightforward recipes. Add shared
manufacturer helpers only when implemented models demonstrate actual reuse.

## Results, completeness and export

Return the opaque captured messages and a compact transfer report. Keep request
progress, capture termination and completion evidence distinct. The report must
identify the requested scope, whether a request was sent or uncertain, retained
message/byte counts, relevant diagnostics and the reason capture ended.

Documented message/block counts or a documented end marker can establish
completion of the requested scope when required checks pass and no loss was
observed. Header/length matching, user stop or a silence timeout alone must not
claim protocol-verified completion. `F7` ends one SysEx message, not necessarily
the whole dump. Partial, failed, cancelled, empty and unverified captures must
remain distinguishable from verified completion.

Export the retained messages with the existing `.syx` writer. Keep the report
available separately because `.syx` does not encode completeness or coverage.
Never describe a capture as a restorable full-machine backup without evidence.
An instrument's dump may omit samples, global settings, edit buffers or other
state; even a complete dump can contain data that its receive protocol cannot
restore directly.

## Manufacturer research and rollout

Begin with VL70-m end to end, then add a different protocol family before
extracting shared helpers. For each manufacturer, select a concrete documented
model and record one of: remote request implemented, manual capture implemented,
research pending, or no usable documented dump path found. Record documentation
and fixture evidence separately from physical-device verification.

| Manufacturer | Starting point | Evidence and remaining work |
| --- | --- | --- |
| Yamaha | VL70-m | Native/VL-XG bulk requests use block addresses. Inventory the required blocks and modes; a block reply is not a whole-machine dump. [List Book](https://data.yamaha.com/files/download/other_assets/7/314687/VL70mG2.pdf), printed pp. 29-30 and address tables. |
| Roland/Boss | Select first model from available hardware; VT-4 is a documented protocol example | RQ1 requests and DT1 replies require model-specific address/size maps and checksums. Do not assume one Roland recipe covers Boss or other Roland models. [VT-4 MIDI implementation](https://static.roland.com/assets/media/pdf/VT-4_MIDI_Imple_eng01_W.pdf). |
| Korg | minilogue xd candidate | The manual describes manual All Dump of programs, live set and global data. Inspect the MIDI implementation for remote requests and boundaries before declaring remote support. [Downloads and MIDI implementation](https://www.korg.com/us/support/download/product/0/811/). |
| Sequential | Prophet Rev2 candidate | Appendix E documents program, edit-buffer and global requests. Resolve inconsistent program-range descriptions before bank enumeration. Global dump replies are documented as not directly accepted on receive. [User's Guide 1.2.4](https://www.sequential.com/wp-content/uploads/2021/02/Prophet-Rev2-Users-Guide-1.2.4.pdf), Appendix E. |
| Novation | Peak candidate | Published guidance describes SysEx patch export. Establish remote request bytes and response rules from protocol documentation, or support manual capture first. [Peak guide](https://userguides.novationmusic.com/hc/en-gb/articles/25494693870354-Novation-Peak-in-detail). |
| Elektron | Digitakt candidate; treat Digitakt II separately | Manual SysEx sending is documented. Dumps cover selected project/pattern/sound data and exclude samples. Do not infer a remote request or interchangeability between generations. [Digitakt OS 1.52A manual](https://www.elektron.se/wp-content/uploads/2025/07/Digitakt-User-Manual_ENG_OS1.52A_250708.pdf), section 15.6. |
| Moog | Subsequent 37 candidate | Manual current-preset, all-preset and bulk dumps are documented. Establish any remote request independently; preserve the distinction between these scopes. [Owner's manual](https://cdn.inmusicbrands.com/Moog/Subsequent37/Subsequent_37_Manual_0.pdf), p. 38. |
| Behringer | DeepMind 12 candidate | Acquire the manufacturer-hosted MIDI/SysEx specification and confirm requests, device addressing and response boundaries. Support remains research pending. [Product/support entry](https://behringer.com/en/products/0722-AAA). |

These are research starting points, not eight completed implementations. Missing
remote-request documentation is not proof that a device cannot be queried.
Support a useful documented scope rather than expanding into reverse engineering
or a vendor editor's proprietary protocol. Additional models/manufacturers are
separate additions, using the same minimal operation.

## Implementation sequence and verification

1. Audit the existing recording, receive and query boundaries. Decide the minimal
   public API and recipe location, preserving Python/Rust parity and the uFor
   ownership boundary. Update relevant existing feature documentation when the
   feature is implemented, not as a claim of support in this planning commit.
2. Implement VL70-m request plus opaque capture using the shared recorder. Add
   manual capture and unverified silence termination through that same path.
3. Implement one distinct documented family, then work through the agreed
   manufacturer list. Unsupported or unresolved cases must remain explicit.
4. Add tracked request/reply fixtures with provenance and focused tests using
   existing test doubles. Test immediate replies, stale/unrelated traffic,
   repeated replies, multiple-message completion, silence/no response, loss,
   truncated SysEx, cancellation, limits, uncertain sends and required handshakes
   only for protocols actually implemented. Assert exact retained bytes and
   matching Python/Rust results. Verify `.syx` round trips without decoding.
5. Run the required Python checks when Python changes and relevant Rust tests
   and target checks. Hardware qualification is a separate explicitly authorized
   activity; lack of hardware evidence must be visible in the support inventory.

Acceptance requires a shared request/manual capture path, no payload decoding,
clear per-model coverage, honest completion evidence, matching public behavior in
both languages, updated examples and passing focused transfer regressions.
Do not claim broad manufacturer coverage merely because a generic capture works.

## Additional work beyond the prompt

None.
