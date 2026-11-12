"""Immutable musical messages registered by the private extension."""

import sys

from . import _core

_core.populate_messages(sys.modules[__name__])

del sys, _core
