//! Version 1 C boundary for the optional dataframe engine.
//!
//! Every handle is a unique Box owner, lent for calls and destroyed exactly once
//! by its matching release function. Input text and strided numeric views are
//! call-scoped and never retained. Output slots start null and change only on
//! success. Mutable views allow scalar writes only, not resizing or retention.
//! Polars/Rust allocation failure may abort; errors do not promise recoverable OOM.
use polars::prelude::*;
use std::{fs::File, panic::AssertUnwindSafe, ptr};

#[derive(Debug)]
pub struct NativeError {
    kind: i32,
    message: String,
}
type Result<T> = std::result::Result<T, NativeError>;

impl NativeError {
    fn new(kind: i32, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}
impl From<PolarsError> for NativeError {
    fn from(error: PolarsError) -> Self {
        Self::new(2, error.to_string())
    }
}
impl From<std::io::Error> for NativeError {
    fn from(error: std::io::Error) -> Self {
        Self::new(3, error.to_string())
    }
}

fn boundary(f: impl FnOnce() -> Result<()>) -> *mut NativeError {
    match std::panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => ptr::null_mut(),
        Ok(Err(error)) => Box::into_raw(Box::new(error)),
        Err(error) => {
            let message = error
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| error.downcast_ref::<&str>().copied())
                .unwrap_or("native engine panic");
            Box::into_raw(Box::new(NativeError::new(4, message)))
        }
    }
}

unsafe fn text<'a>(data: *const u8, len: usize) -> Result<&'a str> {
    if len == 0 {
        return Ok("");
    }
    if data.is_null() || len > isize::MAX as usize {
        return Err(NativeError::new(1, "invalid text view"));
    }
    // SAFETY: the trusted binding lends len readable bytes for this call.
    std::str::from_utf8(unsafe { std::slice::from_raw_parts(data, len) })
        .map_err(|error| NativeError::new(1, error.to_string()))
}
unsafe fn get<'a, T>(ptr: *const T) -> Result<&'a T> {
    // SAFETY: the binding supplies null or a borrowed live matching owner.
    unsafe { ptr.as_ref() }.ok_or_else(|| NativeError::new(1, "null native owner"))
}
unsafe fn put<T>(out: *mut *mut T, value: T) -> Result<()> {
    if out.is_null() {
        return Err(NativeError::new(1, "null output slot"));
    }
    // SAFETY: the binding provides a writable null output slot.
    unsafe { *out = Box::into_raw(Box::new(value)) };
    Ok(())
}

fn check_view<T>(data: *const u8, len: usize, stride: usize) -> Result<()> {
    if len != 0
        && (data.is_null()
            || stride < size_of::<T>()
            || len
                .checked_mul(stride)
                .is_none_or(|bytes| bytes > isize::MAX as usize))
    {
        return Err(NativeError::new(1, "invalid strided numeric view"));
    }
    Ok(())
}
unsafe fn values<T: Copy>(data: *const u8, len: usize, stride: usize) -> Result<Vec<T>> {
    check_view::<T>(data, len, stride)?;
    if len > u32::MAX as usize {
        return Err(NativeError::new(
            1,
            "dataframe exceeds the engine's 32-bit row limit",
        ));
    }
    // SAFETY: each scalar is initialized and readable, and the view is borrowed
    // for the call. Unaligned reads avoid imposing a new alignment requirement.
    Ok((0..len)
        .map(|i| unsafe { data.add(i * stride).cast::<T>().read_unaligned() })
        .collect())
}

macro_rules! export {
    ($name:ident($($arg:ident: $ty:ty),*) $body:block) => {
        /// # Safety
        /// Obey the module's ownership, output-slot and call-scoped view contract.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name($($arg: $ty),*) -> *mut NativeError {
            boundary(|| {
                // SAFETY: all pointers follow the declared trusted binding ABI.
                unsafe { $body }
            })
        }
    };
}
macro_rules! destroy {
    ($name:ident, $ty:ty) => {
        /// # Safety
        /// Pass null or the unique matching live owner, exactly once.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(raw: *mut $ty) {
            if !raw.is_null() {
                // SAFETY: ownership returns to the allocator that created it.
                drop(unsafe { Box::from_raw(raw) });
            }
        }
    };
}
destroy!(pldf_v1_error_drop, NativeError);
destroy!(pldf_v1_column_drop, Column);
destroy!(pldf_v1_frame_drop, DataFrame);
destroy!(pldf_v1_expr_drop, Expr);
destroy!(pldf_v1_exprs_drop, Vec<Expr>);
destroy!(pldf_v1_query_drop, LazyFrame);

/// # Safety
/// Borrow a non-null error owner for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pldf_v1_error_kind(error: *const NativeError) -> i32 {
    unsafe { (*error).kind }
}
/// # Safety
/// Borrow a non-null error owner for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pldf_v1_error_len(error: *const NativeError) -> i64 {
    let error = unsafe { &*error };
    error.message.len() as i64
}
/// # Safety
/// Borrow an error and a mutable u8 view exactly as long as its UTF-8 message.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pldf_v1_error_copy(
    error: *const NativeError,
    data: *mut u8,
    len: usize,
    stride: usize,
) {
    // SAFETY: the binding sizes the destination from error_len and holds both loans.
    unsafe {
        let bytes = (*error).message.as_bytes();
        assert_eq!(len, bytes.len());
        for (i, byte) in bytes.iter().enumerate() {
            data.add(i * stride).write(*byte);
        }
    }
}

export!(pldf_v1_i64(name: *const u8, name_len: usize, data: *const u8, len: usize, stride: usize, out: *mut *mut Column) {
    let series = Series::new(text(name, name_len)?.into(), values::<i64>(data, len, stride)?);
    put(out, series.into())
});
export!(pldf_v1_f64(name: *const u8, name_len: usize, data: *const u8, len: usize, stride: usize, out: *mut *mut Column) {
    let series = Series::new(text(name, name_len)?.into(), values::<f64>(data, len, stride)?);
    put(out, series.into())
});
export!(pldf_v1_frame_new(out: *mut *mut DataFrame) { put(out, DataFrame::empty()) });
export!(pldf_v1_frame_add(frame: *mut DataFrame, column: *const Column) {
    let frame = frame.as_mut().ok_or_else(|| NativeError::new(1, "null dataframe"))?;
    let column = get(column)?;
    if frame.get_column_names().iter().any(|name| **name == *column.name()) {
        return Err(NativeError::new(1, format!("duplicate column name: {}", column.name())));
    }
    if frame.width() != 0 && frame.height() != column.len() {
        return Err(NativeError::new(1, format!("column {} has {} rows; expected {}", column.name(), column.len(), frame.height())));
    }
    frame.with_column(column.clone())?;
    Ok(())
});
export!(pldf_v1_height(frame: *const DataFrame, out: *mut i64) {
    *out.as_mut().ok_or_else(|| NativeError::new(1, "null output"))? = get(frame)?.height() as i64;
    Ok(())
});

macro_rules! scalar {
    ($name:ident, $ty:ty, $method:ident) => {
        export!($name(frame: *const DataFrame, name: *const u8, len: usize, row: i64, out: *mut $ty, valid: *mut u8) {
            let frame = get(frame)?;
            if row < 0 || row as u64 >= frame.height() as u64 { return Err(NativeError::new(7, format!("row {row} is outside dataframe height {}", frame.height()))); }
            let values = frame.column(text(name, len)?)?.$method()?;
            let value = values.get(row as usize);
            *out.as_mut().ok_or_else(|| NativeError::new(1, "null scalar output"))? = value.unwrap_or_default();
            *valid.as_mut().ok_or_else(|| NativeError::new(1, "null validity output"))? = u8::from(value.is_some());
            Ok(())
        });
    };
}
scalar!(pldf_v1_get_i64, i64, i64);
scalar!(pldf_v1_get_f64, f64, f64);

macro_rules! extract {
    ($name:ident, $ty:ty, $method:ident) => {
        export!($name(frame: *const DataFrame, name: *const u8, name_len: usize, data: *mut u8, len: usize, stride: usize) {
            check_view::<$ty>(data, len, stride)?;
            let frame = get(frame)?;
            if len != frame.height() { return Err(NativeError::new(1, "output length does not match dataframe height")); }
            let column = frame.column(text(name, name_len)?)?.$method()?;
            if column.null_count() != 0 { return Err(NativeError::new(5, "column contains nulls; use nullable scalar extraction or fill nulls in the query")); }
            for (i, value) in column.into_no_null_iter().enumerate() {
                data.add(i * stride).cast::<$ty>().write_unaligned(value);
            }
            Ok(())
        });
    };
}
extract!(pldf_v1_values_i64, i64, i64);
extract!(pldf_v1_values_f64, f64, f64);
export!(pldf_v1_write(frame: *const DataFrame, path: *const u8, len: usize) {
    let file = File::create(text(path, len)?)?;
    ParquetWriter::new(file).finish(&mut get(frame)?.clone())?;
    Ok(())
});
export!(pldf_v1_scan(path: *const u8, len: usize, out: *mut *mut LazyFrame) {
    put(out, LazyFrame::scan_parquet(PlRefPath::new(text(path, len)?), Default::default())?)
});
export!(pldf_v1_lazy(frame: *const DataFrame, out: *mut *mut LazyFrame) { put(out, get(frame)?.clone().lazy()) });
export!(pldf_v1_col(name: *const u8, len: usize, out: *mut *mut Expr) { put(out, col(text(name, len)?)) });
export!(pldf_v1_lit_f64(value: f64, out: *mut *mut Expr) { put(out, lit(value)) });
export!(pldf_v1_lit_i64(value: i64, out: *mut *mut Expr) { put(out, lit(value)) });
export!(pldf_v1_null_f64(out: *mut *mut Expr) { put(out, lit(NULL).cast(DataType::Float64)) });
export!(pldf_v1_binary(a: *const Expr, b: *const Expr, op: i32, out: *mut *mut Expr) {
    let (a, b) = (get(a)?.clone(), get(b)?.clone());
    let expr = match op {
        0 => a.gt(b), 1 => a * b, 2 => a + b, 3 => a - b, 4 => a / b,
        5 => a.eq(b), 6 => a.lt(b), 7 => a.gt_eq(b), 8 => a.lt_eq(b),
        9 => a.and(b), 10 => a.or(b), 11 => a.fill_null(b),
        _ => return Err(NativeError::new(1, "unknown expression operation")),
    };
    put(out, expr)
});
export!(pldf_v1_unary(expr: *const Expr, op: i32, out: *mut *mut Expr) {
    let expr = get(expr)?.clone();
    put(out, match op {
        0 => expr.sum(), 1 => expr.count().cast(DataType::Int64), 2 => expr.mean(),
        3 => expr.min(), 4 => expr.max(), 5 => expr.is_null(),
        _ => return Err(NativeError::new(1, "unknown expression operation")),
    })
});
export!(pldf_v1_alias(expr: *const Expr, name: *const u8, len: usize, out: *mut *mut Expr) {
    put(out, get(expr)?.clone().alias(text(name, len)?))
});
export!(pldf_v1_exprs_new(out: *mut *mut Vec<Expr>) { put(out, Vec::new()) });
export!(pldf_v1_exprs_add(exprs: *mut Vec<Expr>, expr: *const Expr) {
    exprs.as_mut().ok_or_else(|| NativeError::new(1, "null expression list"))?.push(get(expr)?.clone());
    Ok(())
});
export!(pldf_v1_filter(query: *const LazyFrame, expr: *const Expr, out: *mut *mut LazyFrame) {
    put(out, get(query)?.clone().filter(get(expr)?.clone()))
});
export!(pldf_v1_with_columns(query: *const LazyFrame, exprs: *const Vec<Expr>, out: *mut *mut LazyFrame) {
    put(out, get(query)?.clone().with_columns(get(exprs)?.as_slice()))
});
export!(pldf_v1_select(query: *const LazyFrame, exprs: *const Vec<Expr>, out: *mut *mut LazyFrame) {
    put(out, get(query)?.clone().select(get(exprs)?.as_slice()))
});
export!(pldf_v1_group(query: *const LazyFrame, keys: *const Vec<Expr>, aggregates: *const Vec<Expr>, out: *mut *mut LazyFrame) {
    put(out, get(query)?.clone().group_by(get(keys)?.as_slice()).agg(get(aggregates)?.as_slice()))
});
export!(pldf_v1_collect(query: *const LazyFrame, out: *mut *mut DataFrame) {
    put(out, get(query)?.clone().collect()?)
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strided_reads_support_empty_unaligned_and_padded_input() {
        // SAFETY: all views below cover readable initialized buffers; the
        // empty view dereferences nothing.
        unsafe {
            assert!(values::<i64>(ptr::null(), 0, 0).unwrap().is_empty());
            let mut bytes = [0u8; 34];
            bytes[1..9].copy_from_slice(&(-7i64).to_ne_bytes());
            bytes[17..25].copy_from_slice(&42i64.to_ne_bytes());
            assert_eq!(
                values::<i64>(bytes.as_ptr().add(1), 2, 16).unwrap(),
                [-7, 42]
            );
        }
        assert!(check_view::<i64>(ptr::null(), 1, 8).is_err());
        assert!(check_view::<i64>([0u8; 8].as_ptr(), 1, 4).is_err());
        assert!(check_view::<i64>([0u8; 8].as_ptr(), usize::MAX, 8).is_err());
    }

    #[test]
    fn errors_leave_outputs_empty_and_preserve_utf8_messages() {
        // SAFETY: initialized null output slots and owners freed once through
        // their matching functions; message storage has the reported length.
        unsafe {
            let mut column = ptr::null_mut();
            let error = pldf_v1_i64([255u8].as_ptr(), 1, ptr::null(), 0, 0, &mut column);
            assert!(!error.is_null());
            assert!(column.is_null());
            assert_eq!(pldf_v1_error_kind(error), 1);
            let mut message = vec![0u8; pldf_v1_error_len(error) as usize];
            pldf_v1_error_copy(error, message.as_mut_ptr(), message.len(), 1);
            assert!(String::from_utf8(message).unwrap().contains("utf-8"));
            pldf_v1_error_drop(error);
            let error = boundary(|| panic!("native test panic"));
            assert_eq!(pldf_v1_error_kind(error), 4);
            pldf_v1_error_drop(error);
        }
    }

    #[test]
    fn plans_own_data_after_original_handles_are_dropped() {
        // SAFETY: handles and output slots satisfy the adapter's contract;
        // originals are dropped only after each retaining call returns.
        unsafe {
            let values = [1.0f64, f64::NAN, 4.0];
            let mut column = ptr::null_mut();
            assert!(
                pldf_v1_f64(b"x".as_ptr(), 1, values.as_ptr().cast(), 3, 8, &mut column).is_null()
            );
            let mut frame = ptr::null_mut();
            assert!(pldf_v1_frame_new(&mut frame).is_null());
            assert!(pldf_v1_frame_add(frame, column).is_null());
            pldf_v1_column_drop(column);
            let mut query = ptr::null_mut();
            assert!(pldf_v1_lazy(frame, &mut query).is_null());
            pldf_v1_frame_drop(frame);
            let mut output = ptr::null_mut();
            assert!(pldf_v1_collect(query, &mut output).is_null());
            pldf_v1_query_drop(query);
            let mut values = [0.0f64; 3];
            assert!(
                pldf_v1_values_f64(output, b"x".as_ptr(), 1, values.as_mut_ptr().cast(), 3, 8)
                    .is_null()
            );
            assert_eq!(values[0], 1.0);
            assert!(values[1].is_nan());
            assert_eq!(values[2], 4.0);
            pldf_v1_frame_drop(output);
        }
    }
}
