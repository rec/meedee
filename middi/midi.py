"""Native MIDI transport registered by the private extension."""

import sys

from . import _core

_core.populate_midi(sys.modules[__name__])

del sys, _core
