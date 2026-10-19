"""Bounded note and pedal observations. No MIDI devices or voices are owned."""

import sys

from . import _core, events

_core.populate_notes(sys.modules[__name__])

del sys, _core, events
