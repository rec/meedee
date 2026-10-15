"""Rust-owned streaming CLIP file codecs."""

import sys

from . import _core, events, messages, ump

_core.populate_clip(sys.modules[__name__])

del sys, _core, events, messages, ump
