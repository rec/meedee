"""MIDI-file, SysEx-stream and middi-archive reading and writing."""

import sys

from . import _core

_core.populate_file_io(sys.modules[__name__])

del sys, _core
