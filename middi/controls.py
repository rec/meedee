"""Pure, optional observations of one source's primitive control messages."""

import sys

from . import _core, messages

_core.populate_controls(sys.modules[__name__])

del sys, _core, messages
