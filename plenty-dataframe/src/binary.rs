//! Binary values cross the boundary as packed bytes, offsets, and validity.
//! Snapshots retain native buffers, never pointers into Plenty owners.
use super::*;

pub struct NativeBinary(BinaryChunked);
destroy!(pldf_v1_binary_values_drop, NativeBinary);

fn present(value: u8) -> Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(NativeError::new(1, "validity must be 0 or 1")),
    }
}

export!(pldf_v1_binary_column(name: *const u8, name_len: usize, data: *const u8, len: usize, stride: usize, offsets: *const u8, offsets_len: usize, offsets_stride: usize, valid: *const u8, valid_len: usize, valid_stride: usize, out: *mut *mut Column) {
    let packed = values::<u8>(data, len, stride)?;
    let offsets = values::<i64>(offsets, offsets_len, offsets_stride)?;
    let valid = values::<u8>(valid, valid_len, valid_stride)?;
    if valid.len().checked_add(1) != Some(offsets.len()) || offsets.first() != Some(&0) || offsets.last().copied() != Some(len as i64) {
        return Err(NativeError::new(1, "binary offsets must cover the packed bytes and have one more entry than validity"));
    }
    let values = offsets.windows(2).zip(valid).map(|(pair, flag)| {
        let start = usize::try_from(pair[0]).ok();
        let end = usize::try_from(pair[1]).ok();
        let bytes = start.zip(end).and_then(|(start, end)| packed.get(start..end))
            .ok_or_else(|| NativeError::new(1, "binary offsets must be ordered byte boundaries"))?;
        Ok(present(flag)?.then_some(bytes))
    }).collect::<Result<Vec<_>>>()?;
    put(out, Series::new(text(name, name_len)?.into(), values).into())
});

export!(pldf_v1_get_binary(frame: *const DataFrame, name: *const u8, name_len: usize, row: i64, out: *mut *mut NativeBinary) {
    let frame = get(frame)?;
    let index = usize::try_from(row).ok().filter(|row| *row < frame.height())
        .ok_or_else(|| NativeError::new(7, "binary row is outside dataframe height"))?;
    let column = frame.column(text(name, name_len)?)?.binary()?;
    put(out, NativeBinary(column.slice(index as i64, 1)))
});

export!(pldf_v1_values_binary(frame: *const DataFrame, name: *const u8, name_len: usize, nullable: u8, out: *mut *mut NativeBinary) {
    let nullable = present(nullable)?;
    let column = get(frame)?.column(text(name, name_len)?)?.binary()?;
    if !nullable && column.null_count() != 0 {
        return Err(NativeError::new(5, "column contains nulls; use nullable extraction or fill nulls in the query"));
    }
    put(out, NativeBinary(column.clone()))
});

impl NativeBinary {
    fn byte_len(&self) -> Result<usize> {
        self.0
            .iter()
            .flatten()
            .try_fold(0usize, |total, value| total.checked_add(value.len()))
            .filter(|total| *total <= isize::MAX as usize)
            .ok_or_else(|| {
                NativeError::new(1, "packed binary data exceeds addressable byte length")
            })
    }
}

export!(pldf_v1_binary_values_meta(values: *const NativeBinary, rows: *mut i64, bytes: *mut i64) {
    let values = get(values)?;
    let rows = rows.as_mut().ok_or_else(|| NativeError::new(1, "null binary row-count output"))?;
    let bytes = bytes.as_mut().ok_or_else(|| NativeError::new(1, "null binary byte-count output"))?;
    *rows = values.0.len() as i64;
    *bytes = values.byte_len()? as i64;
    Ok(())
});

export!(pldf_v1_binary_values_copy(values: *const NativeBinary, bytes: *mut u8, bytes_len: usize, bytes_stride: usize, offsets: *mut u8, offsets_len: usize, offsets_stride: usize, valid: *mut u8, valid_len: usize, valid_stride: usize) {
    let values = get(values)?;
    check_view::<u8>(bytes, bytes_len, bytes_stride)?;
    check_view::<i64>(offsets, offsets_len, offsets_stride)?;
    check_view::<u8>(valid, valid_len, valid_stride)?;
    if values.0.len().checked_add(1) != Some(offsets_len) || valid_len != values.0.len() || bytes_len != values.byte_len()? {
        return Err(NativeError::new(1, "binary output buffers have incorrect lengths"));
    }
    offsets.cast::<i64>().write_unaligned(0);
    let mut position = 0usize;
    for (i, value) in values.0.iter().enumerate() {
        valid.add(i * valid_stride).write(u8::from(value.is_some()));
        if let Some(value) = value {
            for byte in value {
                bytes.add(position * bytes_stride).write(*byte);
                position += 1;
            }
        }
        offsets.add((i + 1) * offsets_stride).cast::<i64>().write_unaligned(position as i64);
    }
    Ok(())
});

export!(pldf_v1_lit_binary(data: *const u8, len: usize, stride: usize, out: *mut *mut Expr) {
    put(out, lit(values::<u8>(data, len, stride)?))
});

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn success(error: *mut NativeError) {
        if !error.is_null() {
            // SAFETY: the ABI transfers unique ownership of each error.
            panic!("{:?}", unsafe { Box::from_raw(error) });
        }
    }

    unsafe fn failure(error: *mut NativeError, kind: i32) {
        assert!(!error.is_null());
        assert_eq!(unsafe { Box::from_raw(error) }.kind, kind);
    }

    unsafe fn construct(packed: &[u8], offsets: &[i64], valid: &[u8]) -> *mut Column {
        let mut out = ptr::null_mut();
        unsafe {
            success(pldf_v1_binary_column(
                b"b".as_ptr(),
                1,
                packed.as_ptr(),
                packed.len(),
                1,
                offsets.as_ptr().cast(),
                offsets.len(),
                8,
                valid.as_ptr(),
                valid.len(),
                1,
                &mut out,
            ));
        }
        out
    }

    #[test]
    fn binary_snapshot_owns_data_and_copies_strided_buffers() {
        // SAFETY: views are initialized for their declared sizes; every native
        // owner is recovered once, and snapshots may outlive the source frame.
        unsafe {
            let column = *Box::from_raw(construct(&[0xff, 0, 0x80], &[0, 3, 3, 3], &[1, 1, 0]));
            assert_eq!(column.dtype(), &DataType::Binary);
            let frame = DataFrame::new(3, vec![column]).unwrap();
            let mut snapshot = ptr::null_mut();
            success(pldf_v1_values_binary(
                &frame,
                b"b".as_ptr(),
                1,
                1,
                &mut snapshot,
            ));
            failure(
                pldf_v1_values_binary(&frame, b"b".as_ptr(), 1, 0, &mut ptr::null_mut()),
                5,
            );
            failure(
                pldf_v1_get_binary(&frame, b"b".as_ptr(), 1, -1, &mut ptr::null_mut()),
                7,
            );
            failure(
                pldf_v1_get_binary(&frame, b"b".as_ptr(), 1, 3, &mut ptr::null_mut()),
                7,
            );
            drop(frame);
            let mut rows = 0;
            let mut len = 0;
            success(pldf_v1_binary_values_meta(snapshot, &mut rows, &mut len));
            assert_eq!((rows, len), (3, 3));
            let mut bytes = [99u8; 6];
            let mut offsets = [99i64; 8];
            let mut validity = [99u8; 6];
            success(pldf_v1_binary_values_copy(
                snapshot,
                bytes.as_mut_ptr(),
                3,
                2,
                offsets.as_mut_ptr().cast(),
                4,
                16,
                validity.as_mut_ptr(),
                3,
                2,
            ));
            assert_eq!(bytes, [0xff, 99, 0, 99, 0x80, 99]);
            assert_eq!(offsets, [0, 99, 3, 99, 3, 99, 3, 99]);
            assert_eq!(validity, [1, 99, 1, 99, 0, 99]);
            failure(
                pldf_v1_binary_values_copy(
                    snapshot,
                    bytes.as_mut_ptr(),
                    2,
                    2,
                    offsets.as_mut_ptr().cast(),
                    4,
                    16,
                    validity.as_mut_ptr(),
                    3,
                    2,
                ),
                1,
            );
            pldf_v1_binary_values_drop(snapshot);
        }
    }

    #[test]
    fn empty_and_all_null_columns_keep_binary_dtype() {
        // SAFETY: slices remain alive during each call, unique outputs consumed once.
        unsafe {
            for (offsets, validity) in [(&[0i64][..], &[][..]), (&[0, 0, 0][..], &[0u8, 0][..])] {
                let column = Box::from_raw(construct(&[], offsets, validity));
                assert_eq!(column.dtype(), &DataType::Binary);
                assert_eq!(column.len(), validity.len());
                assert_eq!(column.null_count(), validity.len());
            }
        }
    }

    #[test]
    fn malformed_binary_offsets_and_validity_are_errors() {
        // SAFETY: invalid metadata still refers only to readable initialized slices.
        unsafe {
            for (offsets, valid) in [
                (vec![1i64, 1], vec![1u8]),
                (vec![0, 2, 1], vec![1, 1]),
                (vec![0, -1, 1], vec![1, 1]),
                (vec![0, 1], vec![2]),
                (vec![0, 1], vec![]),
                (vec![], vec![]),
            ] {
                let mut out = ptr::null_mut();
                failure(
                    pldf_v1_binary_column(
                        b"b".as_ptr(),
                        1,
                        b"x".as_ptr(),
                        1,
                        1,
                        offsets.as_ptr().cast(),
                        offsets.len(),
                        8,
                        valid.as_ptr(),
                        valid.len(),
                        1,
                        &mut out,
                    ),
                    1,
                );
                assert!(out.is_null());
            }
        }
    }
}
