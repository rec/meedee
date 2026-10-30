"""Opt-in UMP CI exchange between two exclusively test-owned virtual endpoints."""

from secrets import randbelow
from time import monotonic_ns
from uuid import uuid4

from meedee import ci, codecs, events, messages, midi, ump


def dispatch(
    session: ci.Session, action: ci.Action, output: midi.OutputConnection
) -> None:
    assert action.route.group is not None
    wire = codecs.encode(action.message)
    try:
        for p in ump.packetize_sysex7(group=action.route.group, data=wire[1:-1]):
            output.send_message(p)
    except midi.MidiError:
        session.report_send(token=action.token, sent=False, now_ns=monotonic_ns())
        raise
    session.report_send(token=action.token, sent=True, now_ns=monotonic_ns())


def receive(
    receiver: midi.TypedInputConnection, assembler: ump.EventAssembler
) -> messages.SysEx:
    while True:
        value = receiver.receive(timeout=3)
        assert isinstance(value, events.Event), value
        for o in assembler.feed(value):
            data = o.value
            if isinstance(data, ump.AssembledData):
                assert data.group == 1 and data.kind == ump.DataKind.SysEx7
                return messages.SysEx(data=data.data)


def connect(name: str) -> midi.OutputConnection:
    client = midi.Output(protocol=midi.Protocol.Ump)
    matches = [p for p in client.ports() if p.name == name]
    assert len(matches) == 1, "test-owned destination was not discovered uniquely"
    return client.connect(matches[0])


def main() -> None:
    suffix = uuid4().hex
    names = [f"meedee-ci-test-{suffix}-{n}" for n in (1, 2)]
    first = randbelow(ci.MAX_MUID + 1)
    second = randbelow(ci.MAX_MUID + 1)
    while second == first:
        second = randbelow(ci.MAX_MUID + 1)
    identity = ci.Identity(
        manufacturer=b"\x7d\x00\x00", family=1, model=1, revision=b"\x00" * 4
    )
    with (
        midi.TypedInput(protocol=midi.Protocol.Ump).create_virtual(names[0]) as a,
        midi.TypedInput(protocol=midi.Protocol.Ump).create_virtual(names[1]) as b,
        connect(names[1]) as a_out,
        connect(names[0]) as b_out,
    ):
        a_session = ci.Session(
            local_muid=first,
            identity=identity,
            route=ci.Route(input=a.stream, output=names[1], group=1),
        )
        b_session = ci.Session(
            local_muid=second,
            identity=identity,
            route=ci.Route(input=b.stream, output=names[0], group=1),
        )
        dispatch(a_session, a_session.discover(now_ns=monotonic_ns()), a_out)
        updates = b_session.feed(
            receive(b, ump.EventAssembler()),
            input=b.stream,
            group=1,
            now_ns=monotonic_ns(),
        )
        replies = [u for u in updates if isinstance(u, ci.Action)]
        assert len(replies) == 1
        dispatch(b_session, replies[0], b_out)
        a_session.feed(
            receive(a, ump.EventAssembler()),
            input=a.stream,
            group=1,
            now_ns=monotonic_ns(),
        )
        assert [p.muid for p in a_session.peers] == [second]
        assert [p.muid for p in b_session.peers] == [first]
        assert a_session.peers[0].info.capabilities == 0
    print("CoreMIDI native UMP SysEx7 discovery/reply passed between test-owned peers")


if __name__ == "__main__":
    main()
