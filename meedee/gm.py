"""Pure gm values and helpers, without MIDI runtime initialization."""

import sys

from . import _core, messages

_core.populate_gm(sys.modules[__name__])

del sys, _core, messages
