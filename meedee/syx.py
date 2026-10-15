"""Rust-owned streaming SYX file codecs."""

import sys

from . import _core, messages

_core.populate_syx(sys.modules[__name__])

del sys, _core, messages
