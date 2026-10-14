"""Pure MIDI 1.0 byte codecs registered by the private extension."""

import sys

from . import _core, messages

_core.populate_codecs(sys.modules[__name__])

del sys, _core, messages
