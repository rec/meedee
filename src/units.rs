//! Authored strings use reccy's parser; numeric inputs retain native validation.
use pyo3::prelude::*;
use pyo3::types::PyString;

pub fn normalize<'py>(
    value: &Bound<'py, PyAny>,
    unit: &str,
    kind: &str,
) -> PyResult<Bound<'py, PyAny>> {
    if value.is_instance_of::<PyString>() {
        value
            .py()
            .import("middy._units")?
            .getattr(kind)?
            .call1((value, unit))
    } else {
        Ok(value.clone())
    }
}

pub fn nanoseconds(value: &Bound<'_, PyAny>) -> PyResult<u64> {
    crate::files::uint(&normalize(value, "nanosecond", "integer")?)
}

pub fn ticks(value: &Bound<'_, PyAny>) -> PyResult<u64> {
    crate::files::uint(&normalize(value, "tick", "integer")?)
}

pub fn byte_offset(value: &Bound<'_, PyAny>) -> PyResult<u64> {
    crate::files::uint(&normalize(value, "byte", "integer")?)
}

pub fn semitones(value: &Bound<'_, PyAny>) -> PyResult<i64> {
    crate::messages::strict_integer(&normalize(value, "semitone", "integer")?)
}

pub fn rational(value: &Bound<'_, PyAny>, unit: &str) -> PyResult<middy::events::Rational> {
    if !value.is_instance_of::<PyString>() {
        return Ok(value
            .extract::<PyRef<'_, crate::event_values::Rational>>()?
            .value);
    }
    let (numerator, denominator) = value
        .py()
        .import("middy._units")?
        .getattr("rational")?
        .call1((value, unit))?
        .extract::<(i128, i128)>()?;
    middy::events::Rational::new(numerator, denominator)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
}

pub fn seconds(value: &Bound<'_, PyAny>) -> PyResult<f64> {
    normalize(value, "second", "real")?.extract()
}

pub fn bytes(value: &Bound<'_, PyAny>) -> PyResult<usize> {
    crate::files::limit(&normalize(value, "byte", "integer")?)
}

pub fn tempo(value: &Bound<'_, PyAny>) -> PyResult<u64> {
    crate::files::uint(
        &value
            .py()
            .import("middy._units")?
            .getattr("tempo")?
            .call1((value, 1_000_000))?,
    )
}

pub fn duration(value: &Bound<'_, PyAny>) -> PyResult<u64> {
    // PyO3 applies from_py_with only to supplied arguments. Binding defaults
    // therefore use native nanoseconds, with public seconds in text_signature.
    crate::files::uint(
        &value
            .py()
            .import("middy._units")?
            .getattr("duration")?
            .call1((value,))?,
    )
}

pub fn field<'py>(value: &Bound<'py, PyAny>, name: &str) -> PyResult<Bound<'py, PyAny>> {
    let unit = match name {
        "ticks" | "tick" | "resolution_ticks" => "tick",
        "semitones" => "semitone",
        "cents" => "cent",
        "hours" => "hour",
        "minutes" => "minute",
        "seconds" => "second",
        "frames" => "frame",
        _ => return Ok(value.clone()),
    };
    normalize(value, unit, "integer")
}
