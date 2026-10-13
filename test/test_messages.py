"""Exercise the public values using the shared representation contract."""

import sys
from pathlib import Path
from subprocess import run

from native import meedee_messages

from meedee import messages


def test_public_messages_preserve_the_frozen_representation_contract() -> None:
    meedee_messages.verify(messages)


def test_message_import_does_not_initialize_midi_runtime() -> None:
    fixture = Path(__file__).parent / "native" / "meedee_message_import.py"
    run([sys.executable, str(fixture)], check=True, timeout=10)
