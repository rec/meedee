"""Versioned lossless binary event archives."""

import sys

from . import _core, events, messages, ump

_core.populate_archive(sys.modules[__name__])

del sys, _core, events, messages, ump
