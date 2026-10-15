"""Private, deadline-bounded local pipes and parent callback delivery."""

from __future__ import annotations

import atexit
import pickle
import subprocess
import sys
import weakref
from collections.abc import Callable
from queue import Empty, Full, Queue
from threading import Event, Lock, Thread
from time import monotonic
from typing import BinaryIO, NoReturn, TypeVar, cast

from . import _native
from ._errors import CompoundSendError
from ._wire import decode, decode_clock, payload_size
from .events import Clock, ReceivedPacket

DEFAULT_RECEIVE_BYTE_LIMIT = 8 * 1024 * 1024


T = TypeVar("T")


class ReceiveQueue(Queue[T]):
    """Bound queued message payloads under the queue's existing mutex."""

    def __init__(
        self,
        byte_limit: int,
        *,
        capacity: int | None = None,
        size_of: Callable[[T], int] | None = None,
    ) -> None:
        super().__init__(RECEIVE_CAPACITY if capacity is None else capacity)
        self.size_of = size_of
        self.byte_limit = byte_limit
        self.payload_bytes = 0

    def size(self, item: T) -> int:
        return (
            payload_size(cast(tuple[object, ...], item))
            if self.size_of is None
            else self.size_of(item)
        )

    def _put(self, item: T) -> None:
        if self.payload_bytes + self.size(item) > self.byte_limit:
            raise Full
        super()._put(item)
        self.payload_bytes += self.size(item)

    def _get(self) -> T:
        item = super()._get()
        self.payload_bytes -= self.size(item)
        return item


class Delivery:
    def __init__(
        self,
        callback: Callable[[ReceivedPacket], object],
        byte_limit: int = DEFAULT_RECEIVE_BYTE_LIMIT,
    ) -> None:
        self.bridge = _native.CallbackBridge(callback)
        self.clock: Clock | None = None
        self.messages = ReceiveQueue[tuple[object, ...]](byte_limit)
        self.retired = False
        self.dropped = 0
        self.worker_dropped = 0
        with REGISTRY_LOCK:
            if SHUTTING_DOWN:
                raise _native.StateError("MIDI interpreter shutdown has begun")
            DELIVERIES.add(self)
        self.thread = Thread(
            target=dispatch,
            args=(weakref.ref(self), self.messages),
            name="meedee callback",
            daemon=True,
        )
        self.thread.start()

    def enqueue(self, packet: tuple[object, ...], dropped: int) -> None:
        self.worker_dropped = max(self.worker_dropped, dropped)
        if not self.retired:
            try:
                self.messages.put_nowait(packet)
            except Full:
                self.dropped += 1

    def retire(self) -> None:
        self.retired = True
        self.bridge.retire()
        while True:
            try:
                self.messages.get_nowait()
            except Empty:
                break
            self.dropped += 1

    def drain(self) -> None:
        self.bridge.drain()
        self.thread.join()

    def __del__(self) -> None:
        if hasattr(self, "bridge"):
            self.retire()


class Context:
    def __init__(
        self, timeout: float, receive_byte_limit: int = DEFAULT_RECEIVE_BYTE_LIMIT
    ) -> None:
        if SHUTTING_DOWN:
            raise _native.StateError("MIDI interpreter shutdown has begun")
        self.timeout = timeout
        self.receive_byte_limit = receive_byte_limit
        self.operation = Lock()
        self.requests: Queue[tuple[str, list[object]]] = Queue(1)
        self.replies: Queue[tuple[str, object]] = Queue(1)
        self.stopped = Event()
        self.failure = ""
        self.failure_exception: Exception | None = None
        self.state = "available"
        self.delivery: weakref.ReferenceType[Delivery] | None = None
        self.generation = 0
        self.worker_dropped = 0
        self.disposing = False
        self.process = start_worker()
        try:
            with REGISTRY_LOCK:
                if SHUTTING_DOWN:
                    raise _native.StateError("MIDI interpreter shutdown has begun")
                CONTEXTS.add(self)
            Thread(
                target=write_requests, args=(self,), daemon=True, name="meedee requests"
            ).start()
            Thread(
                target=read_results, args=(self,), daemon=True, name="meedee results"
            ).start()
        except (RuntimeError, MemoryError, _native.StateError) as error:
            self.fail("Cannot start MIDI communication threads", error)
            self.process.wait(timeout=1)
            cast(BinaryIO, self.process.stdin).close()
            cast(BinaryIO, self.process.stdout).close()
            if isinstance(error, RuntimeError) and not isinstance(
                error, _native.StateError
            ):
                raise _native.ResourceError(str(error)) from error
            raise

    def request(
        self, operation: str, args: list[object], *, serial: bool = False
    ) -> object:
        _native.check_thread()
        deadline = monotonic() + self.timeout
        acquired = (
            self.operation.acquire(timeout=self.timeout)
            if serial
            else self.operation.acquire(blocking=False)
        )
        if not acquired:
            if serial:
                self.expire(operation)
            raise _native.StateError("MIDI client is busy with another operation")
        try:
            return self.exchange(operation, args, deadline)
        except (KeyboardInterrupt, SystemExit):
            self.fail("MIDI worker request interrupted; create a new client")
            raise
        finally:
            self.operation.release()

    def exchange(self, operation: str, args: list[object], deadline: float) -> object:
        if self.failure:
            raise _native.StateError(
                f"MIDI worker failed; create a new client: {self.failure}"
            )
        if operation == "ports" and self.state != "available":
            raise _native.StateError(
                "MIDI client is unavailable while connecting, connected, or closing"
            )
        if operation in ("send", "send_command") and (
            args[1] != self.generation or self.state != "connected"
        ):
            raise _native.StateError("MIDI connection is closed")
        self.requests.put_nowait((operation, args))
        try:
            tag, payload = self.replies.get(timeout=max(0, deadline - monotonic()))
        except Empty:
            self.expire(operation)
        if tag == "dead":
            raise _native.WorkerError(
                f"MIDI worker failed; create a new client: {self.failure}"
            )
        if tag == "error":
            kind, arguments, fatal, dropped = cast(
                tuple[str, tuple[object, ...], bool, int], payload
            )
            self.update_dropped(dropped)
            error = ERROR_TYPES[kind](*arguments)
            if fatal:
                self.fail(str(error), error)
            raise error
        result, dropped = cast(tuple[object, int], payload)
        self.update_dropped(dropped)
        if self.failure and operation != "shutdown":
            if operation == "send_command":
                # A successful reply proves every step was accepted, even if
                # worker death was observed before this thread resumed.
                total = cast(tuple[str, dict[str, int], int], args[0])[2]
                raise CompoundSendError(
                    self.failure, total, total
                ) from self.failure_exception
            raise _native.StateError(
                f"MIDI worker failed; create a new client: {self.failure}"
            )
        return result

    def close(self, generation: int) -> None:
        _native.check_thread()
        deadline = monotonic() + self.timeout
        if not self.operation.acquire(timeout=self.timeout):
            self.expire("close")
        try:
            if generation == self.generation:
                if self.failure:
                    raise _native.StateError(
                        f"MIDI worker failed; create a new client: {self.failure}"
                    )
                self.state = "closing"
                self.exchange("close", [generation], deadline)
                self.state = "available"
        except (KeyboardInterrupt, SystemExit):
            self.fail("MIDI close interrupted; create a new client")
            raise
        finally:
            self.operation.release()

    def open(
        self,
        operation: str,
        identifier: tuple[object, ...] | None,
        name: str,
        callback: Callable[[ReceivedPacket], object] | None,
        clock_identity: str,
    ) -> Delivery | None:
        _native.check_thread()
        if not self.operation.acquire(blocking=False):
            raise _native.StateError("MIDI client is busy with another operation")
        delivery = None
        entered = False
        try:
            if self.failure:
                raise _native.StateError(
                    f"MIDI worker failed; create a new client: {self.failure}"
                )
            if self.state != "available":
                raise _native.StateError(
                    "MIDI client is unavailable while connecting, connected, or closing"
                )
            self.state = "opening"
            self.generation += 1
            self.worker_dropped = 0
            delivery = (
                Delivery(callback, self.receive_byte_limit)
                if callback is not None
                else None
            )
            self.delivery = weakref.ref(delivery) if delivery is not None else None
            entered = True
            clock_packet = self.exchange(
                operation,
                [
                    identifier,
                    name,
                    self.generation,
                    self.receive_byte_limit,
                    clock_identity,
                ],
                monotonic() + self.timeout,
            )
            if delivery is not None:
                delivery.clock = decode_clock(cast(tuple[object, ...], clock_packet))
            self.state = "connected"
            return delivery
        except (KeyboardInterrupt, SystemExit):
            if delivery is not None:
                delivery.retire()
            self.fail("MIDI open interrupted; create a new client")
            raise
        except (
            RuntimeError,
            TimeoutError,
            _native.MidiError,
            NotImplementedError,
            TypeError,
            ValueError,
            MemoryError,
        ):
            if delivery is not None:
                delivery.retire()
            if not self.failure:
                if entered:
                    self.fail("MIDI open did not complete; create a new client")
                else:
                    self.state = "available"
            raise
        finally:
            self.operation.release()

    def update_dropped(self, dropped: int) -> None:
        self.worker_dropped = max(self.worker_dropped, dropped)
        if self.delivery is not None and (delivery := self.delivery()) is not None:
            delivery.worker_dropped = max(delivery.worker_dropped, dropped)

    def expire(self, operation: str) -> NoReturn:
        message = f"MIDI {operation} exceeded {self.timeout:g}s; create a new client"
        error = _native.WorkerTimeoutError(message)
        self.fail(message, error)
        raise error

    def fail(self, message: str, error: Exception | None = None) -> None:
        if not self.failure:
            self.failure = message
            # Keep the diagnosis without retaining operation frames or user data.
            self.failure_exception = (
                _native.WorkerError(message)
                if error is None
                else type(error)(*error.args)
            )
            self.state = "failed"
            self.stopped.set()
            if self.delivery is not None and (delivery := self.delivery()) is not None:
                delivery.retire()
            try:
                self.process.kill()
            except ProcessLookupError:
                pass
            except OSError as error:
                print(f"meedee: cannot terminate worker: {error}", file=sys.stderr)
            try:
                self.replies.put_nowait(("dead", None))
            except Full:
                pass

    def defer_close(self, generation: int) -> None:
        if self.state == "connected" and generation == self.generation:
            try:
                Thread(
                    target=close_deferred,
                    args=(self, generation),
                    daemon=True,
                    name="meedee cleanup",
                ).start()
            except RuntimeError:
                self.fail("Cannot start deferred MIDI close")

    def dispose(self) -> None:
        if not self.disposing and not self.stopped.is_set():
            self.disposing = True
            try:
                Thread(
                    target=dispose, args=(self,), daemon=True, name="meedee disposal"
                ).start()
            except RuntimeError:
                self.fail("Cannot start MIDI worker disposal")


def start_worker() -> subprocess.Popen[bytes]:
    """Reserve startup and retain retiring processes until exit is confirmed."""
    with REGISTRY_LOCK:
        if SHUTTING_DOWN:
            raise _native.StateError("MIDI interpreter shutdown has begun")
        for slot in range(MAX_WORKERS):
            process = WORKERS[slot]
            if not STARTING_WORKERS[slot] and (
                process is None or process.poll() is not None
            ):
                STARTING_WORKERS[slot] = True
                break
        else:
            raise _native.ResourceError(
                f"Isolated MIDI worker limit reached ({MAX_WORKERS} per parent process)"
            )
    process = None
    try:
        process = subprocess.Popen(
            [sys.executable, "-m", "meedee._worker"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
        )
        return process
    finally:
        with REGISTRY_LOCK:
            # The slot already exists: publishing a child needs no new set entry
            # or list growth after the OS process has been created.
            WORKERS[slot] = process
            STARTING_WORKERS[slot] = False


def dispatch(
    reference: weakref.ReferenceType[Delivery], messages: Queue[tuple[object, ...]]
) -> None:
    while True:
        try:
            packet = messages.get(timeout=0.1)
        except Empty:
            delivery = reference()
            if delivery is None or delivery.retired:
                return
            del delivery
            continue
        delivery = reference()
        if delivery is None:
            return
        if delivery.retired:
            delivery.dropped += 1
            return
        delivery.bridge.deliver(decode(packet))
        del delivery


def write_requests(context: Context) -> None:
    stream = cast(BinaryIO, context.process.stdin)
    try:
        while not context.stopped.is_set():
            try:
                request = context.requests.get(timeout=0.1)
            except Empty:
                continue
            pickle.dump(request, stream, protocol=5)
            stream.flush()
    except (OSError, ValueError) as error:
        context.fail(f"MIDI worker pipe failed: {error}")
    finally:
        try:
            stream.close()
        except OSError:
            pass


def read_results(context: Context) -> None:
    stream = cast(BinaryIO, context.process.stdout)
    try:
        while not context.stopped.is_set():
            tag, payload = cast(tuple[str, object], pickle.load(stream))
            if tag == "message":
                generation, packet, dropped = cast(
                    tuple[int, tuple[object, ...], int], payload
                )
                if generation == context.generation:
                    context.update_dropped(dropped)
                    if context.delivery is not None:
                        offer(context.delivery, packet, dropped)
            else:
                context.replies.put_nowait((tag, payload))
                if tag == "error":
                    kind, arguments, fatal, dropped = cast(
                        tuple[str, tuple[object, ...], bool, int], payload
                    )
                    if fatal:
                        # Preserve the reported cause before observing pipe EOF.
                        error = ERROR_TYPES[kind](*arguments)
                        context.fail(str(error), error)
                if tag == "shutdown":
                    context.failure = "MIDI worker shut down"
                    context.state = "failed"
                    context.stopped.set()
                    break
    except (EOFError, OSError, ValueError, pickle.UnpicklingError, Full) as error:
        context.fail(f"MIDI worker exited or communication failed: {error}")
    finally:
        stream.close()
        try:
            context.process.wait(timeout=1)
        except subprocess.TimeoutExpired:
            context.fail("MIDI worker did not exit after termination")


def offer(
    reference: weakref.ReferenceType[Delivery],
    packet: tuple[object, ...],
    dropped: int,
) -> None:
    if (delivery := reference()) is not None:
        delivery.enqueue(packet, dropped)


def close_deferred(context: Context, generation: int) -> None:
    try:
        context.close(generation)
    except (RuntimeError, TimeoutError, _native.MidiError):
        # Deferred errors already invalidate the worker; no unraisable traceback.
        return


def dispose(context: Context) -> None:
    try:
        context.request("shutdown", [], serial=True)
    except (RuntimeError, TimeoutError, _native.MidiError):
        return


def shutdown() -> None:
    global SHUTTING_DOWN
    with REGISTRY_LOCK:
        SHUTTING_DOWN = True
        contexts = list(CONTEXTS)
        deliveries = list(DELIVERIES)
    for d in deliveries:
        d.retire()
    deadline = monotonic() + max(
        (c.timeout for c in contexts if not c.stopped.is_set()), default=0
    )
    for c in contexts:
        if not c.stopped.is_set():
            try:
                c.requests.put_nowait(("shutdown", []))
            except Full:
                c.fail("MIDI shutdown raced with an active request")
    for c in contexts:
        try:
            c.process.wait(timeout=max(0, deadline - monotonic()))
        except subprocess.TimeoutExpired:
            c.fail("MIDI shutdown deadline exceeded")
    reap_deadline = monotonic() + 1
    for c in contexts:
        try:
            c.process.wait(timeout=max(0, reap_deadline - monotonic()))
        except subprocess.TimeoutExpired:
            print("meedee: worker termination could not be confirmed", file=sys.stderr)
    for d in deliveries:
        # Native children are stopped first. Arbitrary Python cannot be cancelled.
        d.bridge.drain()


RECEIVE_CAPACITY = 128
ERROR_TYPES: dict[str, type[Exception]] = {
    "MidiError": _native.MidiError,
    "InitError": _native.InitError,
    "CapabilityError": _native.CapabilityError,
    "PortInfoError": _native.PortInfoError,
    "ConnectError": _native.ConnectError,
    "SendError": _native.SendError,
    "CompoundSendError": CompoundSendError,
    "StateError": _native.StateError,
    "CallbackThreadError": _native.CallbackThreadError,
    "NativePanicError": _native.NativePanicError,
    "ResourceError": _native.ResourceError,
    "WorkerError": _native.WorkerError,
    "WorkerTimeoutError": _native.WorkerTimeoutError,
    "RuntimeError": RuntimeError,
    "TypeError": TypeError,
    "ValueError": ValueError,
    "NotImplementedError": NotImplementedError,
}
CONTEXTS: weakref.WeakSet[Context] = weakref.WeakSet()
MAX_WORKERS = 32
STARTING_WORKERS: list[bool] = [False] * MAX_WORKERS
WORKERS: list[subprocess.Popen[bytes] | None] = [None] * MAX_WORKERS
DELIVERIES: weakref.WeakSet[Delivery] = weakref.WeakSet()
REGISTRY_LOCK = Lock()
SHUTTING_DOWN = False
atexit.register(shutdown)
