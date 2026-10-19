"""Read-only file summaries and capture validation, without MIDI initialization."""

import sys

from . import _core, archive, events, notes, smf

_core.populate_inspection(sys.modules[__name__])

del sys, _core, archive, events, notes, smf
