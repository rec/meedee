"""Caller-driven, bounded routing with explicit delivery confirmation."""

import sys

from . import _core, events

_core.populate_routing(sys.modules[__name__])

del sys, _core, events
