"""Exercise the public tuning module with uFor deliberately unavailable."""

import sys
from importlib.abc import MetaPathFinder
from importlib.machinery import ModuleSpec
from types import ModuleType


class BlockUfor(MetaPathFinder):
    def find_spec(
        self, fullname: str, path: object = None, target: ModuleType | None = None
    ) -> ModuleSpec | None:
        if fullname == "ufor" or fullname.startswith("ufor."):
            raise ModuleNotFoundError("uFor deliberately unavailable", name="ufor")
        return None


sys.meta_path.insert(0, BlockUfor())


def check_without_ufor() -> None:
    from meedee import tuning

    assert tuning.Pitch(semitone=69, fraction=0).frequency_hz == 440
    result = [
        tuning.NoteTuning(
            note=69,
            pitch=tuning.Pitch(semitone=69, fraction=0),
        )
    ]
    assert tuning.bulk_dump(
        result, device_id=1, program=0, name="test", preserve_unmapped=True
    ).table.values[69] == tuning.Pitch(semitone=69, fraction=0)
    assert not any(n == "ufor" or n.startswith("ufor.") for n in sys.modules)


check_without_ufor()
