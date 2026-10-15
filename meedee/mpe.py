"""Pure mpe values and helpers, without MIDI runtime initialization."""

import sys

from . import _core, messages

_core.populate_mpe(sys.modules[__name__])

del sys, _core, messages
