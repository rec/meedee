"""Rust-owned streaming CLIP file codecs."""

import sys

from . import _core, messages, ump

_core.populate_clip(sys.modules[__name__])

del sys, _core, messages, ump
