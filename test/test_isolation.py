"""Shared Rust IPC exercised with a device-free worker and parent callbacks."""

from __future__ import annotations

import gc
import inspect
import os
import subprocess
import sys
import warnings
import weakref
from collections.abc import AsyncIterator, Callable, Iterator
from functools import partial
from pathlib import Path
from queue import Full, Queue
from threading import Event, Thread
from time import monotonic, sleep
from typing import Protocol, cast
from unittest.mock import patch

import pytest

from middy import events, messages, midi, mpe
from middy._clients import Input, Output
from middy._native import CallbackBridge
from middy._transport import ReceiveQueue
from middy.events import Clock, ReceivedPacket


@pytest.fixture(scope="session")
def worker_binary(tmp_path_factory: pytest.TempPathFactory) -> Path:
    project = Path(__file__).resolve().parents[1]
    path = tmp_path_factory.mktemp("rust-worker") / (
        "worker.exe" if sys.platform == "win32" else "worker"
    )
    subprocess.run(
        [
            "rustc",
            "--edition=2021",
            "-O",
            str(project / "crates/middy/tests/support/subprocess_worker.rs"),
            "-o",
            str(path),
        ],
        check=True,
    )
    return path


@pytest.fixture(autouse=True)
def workers(monkeypatch: pytest.MonkeyPatch, worker_binary: Path) -> Iterator[None]:
    monkeypatch.setattr(
        midi,
        "Input",
        partial(
            midi.Input, subprocess_worker="rust", subprocess_executable=worker_binary
        ),
    )
    monkeypatch.setattr(
        midi,
        "Output",
        partial(
            midi.Output, subprocess_worker="rust", subprocess_executable=worker_binary
        ),
    )
    yield None
    gc.collect()


def test_defaults_select_in_process_transport_and_five_second_deadlines() -> None:
    for c in (midi.Input, midi.Output):
        parameters = inspect.signature(c).parameters
        assert parameters["run_in_subprocess"].default is False
        assert "isolated" not in parameters
        assert parameters["timeout"].default == 5.0
    assert (
        inspect.signature(midi.Input).parameters["receive_byte_limit"].default
        == 8388608
    )


def test_receive_budget_recovers_after_dequeue_and_rejects_oversized_messages() -> None:
    messages = ReceiveQueue(8, size_of=lambda v: len(v[-1]))
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


@pytest.mark.parametrize("run_in_subprocess", [True, False])
@pytest.mark.parametrize("form", ["bytearray", "view", "strided_view"])
def test_send_snapshots_mutable_and_strided_buffers(
    run_in_subprocess: bool, form: str, workers: None
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
        worker_failure = None
        clock = Clock(identity="test")
        dropped_packets = 0
        last_message_time = None
        closed = False

        def send(self, message: bytes) -> None:
            buffer[:] = b"x" * len(buffer)
            assert isinstance(message, bytes)
            assert message == expected

        def close(self) -> None:
            pass

    class LocalClient:
        worker_failure = None

        def __init__(self, **settings: object) -> None:
            pass

        def ports(self) -> list[object]:
            return [local_port()]

        def connect(self, port: object) -> LocalConnection:
            return LocalConnection()

    with patch("middy._native.Output", LocalClient):
        output = output_client("check_bytes", run_in_subprocess=run_in_subprocess)
        connection = output.connect(output.ports()[0])
        connection.send(message)
        connection.close()


@pytest.mark.parametrize("form", ["function", "partial", "object", "uninspectable"])
def test_input_callback_can_receive_during_open_without_being_pickled(
    form: str, workers: None
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


@pytest.mark.parametrize("run_in_subprocess", [True, False])
@pytest.mark.parametrize("delivered", [True, False])
def test_last_message_time_uses_monotonic_clock_and_survives_close(
    run_in_subprocess: bool, delivered: bool, workers: None
) -> None:
    received = Event()

    class LocalConnection:
        worker_failure = None
        clock = Clock(identity="test")
        dropped_packets = 0
        closed = False

        def __init__(self, bridge: CallbackBridge) -> None:
            self.bridge = bridge

        @property
        def last_message_time(self) -> float | None:
            return self.bridge.last_message_time

        def close(self) -> None:
            self.bridge.retire()
            self.bridge.drain()

    class LocalClient:
        worker_failure = None

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

    with patch("middy._native.Input", LocalClient):
        source = input_client(
            "early" if delivered else "quiet", run_in_subprocess=run_in_subprocess
        )
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


@pytest.mark.parametrize("run_in_subprocess", [True, False])
def test_incompatible_callback_signatures_do_not_consume_the_client(
    run_in_subprocess: bool, workers: None
) -> None:

    def one_argument(timestamp: int, message: bytes) -> None:
        pass

    def extra_argument(timestamp: int, message: bytes, required: int) -> None:
        pass

    def keyword_only(*, timestamp: int, message: bytes) -> None:
        pass

    class LocalClient:
        worker_failure = None

        def __init__(self, **settings: object) -> None:
            pass

        def ports(self) -> list[object]:
            return [local_port()]

    with patch("middy._native.Input", LocalClient):
        source = input_client("early", run_in_subprocess=run_in_subprocess)
        for c in (one_argument, extra_argument, keyword_only):
            with pytest.raises(TypeError, match="one positional ReceivedPacket"):
                source.connect(source.ports()[0], c)  # ty: ignore[invalid-argument-type]
            with pytest.raises(TypeError, match="one positional ReceivedPacket"):
                source.create_virtual("test", c)  # ty: ignore[invalid-argument-type]
            assert source.ports()


@pytest.mark.parametrize("run_in_subprocess", [True, False])
@pytest.mark.parametrize("operation", ["connect", "create_virtual"])
@pytest.mark.parametrize("kind", ["coroutine", "generator", "async_generator"])
def test_deferred_callbacks_are_rejected_without_consuming_the_client(
    run_in_subprocess: bool, operation: str, kind: str, workers: None
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
        worker_failure = None

        def __init__(self, **settings: object) -> None:
            pass

        def ports(self) -> list[object]:
            return [local_port()]

    with patch("middy._native.Input", LocalClient):
        source = input_client("early", run_in_subprocess=run_in_subprocess)
        for c in (callback, partial(callback), Receiver(), partial(Receiver())):
            with pytest.raises(TypeError, match="synchronous and not a generator"):
                if operation == "connect":
                    source.connect(source.ports()[0], c)
                else:
                    source.create_virtual("test", c)
            assert source.ports()


def test_explicit_close_waits_for_parent_callback_without_cancelling_it(
    workers: None,
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


@pytest.mark.parametrize("run_in_subprocess", [False, True])
@pytest.mark.parametrize(
    "kind", ["coroutine", "generator", "async_generator", "awaitable"]
)
def test_deferred_results_stop_delivery_without_execution(
    run_in_subprocess: bool, kind: str, workers: None
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
        worker_failure = None
        clock = Clock(identity="test")
        dropped_packets = 0
        last_message_time = None
        closed = False

        def __init__(self, bridge: CallbackBridge) -> None:
            self.bridge = bridge

        def close(self) -> None:
            self.bridge.retire()
            self.bridge.drain()

    class LocalClient:
        worker_failure = None

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
        patch("middy._native.Input", LocalClient),
        patch("sys.unraisablehook", error_hook),
        warnings.catch_warnings(record=True) as warning_records,
    ):
        warnings.simplefilter("always")
        source = input_client("twice", run_in_subprocess=run_in_subprocess)
        connection = source.connect(source.ports()[0], receive)
        assert reported.wait(3)
        assert not delivered.is_set()
        assert connection.closed
        assert connection.failure is errors[0]
        connection.close()
        assert messages == [b"\x90<\x7f"]
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


def test_connection_is_the_resource_and_close_is_terminal(
    workers: None,
) -> None:
    output = output_client("output")
    connection = output.connect(output.ports()[0])
    assert connection is output
    del connection
    assert not output.closed
    output.close()
    assert output.closed
    with pytest.raises(midi.StateError):
        output.ports()


@pytest.mark.parametrize("timeout", [0, -1, float("nan"), float("inf")])
def test_invalid_deadlines_are_rejected_before_starting_a_worker(
    timeout: float, workers: None
) -> None:
    with pytest.raises(ValueError, match="timeout"):
        output_client("invalid", timeout=timeout)
    assert not workers


def input_client(mode: str, **settings: object) -> midi.Input[events.ReceivedPacket]:
    """Discover while the child fixture's selected mode is in the environment."""
    with patch.dict(os.environ, {"MIDDY_TEST_MODE": mode}):
        client = cast(Callable[..., Input[events.ReceivedPacket]], midi.Input)(
            **(
                {
                    "raw": True,
                    "run_in_subprocess": True,
                    "backend": midi.Backend.CoreMidiUmp
                    if settings.get("encoding") == midi.Encoding.Ump
                    else midi.Backend.CoreMidi,
                }
                | settings
            )
        )
        client.ports()
        return client


def output_client(mode: str, **settings: object) -> midi.Output:
    """Discover while the child fixture's selected mode is in the environment."""
    with patch.dict(os.environ, {"MIDDY_TEST_MODE": mode}):
        client = cast(Callable[..., Output], midi.Output)(
            **(
                {
                    "run_in_subprocess": True,
                    "backend": midi.Backend.CoreMidiUmp
                    if settings.get("encoding") == midi.Encoding.Ump
                    else midi.Backend.CoreMidi,
                }
                | settings
            )
        )
        client.ports()
        return client


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
    workers: None,
) -> None:
    output = output_client("ambiguous")
    first, second = output.ports()
    assert first.backend_id == second.backend_id == 0
    assert first != second
    assert first.name == "output port"
    with output.connect(second) as connection:
        connection.send(b"\xf8")
    with pytest.raises(midi.StateError, match="closed"):
        output.connect(first)


@pytest.mark.parametrize(
    "timestamps", [midi.TimestampMode.Absolute, midi.TimestampMode.Off]
)
def test_isolated_ump_preserves_words_clock_identity_and_disabled_time(
    timestamps: midi.TimestampMode, workers: None
) -> None:
    packets: Queue[ReceivedPacket] = Queue()
    source = input_client("early", encoding=midi.Encoding.Ump, timestamps=timestamps)
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
    source = input_client("early", encoding=midi.Encoding.Ump, timestamps=timestamps)
    connection = source.connect(source.ports()[0], packets.put_nowait)
    assert connection.clock != old_clock
    connection.close()


class UnraisableEvent(Protocol):
    object: object
    exc_value: BaseException | None


def test_independent_children_have_distinct_clock_identities(
    workers: None,
) -> None:
    first = input_client("early")
    second = input_client("early")
    with (
        first.connect(first.ports()[0], lambda packet: None) as one,
        second.connect(second.ports()[0], lambda packet: None) as two,
    ):
        assert one.clock != two.clock
        assert one.clock.identity != two.clock.identity


def test_native_drop_count_arrives_with_packets_without_an_extra_request(
    workers: None,
) -> None:
    received = Event()
    source = input_client("native_drop")
    connection = source.connect(source.ports()[0], lambda packet: received.set())
    assert received.wait(3)
    assert connection.dropped_packets == 7
    connection.close()
    assert connection.dropped_packets == 7


def test_native_drop_count_arrives_while_input_stays_silent(
    workers: None,
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
    accepted: int, workers: None
) -> None:
    source = output_client(f"partial_command_{accepted}")
    connection = source.connect(source.ports()[0])
    command = messages.ParameterSet(
        family="nrpn", channel=1, parameter=1234, value=10000, resolution=14
    )
    with pytest.raises(midi.CompoundSendError, match="device disconnected") as error:
        connection.send(command)
    assert isinstance(error.value, midi.SendError)
    assert error.value.completed_messages == accepted
    assert error.value.total_messages == 6
    assert connection.closed
    failure = source.worker_failure
    assert isinstance(failure, midi.CompoundSendError)
    assert (failure.completed_messages, failure.total_messages) == (accepted, 6)
    assert failure.__traceback__ is None


@pytest.mark.parametrize("mode", ["hang_send", "crash_send"])
def test_compound_worker_loss_reports_unknown_progress(
    mode: str, workers: None
) -> None:
    source = output_client(mode, timeout=1)
    connection = source.connect(source.ports()[0])
    with pytest.raises(midi.CompoundSendError) as error:
        connection.send(
            messages.ControlChange(resolution=14, channel=1, controller=7, value=10000)
        )
    assert error.value.completed_messages is None
    assert error.value.total_messages == 2
    assert connection.closed


def test_compound_wire_roundtrip_and_validation_precede_worker_operations(
    workers: None,
) -> None:
    source = output_client("check_command")
    connection = source.connect(source.ports()[0])
    with pytest.raises(TypeError):
        connection.send(object())  # ty: ignore[invalid-argument-type]
    connection.send(
        messages.ParameterSet(
            family="nrpn", channel=1, parameter=1234, value=10000, resolution=14
        )
    )
    connection.close()
    assert source.worker_failure is None
    ump = output_client("healthy", encoding=midi.Encoding.Ump)
    with ump.connect(ump.ports()[0]) as connection:
        with pytest.raises(ValueError, match="group"):
            connection.send(
                messages.ControlChange(
                    resolution=14, channel=1, controller=7, value=10000
                )
            )
        assert not connection.closed


@pytest.mark.parametrize("mode", ["check_bank", "check_mpe", "partial_command_2"])
def test_profile_commands_preserve_types_bytes_and_progress_across_the_worker_pipe(
    mode: str, workers: None
) -> None:
    source = output_client(mode)
    connection = source.connect(source.ports()[0])
    command = (
        mpe.ZoneConfiguration(manager_channel=16, members=15)
        if mode == "check_mpe"
        else messages.ProgramChange(channel=16, bank_msb=121, bank_lsb=3, program=128)
    )
    if mode == "partial_command_2":
        with pytest.raises(midi.CompoundSendError) as error:
            connection.send(command)
        assert (error.value.completed_messages, error.value.total_messages) == (2, 3)
        assert connection.closed
    else:
        connection.send(command)
        assert not connection.closed
        connection.close()


def test_typed_input_exposes_receive_drops_without_another_packet(
    workers: None, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("MIDDY_TEST_MODE", "native_drop_idle")
    source = midi.Input(run_in_subprocess=True, timeout=1)
    with source.connect(source.ports()[0]) as connection:
        deadline = monotonic() + 3
        while connection.dropped_packets != 7 and monotonic() < deadline:
            sleep(0.01)
        assert connection.dropped_packets == 7
        assert connection.failure is None
        assert connection.receive(timeout=0) is None
        with pytest.raises(ValueError, match="frozen"):
            connection.dropped_packets = 0  # ty: ignore[invalid-assignment]
    assert connection.dropped_packets == 7


def test_typed_queue_reports_overflow_and_retains_primitive_events(
    workers: None, monkeypatch: pytest.MonkeyPatch
) -> None:

    monkeypatch.setenv("MIDDY_TEST_MODE", "twice")
    source = midi.Input(run_in_subprocess=True, timeout=1, event_limit=1)
    with source.connect(source.ports()[0]) as connection:
        deadline = monotonic() + 2
        while connection.failure is None and monotonic() < deadline:
            sleep(0.01)
        assert isinstance(connection.failure, midi.ResourceError)
        value = connection.receive(timeout=0)
        assert isinstance(value, messages.Message)
        assert isinstance(value, messages.NoteOn)
        with pytest.raises(midi.ResourceError, match="overflow"):
            connection.receive(timeout=0)


def test_typed_receive_wakes_on_cancellation_and_close(
    workers: None, monkeypatch: pytest.MonkeyPatch
) -> None:
    from middy.events import Cancellation

    monkeypatch.setenv("MIDDY_TEST_MODE", "input")
    source = midi.Input(run_in_subprocess=True, timeout=1)
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
    workers: None, monkeypatch: pytest.MonkeyPatch
) -> None:

    monkeypatch.setenv("MIDDY_TEST_MODE", "early")
    called = Event()

    def callback(value: messages.Message | events.StreamDiagnostic) -> None:
        with pytest.raises(midi.CallbackThreadError):
            midi.Output(run_in_subprocess=True, timeout=1)
        called.set()

    source = midi.Input(run_in_subprocess=True, timeout=1)
    with source.connect(source.ports()[0], callback):
        assert called.wait(2)


def test_output_mixes_packets_messages_commands_and_playback_on_one_connection(
    workers: None,
) -> None:
    client = output_client("check_note", timeout="5 s")
    note = messages.NoteOn(channel=1, note=60, velocity=100)
    clock = events.Clock(identity="output-test")
    origin = events.Timestamp(ticks=0, clock=clock)
    with client.connect(client.ports()[0]) as output:
        output.send(b"\x90\x3c\x64")
        output.send(note)
        output.send(
            messages.ControlChange(resolution=14, channel=1, controller=7, value=128)
        )
        report = output.send(
            [
                (note).replace(stream="test", timestamp=origin),
                (
                    messages.ControlChange(
                        resolution=14, channel=1, controller=7, value=128
                    )
                ).replace(stream="test", timestamp=origin),
            ],
            origin=origin,
            metrics=True,
        )
        assert report.sent_events == 2 and not report.cancelled
        assert report.lateness is not None and report.lateness.count == 2
        with pytest.raises(midi.CapabilityError, match="encodings differ"):
            output.send(note.replace(group=1))
        assert not output.closed and output.worker_failure is None
    assert output.closed


def test_python_bootstrap_runs_the_rust_worker_without_native_initialization() -> None:
    def string(value: str) -> bytes:
        data = value.encode()
        return b"\x03" + len(data).to_bytes(8) + data

    magic = b"MDWORK\x00\x01"
    request = b"\x06" + (2).to_bytes(8) + string("shutdown") + b"\x05" + bytes(8)
    with subprocess.Popen(
        [sys.executable, "-m", "middy._worker"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    ) as process:
        assert process.stdin is not None
        request_pipe = process.stdin
        request_pipe.write(magic + request)
        request_pipe.flush()
        process.stdin = None
        try:
            output, errors = process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.communicate()
            raise
        finally:
            request_pipe.close()
        assert process.returncode == 0, errors
    expected = (
        magic
        + b"\x06"
        + (2).to_bytes(8)
        + string("shutdown")
        + b"\x06"
        + (2).to_bytes(8)
        + b"\x00\x02"
        + bytes(16)
    )
    assert output == expected


@pytest.mark.parametrize("worker", ["python", "rust"])
@pytest.mark.parametrize("direction", ["Input", "Output"])
def test_launcher_selection_is_lazy_and_reaches_the_shared_rust_binding(
    worker: str, direction: str, monkeypatch: pytest.MonkeyPatch
) -> None:
    from middy import _native

    seen: dict[str, object] = {}

    class NativeClient:
        worker_failure = None

        def __init__(self, **settings: object) -> None:
            seen.update(settings)

        def ports(self) -> list[midi.Port]:
            return []

    monkeypatch.setattr(_native, direction, NativeClient)
    client = getattr(midi, direction)(
        run_in_subprocess=True,
        subprocess_worker=worker,
        subprocess_executable="chosen-worker",
    )
    assert not seen
    assert client.ports() == []
    assert seen["worker_program"] == "chosen-worker"
    assert seen["worker_args"] == (
        ["-m", "middy._worker"] if worker == "python" else []
    )
    assert seen["worker_timeout"] == 5
    client.close()


@pytest.mark.parametrize(
    "mode,operation,error",
    [
        ("hang_init", "ports", midi.WorkerTimeoutError),
        ("hang_send", "send", midi.WorkerTimeoutError),
        ("crash_send", "send", midi.WorkerError),
        ("native_error", "send", midi.SendError),
        ("hang_close", "close", midi.WorkerTimeoutError),
    ],
)
def test_terminal_subprocess_failures_preserve_categories_and_prevent_reuse(
    mode: str, operation: str, error: type[Exception], workers: None
) -> None:
    started = monotonic()
    if operation == "ports":
        with pytest.raises(error):
            output_client(mode, timeout=1)
    else:
        client = output_client(mode, timeout=1)
        client.connect(client.ports()[0])
        with pytest.raises(error):
            client.send(b"\xf8") if operation == "send" else client.close()
        assert client.closed
        assert isinstance(client.worker_failure, error)
        with pytest.raises(midi.StateError):
            client.ports()
    assert monotonic() - started < 5


@pytest.mark.parametrize(
    "mode,timestamp", [("early_negative", -(2**63)), ("early_absent", None)]
)
def test_subprocess_preserves_negative_and_missing_timestamps(
    mode: str, timestamp: int | None, workers: None
) -> None:
    received: Queue[ReceivedPacket] = Queue()
    source = input_client(mode)
    with source.connect(source.ports()[0], received.put):
        assert received.get(timeout=2).timestamp_ns == timestamp


@pytest.mark.parametrize("input_mode", [True, False])
@pytest.mark.parametrize("connected", [True, False])
def test_subprocess_clients_leaving_scope_remain_collectible(
    input_mode: bool, connected: bool, workers: None
) -> None:
    source = input_client("quiet") if input_mode else output_client("healthy")
    if connected:
        if input_mode:
            source.connect(source.ports()[0], lambda packet: None)
        else:
            source.connect(source.ports()[0])
    reference = weakref.ref(source)
    del source
    for _ in range(10):
        gc.collect()
        if reference() is None:
            break
        sleep(0.01)
    assert reference() is None


@pytest.mark.parametrize("foreign", [True, False])
def test_expired_or_foreign_port_snapshots_fail_terminally_before_remote_open(
    foreign: bool, workers: None
) -> None:
    first = output_client("healthy")
    port = first.ports()[0]
    second = output_client("healthy") if foreign else first
    if not foreign:
        second.ports()
    with pytest.raises(midi.ConnectError, match="snapshot"):
        second.connect(port)
    assert second.closed
    assert isinstance(second.worker_failure, midi.ConnectError)
    with pytest.raises(midi.StateError):
        second.ports()


@pytest.mark.parametrize(
    "payload",
    [
        b"\xf0}\x00\xff\xf7",
        bytearray(b"\xf0}\x00\xff\xf7"),
        memoryview(b"\xf0a}b\x00c\xffd\xf7e")[::2],
    ],
)
def test_worker_preserves_bytes_and_strided_buffers(
    payload: object, workers: None
) -> None:
    source = output_client("check_bytes")
    with source.connect(source.ports()[0]):
        source.send(payload)


def test_worker_preserves_all_ump_word_bits_on_send(workers: None) -> None:
    source = output_client("check_ump", encoding=midi.Encoding.Ump)
    with source.connect(source.ports()[0]):
        source.send(midi.UmpPacket(words=[0x40903C00, 0xFFFFFFFF]))
