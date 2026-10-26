//! Typed, nullable transfers. No pointer into Plenty's tagged values is retained.
use super::*;

#[derive(Debug)]
pub struct NativeStrings(Vec<Option<String>>);
destroy!(pldf_v1_strings_drop, NativeStrings);

fn flag(value: u8) -> Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(NativeError::new(
            1,
            "boolean and validity bytes must be 0 or 1",
        )),
    }
}

pub(super) unsafe fn validity(
    data: *const u8,
    len: usize,
    stride: usize,
    expected: usize,
) -> Result<Vec<bool>> {
    if len != expected {
        return Err(NativeError::new(1, "validity length does not match values"));
    }
    unsafe { values::<u8>(data, len, stride)? }
        .into_iter()
        .map(flag)
        .collect()
}

macro_rules! nullable_input {
    ($name:ident, $ty:ty) => {
        export!($name(name: *const u8, name_len: usize, data: *const u8, len: usize, stride: usize, valid: *const u8, valid_len: usize, valid_stride: usize, out: *mut *mut Column) {
            let valid = validity(valid, valid_len, valid_stride, len)?;
            let values = values::<$ty>(data, len, stride)?.into_iter().zip(valid).map(|(v, present)| present.then_some(v)).collect::<Vec<_>>();
            put(out, Series::new(text(name, name_len)?.into(), values).into())
        });
    };
}
nullable_input!(pldf_v1_i64_nullable, i64);
nullable_input!(pldf_v1_f64_nullable, f64);
export!(pldf_v1_bool(name: *const u8, name_len: usize, data: *const u8, len: usize, stride: usize, out: *mut *mut Column) {
    let values = values::<u8>(data, len, stride)?.into_iter().map(flag).collect::<Result<Vec<_>>>()?;
    put(out, Series::new(text(name, name_len)?.into(), values).into())
});
export!(pldf_v1_bool_nullable(name: *const u8, name_len: usize, data: *const u8, len: usize, stride: usize, valid: *const u8, valid_len: usize, valid_stride: usize, out: *mut *mut Column) {
    let valid = validity(valid, valid_len, valid_stride, len)?;
    let values = values::<u8>(data, len, stride)?.into_iter().zip(valid).map(|(v, present)| flag(v).map(|v| present.then_some(v))).collect::<Result<Vec<_>>>()?;
    put(out, Series::new(text(name, name_len)?.into(), values).into())
});

unsafe fn unpack_strings<'a>(
    packed: *const u8,
    packed_len: usize,
    offsets: *const u8,
    offsets_len: usize,
    offsets_stride: usize,
) -> Result<Vec<&'a str>> {
    let text = unsafe { text(packed, packed_len)? };
    let offsets = unsafe { values::<i64>(offsets, offsets_len, offsets_stride)? };
    if offsets.first() != Some(&0) || offsets.last().copied() != Some(packed_len as i64) {
        return Err(NativeError::new(
            1,
            "string offsets must start at zero and end at the UTF-8 byte length",
        ));
    }
    offsets
        .windows(2)
        .map(|pair| {
            let start = usize::try_from(pair[0]).ok();
            let end = usize::try_from(pair[1]).ok();
            start
                .zip(end)
                .and_then(|(start, end)| text.get(start..end))
                .ok_or_else(|| {
                    NativeError::new(1, "string offsets must be ordered UTF-8 boundaries")
                })
        })
        .collect()
}
export!(pldf_v1_str(name: *const u8, name_len: usize, packed: *const u8, packed_len: usize, offsets: *const u8, offsets_len: usize, offsets_stride: usize, out: *mut *mut Column) {
    let strings = unpack_strings(packed, packed_len, offsets, offsets_len, offsets_stride)?;
    put(out, Series::new(text(name, name_len)?.into(), strings).into())
});
export!(pldf_v1_str_nullable(name: *const u8, name_len: usize, packed: *const u8, packed_len: usize, offsets: *const u8, offsets_len: usize, offsets_stride: usize, valid: *const u8, valid_len: usize, valid_stride: usize, out: *mut *mut Column) {
    let strings = unpack_strings(packed, packed_len, offsets, offsets_len, offsets_stride)?;
    let valid = validity(valid, valid_len, valid_stride, strings.len())?;
    let strings = strings.into_iter().zip(valid).map(|(v, present)| present.then_some(v)).collect::<Vec<_>>();
    put(out, Series::new(text(name, name_len)?.into(), strings).into())
});

macro_rules! nullable_output {
    ($name:ident, $ty:ty, $method:ident) => {
        export!($name(frame: *const DataFrame, name: *const u8, name_len: usize, data: *mut u8, len: usize, stride: usize, valid: *mut u8, valid_len: usize, valid_stride: usize) {
            check_view::<$ty>(data, len, stride)?;
            check_view::<u8>(valid, valid_len, valid_stride)?;
            let frame = get(frame)?;
            if len != frame.height() || valid_len != len { return Err(NativeError::new(1, "output lengths do not match dataframe height")); }
            let column = frame.column(text(name, name_len)?)?.$method()?;
            for (i, value) in column.iter().enumerate() {
                data.add(i * stride).cast::<$ty>().write_unaligned(<$ty>::from(value.unwrap_or_default()));
                valid.add(i * valid_stride).write(u8::from(value.is_some()));
            }
            Ok(())
        });
    };
}
nullable_output!(pldf_v1_values_i64_nullable, i64, i64);
nullable_output!(pldf_v1_values_f64_nullable, f64, f64);
nullable_output!(pldf_v1_values_bool_nullable, u8, bool);
export!(pldf_v1_values_bool(frame: *const DataFrame, name: *const u8, name_len: usize, data: *mut u8, len: usize, stride: usize) {
    check_view::<u8>(data, len, stride)?;
    let frame = get(frame)?;
    if len != frame.height() { return Err(NativeError::new(1, "output length does not match dataframe height")); }
    let column = frame.column(text(name, name_len)?)?.bool()?;
    if column.null_count() != 0 { return Err(NativeError::new(5, "column contains nulls; use nullable extraction or fill nulls in the query")); }
    for (i, value) in column.iter().enumerate() { data.add(i * stride).write(u8::from(value.unwrap_or_default())); }
    Ok(())
});
export!(pldf_v1_get_bool(frame: *const DataFrame, name: *const u8, name_len: usize, row: i64, out: *mut u8, valid: *mut u8) {
    let frame = get(frame)?;
    let row = checked_row(frame, row)?;
    let value = frame.column(text(name, name_len)?)?.bool()?.get(row);
    let out = out.as_mut().ok_or_else(|| NativeError::new(1, "null scalar output"))?;
    let valid = valid.as_mut().ok_or_else(|| NativeError::new(1, "null validity output"))?;
    *out = u8::from(value.unwrap_or_default());
    *valid = u8::from(value.is_some());
    Ok(())
});
fn checked_row(frame: &DataFrame, row: i64) -> Result<usize> {
    usize::try_from(row)
        .ok()
        .filter(|row| *row < frame.height())
        .ok_or_else(|| {
            NativeError::new(
                7,
                format!("row {row} is outside dataframe height {}", frame.height()),
            )
        })
}
export!(pldf_v1_get_str(frame: *const DataFrame, name: *const u8, name_len: usize, row: i64, out: *mut *mut NativeStrings) {
    let frame = get(frame)?;
    let row = checked_row(frame, row)?;
    let value = frame.column(text(name, name_len)?)?.str()?.get(row).map(str::to_owned);
    put(out, NativeStrings(vec![value]))
});
export!(pldf_v1_values_str(frame: *const DataFrame, name: *const u8, name_len: usize, nullable: u8, out: *mut *mut NativeStrings) {
    let nullable = flag(nullable)?;
    let column = get(frame)?.column(text(name, name_len)?)?.str()?;
    if !nullable && column.null_count() != 0 { return Err(NativeError::new(5, "column contains nulls; use nullable extraction or fill nulls in the query")); }
    put(out, NativeStrings(column.iter().map(|s| s.map(str::to_owned)).collect()))
});
impl NativeStrings {
    fn byte_len(&self) -> Result<usize> {
        self.0
            .iter()
            .flatten()
            .try_fold(0usize, |total, s| total.checked_add(s.len()))
            .filter(|total| *total <= isize::MAX as usize)
            .ok_or_else(|| NativeError::new(1, "packed strings exceed addressable byte length"))
    }
}
export!(pldf_v1_strings_meta(strings: *const NativeStrings, rows: *mut i64, bytes: *mut i64) {
    let strings = get(strings)?;
    let count = i64::try_from(strings.0.len()).map_err(|_| NativeError::new(1, "too many strings"))?;
    let byte_len = strings.byte_len()?;
    let rows = rows.as_mut().ok_or_else(|| NativeError::new(1, "null count output"))?;
    let bytes = bytes.as_mut().ok_or_else(|| NativeError::new(1, "null length output"))?;
    *rows = count;
    *bytes = byte_len as i64;
    Ok(())
});
export!(pldf_v1_strings_copy(strings: *const NativeStrings, bytes: *mut u8, bytes_len: usize, bytes_stride: usize, offsets: *mut u8, offsets_len: usize, offsets_stride: usize, valid: *mut u8, valid_len: usize, valid_stride: usize) {
    let strings = get(strings)?;
    check_view::<u8>(bytes, bytes_len, bytes_stride)?;
    check_view::<i64>(offsets, offsets_len, offsets_stride)?;
    check_view::<u8>(valid, valid_len, valid_stride)?;
    if strings.0.len().checked_add(1) != Some(offsets_len) || valid_len != strings.0.len() || bytes_len != strings.byte_len()? {
        return Err(NativeError::new(1, "string output buffers have incorrect lengths"));
    }
    offsets.cast::<i64>().write_unaligned(0);
    let mut position = 0usize;
    for (i, value) in strings.0.iter().enumerate() {
        valid.add(i * valid_stride).write(u8::from(value.is_some()));
        if let Some(value) = value {
            for byte in value.bytes() {
                bytes.add(position * bytes_stride).write(byte);
                position += 1;
            }
        }
        offsets.add((i + 1) * offsets_stride).cast::<i64>().write_unaligned(position as i64);
    }
    Ok(())
});

fn schema_strings(schema: &Schema) -> NativeStrings {
    NativeStrings(
        schema
            .iter()
            .flat_map(|(name, dtype)| {
                [
                    Some(name.to_string()),
                    Some(super::temporal::dtype_descriptor(dtype)),
                ]
            })
            .collect(),
    )
}
export!(pldf_v1_frame_schema(frame: *const DataFrame, out: *mut *mut NativeStrings) {
    put(out, schema_strings(get(frame)?.schema()))
});
export!(pldf_v1_query_schema(query: *const LazyFrame, out: *mut *mut NativeStrings) {
    let schema = get(query)?.clone().collect_schema()?;
    put(out, schema_strings(&schema))
});

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn success(error: *mut NativeError) {
        if !error.is_null() {
            // SAFETY: each returned error transfers unique ownership here.
            panic!("{:?}", unsafe { Box::from_raw(error) });
        }
    }
    unsafe fn failure(error: *mut NativeError, kind: i32) {
        assert!(!error.is_null());
        assert_eq!(unsafe { Box::from_raw(error) }.kind, kind);
    }

    #[test]
    fn numeric_and_boolean_nullable_transfers_preserve_types_nulls_and_nan() {
        // SAFETY: all views are live arrays with their actual scalar strides;
        // returned owners are consumed exactly once.
        unsafe {
            macro_rules! roundtrip {
                ($input:ident, $output:ident, $ty:ty, $values:expr, $dtype:expr) => {{
                    let input: [$ty; 3] = $values;
                    for valid in [[1u8, 0, 1], [0, 0, 0]] {
                        let mut column = ptr::null_mut();
                        success($input(
                            b"v".as_ptr(),
                            1,
                            input.as_ptr().cast(),
                            3,
                            size_of::<$ty>(),
                            valid.as_ptr(),
                            3,
                            1,
                            &mut column,
                        ));
                        let column = *Box::from_raw(column);
                        assert_eq!(column.dtype(), &$dtype);
                        let frame = DataFrame::new(3, vec![column]).unwrap();
                        let mut output = [<$ty>::default(); 3];
                        let mut mask = [2u8; 3];
                        success($output(
                            &frame,
                            b"v".as_ptr(),
                            1,
                            output.as_mut_ptr().cast(),
                            3,
                            size_of::<$ty>(),
                            mask.as_mut_ptr(),
                            3,
                            1,
                        ));
                        assert_eq!(mask, valid);
                        for i in 0..3 {
                            if valid[i] == 0 {
                                assert_eq!(output[i], <$ty>::default());
                            } else {
                                assert_eq!(output[i], input[i]);
                            }
                        }
                    }
                    let mut column = ptr::null_mut();
                    success($input(
                        b"v".as_ptr(),
                        1,
                        ptr::null(),
                        0,
                        0,
                        ptr::null(),
                        0,
                        0,
                        &mut column,
                    ));
                    let column = Box::from_raw(column);
                    assert_eq!(column.dtype(), &$dtype);
                    assert!(column.is_empty());
                }};
            }
            roundtrip!(
                pldf_v1_i64_nullable,
                pldf_v1_values_i64_nullable,
                i64,
                [i64::MIN, 0, i64::MAX],
                DataType::Int64
            );
            roundtrip!(
                pldf_v1_f64_nullable,
                pldf_v1_values_f64_nullable,
                f64,
                [f64::INFINITY, 0.0, -0.0],
                DataType::Float64
            );
            roundtrip!(
                pldf_v1_bool_nullable,
                pldf_v1_values_bool_nullable,
                u8,
                [1, 0, 0],
                DataType::Boolean
            );
            let frame = df!("v" => &[Some(f64::NAN), None]).unwrap();
            let mut values = [0.0f64; 2];
            let mut valid = [0u8; 2];
            success(pldf_v1_values_f64_nullable(
                &frame,
                b"v".as_ptr(),
                1,
                values.as_mut_ptr().cast(),
                2,
                8,
                valid.as_mut_ptr(),
                2,
                1,
            ));
            assert!(values[0].is_nan());
            assert_eq!(valid, [1, 0]);
        }
    }

    unsafe fn packed_copy(strings: *const NativeStrings) -> (Vec<u8>, Vec<i64>, Vec<u8>) {
        // SAFETY: matching owner remains borrowed for metadata and copy calls.
        unsafe {
            let (mut rows, mut count) = (0, 0);
            success(pldf_v1_strings_meta(strings, &mut rows, &mut count));
            let mut bytes = vec![0u8; count as usize];
            let mut offsets = vec![0i64; rows as usize + 1];
            let mut valid = vec![0u8; rows as usize];
            success(pldf_v1_strings_copy(
                strings,
                bytes.as_mut_ptr(),
                bytes.len(),
                1,
                offsets.as_mut_ptr().cast(),
                offsets.len(),
                8,
                valid.as_mut_ptr(),
                valid.len(),
                1,
            ));
            (bytes, offsets, valid)
        }
    }

    #[test]
    fn string_snapshots_own_unicode_empty_null_and_nul_values() {
        // SAFETY: input buffers live during construction; snapshots own copies.
        unsafe {
            let packed = "é😀\0";
            let offsets = [0i64, 2, 2, 2, 7];
            let valid = [1u8, 0, 1, 1];
            let mut column = ptr::null_mut();
            success(pldf_v1_str_nullable(
                b"s".as_ptr(),
                1,
                packed.as_ptr(),
                packed.len(),
                offsets.as_ptr().cast(),
                5,
                8,
                valid.as_ptr(),
                4,
                1,
                &mut column,
            ));
            let frame = DataFrame::new(4, vec![*Box::from_raw(column)]).unwrap();
            let mut strings = ptr::null_mut();
            success(pldf_v1_values_str(
                &frame,
                b"s".as_ptr(),
                1,
                1,
                &mut strings,
            ));
            let mut scalar = ptr::null_mut();
            success(pldf_v1_get_str(&frame, b"s".as_ptr(), 1, 1, &mut scalar));
            let mut rejected = ptr::null_mut();
            failure(
                pldf_v1_values_str(&frame, b"s".as_ptr(), 1, 0, &mut rejected),
                5,
            );
            assert!(rejected.is_null());
            failure(
                pldf_v1_get_str(&frame, b"s".as_ptr(), 1, -1, &mut rejected),
                7,
            );
            drop(frame);
            assert_eq!(
                packed_copy(strings),
                (packed.as_bytes().to_vec(), offsets.to_vec(), valid.to_vec())
            );
            assert_eq!(packed_copy(scalar), (vec![], vec![0, 0], vec![0]));
            pldf_v1_strings_drop(strings);
            pldf_v1_strings_drop(scalar);
            for (offsets, valid) in [(vec![0i64], vec![]), (vec![0, 0, 0], vec![0u8, 0])] {
                let mut column = ptr::null_mut();
                success(pldf_v1_str_nullable(
                    b"s".as_ptr(),
                    1,
                    ptr::null(),
                    0,
                    offsets.as_ptr().cast(),
                    offsets.len(),
                    8,
                    valid.as_ptr(),
                    valid.len(),
                    1,
                    &mut column,
                ));
                let column = Box::from_raw(column);
                assert_eq!(column.dtype(), &DataType::String);
                assert_eq!(column.null_count(), valid.len());
            }
        }
    }

    #[test]
    fn boolean_scalar_and_nonnull_bulk_preserve_padding() {
        #[repr(C)]
        struct Slot {
            value: u8,
            padding: [u8; 7],
        }
        // SAFETY: strided output describes only Slot.value, never padding.
        unsafe {
            let mut column = ptr::null_mut();
            let values = [1u8, 0];
            success(pldf_v1_bool(
                b"b".as_ptr(),
                1,
                values.as_ptr(),
                2,
                1,
                &mut column,
            ));
            let frame = DataFrame::new(2, vec![*Box::from_raw(column)]).unwrap();
            let mut out = [
                Slot {
                    value: 2,
                    padding: [0xaa; 7],
                },
                Slot {
                    value: 2,
                    padding: [0xbb; 7],
                },
            ];
            success(pldf_v1_values_bool(
                &frame,
                b"b".as_ptr(),
                1,
                out.as_mut_ptr().cast(),
                2,
                8,
            ));
            assert_eq!([out[0].value, out[1].value], values);
            assert_eq!(out[0].padding, [0xaa; 7]);
            assert_eq!(out[1].padding, [0xbb; 7]);
            let (mut value, mut valid) = (2, 2);
            success(pldf_v1_get_bool(
                &frame,
                b"b".as_ptr(),
                1,
                1,
                &mut value,
                &mut valid,
            ));
            assert_eq!((value, valid), (0, 1));
            failure(
                pldf_v1_get_bool(&frame, b"b".as_ptr(), 1, 2, &mut value, &mut valid),
                7,
            );
        }
    }

    #[test]
    fn malformed_inputs_and_typed_outputs_fail_before_writing() {
        // SAFETY: even deliberately invalid lengths refer to sufficiently sized
        // buffers, or are rejected before pointer access.
        unsafe {
            let mut out = ptr::null_mut();
            let invalid = [2u8];
            failure(
                pldf_v1_bool(b"v".as_ptr(), 1, invalid.as_ptr(), 1, 1, &mut out),
                1,
            );
            let values = [4i64];
            failure(
                pldf_v1_i64_nullable(
                    b"v".as_ptr(),
                    1,
                    values.as_ptr().cast(),
                    1,
                    8,
                    invalid.as_ptr(),
                    1,
                    1,
                    &mut out,
                ),
                1,
            );
            failure(
                pldf_v1_i64_nullable(
                    b"v".as_ptr(),
                    1,
                    values.as_ptr().cast(),
                    1,
                    8,
                    ptr::null(),
                    0,
                    0,
                    &mut out,
                ),
                1,
            );
            for offsets in [[0i64, 1, 2], [0, 2, 1], [-1, 0, 2], [0, 0, 3]] {
                failure(
                    pldf_v1_str(
                        b"s".as_ptr(),
                        1,
                        "é".as_ptr(),
                        2,
                        offsets.as_ptr().cast(),
                        3,
                        8,
                        &mut out,
                    ),
                    1,
                );
                assert!(out.is_null());
            }
            let frame = df!("v" => &[Some(1i64), None]).unwrap();
            let mut output = [9u8; 2];
            let mut valid = [9u8; 2];
            failure(
                pldf_v1_values_bool_nullable(
                    &frame,
                    b"v".as_ptr(),
                    1,
                    output.as_mut_ptr(),
                    2,
                    1,
                    valid.as_mut_ptr(),
                    2,
                    1,
                ),
                2,
            );
            assert_eq!(output, [9, 9]);
            assert_eq!(valid, [9, 9]);
            let mut number = [42i64; 2];
            failure(
                pldf_v1_values_i64_nullable(
                    &frame,
                    b"v".as_ptr(),
                    1,
                    number.as_mut_ptr().cast(),
                    2,
                    8,
                    valid.as_mut_ptr(),
                    1,
                    1,
                ),
                1,
            );
            assert_eq!(number, [42, 42]);
            let strings = NativeStrings(vec![Some("hi".into())]);
            let mut offsets = [99i64; 2];
            failure(
                pldf_v1_strings_copy(
                    &strings,
                    output.as_mut_ptr(),
                    1,
                    1,
                    offsets.as_mut_ptr().cast(),
                    2,
                    8,
                    valid.as_mut_ptr(),
                    1,
                    1,
                ),
                1,
            );
            assert_eq!(offsets, [99, 99]);
            assert_eq!(output, [9, 9]);
        }
    }

    #[test]
    fn parquet_schema_reports_unsupported_types_without_collecting_rows() {
        let nested = Series::new("nested".into(), &[Series::new("".into(), &[1i64, 2])]);
        let mut frame = DataFrame::new(
            1,
            vec![Series::new("small".into(), &[3i32]).into(), nested.into()],
        )
        .unwrap();
        let path =
            std::env::temp_dir().join(format!("plenty-schema-{}.parquet", std::process::id()));
        ParquetWriter::new(File::create(&path).unwrap())
            .finish(&mut frame)
            .unwrap();
        let query =
            LazyFrame::scan_parquet(PlRefPath::new(path.to_str().unwrap()), Default::default())
                .unwrap();
        // SAFETY: schema snapshots take their own names/type strings.
        unsafe {
            let (mut eager, mut lazy) = (ptr::null_mut(), ptr::null_mut());
            success(pldf_v1_frame_schema(&frame, &mut eager));
            success(pldf_v1_query_schema(&query, &mut lazy));
            drop((frame, query));
            std::fs::remove_file(&path).unwrap();
            assert_eq!((*eager).0, (*lazy).0);
            assert_eq!(
                (*lazy).0,
                vec![
                    Some("small".into()),
                    Some("Int32".into()),
                    Some("nested".into()),
                    Some("List(Int64)".into())
                ]
            );
            assert_eq!(packed_copy(eager), packed_copy(lazy));
            pldf_v1_strings_drop(eager);
            pldf_v1_strings_drop(lazy);
        }
    }
}
