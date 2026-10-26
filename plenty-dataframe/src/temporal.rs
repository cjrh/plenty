//! Checked physical transfers and calendar expressions for Date and Datetime.
use super::*;
use chrono::{DateTime, Datelike, NaiveDate, Utc};

fn flag(value: u8) -> Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(NativeError::new(1, "temporal flag must be 0 or 1")),
    }
}
fn unit(value: i32) -> Result<TimeUnit> {
    match value {
        0 => Ok(TimeUnit::Milliseconds),
        1 => Ok(TimeUnit::Microseconds),
        2 => Ok(TimeUnit::Nanoseconds),
        _ => Err(NativeError::new(1, "unknown datetime unit")),
    }
}
fn datetime_dtype(value: i32, utc: u8) -> Result<DataType> {
    Ok(DataType::Datetime(
        unit(value)?,
        flag(utc)?.then_some(TimeZone::UTC),
    ))
}
fn logical_series(series: Series, dtype: &DataType) -> PolarsResult<Series> {
    // Without Polars' broad timezones feature, a generic numeric cast can drop
    // the timezone. Construct the logical wrapper explicitly: tick values are
    // already in the requested unit and need no localization or conversion.
    match dtype {
        DataType::Date => Ok(series.i32()?.clone().into_date().into_series()),
        DataType::Datetime(unit, timezone) => Ok(series
            .i64()?
            .clone()
            .into_datetime(*unit, timezone.clone())
            .into_series()),
        _ => unreachable!(),
    }
}
pub(super) fn dtype_descriptor(dtype: &DataType) -> String {
    match dtype {
        DataType::Datetime(unit, timezone) => format!(
            "Datetime:{}:{}",
            match unit {
                TimeUnit::Milliseconds => "ms",
                TimeUnit::Microseconds => "us",
                TimeUnit::Nanoseconds => "ns",
            },
            timezone.as_ref().map(|tz| tz.as_str()).unwrap_or("")
        ),
        _ => format!("{dtype:?}"),
    }
}
fn valid_date(days: i32) -> bool {
    days.checked_add(
        NaiveDate::from_ymd_opt(1970, 1, 1)
            .unwrap()
            .num_days_from_ce(),
    )
    .and_then(NaiveDate::from_num_days_from_ce_opt)
    .is_some()
}
fn valid_datetime(ticks: i64, unit: TimeUnit) -> bool {
    let scale = match unit {
        TimeUnit::Milliseconds => 1_000i64,
        TimeUnit::Microseconds => 1_000_000,
        TimeUnit::Nanoseconds => 1_000_000_000,
    };
    DateTime::<Utc>::from_timestamp(
        ticks.div_euclid(scale),
        (ticks.rem_euclid(scale) * (1_000_000_000 / scale)) as u32,
    )
    .is_some()
}
fn invalid_calendar() -> NativeError {
    NativeError::new(
        1,
        "temporal value is outside the supported Chrono calendar range",
    )
}
unsafe fn input<T: Copy>(
    data: *const u8,
    len: usize,
    stride: usize,
    valid: *const u8,
    valid_len: usize,
    valid_stride: usize,
) -> Result<Vec<Option<T>>> {
    let payloads = unsafe { values::<T>(data, len, stride)? };
    if valid_len == 0 {
        return Ok(payloads.into_iter().map(Some).collect());
    }
    if valid_len != len {
        return Err(NativeError::new(
            1,
            "temporal validity length differs from values",
        ));
    }
    let valid = unsafe { values::<u8>(valid, valid_len, valid_stride)? }
        .into_iter()
        .map(flag)
        .collect::<Result<Vec<_>>>()?;
    Ok(payloads
        .into_iter()
        .zip(valid)
        .map(|(value, valid)| valid.then_some(value))
        .collect())
}
fn expect_dtype(column: &Column, dtype: &DataType) -> Result<()> {
    if column.dtype() != dtype {
        return Err(NativeError::new(
            1,
            format!(
                "expected {}, got {}",
                dtype_descriptor(dtype),
                dtype_descriptor(column.dtype())
            ),
        ));
    }
    Ok(())
}
fn row_index(frame: &DataFrame, row: i64) -> Result<usize> {
    usize::try_from(row)
        .ok()
        .filter(|row| *row < frame.height())
        .ok_or_else(|| NativeError::new(7, "temporal row index outside dataframe height"))
}
// Keep the two strided output views in the same layout as their FFI callers.
#[allow(clippy::too_many_arguments)]
unsafe fn output<T: Copy + Default>(
    values: &[Option<T>],
    nullable: u8,
    data: *mut u8,
    len: usize,
    stride: usize,
    valid: *mut u8,
    valid_len: usize,
    valid_stride: usize,
) -> Result<()> {
    let nullable = flag(nullable)?;
    check_view::<T>(data, len, stride)?;
    check_view::<u8>(valid, valid_len, valid_stride)?;
    if len != values.len() || valid_len != if nullable { len } else { 0 } {
        return Err(NativeError::new(
            1,
            "temporal output buffer lengths do not match column",
        ));
    }
    if !nullable && values.iter().any(Option::is_none) {
        return Err(NativeError::new(
            5,
            "column contains nulls; use nullable temporal extraction",
        ));
    }
    for (i, value) in values.iter().enumerate() {
        // SAFETY: the binding lends disjoint initialized writable scalar views.
        unsafe {
            data.add(i * stride)
                .cast::<T>()
                .write_unaligned(value.unwrap_or_default());
            if nullable {
                valid.add(i * valid_stride).write(u8::from(value.is_some()));
            }
        }
    }
    Ok(())
}
export!(pldf_v1_date(name: *const u8, name_len: usize, data: *const u8, len: usize, stride: usize, valid: *const u8, valid_len: usize, valid_stride: usize, out: *mut *mut Column) {
    let values = input::<i32>(data, len, stride, valid, valid_len, valid_stride)?;
    if values.iter().flatten().any(|value| !valid_date(*value)) { return Err(invalid_calendar()); }
    let series = Series::new(text(name, name_len)?.into(), values).i32()?.clone().into_date().into_series();
    put(out, series.into())
});
export!(pldf_v1_datetime(name: *const u8, name_len: usize, data: *const u8, len: usize, stride: usize, valid: *const u8, valid_len: usize, valid_stride: usize, unit_code: i32, utc: u8, out: *mut *mut Column) {
    let dtype = datetime_dtype(unit_code, utc)?;
    let values = input::<i64>(data, len, stride, valid, valid_len, valid_stride)?;
    if values.iter().flatten().any(|value| !valid_datetime(*value, unit(unit_code).unwrap())) { return Err(invalid_calendar()); }
    put(out, logical_series(Series::new(text(name, name_len)?.into(), values), &dtype)?.into())
});
export!(pldf_v1_get_date(frame: *const DataFrame, name: *const u8, name_len: usize, row: i64, out: *mut i32, valid: *mut u8) {
    let frame = get(frame)?;
    let row = row_index(frame, row)?;
    let column = frame.column(text(name, name_len)?)?;
    expect_dtype(column, &DataType::Date)?;
    let value = column.date()?.physical().get(row);
    if value.is_some_and(|v| !valid_date(v)) { return Err(invalid_calendar()); }
    let out = out.as_mut().ok_or_else(|| NativeError::new(1, "null date output"))?;
    let valid = valid.as_mut().ok_or_else(|| NativeError::new(1, "null temporal validity output"))?;
    *out = value.unwrap_or_default(); *valid = u8::from(value.is_some()); Ok(())
});
export!(pldf_v1_get_datetime(frame: *const DataFrame, name: *const u8, name_len: usize, row: i64, unit_code: i32, utc: u8, out: *mut i64, valid: *mut u8) {
    let dtype = datetime_dtype(unit_code, utc)?;
    let frame = get(frame)?;
    let row = row_index(frame, row)?;
    let column = frame.column(text(name, name_len)?)?;
    expect_dtype(column, &dtype)?;
    let value = column.datetime()?.physical().get(row);
    if value.is_some_and(|v| !valid_datetime(v, unit(unit_code).unwrap())) { return Err(invalid_calendar()); }
    let out = out.as_mut().ok_or_else(|| NativeError::new(1, "null datetime output"))?;
    let valid = valid.as_mut().ok_or_else(|| NativeError::new(1, "null temporal validity output"))?;
    *out = value.unwrap_or_default(); *valid = u8::from(value.is_some()); Ok(())
});
export!(pldf_v1_values_date(frame: *const DataFrame, name: *const u8, name_len: usize, nullable: u8, data: *mut u8, len: usize, stride: usize, valid: *mut u8, valid_len: usize, valid_stride: usize) {
    let column = get(frame)?.column(text(name, name_len)?)?;
    expect_dtype(column, &DataType::Date)?;
    let values = column.date()?.physical().iter().collect::<Vec<_>>();
    if values.iter().flatten().any(|value| !valid_date(*value)) { return Err(invalid_calendar()); }
    output(&values, nullable, data, len, stride, valid, valid_len, valid_stride)
});
export!(pldf_v1_values_datetime(frame: *const DataFrame, name: *const u8, name_len: usize, unit_code: i32, utc: u8, nullable: u8, data: *mut u8, len: usize, stride: usize, valid: *mut u8, valid_len: usize, valid_stride: usize) {
    let dtype = datetime_dtype(unit_code, utc)?;
    let column = get(frame)?.column(text(name, name_len)?)?;
    expect_dtype(column, &dtype)?;
    let values = column.datetime()?.physical().iter().collect::<Vec<_>>();
    if values.iter().flatten().any(|value| !valid_datetime(*value, unit(unit_code).unwrap())) { return Err(invalid_calendar()); }
    output(&values, nullable, data, len, stride, valid, valid_len, valid_stride)
});

fn checked_temporal(expr: Expr, format: Option<String>) -> Expr {
    expr.map_with_fmt_str(
        move |column| {
            if column.dtype() == &DataType::Date
                && let Some(format) = &format
            {
                // Date has no time-of-day fields. Chrono formatting reports
                // fmt::Error for such directives; do not invoke to_string,
                // which would panic on that formatting error.
                let date = NaiveDate::from_ymd_opt(2000, 1, 1).unwrap();
                if std::fmt::write(&mut String::new(), format_args!("{}", date.format(format)))
                    .is_err()
                {
                    return Err(PolarsError::InvalidOperation(
                        "format requires fields unavailable on Date".into(),
                    ));
                }
            }
            let valid = match column.dtype() {
                DataType::Date => column.date()?.physical().iter().flatten().all(valid_date),
                DataType::Datetime(unit, tz)
                    if tz.is_none() || tz.as_ref() == Some(&TimeZone::UTC) =>
                {
                    column
                        .datetime()?
                        .physical()
                        .iter()
                        .flatten()
                        .all(|value| valid_datetime(value, *unit))
                }
                _ => {
                    return Err(PolarsError::InvalidOperation(
                        "temporal operation requires Date or naive/UTC Datetime".into(),
                    ));
                }
            };
            if !valid {
                return Err(PolarsError::InvalidOperation(
                    "temporal value is outside supported calendar range".into(),
                ));
            }
            Ok(column)
        },
        |_, field| Ok(field.clone()),
        "checked temporal values",
    )
}

fn format_string(format: &str) -> Result<()> {
    if format.is_empty()
        || chrono::format::StrftimeItems::new(format)
            .any(|item| matches!(item, chrono::format::Item::Error))
    {
        return Err(NativeError::new(1, "invalid or empty temporal format"));
    }
    let mut chars = format.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            continue;
        }
        let Some(mut code) = chars.next() else {
            break;
        };
        if code == '%' {
            continue;
        }
        while matches!(code, '-' | '_' | '0' | '#' | ':' | '.') || code.is_ascii_digit() {
            let Some(next) = chars.next() else {
                break;
            };
            code = next;
        }
        if matches!(code, 'z' | 'Z' | '+') {
            return Err(NativeError::new(
                1,
                "timezone-bearing formats are not supported; select naive or UTC explicitly",
            ));
        }
    }
    Ok(())
}

fn parsed(value: &str, format: &str) -> Option<chrono::format::Parsed> {
    let mut parsed = chrono::format::Parsed::new();
    chrono::format::parse(
        &mut parsed,
        value,
        chrono::format::StrftimeItems::new(format),
    )
    .ok()?;
    // Arrow epoch ticks cannot represent leap seconds distinctly.
    if parsed.second() == Some(60) {
        return None;
    }
    Some(parsed)
}
fn parse_date(value: &str, format: &str) -> Option<i32> {
    let date = parsed(value, format)?.to_naive_date().ok()?;
    date.num_days_from_ce()
        .checked_sub(NaiveDate::from_ymd_opt(1970, 1, 1)?.num_days_from_ce())
}
fn parse_datetime(value: &str, format: &str, unit: TimeUnit) -> Option<i64> {
    let mut parsed = parsed(value, format)?;
    if parsed.timestamp().is_none() {
        if parsed.hour_div_12().is_none() && parsed.hour_mod_12().is_none() {
            parsed.set_hour(0).ok()?;
        }
        if parsed.minute().is_none() {
            parsed.set_minute(0).ok()?;
        }
        if parsed.second().is_none() {
            parsed.set_second(0).ok()?;
        }
    }
    let datetime = parsed.to_naive_datetime_with_offset(0).ok()?.and_utc();
    match unit {
        TimeUnit::Milliseconds => Some(datetime.timestamp_millis()),
        TimeUnit::Microseconds => Some(datetime.timestamp_micros()),
        TimeUnit::Nanoseconds => datetime.timestamp_nanos_opt(),
    }
}
fn parse_expression(expr: Expr, format: &str, dtype: DataType, strict: bool) -> Expr {
    let format = format.to_owned();
    let schema_dtype = dtype.clone();
    // One native column expression, never a Plenty callback/per-cell FFI call.
    // Polars 0.55.2's native nanosecond parser multiplies signed timestamps
    // unchecked; the checked Chrono path also makes leap-second rejection and
    // strict/non-strict errors consistent across all units and Date.
    expr.map_with_fmt_str(
        move |column| {
            let strings = column.str()?;
            macro_rules! convert {
                ($parse:expr, $ty:ty) => {{
                    let mut values: Vec<Option<$ty>> = Vec::with_capacity(strings.len());
                    for (row, value) in strings.iter().enumerate() {
                        let result = value.and_then($parse);
                        if strict && value.is_some() && result.is_none() {
                            return Err(PolarsError::InvalidOperation(
                                format!("invalid or out-of-range temporal value at row {row}")
                                    .into(),
                            ));
                        }
                        values.push(result);
                    }
                    Series::new(column.name().clone(), values)
                }};
            }
            let series = match &dtype {
                DataType::Date => convert!(|value| parse_date(value, &format), i32),
                DataType::Datetime(unit, _) => {
                    convert!(|value| parse_datetime(value, &format, *unit), i64)
                }
                _ => unreachable!(),
            };
            Ok(logical_series(series, &dtype)?.into())
        },
        move |_, field| {
            if field.dtype() != &DataType::String {
                return Err(PolarsError::InvalidOperation(
                    "temporal parsing requires String".into(),
                ));
            }
            Ok(Field::new(field.name().clone(), schema_dtype.clone()))
        },
        "checked temporal parsing",
    )
}
export!(pldf_v1_str_to_date(expr: *const Expr, format: *const u8, len: usize, strict: u8, out: *mut *mut Expr) {
    let format = text(format, len)?;
    format_string(format)?;
    put(out, parse_expression(get(expr)?.clone(), format, DataType::Date, flag(strict)?))
});
export!(pldf_v1_str_to_datetime(expr: *const Expr, format: *const u8, len: usize, unit_code: i32, utc: u8, strict: u8, out: *mut *mut Expr) {
    let format = text(format, len)?;
    format_string(format)?;
    put(out, parse_expression(get(expr)?.clone(), format, datetime_dtype(unit_code, utc)?, flag(strict)?))
});
export!(pldf_v1_dt_component(expr: *const Expr, kind: i32, out: *mut *mut Expr) {
    let expr = checked_temporal(get(expr)?.clone(), None).dt();
    let result = match kind {
        0 => expr.year().cast(DataType::Int64), 1 => expr.month().cast(DataType::Int64), 2 => expr.day().cast(DataType::Int64),
        3 => expr.hour().cast(DataType::Int64), 4 => expr.minute().cast(DataType::Int64), 5 => expr.second().cast(DataType::Int64),
        6 => expr.date(), _ => return Err(NativeError::new(1, "unknown temporal component")),
    };
    put(out, result)
});
export!(pldf_v1_dt_format(expr: *const Expr, format: *const u8, len: usize, out: *mut *mut Expr) {
    let format = text(format, len)?;
    format_string(format)?;
    put(out, checked_temporal(get(expr)?.clone(), Some(format.to_owned())).dt().strftime(format))
});

#[cfg(test)]
#[path = "temporal_tests.rs"]
mod tests;
