"""One caller-thread CI management scan through existing typed connections."""

from __future__ import annotations

from collections import deque
from time import monotonic_ns
from typing import TYPE_CHECKING

from . import ci, codecs, events, messages, ump

if TYPE_CHECKING:
    from . import midi


def run(
    browser: ci.Browser,
    receiver: midi.TypedInputConnection,
    output: midi.OutputConnection,
    *,
    cancel: events.Cancellation | None = None,
    message_limit: int = 1_048_576,
    byte_limit: int = 8_388_608,
) -> ci.BrowseReport:
    """Run a fresh browser; callers own connections and serialize all access.

    A group-less route uses MIDI 1 bytes; an explicit group uses UMP SysEx7.
    Full-action handoffs are counted, and a partial/interrupted action is uncertain.
    """
    from . import midi

    route = browser.route
    if route.input != receiver.stream:
        raise ValueError("browser route must match the receiver stream")
    assembler = ump.EventAssembler(
        message_limit=message_limit, total_limit=byte_limit, max_streams=1
    )
    queue: deque[ci.Action] = deque()
    sending: int | None = None
    try:
        queue.extend(browser.start(monotonic_ns()))
        while browser.snapshot().status in ("discovering", "inquiring", "stopping"):
            if cancel is not None and cancel.cancelled:
                browser.stop("cancelled by caller")
                break
            if receiver.dropped_packets:
                browser.stop("input packet loss reported by transport counter")
                break
            if (error := receiver.failure) is not None:
                raise error
            queue.extend(browser.poll(monotonic_ns()))
            pending = browser.snapshot().pending_tokens
            queue = deque(a for a in queue if a.token in pending)
            if queue:
                action = queue.popleft()
                if route.group is None:
                    packets: list[messages.Message] = [action.message]
                else:
                    wire = codecs.encode(action.message)
                    packets = list(
                        ump.packetize_sysex7(group=route.group, data=wire[1:-1])
                    )
                for packet in packets:
                    if receiver.dropped_packets or receiver.failure is not None:
                        raise midi.WorkerError(
                            "input failed or lost packets during management send"
                        )
                    if cancel is not None and cancel.cancelled:
                        raise KeyboardInterrupt
                    sending = action.token
                    output.send(packet)
                queue.extend(
                    browser.report_send(
                        token=action.token, sent=True, now_ns=monotonic_ns()
                    )
                )
                sending = None
                continue
            if browser.snapshot().status not in ("discovering", "inquiring"):
                continue
            item = receiver.receive(timeout=0.05)
            if item is None:
                continue
            if isinstance(item, events.StreamDiagnostic):
                assembler.reset()
                browser.stop(item.reason)
                break
            if item.stream != route.input:
                browser.stop("received event from an unrelated stream")
                break
            if route.group is None:
                if isinstance(item.message, messages.SysEx):
                    queue.extend(
                        browser.feed(
                            item.message, input=route.input, now_ns=monotonic_ns()
                        )
                    )
            elif (
                isinstance(item.message, ump.SysEx7Packet)
                and item.message.group == route.group
            ):
                for observation in assembler.feed(item):
                    value = observation.value
                    if isinstance(value, ump.AssembledData):
                        queue.extend(
                            browser.feed(
                                messages.SysEx(data=value.data),
                                input=route.input,
                                group=route.group,
                                now_ns=monotonic_ns(),
                            )
                        )
                    else:
                        browser.stop("UMP SysEx7 assembly diagnostic: " + str(value))
    except KeyboardInterrupt:
        if sending is not None and sending in browser.snapshot().pending_tokens:
            browser.report_send(token=sending, sent=False, now_ns=monotonic_ns())
            sending = None
        browser.stop("interrupted")
    except (
        midi.MidiError,
        midi.StateError,
        midi.CallbackThreadError,
        midi.NativePanicError,
        midi.ResourceError,
        midi.WorkerError,
        midi.WorkerTimeoutError,
        OSError,
        ValueError,
    ) as error:
        if sending is not None and sending in browser.snapshot().pending_tokens:
            browser.report_send(token=sending, sent=False, now_ns=monotonic_ns())
            sending = None
        browser.stop(str(error))
        raise
    finally:
        if sending is not None and sending in browser.snapshot().pending_tokens:
            browser.report_send(token=sending, sent=False, now_ns=monotonic_ns())
        assembler.reset()
        if browser.snapshot().status in ("discovering", "inquiring", "stopping"):
            browser.stop("scan did not complete")
    return browser.snapshot()
