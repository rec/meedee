"""Production ownership and IPC exercised against controlled child faults."""

from __future__ import annotations

import gc
import inspect
import os
import sys
import warnings
import weakref
from collections.abc import AsyncIterator, Callable, Iterator
from functools import partial
from pathlib import Path
from queue import Full, Queue
from subprocess import Popen
from threading import Event, Thread
from time import monotonic, sleep
from typing import Protocol, Self, cast
from unittest.mock import patch

import pytest

from meedee import gm, messages, midi, mpe
from meedee._native import CallbackBridge
from meedee._transport import (
    RECEIVE_CAPACITY,
    Context,
    Delivery,
    ReceiveQueue,
    shutdown,
)
from meedee._wire import encode
from meedee.events import Clock, ReceivedPacket


@pytest.fixture
def workers(monkeypatch: pytest.MonkeyPatch) -> Iterator[list[Popen[bytes]]]:
    children: list[Popen[bytes]] = []
    fixture = Path(__file__).parent / "native" / "meedee_worker.py"

    def launch(command: list[str], stdin: int, stdout: int) -> Popen[bytes]:
        assert command == [sys.executable, "-m", "meedee._worker"]
        child = Popen([sys.executable, str(fixture)], stdin=stdin, stdout=stdout)
        children.append(child)
        return child

    monkeypatch.setattr("meedee._transport.subprocess.Popen", launch)
    yield children
    for c in children:
        if c.poll() is None:
            c.kill()
        c.wait(timeout=3)


def test_worker_cap_counts_discovery_clients_until_exit_is_confirmed(
    workers: list[Popen[bytes]], monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr("meedee._transport.MAX_WORKERS", 2)
    first = input_client("input", timeout=1)
    second = output_client("healthy", timeout=1)
    assert first.ports() and second.ports()
    with pytest.raises(midi.ResourceError, match="worker limit"):
        input_client("input", timeout=1)
    assert len(workers) == 2

    child = workers[0]
    poll = child.poll
    confirmed = Event()
    monkeypatch.setattr(child, "poll", lambda: poll() if confirmed.is_set() else None)
    del first
    gc.collect()
    child.wait(timeout=3)
    # Even after the client disappears, admission needs confirmed process exit.
    with pytest.raises(midi.ResourceError, match="worker limit"):
        input_client("input", timeout=1)
    confirmed.set()
    replacement = input_client("input", timeout=1)
    assert replacement.ports() and second.ports()
    assert len(workers) == 3
    monkeypatch.setattr(child, "poll", poll)


@pytest.mark.parametrize("kind", [OSError, MemoryError, KeyboardInterrupt])
def test_failed_worker_start_releases_its_reservation(
    kind: type[BaseException],
    workers: list[Popen[bytes]],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr("meedee._transport.MAX_WORKERS", 1)
    with patch("meedee._transport.subprocess.Popen", side_effect=kind("start failed")):
        with pytest.raises(kind):
            input_client("input", timeout=1)
    recovered = input_client("input", timeout=1)
    assert recovered.ports() and len(workers) == 1


def test_worker_cap_reserves_in_progress_starts_without_blocking_admission(
    workers: list[Popen[bytes]], monkeypatch: pytest.MonkeyPatch
) -> None:
    from meedee import _transport

    monkeypatch.setattr(_transport, "MAX_WORKERS", 1)
    entered = Event()
    release = Event()
    created: Queue[midi.Input] = Queue()
    launch = _transport.subprocess.Popen

    def delayed(command: list[str], stdin: int, stdout: int) -> Popen[bytes]:
        entered.set()
        assert release.wait(3)
        return launch(command, stdin=stdin, stdout=stdout)

    def create() -> None:
        created.put(input_client("input", timeout=1))

    monkeypatch.setattr(_transport.subprocess, "Popen", delayed)
    thread = Thread(target=create)
    thread.start()
    try:
        assert entered.wait(3)
        with pytest.raises(midi.ResourceError, match="worker limit"):
            output_client("healthy", timeout=1)
        assert not workers
    finally:
        release.set()
        thread.join(3)
    assert not thread.is_alive()
    assert created.get(timeout=1).ports()
    assert len(workers) == 1


def test_defaults_select_isolation_and_five_second_deadlines() -> None:
    for c in (midi.Input, midi.Output):
        parameters = inspect.signature(c).parameters
        assert parameters["isolated"].default is True
        assert parameters["timeout"].default == 5.0
    assert (
        inspect.signature(midi.Input).parameters["receive_byte_limit"].default
        == 8388608
    )


def test_receive_budget_recovers_after_dequeue_and_rejects_oversized_messages() -> None:
    messages = ReceiveQueue(8)
    messages.put_nowait((0, b"12345"))
    with pytest.raises(Full):
        messages.put_nowait((1, b"1234"))
    assert messages.get_nowait() == (0, b"12345")
    with pytest.raises(Full):
        messages.put_nowait((2, b"123456789"))
    messages.put_nowait((3, b"12345678"))
    assert messages.get_nowait() == (3, b"12345678")


@pytest.mark.parametrize("limit", [0, -1, True, 1.0])
def test_invalid_receive_limits_are_rejected_before_startup(limit: object) -> None:
    with pytest.raises((TypeError, ValueError)):
        input_client("test", receive_byte_limit=limit)


@pytest.mark.parametrize("limit", [2, 3])
def test_receive_byte_limit_is_applied_in_the_child_and_exposes_drops(
    limit: int, workers: list[Popen[bytes]]
) -> None:
    received = Event()
    source = input_client("early", receive_byte_limit=limit)
    with source.connect(source.ports()[0], lambda packet: received.set()) as connection:
        if limit == 3:
            assert received.wait(3)
        assert connection.dropped_packets == (1 if limit == 2 else 0)
        if limit == 2:
            assert not received.is_set()


def test_partial_thread_startup_failure_terminates_and_reaps_its_child(
    workers: list[Popen[bytes]],
) -> None:
    start = Thread.start
    attempts = 0

    def limited_start(thread: Thread) -> None:
        nonlocal attempts
        attempts += 1
        if attempts == 2:
            raise RuntimeError("thread quota reached")
        start(thread)

    with patch.object(Thread, "start", limited_start):
        with pytest.raises(midi.ResourceError, match="thread quota"):
            output_client("output")
    assert len(workers) == 1
    assert workers[0].poll() is not None


@pytest.mark.parametrize("client", [midi.Input, midi.Output])
def test_in_process_mode_does_not_start_child_processes(
    client: type[midi.Input] | type[midi.Output], workers: list[Popen[bytes]]
) -> None:

    class LocalClient:
        def __init__(self, **settings: object) -> None:
            pass

        def ports(self) -> list[object]:
            return []

    with (
        patch("meedee._native.Input", LocalClient),
        patch("meedee._native.Output", LocalClient),
    ):
        local = client(isolated=False)
        assert local.ports() == []
    assert not workers


@pytest.mark.parametrize("mode", ["hang_send", "crash_send", "native_error"])
def test_native_failure_invalidates_only_its_worker_and_requires_a_fresh_client(
    mode: str, workers: list[Popen[bytes]]
) -> None:
    broken = output_client(mode, timeout=1)
    healthy = output_client("healthy")
    bad_connection = broken.connect(broken.ports()[0])
    good_connection = healthy.connect(healthy.ports()[0])
    error = (
        midi.WorkerTimeoutError
        if mode == "hang_send"
        else midi.SendError
        if mode == "native_error"
        else midi.WorkerError
    )
    started = monotonic()
    with pytest.raises(error):
        bad_connection.send(b"\xf8")
    assert monotonic() - started < 3
    assert bad_connection.closed
    assert isinstance(broken.worker_failure, error)
    assert isinstance(bad_connection.worker_failure, error)
    assert broken.worker_failure.__traceback__ is None
    assert good_connection.worker_failure is None
    with pytest.raises(RuntimeError, match="new client"):
        broken.ports()
    with pytest.raises(RuntimeError, match="new client"):
        bad_connection.close()
    good_connection.send(b"\xf8")
    good_connection.close()
    workers[0].wait(timeout=3)
    assert workers[1].poll() is None
    fresh = output_client("fresh")
    with fresh.connect(fresh.ports()[0]) as connection:
        connection.send(b"\xf8")


@pytest.mark.parametrize("isolated", [True, False])
@pytest.mark.parametrize("form", ["bytearray", "view", "strided_view"])
def test_send_snapshots_mutable_and_strided_buffers(
    isolated: bool, form: str, workers: list[Popen[bytes]]
) -> None:
    expected = b"\xf0}\x00\xff\xf7"
    buffer = bytearray(
        expected if form != "strided_view" else b"\xf0a}b\x00c\xffd\xf7e"
    )
    message = (
        buffer
        if form == "bytearray"
        else memoryview(buffer)
        if form == "view"
        else memoryview(buffer)[::2]
    )

    class LocalConnection:
        def send(self, message: bytes) -> None:
            buffer[:] = b"x" * len(buffer)
            assert isinstance(message, bytes)
            assert message == expected

        def close(self) -> None:
            pass

    class LocalClient:
        def __init__(self, **settings: object) -> None:
            pass

        def ports(self) -> list[object]:
            return [local_port()]

        def connect(self, port: object) -> LocalConnection:
            return LocalConnection()

    with patch("meedee._native.Output", LocalClient):
        output = output_client("check_bytes", isolated=isolated)
        connection = output.connect(output.ports()[0])
        connection.send(message)
        connection.close()


def test_native_close_timeout_consumes_parent_ownership_without_restoring_a_client(
    workers: list[Popen[bytes]],
) -> None:
    output = output_client("hang_close", timeout=1)
    connection = output.connect(output.ports()[0])
    with pytest.raises(TimeoutError):
        connection.close()
    assert connection.closed
    workers[0].wait(timeout=3)
    with pytest.raises(RuntimeError, match="new client"):
        output.ports()


@pytest.mark.parametrize("isolated", [True, False])
@pytest.mark.parametrize("direction", ["input", "output"])
@pytest.mark.parametrize("interrupt", [True, False])
def test_context_exit_preserves_body_and_close_failures(
    isolated: bool, direction: str, interrupt: bool, workers: list[Popen[bytes]]
) -> None:

    class LocalConnection:
        closed = False

        def __enter__(self) -> Self:
            return self

        def close(self) -> None:
            raise RuntimeError("close failed")

    class LocalClient:
        def __init__(self, **settings: object) -> None:
            pass

        def ports(self) -> list[object]:
            return [local_port()]

        def connect(
            self,
            port: object,
            callback: Callable[[ReceivedPacket], object] | None = None,
            clock_identity: str = "",
        ) -> LocalConnection:
            return LocalConnection()

    with (
        patch("meedee._native.Input", LocalClient),
        patch("meedee._native.Output", LocalClient),
    ):
        if direction == "input":
            source = input_client("hang_close", isolated=isolated, timeout=0.5)
            connection = source.connect(source.ports()[0], lambda packet: None)
        else:
            output = output_client("hang_close", isolated=isolated, timeout=0.5)
            connection = output.connect(output.ports()[0])
        body_error = (
            KeyboardInterrupt("body interrupted")
            if interrupt
            else ValueError("body failed")
        )
        with pytest.raises(BaseExceptionGroup) as caught:
            with connection:
                raise body_error
        assert caught.value.exceptions[0] is body_error
        assert isinstance(
            caught.value.exceptions[1], TimeoutError if isolated else RuntimeError
        )
        assert isinstance(
            caught.value, BaseExceptionGroup if interrupt else ExceptionGroup
        )


def test_failed_open_discards_the_worker_instead_of_restoring_a_suspect_native_client(
    workers: list[Popen[bytes]],
) -> None:
    output = output_client("failed_open")
    with pytest.raises(midi.ConnectError, match="vanished"):
        output.connect(output.ports()[0])
    assert isinstance(output.worker_failure, midi.ConnectError)
    workers[0].wait(timeout=3)
    with pytest.raises(RuntimeError, match="new client"):
        output.ports()


@pytest.mark.parametrize("form", ["function", "partial", "object", "uninspectable"])
def test_input_callback_can_receive_during_open_without_being_pickled(
    form: str, workers: list[Popen[bytes]]
) -> None:
    messages: Queue[tuple[int, bytes]] = Queue()

    def receive(packet: ReceivedPacket) -> None:
        timestamp = packet.timestamp_ns
        assert timestamp is not None
        message = packet.packet
        assert isinstance(message, bytes)
        messages.put((timestamp, message))

    class Receiver:
        __call__ = staticmethod(receive)

    class UninspectableReceiver(Receiver):
        @property
        def __signature__(self) -> inspect.Signature:
            raise ValueError("No signature available")

    callback = {
        "function": receive,
        "partial": partial(receive),
        "object": Receiver(),
        "uninspectable": UninspectableReceiver(),
    }[form]
    source = input_client("early")
    with source.connect(source.ports()[0], callback) as connection:
        assert messages.get(timeout=3) == (17, b"\x90<\x7f")
        assert connection.dropped_packets == 0
        assert connection.last_message_time is not None


@pytest.mark.parametrize("isolated", [True, False])
@pytest.mark.parametrize("delivered", [True, False])
def test_last_message_time_uses_monotonic_clock_and_survives_close(
    isolated: bool, delivered: bool, workers: list[Popen[bytes]]
) -> None:
    received = Event()

    class LocalConnection:
        def __init__(self, bridge: CallbackBridge) -> None:
            self.bridge = bridge

        @property
        def last_message_time(self) -> float | None:
            return self.bridge.last_message_time

        def close(self) -> None:
            self.bridge.retire()
            self.bridge.drain()

    class LocalClient:
        def __init__(self, **settings: object) -> None:
            pass

        def ports(self) -> list[object]:
            return [local_port()]

        def connect(
            self,
            port: object,
            callback: Callable[[ReceivedPacket], object],
            clock_identity: str,
        ) -> LocalConnection:
            bridge = CallbackBridge(callback)
            if delivered:
                bridge.deliver(packet_value(17, b"\xf8"))
            return LocalConnection(bridge)

    with patch("meedee._native.Input", LocalClient):
        source = input_client("early" if delivered else "quiet", isolated=isolated)
        start = monotonic()
        connection = source.connect(source.ports()[0], lambda packet: received.set())
        if delivered:
            assert received.wait(3)
            assert connection.last_message_time is not None
            assert start <= connection.last_message_time <= monotonic()
        else:
            assert connection.last_message_time is None
        observed = connection.last_message_time
        connection.close()
        assert connection.last_message_time == observed
        assert connection.worker_failure is None


@pytest.mark.parametrize("isolated", [True, False])
def test_incompatible_callback_signatures_do_not_consume_the_client(
    isolated: bool, workers: list[Popen[bytes]]
) -> None:

    def one_argument(timestamp: int, message: bytes) -> None:
        pass

    def extra_argument(timestamp: int, message: bytes, required: int) -> None:
        pass

    def keyword_only(*, timestamp: int, message: bytes) -> None:
        pass

    class LocalClient:
        def __init__(self, **settings: object) -> None:
            pass

        def ports(self) -> list[object]:
            return [local_port()]

    with patch("meedee._native.Input", LocalClient):
        source = input_client("early", isolated=isolated)
        for c in (one_argument, extra_argument, keyword_only):
            with pytest.raises(TypeError, match="one positional ReceivedPacket"):
                source.connect(source.ports()[0], c)  # ty: ignore[invalid-argument-type]
            with pytest.raises(TypeError, match="one positional ReceivedPacket"):
                source.create_virtual("test", c)  # ty: ignore[invalid-argument-type]
            assert source.ports()


@pytest.mark.parametrize("isolated", [True, False])
@pytest.mark.parametrize("operation", ["connect", "create_virtual"])
@pytest.mark.parametrize("kind", ["coroutine", "generator", "async_generator"])
def test_deferred_callbacks_are_rejected_without_consuming_the_client(
    isolated: bool, operation: str, kind: str, workers: list[Popen[bytes]]
) -> None:

    async def coroutine(packet: ReceivedPacket) -> None:
        pass

    def generator(packet: ReceivedPacket) -> Iterator[None]:
        yield None

    async def async_generator(packet: ReceivedPacket) -> AsyncIterator[None]:
        yield None

    callback: Callable[[ReceivedPacket], object] = {
        "coroutine": coroutine,
        "generator": generator,
        "async_generator": async_generator,
    }[kind]

    class Receiver:
        __call__ = staticmethod(callback)

    class LocalClient:
        def __init__(self, **settings: object) -> None:
            pass

        def ports(self) -> list[object]:
            return [local_port()]

    with patch("meedee._native.Input", LocalClient):
        source = input_client("early", isolated=isolated)
        for c in (callback, partial(callback), Receiver(), partial(Receiver())):
            with pytest.raises(TypeError, match="synchronous and not a generator"):
                if operation == "connect":
                    source.connect(source.ports()[0], c)
                else:
                    source.create_virtual("test", c)
            assert source.ports()


def test_parent_queue_drops_new_messages_and_counts_both_queue_stages() -> None:
    started = Event()
    release = Event()
    finished = Event()
    messages: list[int] = []

    def receive(packet: ReceivedPacket) -> None:
        timestamp = packet.timestamp_ns
        assert timestamp is not None
        messages.append(timestamp)
        started.set()
        assert release.wait(3)
        finished.set()

    delivery = Delivery(receive)
    delivery.enqueue(encode(packet_value(0, b"\xf8")), 7)
    assert started.wait(3)
    for i in range(1, RECEIVE_CAPACITY + 4):
        delivery.enqueue(encode(packet_value(i, b"\xf8")), 7)
    assert delivery.dropped == 3
    assert delivery.worker_dropped == 7
    delivery.enqueue(encode(packet_value(RECEIVE_CAPACITY + 4, b"\xf8")), 3)
    assert delivery.worker_dropped == 7
    assert delivery.dropped == 4
    delivery.retire()
    release.set()
    delivery.bridge.drain()
    assert finished.wait(3)
    assert messages == [0]


def test_child_queue_overflow_is_exposed_after_native_close(
    workers: list[Popen[bytes]],
) -> None:
    source = input_client("burst")
    connection = source.connect(source.ports()[0], lambda packet: None)
    connection.close()
    assert connection.dropped_packets > 0


def test_explicit_close_waits_for_parent_callback_without_cancelling_it(
    workers: list[Popen[bytes]],
) -> None:
    started = Event()
    release = Event()
    closing = Event()
    finished: Queue[object] = Queue()

    def receive(packet: ReceivedPacket) -> None:
        started.set()
        assert release.wait(3)

    source = input_client("early", timeout=1)
    connection = source.connect(source.ports()[0], receive)
    assert started.wait(3)

    def close() -> None:
        closing.set()
        connection.close()
        finished.put(None)

    thread = Thread(target=close)
    thread.start()
    assert closing.wait(3)
    try:
        sleep(0.15)
        assert finished.empty()
    finally:
        release.set()
        thread.join(timeout=3)
    assert finished.get(timeout=3) is None
    assert connection.closed


def test_shutdown_terminates_and_reaps_a_child_stuck_in_native_close(
    workers: list[Popen[bytes]],
) -> None:
    output = output_client("hang_close", timeout=1)
    connection = output.connect(output.ports()[0])
    started = monotonic()
    with patch("meedee._transport.SHUTTING_DOWN", False):
        shutdown()
        with pytest.raises(RuntimeError, match="shutdown"):
            output_client("late client")
    assert monotonic() - started < 3
    assert connection.closed
    assert workers[0].poll() is not None


def test_initialization_timeout_reaps_the_new_worker(
    workers: list[Popen[bytes]],
) -> None:
    with pytest.raises(TimeoutError):
        output_client("hang_init", timeout=1)
    workers[0].wait(timeout=3)


@pytest.mark.parametrize("isolated", [False, True])
@pytest.mark.parametrize(
    "kind", ["coroutine", "generator", "async_generator", "awaitable"]
)
def test_deferred_results_are_reported_without_execution_and_delivery_continues(
    isolated: bool, kind: str, workers: list[Popen[bytes]]
) -> None:
    executed = Event()
    delivered = Event()
    reported = Event()
    errors: list[BaseException] = []
    returned: list[object] = []
    messages: list[bytes] = []

    async def coroutine() -> None:
        executed.set()

    def generator() -> Iterator[None]:
        executed.set()
        yield None

    async def async_generator() -> AsyncIterator[None]:
        executed.set()
        yield None

    class Awaitable:
        def __await__(self) -> Iterator[None]:
            executed.set()
            yield None

    factories: dict[str, Callable[[], object]] = {
        "coroutine": coroutine,
        "generator": generator,
        "async_generator": async_generator,
        "awaitable": Awaitable,
    }

    def receive(packet: ReceivedPacket) -> object:
        message = packet.packet
        assert isinstance(message, bytes)
        messages.append(message)
        if len(messages) == 1:
            result = factories[kind]()
            returned.append(result)
            return result
        delivered.set()
        return 37

    def error_hook(event: UnraisableEvent) -> None:
        assert event.object is receive
        error = event.exc_value
        assert isinstance(error, TypeError)
        errors.append(error)
        reported.set()

    class LocalConnection:
        def __init__(self, bridge: CallbackBridge) -> None:
            self.bridge = bridge

        def close(self) -> None:
            self.bridge.retire()
            self.bridge.drain()

    class LocalClient:
        def __init__(self, **settings: object) -> None:
            pass

        def ports(self) -> list[midi.Port]:
            return [local_port()]

        def connect(
            self,
            port: midi.Port,
            callback: Callable[[ReceivedPacket], object],
            clock_identity: str,
        ) -> LocalConnection:
            bridge = CallbackBridge(callback)
            bridge.deliver(packet_value(17, b"\x90<\x7f"))
            bridge.deliver(packet_value(18, b"\xf8"))
            return LocalConnection(bridge)

    with (
        patch("meedee._native.Input", LocalClient),
        patch("sys.unraisablehook", error_hook),
        warnings.catch_warnings(record=True) as warning_records,
    ):
        warnings.simplefilter("always")
        source = input_client("twice", isolated=isolated)
        connection = source.connect(source.ports()[0], receive)
        assert reported.wait(3)
        assert delivered.wait(3)
        connection.close()
        assert messages == [b"\x90<\x7f", b"\xf8"]
        assert len(errors) == 1 and "deferred work" in str(errors[0])
        assert not executed.is_set()
        if inspect.iscoroutine(returned[0]):
            assert inspect.getcoroutinestate(returned[0]) == inspect.CORO_CLOSED
        returned.clear()
        gc.collect()
        assert not warning_records


def test_reporting_a_suspended_coroutine_does_not_cancel_it() -> None:

    class Pause:
        def __await__(self) -> Iterator[None]:
            yield None

    async def coroutine() -> None:
        await Pause()

    result = coroutine()
    result.send(None)
    errors: list[BaseException] = []

    def receive(packet: ReceivedPacket) -> object:
        return result

    def error_hook(event: UnraisableEvent) -> None:
        assert event.object is receive
        error = event.exc_value
        assert isinstance(error, TypeError)
        errors.append(error)

    bridge = CallbackBridge(receive)
    try:
        with patch("sys.unraisablehook", error_hook):
            bridge.deliver(packet_value(1, b"\xf8"))
        assert len(errors) == 1
        assert inspect.getcoroutinestate(result) == inspect.CORO_SUSPENDED
    finally:
        result.close()
        bridge.retire()
        bridge.drain()


def test_callback_and_error_hook_cannot_start_or_send_native_work(
    workers: list[Popen[bytes]],
) -> None:
    reported = Event()
    errors: list[str] = []
    output = output_client("output")
    target = output.connect(output.ports()[0])

    def receive(packet: ReceivedPacket) -> None:
        message = packet.packet
        assert isinstance(message, bytes)
        target.send(message)

    def error_hook(event: UnraisableEvent) -> None:
        assert event.object is receive
        try:
            input_client("forbidden")
        except midi.CallbackThreadError as error:
            errors.append(str(error))
        reported.set()

    source = input_client("early")
    with patch("sys.unraisablehook", error_hook):
        connection = source.connect(source.ports()[0], receive)
        assert reported.wait(3)
        assert connection.last_message_time is not None
        connection.close()
    assert len(errors) == 1
    assert "callback" in errors[0]
    assert len(workers) == 2
    target.send(b"\xf8")
    target.close()


def test_cyclic_collection_releases_the_client_and_child(
    workers: list[Popen[bytes]],
) -> None:
    source = input_client("quiet")

    class Receiver:
        connection: midi.InputConnection

        def __call__(self, packet: ReceivedPacket) -> None:
            pass

    receiver = Receiver()
    connection = source.connect(source.ports()[0], receiver)
    receiver.connection = connection
    reference = weakref.ref(connection)
    del receiver, connection, source
    gc.collect()
    assert reference() is None
    workers[0].wait(timeout=3)


def test_destruction_schedules_close_and_restores_a_still_live_client(
    workers: list[Popen[bytes]],
) -> None:
    output = output_client("output")
    connection = output.connect(output.ports()[0])
    del connection
    deadline = monotonic() + 3
    while True:
        try:
            ports = output.ports()
            break
        except RuntimeError:
            assert monotonic() < deadline
            sleep(0.01)
    assert len(ports) == 1
    assert workers[0].poll() is None


@pytest.mark.parametrize("timeout", [0, -1, float("nan"), float("inf")])
def test_invalid_deadlines_are_rejected_before_starting_a_worker(
    timeout: float, workers: list[Popen[bytes]]
) -> None:
    with pytest.raises(ValueError, match="positive and finite"):
        output_client("invalid", timeout=timeout)
    assert not workers


def input_client(mode: str, **settings: object) -> midi.Input:
    with patch.dict(os.environ, {"MEEDEE_TEST_MODE": mode}):
        return cast(Callable[..., midi.Input], midi.Input)(**settings)


def output_client(mode: str, **settings: object) -> midi.Output:
    with patch.dict(os.environ, {"MEEDEE_TEST_MODE": mode}):
        return cast(Callable[..., midi.Output], midi.Output)(**settings)


def local_port() -> midi.Port:
    return midi.Port._from_wire("local", 1, 0, "local", midi.Backend.CoreMidi, 0, True)


def packet_value(timestamp: int, payload: bytes) -> ReceivedPacket:
    return ReceivedPacket(
        timestamp_ns=timestamp,
        clock=Clock(
            identity="local",
            backend=midi.Backend.CoreMidi,
            mode=midi.TimestampMode.Absolute,
        ),
        packet=payload,
    )


def test_isolated_snapshots_disambiguate_duplicate_ids_and_do_not_rediscover_on_connect(
    workers: list[Popen[bytes]],
) -> None:
    output = output_client("ambiguous")
    first, second = output.ports()
    assert first.backend_id == second.backend_id == 0
    assert first != second
    assert first.name == "output port"
    with output.connect(second) as connection:
        connection.send(b"\xf8")
    with output.connect(first) as connection:
        connection.send(b"\xf8")


@pytest.mark.parametrize("direction", ["input", "output"])
def test_discovery_metadata_failure_preserves_coremidi_worker_for_explicit_retry(
    workers: list[Popen[bytes]], direction: str
) -> None:
    client = (
        input_client("discovery_error_once")
        if direction == "input"
        else output_client("discovery_error_once")
    )
    with pytest.raises(midi.PortInfoError, match="endpoint disappeared"):
        client.ports()
    if sys.platform == "darwin":
        assert client.ports()
        assert client.worker_failure is None
        assert len(workers) == 1
    else:
        with pytest.raises((midi.StateError, midi.WorkerError)):
            client.ports()


@pytest.mark.parametrize("foreign", [True, False])
def test_foreign_or_refreshed_snapshots_fail_open_and_invalidate_the_worker(
    foreign: bool, workers: list[Popen[bytes]]
) -> None:
    output = output_client("output")
    port = output.ports()[0]
    if foreign:
        other = output_client("other")
        port = other.ports()[0]
    else:
        output.ports()
    with pytest.raises(midi.ConnectError, match="expired discovery snapshot"):
        output.connect(port)
    with pytest.raises(midi.StateError):
        output.ports()


@pytest.mark.parametrize(
    "timestamps", [midi.TimestampMode.Absolute, midi.TimestampMode.Off]
)
def test_isolated_ump_preserves_words_clock_identity_and_disabled_time(
    timestamps: midi.TimestampMode, workers: list[Popen[bytes]]
) -> None:
    packets: Queue[ReceivedPacket] = Queue()
    source = input_client("early", protocol=midi.Protocol.Ump, timestamps=timestamps)
    connection = source.connect(source.ports()[0], packets.put_nowait)
    packet = packets.get(timeout=3)
    assert isinstance(packet.packet, midi.UmpPacket)
    assert packet.packet.words == (0x40903C00, 0xC0000000)
    assert packet.timestamp_ns == (
        17 if timestamps == midi.TimestampMode.Absolute else None
    )
    assert packet.clock == connection.clock
    assert packet.clock.backend == midi.Backend.CoreMidiUmp
    old_clock = connection.clock
    connection.close()
    connection = source.connect(source.ports()[0], packets.put_nowait)
    assert connection.clock != old_clock
    connection.close()


class UnraisableEvent(Protocol):
    object: object
    exc_value: BaseException | None


def test_independent_children_have_distinct_clock_identities(
    workers: list[Popen[bytes]],
) -> None:
    first = input_client("early")
    second = input_client("early")
    with (
        first.connect(first.ports()[0], lambda packet: None) as one,
        second.connect(second.ports()[0], lambda packet: None) as two,
    ):
        assert one.clock != two.clock
        assert one.clock.identity != two.clock.identity
    assert len(workers) == 2


@pytest.mark.parametrize("operation", ["open", "send", "close"])
def test_interrupted_worker_request_discards_uncertain_ownership(
    operation: str, workers: list[Popen[bytes]]
) -> None:
    output = output_client("output")
    port = output.ports()[0]
    connection = None if operation == "open" else output.connect(port)
    context = cast(Context, output._context)  # ty: ignore[unresolved-attribute]
    with patch.object(
        context.replies, "get", side_effect=KeyboardInterrupt("interrupted")
    ):
        with pytest.raises(KeyboardInterrupt, match="interrupted"):
            if operation == "open":
                output.connect(port)
            elif operation == "send":
                assert connection is not None
                connection.send(b"\xf8")
            else:
                assert connection is not None
                connection.close()
    assert isinstance(output.worker_failure, midi.WorkerError)
    if connection is not None:
        assert connection.closed
    with pytest.raises(midi.StateError):
        output.ports()
    workers[0].wait(timeout=3)


@pytest.mark.parametrize("input_port", [True, False])
def test_connection_allocation_failure_precedes_remote_open(
    input_port: bool, workers: list[Popen[bytes]]
) -> None:
    if input_port:
        source = input_client("quiet")
        handle = midi.InputConnection
    else:
        source = output_client("quiet")
        handle = midi.OutputConnection
    port = source.ports()[0]
    with patch.object(
        handle, "_create", side_effect=MemoryError("connection allocation")
    ):
        with pytest.raises(MemoryError, match="connection allocation"):
            if isinstance(source, midi.Input):
                source.connect(port, lambda packet: None)
            else:
                source.connect(port)
    assert source.worker_failure is None
    port = source.ports()[0]
    connection = (
        source.connect(port, lambda packet: None)
        if isinstance(source, midi.Input)
        else source.connect(port)
    )
    connection.close()


def test_native_drop_count_arrives_with_packets_without_an_extra_request(
    workers: list[Popen[bytes]],
) -> None:
    received = Event()
    source = input_client("native_drop")
    connection = source.connect(source.ports()[0], lambda packet: received.set())
    assert received.wait(3)
    assert connection.dropped_packets == 7
    connection.close()
    assert connection.dropped_packets == 7


def test_native_drop_count_arrives_while_input_stays_silent(
    workers: list[Popen[bytes]],
) -> None:
    received = Event()
    source = input_client("native_drop_idle")
    connection = source.connect(source.ports()[0], lambda packet: received.set())
    deadline = monotonic() + 3
    while connection.dropped_packets != 7 and monotonic() < deadline:
        sleep(0.01)
    assert connection.dropped_packets == 7
    assert not received.is_set()
    # An idle status must not enter the command-reply queue or poison the worker.
    assert source.worker_failure is None
    connection.close()
    assert connection.dropped_packets == 7


@pytest.mark.parametrize("accepted", range(6))
def test_compound_failure_preserves_progress_through_the_worker_pipe(
    accepted: int, workers: list[Popen[bytes]]
) -> None:
    source = output_client(f"partial_command_{accepted}")
    connection = source.connect(source.ports()[0])
    command = messages.NrpnSet(channel=1, parameter=1234, value=10000, resolution=14)
    with pytest.raises(midi.CompoundSendError, match="device disconnected") as error:
        connection.send_command(command)
    assert isinstance(error.value, midi.SendError)
    assert error.value.completed_messages == accepted
    assert error.value.total_messages == 6
    assert connection.closed
    failure = source.worker_failure
    assert isinstance(failure, midi.CompoundSendError)
    assert (failure.completed_messages, failure.total_messages) == (accepted, 6)
    assert failure.__traceback__ is None
    workers[0].wait(timeout=3)


@pytest.mark.parametrize("mode", ["hang_send", "crash_send"])
def test_compound_worker_loss_reports_unknown_progress(
    mode: str, workers: list[Popen[bytes]]
) -> None:
    source = output_client(mode, timeout=1)
    connection = source.connect(source.ports()[0])
    with pytest.raises(midi.CompoundSendError) as error:
        connection.send_command(
            messages.ControlChange14(channel=1, controller=7, value=10000)
        )
    assert error.value.completed_messages is None
    assert error.value.total_messages == 2
    assert connection.closed
    workers[0].wait(timeout=3)


def test_compound_wire_roundtrip_and_validation_precede_worker_operations(
    workers: list[Popen[bytes]],
) -> None:
    source = output_client("check_command")
    connection = source.connect(source.ports()[0])
    with pytest.raises(TypeError):
        connection.send_command(b"\xb0\x07\x01")  # ty: ignore[invalid-argument-type]
    connection.send_command(
        messages.NrpnSet(channel=1, parameter=1234, value=10000, resolution=14)
    )
    connection.close()
    assert source.worker_failure is None
    ump = output_client("healthy", protocol=midi.Protocol.Ump)
    with ump.connect(ump.ports()[0]) as connection:
        with pytest.raises(midi.CapabilityError):
            connection.send_command(
                messages.ControlChange14(channel=1, controller=7, value=10000)
            )
        assert not connection.closed


def test_worker_failure_after_command_acknowledgment_preserves_complete_progress(
    workers: list[Popen[bytes]],
) -> None:
    source = output_client("healthy")
    connection = source.connect(source.ports()[0])
    context = cast(Context, source._context)  # ty: ignore[unresolved-attribute]

    def acknowledged(*args: object, **kwargs: object) -> tuple[str, tuple[None, int]]:
        context.fail("worker exited after acknowledging command")
        return "ok", (None, 0)

    with patch.object(context.replies, "get", side_effect=acknowledged):
        with pytest.raises(midi.CompoundSendError) as error:
            connection.send_command(
                messages.ControlChange14(channel=1, controller=7, value=10000)
            )
    assert error.value.completed_messages == error.value.total_messages == 2
    assert connection.closed
    workers[0].wait(timeout=3)


def test_failed_clock_allocation_discards_the_open_worker(
    workers: list[Popen[bytes]],
) -> None:
    source = input_client("quiet")
    port = source.ports()[0]
    with patch(
        "meedee._transport.decode_clock", side_effect=MemoryError("clock allocation")
    ):
        with pytest.raises(MemoryError, match="clock allocation"):
            source.connect(port, lambda packet: None)
    assert isinstance(source.worker_failure, midi.WorkerError)
    with pytest.raises(midi.StateError):
        source.ports()
    workers[0].wait(timeout=3)


def test_shutdown_drains_callbacks_even_without_a_live_client() -> None:
    entered = Event()
    release = Event()
    finished = Event()

    def callback(packet: ReceivedPacket) -> None:
        entered.set()
        release.wait(3)

    delivery = Delivery(callback)
    clock = Clock(
        identity="detached",
        backend=midi.Backend.CoreMidi,
        mode=midi.TimestampMode.Absolute,
    )
    delivery.enqueue(
        encode(ReceivedPacket(timestamp_ns=0, clock=clock, packet=b"\xf8")), 0
    )
    assert entered.wait(3)

    def stop() -> None:
        shutdown()
        finished.set()

    with patch("meedee._transport.SHUTTING_DOWN", False):
        closer = Thread(target=stop)
        closer.start()
        try:
            assert not finished.wait(0.05)
        finally:
            release.set()
            closer.join(3)
    assert finished.is_set()


@pytest.mark.parametrize("mode", ["check_bank", "check_mpe", "partial_command_2"])
def test_profile_commands_preserve_types_bytes_and_progress_across_the_worker_pipe(
    mode: str, workers: list[Popen[bytes]]
) -> None:
    source = output_client(mode)
    connection = source.connect(source.ports()[0])
    command = (
        mpe.ZoneConfiguration(manager_channel=16, members=15)
        if mode == "check_mpe"
        else gm.BankProgramSelect(channel=16, bank_msb=121, bank_lsb=3, program=128)
    )
    if mode == "partial_command_2":
        with pytest.raises(midi.CompoundSendError) as error:
            connection.send_command(command)
        assert (error.value.completed_messages, error.value.total_messages) == (2, 3)
        assert connection.closed
    else:
        connection.send_command(command)
        assert not connection.closed
        connection.close()


def test_typed_queue_reports_overflow_and_retains_primitive_events(
    workers: list[Popen[bytes]], monkeypatch: pytest.MonkeyPatch
) -> None:
    from meedee.events import Event

    monkeypatch.setenv("MEEDEE_TEST_MODE", "twice")
    source = midi.TypedInput(timeout=1, event_limit=1)
    with source.connect(source.ports()[0]) as connection:
        deadline = monotonic() + 2
        while connection.failure is None and monotonic() < deadline:
            sleep(0.01)
        assert isinstance(connection.failure, midi.ResourceError)
        value = connection.receive(timeout=0)
        assert isinstance(value, Event)
        assert isinstance(value.message, messages.NoteOn)
        with pytest.raises(midi.ResourceError, match="overflow"):
            connection.receive(timeout=0)


def test_typed_receive_wakes_on_cancellation_and_close(
    workers: list[Popen[bytes]], monkeypatch: pytest.MonkeyPatch
) -> None:
    from meedee.events import Cancellation

    monkeypatch.setenv("MEEDEE_TEST_MODE", "input")
    source = midi.TypedInput(timeout=1)
    connection = source.connect(source.ports()[0])
    cancel = Cancellation()
    errors: Queue[BaseException] = Queue()

    def receive() -> None:
        try:
            connection.receive(cancel=cancel)
        except (InterruptedError, midi.StateError) as error:
            errors.put(error)

    thread = Thread(target=receive)
    thread.start()
    cancel.cancel()
    thread.join(2)
    assert not thread.is_alive() and isinstance(errors.get_nowait(), InterruptedError)
    cancel = Cancellation()
    thread = Thread(target=receive)
    thread.start()
    connection.close()
    thread.join(2)
    assert not thread.is_alive() and isinstance(errors.get_nowait(), midi.StateError)


def test_typed_callback_retains_the_blocking_operation_guard(
    workers: list[Popen[bytes]], monkeypatch: pytest.MonkeyPatch
) -> None:
    from meedee import events

    monkeypatch.setenv("MEEDEE_TEST_MODE", "early")
    called = Event()

    def callback(value: events.Event | events.StreamDiagnostic) -> None:
        with pytest.raises(midi.CallbackThreadError):
            midi.TypedOutput(timeout=1)
        called.set()

    source = midi.TypedInput(timeout=1)
    with source.connect(source.ports()[0], callback):
        assert called.wait(2)
