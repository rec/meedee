"""Normalize authored quantities at Python boundaries, before native conversion."""

from fractions import Fraction
from typing import cast


def integer(value: int | str, unit: str) -> int:
    if not isinstance(value, str):
        return value
    from reccy.configuration.units import magnitude

    parsed = Fraction(cast(Fraction, magnitude(value, unit, exact=True)))
    if parsed.denominator != 1:
        raise ValueError(f"Quantity must be a whole number of {unit}")
    return parsed.numerator


def real(value: float | str, unit: str) -> float:
    if not isinstance(value, str):
        return value
    from reccy.configuration.units import magnitude

    return float(cast(Fraction, magnitude(value, unit)))


def tempo(value: int | str, unit: str, scale: int = 1) -> int:
    """Convert authored BPM or beat periods to an exact MIDI tempo integer."""
    if not isinstance(value, str):
        return value
    from reccy.configuration.units import magnitude, quantity_unit

    supplied_unit = quantity_unit(value)
    if supplied_unit is None:
        return integer(value, unit)
    if supplied_unit == "beat / second":
        rate = Fraction(cast(Fraction, magnitude(value, "beat / second", exact=True)))
        if rate <= 0:
            raise ValueError("Tempo must be positive")
        value = f"{1 / rate} second / beat"
    period = Fraction(cast(Fraction, magnitude(value, unit, exact=True))) / scale
    if period.denominator != 1:
        raise ValueError("Tempo cannot be represented exactly in the MIDI field")
    return period.numerator
