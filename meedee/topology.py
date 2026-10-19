"""Pure, bounded discovery snapshots; no physical identity or health inference."""

import sys

from . import _core

_core.populate_topology(sys.modules[__name__])

del sys, _core
