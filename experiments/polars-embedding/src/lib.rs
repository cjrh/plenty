//! Optional Polars adapter for the Plenty embedding experiment.
//!
//! ABI contract: handles are unique Box owners, borrowed by calls and released
//! only by their matching destroy function. Input bytes are borrowed for the
//! call; output pointers refer to writable, initialized slots. No pointers are
//! retained. Errors never consume inputs. Null destruction is allowed.
//! Rust allocation failure may abort: status codes are NOT an OOM guarantee.
use polars::prelude::*;
use std::{fs::File, panic::AssertUnwindSafe};

pub type Status = Result<(), i32>;
const INVALID: i32 = 1;
const POLARS: i32 = 2;
const IO: i32 = 3;
const PANIC: i32 = 4;
const NULL: i32 = 5;
const TYPE: i32 = 6;
const BOUNDS: i32 = 7;

pub enum ColumnBuilder {
    I64(String, Vec<Option<i64>>),
    F64(String, Vec<Option<f64>>),
}

fn boundary(f: impl FnOnce() -> Status) -> i32 {
    match std::panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => 0,
        Ok(Err(code)) => code,
        Err(_) => PANIC,
    }
}

unsafe fn text<'a>(data: *const u8, len: usize) -> Result<&'a str, i32> {
    if len == 0 {
        return Ok("");
    }
    if data.is_null() || len > isize::MAX as usize {
        return Err(INVALID);
    }
    // SAFETY: the binding lends len valid immutable bytes for this call.
    std::str::from_utf8(unsafe { std::slice::from_raw_parts(data, len) }).map_err(|_| INVALID)
}

unsafe fn get<'a, T>(ptr: *const T) -> Result<&'a T, i32> {
    // SAFETY: the binding borrows a live matching Box, or supplies null.
    unsafe { ptr.as_ref() }.ok_or(INVALID)
}

unsafe fn put<T>(out: *mut *mut T, value: T) -> Status {
    if out.is_null() {
        return Err(INVALID);
    }
    // SAFETY: caller provides a writable null output slot; set only on success.
    unsafe { *out = Box::into_raw(Box::new(value)) };
    Ok(())
}

macro_rules! export {
    ($name:ident($($arg:ident: $ty:ty),*) $body:block) => {
        /// # Safety
        /// Obey the module's handle, call-scoped borrowing and output-slot contract.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name($($arg: $ty),*) -> i32 {
            boundary(|| {
                // SAFETY: all pointer operations follow the exported ABI contract.
                unsafe { $body }
            })
        }
    };
}

macro_rules! destroy {
    ($name:ident, $ty:ty) => {
        /// # Safety
        /// Pass null or a live unique owner returned by this adapter, exactly once.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(ptr: *mut $ty) {
            if !ptr.is_null() {
                // SAFETY: ownership transfers back to the matching allocator.
                drop(unsafe { Box::from_raw(ptr) });
            }
        }
    };
}

destroy!(pp_column_drop, ColumnBuilder);
destroy!(pp_frame_drop, DataFrame);
destroy!(pp_expr_drop, Expr);
destroy!(pp_query_drop, LazyFrame);

export!(pp_column_new(name: *const u8, len: usize, kind: i32, out: *mut *mut ColumnBuilder) {
    let name = text(name, len)?.to_owned();
    let column = match kind {
        0 => ColumnBuilder::I64(name, Vec::new()),
        1 => ColumnBuilder::F64(name, Vec::new()),
        _ => return Err(TYPE),
    };
    put(out, column)
});
export!(pp_column_i64(ptr: *mut ColumnBuilder, value: i64, valid: i32) {
    match ptr.as_mut().ok_or(INVALID)? {
        ColumnBuilder::I64(_, values) => values.push((valid != 0).then_some(value)),
        _ => return Err(TYPE),
    }
    Ok(())
});
export!(pp_column_f64(ptr: *mut ColumnBuilder, value: f64, valid: i32) {
    match ptr.as_mut().ok_or(INVALID)? {
        ColumnBuilder::F64(_, values) => values.push((valid != 0).then_some(value)),
        _ => return Err(TYPE),
    }
    Ok(())
});

fn series(column: &ColumnBuilder) -> Column {
    match column {
        ColumnBuilder::I64(name, values) => {
            Series::new(name.as_str().into(), values.as_slice()).into()
        }
        ColumnBuilder::F64(name, values) => {
            Series::new(name.as_str().into(), values.as_slice()).into()
        }
    }
}

export!(pp_frame_two(a: *const ColumnBuilder, b: *const ColumnBuilder, out: *mut *mut DataFrame) {
    let frame = DataFrame::new_infer_height(vec![series(get(a)?), series(get(b)?)]).map_err(|_| POLARS)?;
    put(out, frame)
});
export!(pp_frame_height(frame: *const DataFrame, out: *mut u64) {
    *out.as_mut().ok_or(INVALID)? = get(frame)?.height() as u64;
    Ok(())
});
export!(pp_frame_i64(frame: *const DataFrame, name: *const u8, len: usize, row: u64, out: *mut i64) {
    let frame = get(frame)?;
    let row = usize::try_from(row).map_err(|_| BOUNDS)?;
    if row >= frame.height() { return Err(BOUNDS); }
    let values = frame.column(text(name, len)?).map_err(|_| POLARS)?.i64().map_err(|_| TYPE)?;
    *out.as_mut().ok_or(INVALID)? = values.get(row).ok_or(NULL)?;
    Ok(())
});
export!(pp_frame_f64(frame: *const DataFrame, name: *const u8, len: usize, row: u64, out: *mut f64) {
    let frame = get(frame)?;
    let row = usize::try_from(row).map_err(|_| BOUNDS)?;
    if row >= frame.height() { return Err(BOUNDS); }
    let values = frame.column(text(name, len)?).map_err(|_| POLARS)?.f64().map_err(|_| TYPE)?;
    *out.as_mut().ok_or(INVALID)? = values.get(row).ok_or(NULL)?;
    Ok(())
});
export!(pp_frame_write(frame: *const DataFrame, path: *const u8, len: usize) {
    let file = File::create(text(path, len)?).map_err(|_| IO)?;
    ParquetWriter::new(file).finish(&mut get(frame)?.clone()).map_err(|_| POLARS)?;
    Ok(())
});
export!(pp_scan(path: *const u8, len: usize, out: *mut *mut LazyFrame) {
    let query = LazyFrame::scan_parquet(PlRefPath::new(text(path, len)?), Default::default()).map_err(|_| POLARS)?;
    put(out, query)
});
export!(pp_lazy(frame: *const DataFrame, out: *mut *mut LazyFrame) {
    put(out, get(frame)?.clone().lazy())
});
export!(pp_col(name: *const u8, len: usize, out: *mut *mut Expr) {
    put(out, col(text(name, len)?))
});
export!(pp_lit(value: f64, out: *mut *mut Expr) {
    put(out, lit(value))
});
export!(pp_expr_binary(a: *const Expr, b: *const Expr, op: i32, out: *mut *mut Expr) {
    let a = get(a)?.clone();
    let b = get(b)?.clone();
    let expr = match op {
        0 => a.gt(b),
        1 => a * b,
        _ => return Err(INVALID),
    };
    put(out, expr)
});
export!(pp_expr_sum(expr: *const Expr, out: *mut *mut Expr) {
    put(out, get(expr)?.clone().sum())
});
export!(pp_expr_count(expr: *const Expr, out: *mut *mut Expr) {
    put(out, get(expr)?.clone().count().cast(DataType::Int64))
});
export!(pp_expr_alias(expr: *const Expr, name: *const u8, len: usize, out: *mut *mut Expr) {
    put(out, get(expr)?.clone().alias(text(name, len)?))
});
export!(pp_filter(query: *const LazyFrame, expr: *const Expr, out: *mut *mut LazyFrame) {
    put(out, get(query)?.clone().filter(get(expr)?.clone()))
});
export!(pp_with_column(query: *const LazyFrame, expr: *const Expr, out: *mut *mut LazyFrame) {
    put(out, get(query)?.clone().with_columns([get(expr)?.clone()]))
});
export!(pp_select(query: *const LazyFrame, expr: *const Expr, out: *mut *mut LazyFrame) {
    put(out, get(query)?.clone().select([get(expr)?.clone()]))
});
export!(pp_group(query: *const LazyFrame, key: *const Expr, aggregate: *const Expr, out: *mut *mut LazyFrame) {
    put(out, get(query)?.clone().group_by([get(key)?.clone()]).agg([get(aggregate)?.clone()]))
});
export!(pp_collect(query: *const LazyFrame, out: *mut *mut DataFrame) {
    put(out, get(query)?.clone().collect().map_err(|_| POLARS)?)
});

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn errors_preserve_outputs_and_inputs() {
        let frame = df!("group" => [1i64], "value" => [Some(2.0)]).unwrap();
        let mut out = 42.0;
        // SAFETY: all pointers address live objects of the declared type.
        unsafe {
            assert_eq!(
                pp_frame_f64(&frame, b"value".as_ptr(), 5, 9, &mut out),
                BOUNDS
            );
            assert_eq!(out, 42.0);
            assert_eq!(
                pp_frame_f64(&frame, b"group".as_ptr(), 5, 0, &mut out),
                TYPE
            );
            assert_eq!(
                pp_frame_f64(ptr::null(), b"value".as_ptr(), 5, 0, &mut out),
                INVALID
            );
            let mut result = ptr::null_mut();
            assert_eq!(
                pp_collect(&frame.clone().lazy().select([col("absent")]), &mut result),
                POLARS
            );
            assert!(result.is_null());
            pp_frame_drop(result);
        }
        assert_eq!(frame.height(), 1);
    }

    #[test]
    fn null_and_nan_are_distinct() {
        let frame = df!("value" => [None, Some(f64::NAN), Some(-0.0)]).unwrap();
        let mut value = 1.0;
        // SAFETY: pointers borrow live frame/name/output objects for each call.
        unsafe {
            assert_eq!(
                pp_frame_f64(&frame, b"value".as_ptr(), 5, 0, &mut value),
                NULL
            );
            assert_eq!(pp_frame_f64(&frame, b"value".as_ptr(), 5, 1, &mut value), 0);
            assert!(value.is_nan());
            assert_eq!(pp_frame_f64(&frame, b"value".as_ptr(), 5, 2, &mut value), 0);
            assert!(value.is_sign_negative());
        }
    }

    #[test]
    fn ordinary_panics_do_not_cross_abi() {
        assert_eq!(boundary(|| panic!("probe")), PANIC);
    }
}
