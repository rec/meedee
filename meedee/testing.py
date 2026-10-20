"""Deterministic in-memory test connections, without devices, threads or sleeps."""

import sys

from . import _core, events, messages, ump

_core.populate_testing(sys.modules[__name__])

del sys, _core, events, messages, ump
