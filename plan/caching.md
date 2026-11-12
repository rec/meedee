# MIDI universe state cache

Give each MIDI universe a sparse, queryable snapshot of every supported state it has observed. Enable full retention by default. A memory-constrained application can choose minimum retention and receive the same decoded observations, diagnostics and contributor timestamps.

## Ownership

A universe follows one ordered stream of received events. An application reading two input ports gives each its own universe. A replay or an anonymous sequence can have its own instance. Preserve the existing source binding rule: an instance bound to a stream requires a reset before rebinding. An outgoing command alone supplies no evidence about the receiver.

Within the universe, keep the existing group addressing: ungrouped MIDI 1 has its own scope, and each UMP group has its own scope. Channel and per-note state live under the appropriate group. Keep protocol-specific values at native resolution. Sharing a channel number does not justify combining MIDI 1 Data Entry bytes with a native MIDI 2 parameter value.

Feed each accepted event once. Reuse the state machines behind the existing event views, including the control interpreter in `crates/meedee/src/event_views.rs` and its per-channel state in `controls.rs`. A control observation and a lookup of the same parameter must read the same underlying state. Keep standalone interpreter entry points and their existing semantics.

## What full retention covers

“Current” means the latest state established by the observed stream since the applicable reset or loss boundary. A lookup before the first relevant event yields unknown. Preserve explicit zeroes and partial knowledge. Allocate entries as values arrive.

| Category | State available in full mode | Semantics to preserve |
| --- | --- | --- |
| Channel controls and parameters | Controller values, paired controls, active selector, RPN and NRPN selections and values | Keep each parameter's coarse and fine knowledge independent, including across reselection. Keep MIDI 2 controller and parameter precision. |
| Channel performance | Program, bank selection, pitch bend and channel pressure | Track pending bank selection separately from the bank associated with an observed program change. |
| Notes and expression | Observed key state, polyphonic pressure and supported per-note controls | Key release and sounding voice lifetime are different facts. Retain only facts supported by the messages, including when note identity is ambiguous. |
| System and interpreted state | Recognized transport state and position, timecode, tuning and MPE configuration | Use the scope, completeness and reset rules of the owning interpreter. A clock pulse is an event. A decoded position is queryable state. |

Inventory the remaining message and view variants before implementing their category. For each, record its address, the event that establishes or clears it, and whether it participates in later decoding. Opaque SysEx, diagnostics and individual timing events have no generic current value. A recognized SysEx operation can update the specific state it describes. The snapshot contains no event history.

## Configuration and minimum mode

Use two settings, `full` and `minimum`, with inheritance. Start with an application-supplied default (omission means `full`). A universe may override that default. Within that universe a category may override the universe setting. The most specific explicit setting wins. For example, a recorder can use minimum by default, give its control-surface input full retention, and use minimum for notes on that input. Resolve the policy at construction so later changes to application defaults cannot silently alter a live stream.

Minimum retains everything required by subsequent decoding or an existing public interpreter contract. That includes selector halves for both families, the per-parameter components exposed by `ControlInterpreter.parameter_state`, and the contributors needed for observation spans. If parameter A receives coarse 10, parameter B is selected, and A later receives fine 5, both modes must produce 1285 for A. A minimum-mode implementation cannot discard A's coarse value when B is selected.

Save memory by omitting optional snapshot storage, such as the latest program or pressure when no interpreter needs it. Read mandatory state directly wherever it already answers a query. Minimum mode therefore has a workload-dependent footprint. Arbitrary parameter reselection requires storage that grows with the number of parameters observed even in this mode. No silent eviction or fixed memory ceiling is part of this policy.

## Queries and invalidation

Give Python and Rust the same effective-policy inspection, addressed lookup and snapshot enumeration. A caller can check whether a category has full retention before relying on its coverage. A point lookup returns the known value or absence. Enumeration returns immutable copies of known entries with their addresses and partial components. Reading a snapshot must neither allocate missing entries in the universe nor bind a source, consume an event index or change selection.

Preserve the control interpreter's component rules. Fine-only input records a known fine byte. Fresh coarse Data Entry clears the previous fine byte. Increment or decrement invalidates the affected parameter's absolute value where the step semantics are unknown. A null RPN selection ends active selection while retaining observed parameter values.

Apply channel resets to the state covered by that message. Preserve existing view behavior on System Reset, recognized GM resets, clock changes and loss diagnostics. Invalidate optional entries over the same affected scope as their inputs. A lost stream can leave every group on that stream uncertain. An explicit universe reset clears its state and releases the source binding. Re-enabling observation after loss starts with unknown values until new evidence arrives.

## Implementation and acceptance

1. Inventory state and reset semantics against the unified MIDI message model and existing views. Define addresses, category membership and the matching Rust and Python query shapes. Distinguish state needed for decoding from state retained solely for inspection.
2. Implement universe ownership and policy resolution with controls as the first category. Replay the same stream through the standalone interpreter and both universe modes. Compare values, diagnostics, indexes and timestamps across fine-only input, reselection, family changes, relative steps and loss.
3. Add the remaining categories using the inventory. Test identical channel and parameter numbers across two sources and two groups, native MIDI 2 precision, bank selection followed by program change, and scoped resets. Verify unknown, partial and known-zero results in both languages.
4. Exercise inherited defaults and category overrides. Verify optional values survive in full mode and their storage is omitted in minimum mode. Compare memory on an idle universe, a performance stream and a stream touching many distinct parameters. Report those workloads separately so constrained users can estimate the savings. Document the same configuration and lookup example for Python and Rust.

## Additional work beyond the prompt

None.
