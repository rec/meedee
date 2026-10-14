"""Immutable receive packets and clock metadata, without transport startup."""

import sys

from . import _core

_core.populate_events(sys.modules[__name__])

del sys, _core
