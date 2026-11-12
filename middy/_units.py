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


def duration(value: float | str) -> int:
    """Convert seconds to exact native nanoseconds."""
    if isinstance(value, str):
        from reccy.configuration.units import magnitude, quantity_unit

        unit = (
            "dimensionless"
            if ":" not in value and quantity_unit(value) is None
            else "second"
        )
        seconds = Fraction(cast(Fraction, magnitude(value, unit, exact=True)))
    else:
        if type(value) not in (int, float):
            raise TypeError("Duration must be seconds or a unit expression")
        seconds = Fraction(str(value))
    nanoseconds = seconds * 1_000_000_000
    if nanoseconds.denominator != 1:
        raise ValueError("Duration cannot be represented exactly in nanoseconds")
    return nanoseconds.numerator


def tempo(value: float | str, units_per_second: int) -> int:
    """Convert BPM or an authored beat period to an exact MIDI tempo integer."""
    if isinstance(value, str):
        from reccy.configuration.units import magnitude, quantity_unit

        if quantity_unit(value) == "second / beat":
            period = Fraction(
                cast(Fraction, magnitude(value, "second / beat", exact=True))
            )
        else:
            rate = Fraction(cast(Fraction, magnitude(value, "bpm", exact=True)))
            if rate <= 0:
                raise ValueError("Tempo must be positive")
            period = 60 / rate
    else:
        if type(value) not in (int, float):
            raise TypeError("Tempo must be BPM or a unit expression")
        rate = Fraction(str(value))
        if rate <= 0:
            raise ValueError("Tempo must be positive")
        period = 60 / rate
    if period <= 0:
        raise ValueError("Tempo must be positive")
    period *= units_per_second
    if period.denominator != 1:
        raise ValueError("Tempo cannot be represented exactly in the MIDI field")
    return period.numerator


def musical_duration(value: float | str) -> tuple[Fraction, str | None]:
    """Read a nonnegative duration without requiring a playback tempo yet."""
    if isinstance(value, str):
        from reccy.configuration.units import magnitude, quantity_unit

        unit = "second" if ":" in value else quantity_unit(value)
        if unit not in (None, "second", "beat"):
            raise ValueError("Duration must be a number, time or beats")
        amount = Fraction(
            cast(Fraction, magnitude(value, unit or "dimensionless", exact=True))
        )
    else:
        if type(value) not in (int, float):
            raise TypeError("Duration must be a number or unit expression")
        amount, unit = Fraction(str(value)), None
    if amount < 0:
        raise ValueError("Duration must be nonnegative")
    return amount, unit


def sequence_duration(value: float | str, beat_period: Fraction | None) -> Fraction:
    """Resolve contextual numbers and explicit units to exact seconds."""
    amount, unit = musical_duration(value)
    if unit == "beat" and beat_period is None:
        raise ValueError("Beat durations require Output(bpm=...)")
    if unit != "second" and beat_period is not None:
        amount *= beat_period
    return amount
