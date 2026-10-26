use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    unsafe fn success(error: *mut NativeError) {
        if !error.is_null() {
            // SAFETY: returned errors are uniquely owned by the caller.
            panic!("{:?}", unsafe { Box::from_raw(error) });
        }
    }
    unsafe fn failure(error: *mut NativeError, kind: i32) {
        assert!(!error.is_null());
        assert_eq!(unsafe { Box::from_raw(error) }.kind, kind);
    }

    macro_rules! roundtrip {
        ($test:ident, $ty:ty, $dtype:ident, $input:ident, $nullable:ident, $scalar:ident, $output:ident, $output_nullable:ident, $literal:ident, $code:expr) => {
            #[test]
            fn $test() {
                // SAFETY: every view references a live array with its true stride.
                // Successful output handles transfer exactly once to Box.
                unsafe {
                    let input = [<$ty>::MIN, 0 as $ty, <$ty>::MAX];
                    let mut column = ptr::null_mut();
                    success($input(b"v".as_ptr(), 1, input.as_ptr().cast(), 3, size_of::<$ty>(), &mut column));
                    let column = *Box::from_raw(column);
                    assert_eq!(column.dtype(), &DataType::$dtype);
                    let mut frame = DataFrame::new(3, vec![column]).unwrap();
                    let mut output = [0 as $ty; 3];
                    success($output(&frame, b"v".as_ptr(), 1, output.as_mut_ptr().cast(), 3, size_of::<$ty>()));
                    assert_eq!(output, input);
                    let (mut scalar, mut present) = (0 as $ty, 0);
                    success($scalar(&frame, b"v".as_ptr(), 1, 2, &mut scalar, &mut present));
                    assert_eq!((scalar, present), (<$ty>::MAX, 1));
                    failure($scalar(&frame, b"v".as_ptr(), 1, -1, &mut scalar, &mut present), 7);
                    failure($scalar(&frame, b"v".as_ptr(), 1, 3, &mut scalar, &mut present), 7);
                    failure($output(&frame, b"missing".as_ptr(), 7, output.as_mut_ptr().cast(), 3, size_of::<$ty>()), 2);
                    failure($output(&frame, b"v".as_ptr(), 1, output.as_mut_ptr().cast(), 2, size_of::<$ty>()), 1);
                    let wrong = df!("v" => &[1i64, 2, 3]).unwrap();
                    failure($output(&wrong, b"v".as_ptr(), 1, output.as_mut_ptr().cast(), 3, size_of::<$ty>()), 2);

                    let path = std::env::temp_dir().join(format!("plenty-numeric-{}-{}.parquet", stringify!($ty), std::process::id()));
                    ParquetWriter::new(File::create(&path).unwrap()).finish(&mut frame).unwrap();
                    let restored = ParquetReader::new(File::open(&path).unwrap()).finish().unwrap();
                    std::fs::remove_file(path).unwrap();
                    assert_eq!(restored.schema(), frame.schema());
                    success($output(&restored, b"v".as_ptr(), 1, output.as_mut_ptr().cast(), 3, size_of::<$ty>()));
                    assert_eq!(output, input);

                    let mut literal = ptr::null_mut();
                    success($literal(<$ty>::MAX, &mut literal));
                    let literal = *Box::from_raw(literal);
                    let result = frame.clone().lazy().select([literal.alias("v")]).collect().unwrap();
                    assert_eq!(result.column("v").unwrap().dtype(), &DataType::$dtype);
                    success($scalar(&result, b"v".as_ptr(), 1, 0, &mut scalar, &mut present));
                    assert_eq!(scalar, <$ty>::MAX);
                    assert_eq!(numeric_dtype($code), Some(DataType::$dtype));

                    for mask in [[1u8, 0, 1], [0u8, 0, 0]] {
                        let mut column = ptr::null_mut();
                        success($nullable(b"v".as_ptr(), 1, input.as_ptr().cast(), 3, size_of::<$ty>(), mask.as_ptr(), 3, 1, &mut column));
                        let frame = DataFrame::new(3, vec![*Box::from_raw(column)]).unwrap();
                        let mut valid = [9u8; 3];
                        success($output_nullable(&frame, b"v".as_ptr(), 1, output.as_mut_ptr().cast(), 3, size_of::<$ty>(), valid.as_mut_ptr(), 3, 1));
                        assert_eq!(valid, mask);
                        for i in 0..3 { assert_eq!(output[i], if mask[i] == 0 { 0 as $ty } else { input[i] }); }
                        failure($output(&frame, b"v".as_ptr(), 1, output.as_mut_ptr().cast(), 3, size_of::<$ty>()), 5);
                        success($scalar(&frame, b"v".as_ptr(), 1, 1, &mut scalar, &mut present));
                        assert_eq!((scalar, present), (0 as $ty, 0));
                    }
                    let mut empty = ptr::null_mut();
                    success($nullable(b"v".as_ptr(), 1, ptr::null(), 0, 0, ptr::null(), 0, 0, &mut empty));
                    let empty = Box::from_raw(empty);
                    assert_eq!(empty.dtype(), &DataType::$dtype);
                    assert!(empty.is_empty());
                    let mut rejected = ptr::null_mut();
                    failure($nullable(b"v".as_ptr(), 1, input.as_ptr().cast(), 3, size_of::<$ty>(), [1u8, 2, 1].as_ptr(), 3, 1, &mut rejected), 1);
                    assert!(rejected.is_null());
                }
            }
        };
    }
    roundtrip!(
        roundtrip_i8,
        i8,
        Int8,
        pldf_v1_i8,
        pldf_v1_i8_nullable,
        pldf_v1_get_i8,
        pldf_v1_values_i8,
        pldf_v1_values_i8_nullable,
        pldf_v1_lit_i8,
        10
    );
    roundtrip!(
        roundtrip_i16,
        i16,
        Int16,
        pldf_v1_i16,
        pldf_v1_i16_nullable,
        pldf_v1_get_i16,
        pldf_v1_values_i16,
        pldf_v1_values_i16_nullable,
        pldf_v1_lit_i16,
        11
    );
    roundtrip!(
        roundtrip_i32,
        i32,
        Int32,
        pldf_v1_i32,
        pldf_v1_i32_nullable,
        pldf_v1_get_i32,
        pldf_v1_values_i32,
        pldf_v1_values_i32_nullable,
        pldf_v1_lit_i32,
        12
    );
    roundtrip!(
        roundtrip_u8,
        u8,
        UInt8,
        pldf_v1_u8,
        pldf_v1_u8_nullable,
        pldf_v1_get_u8,
        pldf_v1_values_u8,
        pldf_v1_values_u8_nullable,
        pldf_v1_lit_u8,
        13
    );
    roundtrip!(
        roundtrip_u16,
        u16,
        UInt16,
        pldf_v1_u16,
        pldf_v1_u16_nullable,
        pldf_v1_get_u16,
        pldf_v1_values_u16,
        pldf_v1_values_u16_nullable,
        pldf_v1_lit_u16,
        14
    );
    roundtrip!(
        roundtrip_u32,
        u32,
        UInt32,
        pldf_v1_u32,
        pldf_v1_u32_nullable,
        pldf_v1_get_u32,
        pldf_v1_values_u32,
        pldf_v1_values_u32_nullable,
        pldf_v1_lit_u32,
        15
    );
    roundtrip!(
        roundtrip_u64,
        u64,
        UInt64,
        pldf_v1_u64,
        pldf_v1_u64_nullable,
        pldf_v1_get_u64,
        pldf_v1_values_u64,
        pldf_v1_values_u64_nullable,
        pldf_v1_lit_u64,
        16
    );
    roundtrip!(
        roundtrip_f32,
        f32,
        Float32,
        pldf_v1_f32,
        pldf_v1_f32_nullable,
        pldf_v1_get_f32,
        pldf_v1_values_f32,
        pldf_v1_values_f32_nullable,
        pldf_v1_lit_f32,
        17
    );

    #[test]
    fn f32_special_values_and_strided_padding() {
        #[repr(C)]
        struct Slot {
            value: f32,
            padding: u32,
        }
        let input = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.0];
        // SAFETY: views describe the scalar member only, with the actual Slot stride.
        unsafe {
            let mut column = ptr::null_mut();
            success(pldf_v1_f32(
                b"v".as_ptr(),
                1,
                input.as_ptr().cast(),
                4,
                4,
                &mut column,
            ));
            let frame = DataFrame::new(4, vec![*Box::from_raw(column)]).unwrap();
            let mut out = std::array::from_fn::<Slot, 4, _>(|_| Slot {
                value: 0.0,
                padding: 0x12345678,
            });
            success(pldf_v1_values_f32(
                &frame,
                b"v".as_ptr(),
                1,
                out.as_mut_ptr().cast(),
                4,
                size_of::<Slot>(),
            ));
            for (slot, input) in out.iter().zip(input) {
                assert_eq!(slot.value.to_bits(), input.to_bits());
                assert_eq!(slot.padding, 0x12345678);
            }
        }
    }

    #[test]
    fn casts_preserve_u64_range_and_check_narrowing() {
        // SAFETY: plans and scalar output slots remain live during each native call.
        unsafe {
            let mut expr = ptr::null_mut();
            success(pldf_v1_lit_u64(u64::MAX, &mut expr));
            let expr = Box::from_raw(expr);
            let mut cast = ptr::null_mut();
            success(pldf_v1_cast(&*expr, 0, 1, &mut cast));
            assert!(
                DataFrame::empty()
                    .lazy()
                    .select([*Box::from_raw(cast)])
                    .collect()
                    .is_err()
            );
            let mut cast = ptr::null_mut();
            success(pldf_v1_cast(&*expr, 0, 0, &mut cast));
            let result = DataFrame::empty()
                .lazy()
                .select([*Box::from_raw(cast)])
                .collect()
                .unwrap();
            assert_eq!(result.columns()[0].null_count(), 1);
            for code in 10..18 {
                let mut cast = ptr::null_mut();
                let expr = lit(1i64);
                success(pldf_v1_cast(&expr, code, 1, &mut cast));
                let result = DataFrame::empty()
                    .lazy()
                    .select([*Box::from_raw(cast)])
                    .collect()
                    .unwrap();
                assert_eq!(result.columns()[0].dtype(), &numeric_dtype(code).unwrap());
            }
        }
    }
}
