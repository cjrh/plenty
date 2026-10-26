//! Exact-width numeric transfers. Native buffers never alias Plenty owners.
use super::*;

#[cfg(test)]
#[path = "numeric_tests.rs"]
mod numeric_tests;

pub(super) fn numeric_dtype(code: i32) -> Option<DataType> {
    match code {
        10 => Some(DataType::Int8),
        11 => Some(DataType::Int16),
        12 => Some(DataType::Int32),
        13 => Some(DataType::UInt8),
        14 => Some(DataType::UInt16),
        15 => Some(DataType::UInt32),
        16 => Some(DataType::UInt64),
        17 => Some(DataType::Float32),
        _ => None,
    }
}

macro_rules! numeric {
    ($ty:ty, $method:ident, $input:ident, $nullable:ident, $scalar:ident, $output:ident, $output_nullable:ident, $literal:ident) => {
        export!($input(name: *const u8, name_len: usize, data: *const u8, len: usize, stride: usize, out: *mut *mut Column) {
            put(out, Series::new(text(name, name_len)?.into(), values::<$ty>(data, len, stride)?).into())
        });
        export!($nullable(name: *const u8, name_len: usize, data: *const u8, len: usize, stride: usize, valid: *const u8, valid_len: usize, valid_stride: usize, out: *mut *mut Column) {
            let valid = super::columns::validity(valid, valid_len, valid_stride, len)?;
            let values = values::<$ty>(data, len, stride)?.into_iter().zip(valid)
                .map(|(value, present)| present.then_some(value)).collect::<Vec<_>>();
            put(out, Series::new(text(name, name_len)?.into(), values).into())
        });
        export!($scalar(frame: *const DataFrame, name: *const u8, name_len: usize, row: i64, out: *mut $ty, valid: *mut u8) {
            let frame = get(frame)?;
            let row = usize::try_from(row).ok().filter(|row| *row < frame.height())
                .ok_or_else(|| NativeError::new(7, format!("row {row} is outside dataframe height {}", frame.height())))?;
            let value = frame.column(text(name, name_len)?)?.$method()?.get(row);
            let out = out.as_mut().ok_or_else(|| NativeError::new(1, "null scalar output"))?;
            let valid = valid.as_mut().ok_or_else(|| NativeError::new(1, "null validity output"))?;
            *out = value.unwrap_or_default();
            *valid = u8::from(value.is_some());
            Ok(())
        });
        export!($output(frame: *const DataFrame, name: *const u8, name_len: usize, data: *mut u8, len: usize, stride: usize) {
            check_view::<$ty>(data, len, stride)?;
            let frame = get(frame)?;
            if len != frame.height() { return Err(NativeError::new(1, "output length does not match dataframe height")); }
            let column = frame.column(text(name, name_len)?)?.$method()?;
            if column.null_count() != 0 { return Err(NativeError::new(5, "column contains nulls; use nullable extraction or fill nulls in the query")); }
            for (i, value) in column.into_no_null_iter().enumerate() {
                data.add(i * stride).cast::<$ty>().write_unaligned(value);
            }
            Ok(())
        });
        export!($output_nullable(frame: *const DataFrame, name: *const u8, name_len: usize, data: *mut u8, len: usize, stride: usize, valid: *mut u8, valid_len: usize, valid_stride: usize) {
            check_view::<$ty>(data, len, stride)?;
            check_view::<u8>(valid, valid_len, valid_stride)?;
            let frame = get(frame)?;
            if len != frame.height() || valid_len != len { return Err(NativeError::new(1, "output lengths do not match dataframe height")); }
            let column = frame.column(text(name, name_len)?)?.$method()?;
            for (i, value) in column.iter().enumerate() {
                data.add(i * stride).cast::<$ty>().write_unaligned(value.unwrap_or_default());
                valid.add(i * valid_stride).write(u8::from(value.is_some()));
            }
            Ok(())
        });
        export!($literal(value: $ty, out: *mut *mut Expr) {
            // Untyped literals let Polars infer a wider/signed dtype and can
            // require i128 when optimizing casts of a large u64 constant.
            put(out, lit(Scalar::from(value)))
        });
    };
}
numeric!(
    i8,
    i8,
    pldf_v1_i8,
    pldf_v1_i8_nullable,
    pldf_v1_get_i8,
    pldf_v1_values_i8,
    pldf_v1_values_i8_nullable,
    pldf_v1_lit_i8
);
numeric!(
    i16,
    i16,
    pldf_v1_i16,
    pldf_v1_i16_nullable,
    pldf_v1_get_i16,
    pldf_v1_values_i16,
    pldf_v1_values_i16_nullable,
    pldf_v1_lit_i16
);
numeric!(
    i32,
    i32,
    pldf_v1_i32,
    pldf_v1_i32_nullable,
    pldf_v1_get_i32,
    pldf_v1_values_i32,
    pldf_v1_values_i32_nullable,
    pldf_v1_lit_i32
);
numeric!(
    u8,
    u8,
    pldf_v1_u8,
    pldf_v1_u8_nullable,
    pldf_v1_get_u8,
    pldf_v1_values_u8,
    pldf_v1_values_u8_nullable,
    pldf_v1_lit_u8
);
numeric!(
    u16,
    u16,
    pldf_v1_u16,
    pldf_v1_u16_nullable,
    pldf_v1_get_u16,
    pldf_v1_values_u16,
    pldf_v1_values_u16_nullable,
    pldf_v1_lit_u16
);
numeric!(
    u32,
    u32,
    pldf_v1_u32,
    pldf_v1_u32_nullable,
    pldf_v1_get_u32,
    pldf_v1_values_u32,
    pldf_v1_values_u32_nullable,
    pldf_v1_lit_u32
);
numeric!(
    u64,
    u64,
    pldf_v1_u64,
    pldf_v1_u64_nullable,
    pldf_v1_get_u64,
    pldf_v1_values_u64,
    pldf_v1_values_u64_nullable,
    pldf_v1_lit_u64
);
numeric!(
    f32,
    f32,
    pldf_v1_f32,
    pldf_v1_f32_nullable,
    pldf_v1_get_f32,
    pldf_v1_values_f32,
    pldf_v1_values_f32_nullable,
    pldf_v1_lit_f32
);
