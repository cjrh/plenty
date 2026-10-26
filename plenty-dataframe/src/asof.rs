//! Checked, ungrouped i64 as-of joins. Checks remain in native lazy execution.
use super::*;

fn checked_input(query: LazyFrame, key: &str) -> LazyFrame {
    let key = key.to_owned();
    query.map(
        move |frame| {
            let mut previous = None;
            for value in frame.column(&key)?.i64()?.iter() {
                match value {
                    None if previous.is_none() => (),
                    Some(value) if previous.is_none_or(|last| last <= value) => {
                        previous = Some(value)
                    }
                    _ => {
                        return Err(PolarsError::InvalidOperation(
                            format!("as-of key {key:?} must be sorted ascending with nulls first")
                                .into(),
                        ));
                    }
                }
            }
            Ok(frame)
        },
        // Polars 0.55.2's streaming forward as-of can miss an unsorted right
        // tail. A non-streamable whole-frame check cannot be bypassed by key
        // metadata, predicate/projection/slice pushdown or chunk boundaries.
        // This intentionally materializes each input at execution time.
        AllowedOptimizations::empty(),
        None,
        Some("CHECKED I64 ASOF INPUT"),
    )
}

export!(pldf_v1_join_asof(left: *const LazyFrame, right: *const LazyFrame, left_name: *const u8, left_len: usize, right_name: *const u8, right_len: usize, strategy: i32, has_tolerance: u8, tolerance: i64, allow_exact: u8, suffix: *const u8, suffix_len: usize, out: *mut *mut LazyFrame) {
    if has_tolerance > 1 || allow_exact > 1 {
        return Err(NativeError::new(1, "as-of flags must be 0 or 1"));
    }
    if tolerance < 0 {
        return Err(NativeError::new(1, "as-of tolerance must be nonnegative"));
    }
    let strategy = match strategy {
        0 => AsofStrategy::Backward,
        1 => AsofStrategy::Forward,
        2 => AsofStrategy::Nearest,
        _ => return Err(NativeError::new(1, "unknown as-of strategy")),
    };
    let (left_name, right_name) = (text(left_name, left_len)?, text(right_name, right_len)?);
    let (mut left, mut right) = (get(left)?.clone(), get(right)?.clone());
    for (query, name) in [(&mut left, left_name), (&mut right, right_name)] {
        if query.collect_schema()?.get(name) != Some(&DataType::Int64) {
            return Err(NativeError::new(1, format!("as-of key {name:?} must name an i64 column")));
        }
    }
    let options = AsOfOptions {
        strategy,
        tolerance: (has_tolerance != 0).then(|| Scalar::new(DataType::Int64, AnyValue::Int64(tolerance))),
        allow_eq: allow_exact != 0,
        check_sortedness: true,
        ..Default::default()
    };
    let args = JoinArgs {
        suffix: Some(text(suffix, suffix_len)?.into()),
        coalesce: JoinCoalesce::KeepColumns,
        ..JoinArgs::new(JoinType::AsOf(Box::new(options)))
    };
    // Schema resolution above reads metadata only. Full-input validation is
    // lazy and leaves rows unchanged; the as-of node itself stays native.
    let left = checked_input(left, left_name);
    let right = checked_input(right, right_name);
    // Keys are literal names, including names such as "*" or "^key$" that the
    // general col() helper would interpret as selectors.
    put(out, left.join(right, [Expr::Column(left_name.into())], [Expr::Column(right_name.into())], args))
});

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn success(error: *mut NativeError) {
        if !error.is_null() {
            // SAFETY: each error transfers unique ownership into this helper.
            panic!("{:?}", unsafe { Box::from_raw(error) });
        }
    }
    fn join(
        left: &LazyFrame,
        right: &LazyFrame,
        strategy: i32,
        tolerance: Option<i64>,
        exact: bool,
    ) -> LazyFrame {
        // SAFETY: borrowed inputs and names live for the call; output consumed once.
        unsafe {
            let mut out = ptr::null_mut();
            success(pldf_v1_join_asof(
                left,
                right,
                b"key".as_ptr(),
                3,
                b"key".as_ptr(),
                3,
                strategy,
                u8::from(tolerance.is_some()),
                tolerance.unwrap_or(0),
                u8::from(exact),
                b"_r".as_ptr(),
                2,
                &mut out,
            ));
            *Box::from_raw(out)
        }
    }
    fn both(query: LazyFrame) -> DataFrame {
        let ordinary = query.clone().collect().unwrap();
        let streaming = query
            .collect_with_engine(Engine::Streaming)
            .unwrap()
            .unwrap_single();
        assert!(ordinary.equals_missing(&streaming));
        ordinary
    }
    fn matches(frame: &DataFrame) -> Vec<Option<i64>> {
        frame.column("row").unwrap().i64().unwrap().iter().collect()
    }

    #[test]
    fn directions_duplicate_keys_exact_policy_and_nearest_ties() {
        let left = df!("key" => &[0i64, 1, 2, 3, 4, 5, 6]).unwrap().lazy();
        let right = df!("key" => &[1i64, 1, 3, 5, 5], "row" => &[10i64, 11, 30, 50, 51])
            .unwrap()
            .lazy();
        for (strategy, exact, expected) in [
            (
                0,
                true,
                vec![
                    None,
                    Some(11),
                    Some(11),
                    Some(30),
                    Some(30),
                    Some(51),
                    Some(51),
                ],
            ),
            (
                1,
                true,
                vec![
                    Some(10),
                    Some(10),
                    Some(30),
                    Some(30),
                    Some(50),
                    Some(50),
                    None,
                ],
            ),
            (
                2,
                true,
                vec![
                    Some(11),
                    Some(11),
                    Some(30),
                    Some(30),
                    Some(51),
                    Some(51),
                    Some(51),
                ],
            ),
            (
                0,
                false,
                vec![None, None, Some(11), Some(11), Some(30), Some(30), Some(51)],
            ),
            (
                1,
                false,
                vec![Some(10), Some(30), Some(30), Some(50), Some(50), None, None],
            ),
            (
                2,
                false,
                vec![
                    Some(11),
                    Some(30),
                    Some(30),
                    Some(51),
                    Some(51),
                    Some(30),
                    Some(51),
                ],
            ),
        ] {
            let result = both(join(&left, &right, strategy, None, exact));
            assert_eq!(
                matches(&result),
                expected,
                "strategy={strategy}, exact={exact}"
            );
            assert!(result.column("key_r").is_ok());
        }
        for strategy in [0, 1, 2] {
            let result = both(join(&left, &right, strategy, Some(0), false));
            assert_eq!(result.column("row").unwrap().null_count(), 7);
        }
    }

    #[test]
    fn tolerance_is_inclusive_and_extreme_distances_do_not_overflow() {
        let left = df!("key" => &[i64::MIN, -1, 0, i64::MAX]).unwrap().lazy();
        let right = df!("key" => &[i64::MIN, i64::MAX], "row" => &[1i64, 2])
            .unwrap()
            .lazy();
        assert_eq!(
            matches(&both(join(&left, &right, 2, None, true))),
            [Some(1), Some(1), Some(2), Some(2)]
        );
        assert_eq!(
            matches(&both(join(&left, &right, 0, Some(i64::MAX), true))),
            [Some(1), Some(1), None, Some(2)]
        );
        assert_eq!(
            matches(&both(join(&left, &right, 1, Some(i64::MAX), true))),
            [Some(1), None, Some(2), Some(2)]
        );
        for strategy in [0, 1, 2] {
            assert_eq!(
                matches(&both(join(&left, &right, strategy, Some(0), true))),
                [Some(1), None, None, Some(2)]
            );
        }
        let left = df!("key" => &[i64::MIN]).unwrap().lazy();
        let right = df!("key" => &[i64::MAX], "row" => &[1i64]).unwrap().lazy();
        assert_eq!(
            matches(&both(join(&left, &right, 2, None, true))),
            [Some(1)]
        );
        assert_eq!(
            matches(&both(join(&left, &right, 2, Some(i64::MAX), true))),
            [None]
        );
    }

    #[test]
    fn sorted_nulls_are_unmatched_on_left_and_skipped_on_right() {
        for nulls_last in [false, true] {
            let left_values = if nulls_last {
                vec![Some(1i64), Some(2), None]
            } else {
                vec![None, Some(1i64), Some(2)]
            };
            let right_values = if nulls_last {
                vec![Some(1i64), Some(3), None]
            } else {
                vec![None, Some(1i64), Some(3)]
            };
            let left = df!("key" => left_values).unwrap().lazy().sort(
                ["key"],
                SortMultipleOptions::default().with_nulls_last(false),
            );
            let right = df!("key" => right_values.clone(), "row" => right_values)
                .unwrap()
                .lazy()
                .sort(
                    ["key"],
                    SortMultipleOptions::default().with_nulls_last(false),
                );
            for strategy in [0, 1, 2] {
                let result = both(join(&left, &right, strategy, None, true));
                assert_eq!(result.height(), 3);
                assert_eq!(matches(&result)[0], None);
                assert_eq!(result.column("row").unwrap().null_count(), 1);
            }
        }
        let left = df!("key" => &[None::<i64>, None]).unwrap().lazy();
        let right = df!("key" => &[None::<i64>], "row" => &[1i64])
            .unwrap()
            .lazy();
        for strategy in [0, 1, 2] {
            assert_eq!(
                matches(&both(join(&left, &right, strategy, None, true))),
                [None, None]
            );
        }
    }

    #[test]
    fn unsorted_inputs_fail_during_actual_execution_in_both_engines() {
        let sorted = df!("key" => &[1i64, 2, 3], "row" => &[1i64, 2, 3])
            .unwrap()
            .lazy();
        let unsorted = df!("key" => &[1i64, 3, 2], "row" => &[1i64, 3, 2])
            .unwrap()
            .lazy();
        let mut accepted = Vec::new();
        for (side, left, right) in [("right", &sorted, &unsorted), ("left", &unsorted, &sorted)] {
            for strategy in [0, 1, 2] {
                let query = join(left, right, strategy, None, true);
                let error = query.clone().collect().unwrap_err().to_string();
                assert!(error.contains("sorted"), "{error}");
                if query.collect_with_engine(Engine::Streaming).is_ok() {
                    accepted.push((side, strategy));
                }
            }
        }
        assert!(
            accepted.is_empty(),
            "Streaming unexpectedly accepted unsorted inputs: {accepted:?}"
        );
    }

    #[test]
    fn empty_inputs_and_plan_ownership_preserve_expected_left_shape() {
        for (empty_left, empty_right) in [(true, false), (false, true), (true, true)] {
            let query;
            {
                let left = df!("key" => &[1i64, 2]).unwrap();
                let right = df!("key" => &[1i64], "row" => &[1i64]).unwrap();
                let left = if empty_left { left.clear() } else { left }.lazy();
                let right = if empty_right { right.clear() } else { right }.lazy();
                query = join(&left, &right, 0, None, true);
            }
            let result = both(query);
            assert_eq!(result.height(), if empty_left { 0 } else { 2 });
            assert_eq!(result.width(), 3);
            if empty_right {
                assert_eq!(result.column("row").unwrap().null_count(), result.height());
            }
        }
    }

    #[test]
    fn validation_barrier_cannot_trust_metadata_or_pushdowns() {
        let sorted = df!("key" => &[1i64, 2, 3], "row" => &[1i64, 2, 3])
            .unwrap()
            .lazy();
        let unsorted = df!("key" => &[1i64, 3, 2], "row" => &[1i64, 3, 2])
            .unwrap()
            .lazy();
        let base = join(&sorted, &unsorted, 1, None, true);
        for query in [
            base.clone().limit(1),
            base.clone().select([col("row")]),
            base.clone().filter(col("key").eq(lit(1i64))),
            base.select([col("row")])
                .filter(col("row").eq(lit(1i64)))
                .limit(1),
        ] {
            assert!(query.clone().collect().is_err());
            assert!(query.collect_with_engine(Engine::Streaming).is_err());
        }
        // Sorting with nulls last produces trusted ascending metadata. The
        // explicit scan still rejects this unsupported null placement.
        let nulls_last = df!("key" => &[Some(1i64), None], "row" => &[1i64, 2])
            .unwrap()
            .lazy()
            .sort(
                ["key"],
                SortMultipleOptions::default().with_nulls_last(true),
            );
        for (left, right) in [(&sorted, &nulls_last), (&nulls_last, &sorted)] {
            let query = join(left, right, 0, None, true);
            assert!(query.clone().collect().is_err());
            assert!(query.collect_with_engine(Engine::Streaming).is_err());
        }
        // Even forged sorted metadata in a native test cannot bypass the scan.
        let mut falsely_sorted = Series::new("key".into(), &[1i64, 3, 2]);
        falsely_sorted.set_sorted_flag(polars::series::IsSorted::Ascending);
        let falsely_sorted = DataFrame::new(3, vec![falsely_sorted.into()])
            .unwrap()
            .lazy();
        let query = join(&sorted, &falsely_sorted, 1, None, true);
        assert!(query.clone().collect().is_err());
        assert!(query.collect_with_engine(Engine::Streaming).is_err());
    }

    #[test]
    fn invalid_options_missing_or_non_i64_keys_and_collisions_fail() {
        let numeric = df!("key" => &[1i64], "key_r" => &[9i64]).unwrap().lazy();
        let float = df!("key" => &[1.0f64]).unwrap().lazy();
        assert!(join(&numeric, &numeric, 0, None, true).collect().is_err());
        // SAFETY: malformed scalar options still use valid live borrowed owners;
        // failures leave the null output slot untouched.
        unsafe {
            for (right, name, strategy, present, tolerance, exact) in [
                (&numeric, "key", 3, 0, 0, 1),
                (&numeric, "key", 0, 2, 0, 1),
                (&numeric, "key", 0, 1, -1, 1),
                (&numeric, "key", 0, 0, 0, 2),
                (&numeric, "missing", 0, 0, 0, 1),
                (&float, "key", 0, 0, 0, 1),
            ] {
                let mut out = ptr::null_mut();
                let error = pldf_v1_join_asof(
                    &numeric,
                    right,
                    b"key".as_ptr(),
                    3,
                    name.as_ptr(),
                    name.len(),
                    strategy,
                    present,
                    tolerance,
                    exact,
                    b"_r".as_ptr(),
                    2,
                    &mut out,
                );
                assert!(!error.is_null());
                assert_eq!(Box::from_raw(error).kind, 1);
                assert!(out.is_null());
            }
        }
    }

    #[test]
    fn key_names_are_literal_and_may_differ_between_inputs() {
        let left = df!("*" => &[1i64, 2], "left" => &[10i64, 11])
            .unwrap()
            .lazy();
        let right = df!("^key$" => &[1i64], "row" => &[20i64]).unwrap().lazy();
        // SAFETY: name views and owners are live during construction; output is unique.
        let query = unsafe {
            let mut out = ptr::null_mut();
            success(pldf_v1_join_asof(
                &left,
                &right,
                b"*".as_ptr(),
                1,
                b"^key$".as_ptr(),
                5,
                0,
                0,
                0,
                1,
                b"_r".as_ptr(),
                2,
                &mut out,
            ));
            *Box::from_raw(out)
        };
        let result = both(query);
        assert_eq!(matches(&result), [Some(20), Some(20)]);
        assert!(result.column("*").is_ok());
        assert!(result.column("^key$").is_ok());
    }
}
