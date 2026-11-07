"""Opt-in repeated virtual open/close, including full-width MIDI 2 controls."""

from uuid import uuid4

from meedee import messages, midi


def main() -> None:
    for protocol in (midi.Protocol.Midi1, midi.Protocol.Ump):
        for run_in_subprocess in (True, False):
            for cycle in range(1, 4):
                source = midi.Input(
                    protocol=protocol, run_in_subprocess=run_in_subprocess
                )
                sender = midi.Output(
                    protocol=protocol, run_in_subprocess=run_in_subprocess
                )
                name = f"meedee-reopen-{uuid4().hex}"
                context = (
                    f"{protocol}, run_in_subprocess={run_in_subprocess}, "
                    f"cycle={cycle}, name={name}"
                )
                print(f"creating virtual input: {context}", flush=True)
                with source.create_virtual(name) as receiver:
                    print(f"discovering output ports: {context}", flush=True)
                    matches = [p for p in sender.ports() if p.name == name]
                    assert len(matches) == 1, (context, matches)
                    print(f"connecting output: {context}", flush=True)
                    with sender.connect(matches[0]) as output:
                        value = (
                            messages.ControlChange(channel=16, controller=7, value=127)
                            if protocol == midi.Protocol.Midi1
                            else messages.ControlChange(
                                version=2,
                                group=16,
                                channel=16,
                                controller=7,
                                value=0xFEDCBA98,
                            )
                        )
                        output.send(value)
                        received = receiver.receive(timeout=2)
                        assert isinstance(received, messages.Message), received
                        assert received == value
                        assert received.timestamp is not None
                        print(
                            f"delivery passed; closing connections: {context}",
                            flush=True,
                        )
                assert receiver.closed and output.closed
                print(f"connections closed: {context}", flush=True)
            print(
                f"three open/send/close cycles passed: {protocol}, "
                f"run_in_subprocess={run_in_subprocess}"
            )


if __name__ == "__main__":
    main()
