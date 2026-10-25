//! Optimizer-visible Polars expressions; every returned plan owns its inputs.
use super::*;

export!(pldf_v1_lit_str(value: *const u8, len: usize, out: *mut *mut Expr) {
    put(out, lit(text(value, len)?.to_owned()))
});
export!(pldf_v1_lit_bool(value: u8, out: *mut *mut Expr) {
    let value = match value {
        0 => false, 1 => true,
        _ => return Err(NativeError::new(1, "boolean literal must be 0 or 1")),
    };
    put(out, lit(value))
});
export!(pldf_v1_when(predicate: *const Expr, then_expr: *const Expr, otherwise: *const Expr, out: *mut *mut Expr) {
    put(out, when(get(predicate)?.clone()).then(get(then_expr)?.clone()).otherwise(get(otherwise)?.clone()))
});
export!(pldf_v1_cast(expr: *const Expr, dtype: i32, strict: u8, out: *mut *mut Expr) {
    let dtype = match dtype {
        0 => DataType::Int64, 1 => DataType::Float64,
        2 => DataType::Boolean, 3 => DataType::String,
        _ => return Err(NativeError::new(1, "unsupported cast target")),
    };
    let expr = get(expr)?.clone();
    put(out, match strict {
        0 => expr.cast(dtype), 1 => expr.strict_cast(dtype),
        _ => return Err(NativeError::new(1, "cast strictness must be 0 or 1")),
    })
});
export!(pldf_v1_string_unary(expr: *const Expr, op: i32, out: *mut *mut Expr) {
    let expr = get(expr)?.clone().str();
    put(out, match op {
        0 => expr.to_lowercase(), 1 => expr.to_uppercase(),
        2 => expr.len_chars().cast(DataType::Int64),
        _ => return Err(NativeError::new(1, "unknown string operation")),
    })
});
export!(pldf_v1_string_binary(expr: *const Expr, pattern: *const Expr, op: i32, out: *mut *mut Expr) {
    let expr = get(expr)?.clone().str();
    let pattern = get(pattern)?.clone();
    put(out, match op {
        0 => expr.contains_literal(pattern),
        1 => expr.starts_with(pattern), 2 => expr.ends_with(pattern),
        _ => return Err(NativeError::new(1, "unknown string predicate")),
    })
});
export!(pldf_v1_n_unique(expr: *const Expr, out: *mut *mut Expr) {
    put(out, get(expr)?.clone().n_unique().cast(DataType::Int64))
});
export!(pldf_v1_over(expr: *const Expr, partition: *const Vec<Expr>, out: *mut *mut Expr) {
    let partition = get(partition)?;
    if partition.is_empty() { return Err(NativeError::new(1, "window requires at least one partition expression")); }
    put(out, get(expr)?.clone().over(partition.as_slice())?)
});

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn success(error: *mut NativeError) {
        if !error.is_null() {
            // SAFETY: each native error transfers unique ownership to this helper.
            panic!("{:?}", unsafe { Box::from_raw(error) });
        }
    }
    unsafe fn failure(error: *mut NativeError) {
        assert!(!error.is_null());
        assert_eq!(unsafe { Box::from_raw(error) }.kind, 1);
    }
    unsafe fn owned(expr: *mut Expr) -> Expr {
        // SAFETY: each caller supplies a successful unique native Expr output.
        unsafe { *Box::from_raw(expr) }
    }
    fn evaluate(frame: DataFrame, expr: Expr) -> PolarsResult<DataFrame> {
        frame.lazy().select([expr.alias("out")]).collect()
    }

    #[test]
    fn conditional_is_nullable_and_requires_valid_independent_branches() {
        let frame = df!("p" => &[Some(true), Some(false), None], "x" => &[1i64, 2, 3]).unwrap();
        // SAFETY: borrowed Exprs live for calls; native outputs consumed once.
        unsafe {
            let mut out = ptr::null_mut();
            success(pldf_v1_when(&col("p"), &lit(10i64), &col("x"), &mut out));
            assert_eq!(
                evaluate(frame.clone(), owned(out))
                    .unwrap()
                    .column("out")
                    .unwrap()
                    .i64()
                    .unwrap()
                    .iter()
                    .collect::<Vec<_>>(),
                [Some(10), Some(2), Some(3)]
            );
            success(pldf_v1_when(&col("x"), &lit(1i64), &lit(2i64), &mut out));
            assert!(evaluate(frame.clone(), owned(out)).is_err());
            // Even an unused branch is checked and evaluated independently:
            // conditional selection cannot guard an invalid strict conversion.
            let invalid = col("s").strict_cast(DataType::Int64);
            success(pldf_v1_when(&col("p"), &lit(1i64), &invalid, &mut out));
            let all_true = df!("p" => &[true, true], "s" => &["bad", "bad"]).unwrap();
            assert!(evaluate(all_true, owned(out)).is_err());
        }
    }

    #[test]
    fn casts_distinguish_conversion_failure_from_unsupported_conversion() {
        let frame = df!("s" => &[Some("42"), Some("bad"), None]).unwrap();
        // SAFETY: matching live input/output owners for every native call.
        unsafe {
            let mut out = ptr::null_mut();
            success(pldf_v1_cast(&col("s"), 0, 1, &mut out));
            assert!(evaluate(frame.clone(), owned(out)).is_err());
            success(pldf_v1_cast(&col("s"), 0, 0, &mut out));
            assert_eq!(
                evaluate(frame.clone(), owned(out))
                    .unwrap()
                    .column("out")
                    .unwrap()
                    .i64()
                    .unwrap()
                    .iter()
                    .collect::<Vec<_>>(),
                [Some(42), None, None]
            );
            // Polars does not implement String -> Boolean even non-strictly.
            success(pldf_v1_cast(&col("s"), 2, 0, &mut out));
            assert!(evaluate(frame, owned(out)).is_err());
            let frame = df!("n" => &[Some(1.9f64), Some(-2.8), Some(f64::NAN), None]).unwrap();
            success(pldf_v1_cast(&col("n"), 0, 0, &mut out));
            assert_eq!(
                evaluate(frame, owned(out))
                    .unwrap()
                    .column("out")
                    .unwrap()
                    .i64()
                    .unwrap()
                    .iter()
                    .collect::<Vec<_>>(),
                [Some(1), Some(-2), None, None]
            );
        }
    }

    #[test]
    fn strings_are_unicode_scalar_aware_and_predicates_are_literal() {
        let frame = df!("s" => &[Some("Éß😀.*"), Some("e\u{301}"), None, Some("")]).unwrap();
        // SAFETY: all expressions are borrowed or consumed exactly once.
        unsafe {
            let mut out = ptr::null_mut();
            success(pldf_v1_string_unary(&col("s"), 0, &mut out));
            assert_eq!(
                evaluate(frame.clone(), owned(out))
                    .unwrap()
                    .column("out")
                    .unwrap()
                    .str()
                    .unwrap()
                    .iter()
                    .collect::<Vec<_>>(),
                [Some("éß😀.*"), Some("e\u{301}"), None, Some("")]
            );
            success(pldf_v1_string_unary(&col("s"), 1, &mut out));
            assert_eq!(
                evaluate(frame.clone(), owned(out))
                    .unwrap()
                    .column("out")
                    .unwrap()
                    .str()
                    .unwrap()
                    .get(0),
                Some("ÉSS😀.*")
            );
            success(pldf_v1_string_unary(&col("s"), 2, &mut out));
            assert_eq!(
                evaluate(frame.clone(), owned(out))
                    .unwrap()
                    .column("out")
                    .unwrap()
                    .i64()
                    .unwrap()
                    .iter()
                    .collect::<Vec<_>>(),
                [Some(5), Some(2), None, Some(0)]
            );
            for (op, pattern) in [(0, ".*"), (1, "É"), (2, ".*")] {
                success(pldf_v1_string_binary(
                    &col("s"),
                    &lit(pattern),
                    op,
                    &mut out,
                ));
                assert_eq!(
                    evaluate(frame.clone(), owned(out))
                        .unwrap()
                        .column("out")
                        .unwrap()
                        .bool()
                        .unwrap()
                        .iter()
                        .collect::<Vec<_>>(),
                    [Some(true), Some(false), None, Some(false)]
                );
            }
            success(pldf_v1_string_binary(
                &col("s"),
                &lit(NULL).cast(DataType::String),
                0,
                &mut out,
            ));
            assert_eq!(
                evaluate(frame.clone(), owned(out))
                    .unwrap()
                    .column("out")
                    .unwrap()
                    .null_count(),
                4
            );
            success(pldf_v1_string_unary(&lit(2i64), 0, &mut out));
            assert!(evaluate(frame, owned(out)).is_err());
        }
    }

    #[test]
    fn unique_counts_include_one_null_and_zero_for_empty_input() {
        // SAFETY: native outputs uniquely owned until converted back to Expr.
        unsafe {
            for (values, expected) in [
                (vec![Some(1i64), Some(1), None, None], 2),
                (vec![], 0),
                (vec![None, None], 1),
            ] {
                let frame = df!("x" => values).unwrap();
                let mut out = ptr::null_mut();
                success(pldf_v1_n_unique(&col("x"), &mut out));
                let result = evaluate(frame, owned(out)).unwrap();
                assert_eq!(
                    result.column("out").unwrap().i64().unwrap().get(0),
                    Some(expected)
                );
            }
        }
    }

    #[test]
    fn string_predicates_follow_native_common_type_coercion() {
        // SAFETY: borrowed expressions remain live during construction; outputs
        // are unique owners. Polars coerces primitive patterns, while the
        // receiver must itself resolve to String.
        unsafe {
            for (op, source, pattern) in [
                (0, lit("123"), lit(2i64)),
                (1, lit("123"), lit(1i64)),
                (2, lit("123"), lit(3i64)),
            ] {
                let mut out = ptr::null_mut();
                success(pldf_v1_string_binary(&source, &pattern, op, &mut out));
                let result = evaluate(DataFrame::empty(), owned(out)).unwrap();
                assert_eq!(
                    result.column("out").unwrap().bool().unwrap().get(0),
                    Some(true)
                );
            }
            for pattern in [lit(2i64), lit("2")] {
                let mut out = ptr::null_mut();
                success(pldf_v1_string_binary(&lit(123i64), &pattern, 0, &mut out));
                assert!(evaluate(DataFrame::empty(), owned(out)).is_err());
            }
        }
    }

    #[test]
    fn windows_broadcast_group_aggregates_in_row_order_and_own_plans() {
        let frame = df!("group" => &[Some("a"), None, Some("a"), Some("b"), None], "value" => &[1i64, 2, 3, 4, 5]).unwrap();
        // SAFETY: cloned native window plan survives all its source expressions.
        unsafe {
            let mut out = ptr::null_mut();
            {
                let keys = vec![col("group")];
                let aggregate = col("value").sum();
                success(pldf_v1_over(&aggregate, &keys, &mut out));
            }
            let result = evaluate(frame.clone(), owned(out)).unwrap();
            assert_eq!(
                result
                    .column("out")
                    .unwrap()
                    .i64()
                    .unwrap()
                    .iter()
                    .collect::<Vec<_>>(),
                [Some(4), Some(7), Some(4), Some(4), Some(7)]
            );
            success(pldf_v1_over(
                &col("value").sum(),
                &vec![col("missing")],
                &mut out,
            ));
            assert!(evaluate(frame, owned(out)).is_err());
        }
    }

    #[test]
    fn literals_own_their_data_and_invalid_codes_leave_empty_outputs() {
        // SAFETY: the temporary string is live for construction, then released
        // before the owned literal plan is evaluated.
        unsafe {
            let mut out = ptr::null_mut();
            {
                let value = String::from("é\0😀");
                success(pldf_v1_lit_str(value.as_ptr(), value.len(), &mut out));
            }
            let result = evaluate(DataFrame::empty(), owned(out)).unwrap();
            assert_eq!(
                result.column("out").unwrap().str().unwrap().get(0),
                Some("é\0😀")
            );
            success(pldf_v1_lit_bool(1, &mut out));
            assert_eq!(
                evaluate(DataFrame::empty(), owned(out))
                    .unwrap()
                    .column("out")
                    .unwrap()
                    .bool()
                    .unwrap()
                    .get(0),
                Some(true)
            );
            out = ptr::null_mut();
            failure(pldf_v1_lit_bool(2, &mut out));
            failure(pldf_v1_cast(&lit(1i64), 4, 1, &mut out));
            failure(pldf_v1_cast(&lit(1i64), 0, 2, &mut out));
            failure(pldf_v1_string_unary(&lit("x"), 3, &mut out));
            failure(pldf_v1_string_binary(&lit("x"), &lit("x"), 3, &mut out));
            failure(pldf_v1_over(&lit(1i64), &vec![], &mut out));
            assert!(out.is_null());
        }
    }
}
