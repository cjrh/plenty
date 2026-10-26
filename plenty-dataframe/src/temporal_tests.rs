use super::*;

unsafe fn success(error: *mut NativeError) {
    if !error.is_null() {
        panic!("{:?}", unsafe { Box::from_raw(error) });
    }
}
unsafe fn failure(error: *mut NativeError, kind: i32) {
    assert!(!error.is_null());
    let error = unsafe { Box::from_raw(error) };
    assert_eq!(error.kind, kind, "{error:?}");
}
fn date_column(values: &[i32], valid: &[u8]) -> Column {
    // SAFETY: scalar views are live, output transferred once.
    unsafe {
        let mut out = ptr::null_mut();
        success(pldf_v1_date(
            b"v".as_ptr(),
            1,
            values.as_ptr().cast(),
            values.len(),
            4,
            valid.as_ptr(),
            valid.len(),
            1,
            &mut out,
        ));
        *Box::from_raw(out)
    }
}
fn datetime_column(values: &[i64], valid: &[u8], unit: i32, utc: u8) -> Column {
    // SAFETY: scalar views are live, output transferred once.
    unsafe {
        let mut out = ptr::null_mut();
        success(pldf_v1_datetime(
            b"v".as_ptr(),
            1,
            values.as_ptr().cast(),
            values.len(),
            8,
            valid.as_ptr(),
            valid.len(),
            1,
            unit,
            utc,
            &mut out,
        ));
        *Box::from_raw(out)
    }
}
fn parse_expr(format: &str, unit: Option<i32>, utc: u8, strict: bool) -> Expr {
    // SAFETY: matching borrowed Expr and UTF-8 format; unique output.
    unsafe {
        let mut out = ptr::null_mut();
        if let Some(unit) = unit {
            success(pldf_v1_str_to_datetime(
                &col("s"),
                format.as_ptr(),
                format.len(),
                unit,
                utc,
                u8::from(strict),
                &mut out,
            ));
        } else {
            success(pldf_v1_str_to_date(
                &col("s"),
                format.as_ptr(),
                format.len(),
                u8::from(strict),
                &mut out,
            ));
        }
        *Box::from_raw(out)
    }
}
fn both(query: LazyFrame) -> DataFrame {
    let normal = query.clone().collect().unwrap();
    let streaming = query
        .collect_with_engine(Engine::Streaming)
        .unwrap()
        .unwrap_single();
    assert!(normal.equals_missing(&streaming));
    normal
}

#[test]
fn date_transfers_preserve_nulls_pre_epoch_empty_and_calendar_endpoints() {
    let frame = DataFrame::new(3, vec![date_column(&[-1, 0, i32::MAX], &[1, 1, 0])]).unwrap();
    // SAFETY: exact initialized output shapes, matching scalar views.
    unsafe {
        let (mut scalar, mut valid) = (99i32, 9u8);
        success(pldf_v1_get_date(
            &frame,
            b"v".as_ptr(),
            1,
            0,
            &mut scalar,
            &mut valid,
        ));
        assert_eq!((scalar, valid), (-1, 1));
        success(pldf_v1_get_date(
            &frame,
            b"v".as_ptr(),
            1,
            2,
            &mut scalar,
            &mut valid,
        ));
        assert_eq!((scalar, valid), (0, 0));
        let mut output = [99i32; 3];
        let mut mask = [9u8; 3];
        success(pldf_v1_values_date(
            &frame,
            b"v".as_ptr(),
            1,
            1,
            output.as_mut_ptr().cast(),
            3,
            4,
            mask.as_mut_ptr(),
            3,
            1,
        ));
        assert_eq!(output, [-1, 0, 0]);
        assert_eq!(mask, [1, 1, 0]);
        failure(
            pldf_v1_values_date(
                &frame,
                b"v".as_ptr(),
                1,
                0,
                output.as_mut_ptr().cast(),
                3,
                4,
                ptr::null_mut(),
                0,
                0,
            ),
            5,
        );
        failure(
            pldf_v1_get_date(&frame, b"v".as_ptr(), 1, -1, &mut scalar, &mut valid),
            7,
        );
    }
    assert_eq!(date_column(&[], &[]).dtype(), &DataType::Date);
    assert_eq!(date_column(&[i32::MIN, i32::MAX], &[0, 0]).null_count(), 2);
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
    for date in [NaiveDate::MIN, NaiveDate::MAX] {
        let days = date.signed_duration_since(epoch).num_days() as i32;
        assert_eq!(
            date_column(&[days], &[]).date().unwrap().physical().get(0),
            Some(days)
        );
    }
}

#[test]
fn datetime_units_timezone_and_nullable_transfers_are_checked_not_converted() {
    for unit_code in [0, 1, 2] {
        for utc in [0, 1] {
            let frame = DataFrame::new(
                3,
                vec![datetime_column(
                    &[-1, 0, i64::MAX],
                    &[1, 1, 0],
                    unit_code,
                    utc,
                )],
            )
            .unwrap();
            // SAFETY: matching live owners and exact scalar output views.
            unsafe {
                let (mut value, mut valid) = (99i64, 9u8);
                success(pldf_v1_get_datetime(
                    &frame,
                    b"v".as_ptr(),
                    1,
                    0,
                    unit_code,
                    utc,
                    &mut value,
                    &mut valid,
                ));
                assert_eq!((value, valid), (-1, 1));
                failure(
                    pldf_v1_get_datetime(
                        &frame,
                        b"v".as_ptr(),
                        1,
                        0,
                        (unit_code + 1) % 3,
                        utc,
                        &mut value,
                        &mut valid,
                    ),
                    1,
                );
                failure(
                    pldf_v1_get_datetime(
                        &frame,
                        b"v".as_ptr(),
                        1,
                        0,
                        unit_code,
                        1 - utc,
                        &mut value,
                        &mut valid,
                    ),
                    1,
                );
                let mut values = [99i64; 3];
                let mut valid = [9u8; 3];
                success(pldf_v1_values_datetime(
                    &frame,
                    b"v".as_ptr(),
                    1,
                    unit_code,
                    utc,
                    1,
                    values.as_mut_ptr().cast(),
                    3,
                    8,
                    valid.as_mut_ptr(),
                    3,
                    1,
                ));
                assert_eq!(values, [-1, 0, 0]);
                assert_eq!(valid, [1, 1, 0]);
                failure(
                    pldf_v1_values_datetime(
                        &frame,
                        b"v".as_ptr(),
                        1,
                        unit_code,
                        utc,
                        0,
                        values.as_mut_ptr().cast(),
                        3,
                        8,
                        ptr::null_mut(),
                        0,
                        0,
                    ),
                    5,
                );
            }
            assert_eq!(
                datetime_column(&[], &[], unit_code, utc).dtype(),
                &datetime_dtype(unit_code, utc).unwrap()
            );
            assert_eq!(
                datetime_column(&[i64::MIN, i64::MAX], &[0, 0], unit_code, utc).null_count(),
                2
            );
        }
    }
    assert_eq!(
        datetime_column(&[i64::MIN, i64::MAX], &[], 2, 0).null_count(),
        0
    );
}

#[test]
fn malformed_temporal_inputs_fail_without_publishing_an_owner() {
    // SAFETY: malformed scalar data/options still refer to valid borrowed buffers.
    unsafe {
        for value in [i32::MIN, i32::MAX] {
            let mut out = ptr::null_mut();
            failure(
                pldf_v1_date(
                    b"v".as_ptr(),
                    1,
                    (&value as *const i32).cast(),
                    1,
                    4,
                    ptr::null(),
                    0,
                    0,
                    &mut out,
                ),
                1,
            );
            assert!(out.is_null());
        }
        for (unit, utc, value) in [(0, 0, i64::MAX), (1, 0, i64::MIN), (3, 0, 0), (0, 2, 0)] {
            let mut out = ptr::null_mut();
            failure(
                pldf_v1_datetime(
                    b"v".as_ptr(),
                    1,
                    (&value as *const i64).cast(),
                    1,
                    8,
                    ptr::null(),
                    0,
                    0,
                    unit,
                    utc,
                    &mut out,
                ),
                1,
            );
            assert!(out.is_null());
        }
        let value = 0i32;
        let valid = 2u8;
        let mut out = ptr::null_mut();
        failure(
            pldf_v1_date(
                b"v".as_ptr(),
                1,
                (&value as *const i32).cast(),
                1,
                4,
                &valid,
                1,
                1,
                &mut out,
            ),
            1,
        );
        assert!(out.is_null());
    }
}

#[test]
fn checked_parsing_is_exact_nullable_and_rejects_invalid_calendar_and_leap_seconds() {
    let frame =
        df!("s" => &[Some("2024-02-29"), Some("1900-02-29"), None, Some("prefix2024-02-29")])
            .unwrap();
    let result = both(
        frame
            .clone()
            .lazy()
            .select([parse_expr("%Y-%m-%d", None, 0, false)]),
    );
    assert_eq!(
        result
            .column("s")
            .unwrap()
            .date()
            .unwrap()
            .physical()
            .iter()
            .collect::<Vec<_>>(),
        [Some(19782), None, None, None]
    );
    assert!(
        frame
            .lazy()
            .select([parse_expr("%Y-%m-%d", None, 0, true)])
            .collect()
            .is_err()
    );
    for unit in [0, 1, 2] {
        for utc in [0, 1] {
            let frame = df!("s" => &[Some("1970-01-01 00:00:00.123456789"), Some("1969-12-31 23:59:59.999999999"), Some("2016-12-31 23:59:60.000000000"), None]).unwrap();
            let result = both(frame.clone().lazy().select([parse_expr(
                "%Y-%m-%d %H:%M:%S%.f",
                Some(unit),
                utc,
                false,
            )]));
            assert_eq!(
                result.column("s").unwrap().dtype(),
                &datetime_dtype(unit, utc).unwrap()
            );
            let expected = [123i64, 123456, 123456789][unit as usize];
            assert_eq!(
                result
                    .column("s")
                    .unwrap()
                    .datetime()
                    .unwrap()
                    .physical()
                    .iter()
                    .collect::<Vec<_>>(),
                [Some(expected), Some(-1), None, None]
            );
            assert!(
                frame
                    .lazy()
                    .select([parse_expr("%Y-%m-%d %H:%M:%S%.f", Some(unit), utc, true)])
                    .collect()
                    .is_err()
            );
        }
    }
    let empty = df!("s" => Vec::<Option<&str>>::new()).unwrap();
    assert_eq!(
        both(empty.lazy().select([parse_expr("%F", Some(1), 1, true)]))
            .column("s")
            .unwrap()
            .dtype(),
        &datetime_dtype(1, 1).unwrap()
    );
    let nulls = df!("s" => &[None::<&str>, None]).unwrap();
    assert_eq!(
        both(nulls.lazy().select([parse_expr("%F", None, 0, true)]))
            .column("s")
            .unwrap()
            .null_count(),
        2
    );
}

#[test]
fn nanosecond_parse_bounds_are_checked_instead_of_native_wrapping() {
    // Polars 0.55.2 datetime_to_timestamp_ns uses unchecked seconds*1e9.
    // Keep these cases on the checked path even if the upstream parser changes.
    let values = [
        "1677-09-21 00:12:43.145224192",
        "1677-09-21 00:12:43.145224191",
        "2262-04-11 23:47:16.854775807",
        "2262-04-11 23:47:16.854775808",
        "3000-01-01 00:00:00",
    ];
    let frame = df!("s" => values).unwrap();
    let result = both(
        frame
            .clone()
            .lazy()
            .select([parse_expr("%F %T%.f", Some(2), 0, false)]),
    );
    assert_eq!(
        result
            .column("s")
            .unwrap()
            .datetime()
            .unwrap()
            .physical()
            .iter()
            .collect::<Vec<_>>(),
        [Some(i64::MIN), None, Some(i64::MAX), None, None]
    );
    assert!(
        frame
            .lazy()
            .select([parse_expr("%F %T%.f", Some(2), 0, true)])
            .collect()
            .is_err()
    );
}

#[test]
fn temporal_components_and_unicode_formatting_preserve_naive_and_utc() {
    for utc in [0, 1] {
        let frame = DataFrame::new(2, vec![datetime_column(&[0, -1], &[], 1, utc)]).unwrap();
        // SAFETY: expressions are borrowed for construction and uniquely consumed.
        unsafe {
            for (kind, expected) in [
                (0, vec![1970, 1969]),
                (1, vec![1, 12]),
                (2, vec![1, 31]),
                (3, vec![0, 23]),
                (4, vec![0, 59]),
                (5, vec![0, 59]),
            ] {
                let mut out = ptr::null_mut();
                success(pldf_v1_dt_component(&col("v"), kind, &mut out));
                let result = both(frame.clone().lazy().select([*Box::from_raw(out)]));
                assert_eq!(
                    result
                        .column("v")
                        .unwrap()
                        .i64()
                        .unwrap()
                        .iter()
                        .flatten()
                        .collect::<Vec<_>>(),
                    expected
                );
            }
            let mut out = ptr::null_mut();
            success(pldf_v1_dt_component(&col("v"), 6, &mut out));
            let result = both(frame.clone().lazy().select([*Box::from_raw(out)]));
            assert_eq!(
                result
                    .column("v")
                    .unwrap()
                    .date()
                    .unwrap()
                    .physical()
                    .iter()
                    .collect::<Vec<_>>(),
                [Some(0), Some(-1)]
            );
            let format = "%Y年%m月%d日 %H:%M:%S";
            success(pldf_v1_dt_format(
                &col("v"),
                format.as_ptr(),
                format.len(),
                &mut out,
            ));
            let result = both(frame.lazy().select([*Box::from_raw(out)]));
            assert_eq!(
                result.column("v").unwrap().str().unwrap().get(0),
                Some("1970年01月01日 00:00:00")
            );
        }
    }
    let date = DataFrame::new(1, vec![date_column(&[0], &[])]).unwrap();
    unsafe {
        let mut out = ptr::null_mut();
        success(pldf_v1_dt_format(&col("v"), b"%H".as_ptr(), 2, &mut out));
        let query = date.lazy().select([*Box::from_raw(out)]);
        let mut frame = ptr::null_mut();
        failure(pldf_v1_collect(&query, &mut frame), 2);
        assert!(frame.is_null());
    }
}

#[test]
fn timezone_and_invalid_formats_are_rejected_before_plan_construction() {
    unsafe {
        for format in [
            "",
            "%Q",
            "%",
            "%F %T %z",
            "%F %T %:z",
            "%F %T %::z",
            "%F %T %Z",
            "%+",
        ] {
            let mut out = ptr::null_mut();
            failure(
                pldf_v1_str_to_datetime(
                    &col("s"),
                    format.as_ptr(),
                    format.len(),
                    1,
                    0,
                    1,
                    &mut out,
                ),
                1,
            );
            failure(
                pldf_v1_dt_format(&col("s"), format.as_ptr(), format.len(), &mut out),
                1,
            );
            assert!(out.is_null());
        }
    }
    assert!(format_string("%F literal%%z").is_ok());
}

#[test]
fn parquet_preserves_temporal_metadata_and_invalid_imports_fail_safely() {
    let named = TimeZone::opt_try_new(Some("Europe/Rome")).unwrap().unwrap();
    let mut columns = vec![
        date_column(&[0, -1], &[]),
        datetime_column(&[0, -1], &[], 0, 0),
        datetime_column(&[0, -1], &[], 1, 1),
        Series::new("named".into(), &[0i64, -1])
            .i64()
            .unwrap()
            .clone()
            .into_datetime(TimeUnit::Nanoseconds, Some(named))
            .into_series()
            .into(),
    ];
    for (column, name) in columns.iter_mut().zip(["date", "naive", "utc", "named"]) {
        column.rename(name.into());
    }
    let mut original = DataFrame::new(2, columns).unwrap();
    let path = std::env::temp_dir().join(format!("plenty-temporal-{}.parquet", std::process::id()));
    ParquetWriter::new(File::create(&path).unwrap())
        .finish(&mut original)
        .unwrap();
    let restored =
        LazyFrame::scan_parquet(PlRefPath::new(path.to_str().unwrap()), Default::default())
            .unwrap()
            .collect()
            .unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(original.equals_missing(&restored));
    assert_eq!(
        dtype_descriptor(restored.column("naive").unwrap().dtype()),
        "Datetime:ms:"
    );
    assert_eq!(
        dtype_descriptor(restored.column("utc").unwrap().dtype()),
        "Datetime:us:UTC"
    );
    assert_eq!(
        dtype_descriptor(restored.column("named").unwrap().dtype()),
        "Datetime:ns:Europe/Rome"
    );
    let invalid_date = Series::new("v".into(), &[i32::MAX])
        .i32()
        .unwrap()
        .clone()
        .into_date()
        .into_series();
    let invalid_datetime = Series::new("v".into(), &[i64::MAX])
        .cast(&DataType::Datetime(TimeUnit::Milliseconds, None))
        .unwrap();
    for series in [invalid_date, invalid_datetime] {
        let mut original = DataFrame::new(1, vec![series.into()]).unwrap();
        let path = std::env::temp_dir().join(format!(
            "plenty-invalid-temporal-{}.parquet",
            std::process::id()
        ));
        ParquetWriter::new(File::create(&path).unwrap())
            .finish(&mut original)
            .unwrap();
        let frame =
            LazyFrame::scan_parquet(PlRefPath::new(path.to_str().unwrap()), Default::default())
                .unwrap()
                .collect()
                .unwrap();
        std::fs::remove_file(path).unwrap();
        unsafe {
            let mut expr = ptr::null_mut();
            success(pldf_v1_dt_component(&col("v"), 0, &mut expr));
            let query = frame.clone().lazy().select([*Box::from_raw(expr)]);
            let mut out = ptr::null_mut();
            failure(pldf_v1_collect(&query, &mut out), 2);
            assert!(out.is_null());
            if frame.column("v").unwrap().dtype() == &DataType::Date {
                let (mut value, mut valid) = (0i32, 0u8);
                failure(
                    pldf_v1_get_date(&frame, b"v".as_ptr(), 1, 0, &mut value, &mut valid),
                    1,
                );
            } else {
                let (mut value, mut valid) = (0i64, 0u8);
                failure(
                    pldf_v1_get_datetime(&frame, b"v".as_ptr(), 1, 0, 0, 0, &mut value, &mut valid),
                    1,
                );
            }
        }
    }
    unsafe {
        let (mut value, mut valid) = (0i64, 0u8);
        failure(
            pldf_v1_get_datetime(
                &restored,
                b"named".as_ptr(),
                5,
                0,
                2,
                0,
                &mut value,
                &mut valid,
            ),
            1,
        );
        let mut expr = ptr::null_mut();
        success(pldf_v1_dt_component(&col("named"), 0, &mut expr));
        let query = restored.lazy().select([*Box::from_raw(expr)]);
        let mut out = ptr::null_mut();
        failure(pldf_v1_collect(&query, &mut out), 2);
    }
}
