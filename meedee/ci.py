"""Explicit MIDI-CI management codecs and caller-driven sessions. No MIDI startup."""

import sys

from . import _core, messages

_core.populate_ci(sys.modules[__name__])

del sys, _core, messages
