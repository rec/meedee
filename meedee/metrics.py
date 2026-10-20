"""Bounded timing aggregates and host-delivery observations; no runtime startup."""

import sys

from . import _core

_core.populate_metrics(sys.modules[__name__])

del sys, _core
