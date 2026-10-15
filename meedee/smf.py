"""Rust-owned streaming SMF file codecs."""

import sys

from . import _core, messages

_core.populate_smf(sys.modules[__name__])

del sys, _core, messages
