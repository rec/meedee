"""Explicit MIDI protocol conversion with native values and loss reports."""

import sys

from . import _core, messages

_core.populate_translation(sys.modules[__name__])

del sys, _core, messages
