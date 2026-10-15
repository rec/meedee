"""Opt-in repeated virtual open/close, including full-width MIDI 2 controls."""

from uuid import uuid4

from meedee import events, messages, midi, ump


def main() -> None:
    for protocol in (midi.Protocol.Midi1, midi.Protocol.Ump):
        for isolated in (True, False):
            source = midi.TypedInput(protocol=protocol, isolated=isolated)
            sender = midi.TypedOutput(protocol=protocol, isolated=isolated)
            for _ in range(3):
                name = f"meedee-reopen-{uuid4().hex}"
                with source.create_virtual(name) as receiver:
                    matches = [p for p in sender.ports() if p.name == name]
                    assert len(matches) == 1
                    with sender.connect(matches[0]) as output:
                        value = (
                            messages.ControlChange(channel=16, controller=7, value=127)
                            if protocol == midi.Protocol.Midi1
                            else ump.Midi2ControlChange(
                                group=16,
                                channel=16,
                                controller=7,
                                value=0xFEDCBA98,
                            )
                        )
                        output.send_message(value)
                        received = receiver.receive(timeout=2)
                        assert isinstance(received, events.Event), received
                        assert received.message == value
                        assert received.timestamp is not None
                assert receiver.closed and output.closed
            print(
                f"three open/send/close cycles passed: {protocol}, isolated={isolated}"
            )


if __name__ == "__main__":
    main()
