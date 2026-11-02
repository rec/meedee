"""Track confirmed MIDI 1 output for connection-local release and reset."""

from . import codecs, messages


class OutputState:
    def __init__(self) -> None:
        self.decoder = codecs.Decoder()
        self.held: dict[tuple[int, int], int] = {}
        self.resets: dict[tuple[int, str, int], messages.Message] = {}

    def feed(self, packet: bytes) -> None:
        for m in self.decoder.feed(packet):
            if isinstance(m, messages.SystemReset):
                self.held.clear()
                self.resets.clear()
            channel = getattr(m, "channel", None)
            if channel is None:
                continue
            if isinstance(m, (messages.NoteOn, messages.NoteOff)):
                key = (channel, m.note)
                if isinstance(m, messages.NoteOn) and m.velocity:
                    self.held[key] = self.held.get(key, 0) + 1
                elif self.held.get(key, 0) > 1:
                    self.held[key] -= 1
                else:
                    self.held.pop(key, None)
            elif isinstance(m, messages.ControlChange):
                if m.controller in (120, 123):
                    self.held = {k: v for k, v in self.held.items() if k[0] != channel}
                elif m.controller == 121:
                    self.resets = {
                        k: v for k, v in self.resets.items() if k[0] != channel
                    }
                elif m.controller in (1, 64, 66, 67):
                    self._reset(
                        channel, "cc", m.controller, m.value, m.replace(value=0)
                    )
            elif isinstance(m, messages.PitchBend):
                self._reset(channel, "bend", 0, m.value, m.replace(value=0))
            elif isinstance(m, messages.ChannelPressure):
                self._reset(channel, "pressure", 0, m.pressure, m.replace(pressure=0))
            elif isinstance(m, messages.PolyPressure):
                self._reset(channel, "poly", m.note, m.pressure, m.replace(pressure=0))

    def cleanup(self) -> list[messages.Message]:
        return [
            messages.NoteOff(n, channel=c)
            for (c, n), count in self.held.items()
            for _ in range(count)
        ] + list(self.resets.values())

    def _reset(
        self, channel: int, kind: str, index: int, value: int, reset: messages.Message
    ) -> None:
        key = (channel, kind, index)
        if value:
            self.resets[key] = reset
        else:
            self.resets.pop(key, None)
