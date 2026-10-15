"""MIDI Tuning Standard values, explicit SysEx interpretation and reply tracking."""

import sys

from . import _core, messages

_core.populate_tuning(sys.modules[__name__])

del sys, _core, messages
