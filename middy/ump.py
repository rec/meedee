"""Pure UMP/MIDI 2.0 values and codecs, without MIDI runtime initialization."""

import sys

from . import _core, messages

_core.populate_ump(sys.modules[__name__])

del sys, _core, messages
