use super::*;

unsafe fn success(error: *mut NativeError) {
    if !error.is_null() {
        // SAFETY: each returned native error transfers unique ownership here.
        panic!("{:?}", unsafe { Box::from_raw(error) });
    }
}
fn join(
    left: &LazyFrame,
    right: &LazyFrame,
    keys: &[&str],
    kind: i32,
    nulls: bool,
    suffix: &str,
) -> LazyFrame {
    let keys = keys.iter().map(|name| col(*name)).collect::<Vec<_>>();
    // SAFETY: input owners and suffix bytes are lent for one call; consume output once.
    unsafe {
        let mut out = ptr::null_mut();
        success(pldf_v1_join(
            left,
            right,
            &keys,
            &keys,
            kind,
            suffix.as_ptr(),
            suffix.len(),
            u8::from(nulls),
            &mut out,
        ));
        *Box::from_raw(out)
    }
}
fn rows(frame: &DataFrame, names: &[&str]) -> Vec<Vec<Option<i64>>> {
    let columns = names
        .iter()
        .map(|name| frame.column(name).unwrap().i64().unwrap())
        .collect::<Vec<_>>();
    let mut out = (0..frame.height())
        .map(|i| {
            columns
                .iter()
                .map(|column| column.get(i))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    out.sort();
    out
}
fn column_names(frame: &DataFrame) -> Vec<&str> {
    frame
        .get_column_names()
        .into_iter()
        .map(|name| name.as_str())
        .collect()
}
fn collect_both(query: LazyFrame) -> DataFrame {
    let eager = query.clone().collect().unwrap();
    let streaming = query
        .collect_with_engine(Engine::Streaming)
        .unwrap()
        .unwrap_single();
    assert_eq!(eager.schema(), streaming.schema());
    let names = column_names(&eager);
    assert_eq!(rows(&eager, &names), rows(&streaming, &names));
    eager
}

#[test]
fn full_is_many_to_many_and_semi_anti_keep_only_left_multiplicity() {
    let left =
        df!("key" => &[Some(1i64), Some(1), Some(2), None], "leftrow" => &[10i64, 11, 12, 13])
            .unwrap()
            .lazy();
    let right =
        df!("key" => &[Some(1i64), Some(1), Some(3), None], "rightrow" => &[20i64, 21, 22, 23])
            .unwrap()
            .lazy();
    for nulls in [false, true] {
        let full = collect_both(join(&left, &right, &["key"], 2, nulls, "_r"));
        assert_eq!(full.height(), if nulls { 7 } else { 8 });
        assert!(full.column("key_r").is_ok());
        assert_eq!(
            full.column("leftrow").unwrap().null_count(),
            if nulls { 1 } else { 2 }
        );
        assert_eq!(
            full.column("rightrow").unwrap().null_count(),
            if nulls { 1 } else { 2 }
        );
        let pairs = rows(&full, &["leftrow", "rightrow"]);
        for l in [10, 11] {
            for r in [20, 21] {
                assert!(pairs.contains(&vec![Some(l), Some(r)]));
            }
        }
        assert!(pairs.contains(&vec![Some(12), None]));
        assert!(pairs.contains(&vec![None, Some(22)]));
        if nulls {
            assert!(pairs.contains(&vec![Some(13), Some(23)]));
        }
        let semi = collect_both(join(&left, &right, &["key"], 3, nulls, "_r"));
        let anti = collect_both(join(&left, &right, &["key"], 4, nulls, "_r"));
        assert_eq!(column_names(&semi), ["key", "leftrow"]);
        assert_eq!(column_names(&anti), ["key", "leftrow"]);
        assert_eq!(
            rows(&semi, &["leftrow"]),
            if nulls {
                vec![vec![Some(10)], vec![Some(11)], vec![Some(13)]]
            } else {
                vec![vec![Some(10)], vec![Some(11)]]
            }
        );
        assert_eq!(
            rows(&anti, &["leftrow"]),
            if nulls {
                vec![vec![Some(12)]]
            } else {
                vec![vec![Some(12)], vec![Some(13)]]
            }
        );
    }
}

#[test]
fn composite_null_keys_use_the_same_policy_for_every_join_kind() {
    let left = df!("a" => &[Some(1i64), Some(1), None, None], "b" => &[None, Some(2i64), Some(2), None], "leftrow" => &[0i64, 1, 2, 3]).unwrap().lazy();
    let right = df!("a" => &[Some(1i64), None, None, Some(1), Some(1)], "b" => &[None, Some(2i64), None, Some(3), None], "rightrow" => &[0i64, 1, 2, 3, 4]).unwrap().lazy();
    for (kind, unequal_height, equal_height) in
        [(0, 0, 4), (1, 4, 5), (2, 9, 6), (3, 0, 3), (4, 4, 1)]
    {
        for (nulls, height) in [(false, unequal_height), (true, equal_height)] {
            let frame = collect_both(join(&left, &right, &["a", "b"], kind, nulls, "_r"));
            assert_eq!(frame.height(), height, "kind={kind} nulls_equal={nulls}");
        }
    }
}

#[test]
fn new_joins_handle_either_empty_input_and_preserve_schema() {
    let left = df!("key" => &[1i64, 2], "leftrow" => &[10i64, 11]).unwrap();
    let right = df!("key" => &[1i64, 3], "rightrow" => &[20i64, 21]).unwrap();
    for (empty_left, empty_right) in [(true, false), (false, true), (true, true)] {
        let left = if empty_left {
            left.clear()
        } else {
            left.clone()
        }
        .lazy();
        let right = if empty_right {
            right.clear()
        } else {
            right.clone()
        }
        .lazy();
        for kind in [2, 3, 4] {
            let result = collect_both(join(&left, &right, &["key"], kind, false, "_r"));
            let height = match kind {
                2 => {
                    if empty_left && empty_right {
                        0
                    } else {
                        2
                    }
                }
                3 => 0,
                4 => {
                    if empty_left {
                        0
                    } else {
                        2
                    }
                }
                _ => unreachable!(),
            };
            assert_eq!(result.height(), height);
            assert_eq!(result.width(), if kind == 2 { 4 } else { 2 });
            assert!(
                result
                    .columns()
                    .iter()
                    .all(|column| column.dtype() == &DataType::Int64)
            );
        }
    }
}

#[test]
fn suffix_collisions_affect_full_but_not_left_only_semi_anti() {
    let left = df!("key" => &[1i64, 2], "key_r" => &[10i64, 11])
        .unwrap()
        .lazy();
    let right = df!("key" => &[1i64], "key_r" => &[20i64]).unwrap().lazy();
    for suffix in ["_r", ""] {
        assert!(
            join(&left, &right, &["key"], 2, false, suffix)
                .collect()
                .is_err()
        );
        for kind in [3, 4] {
            let frame = collect_both(join(&left, &right, &["key"], kind, false, suffix));
            assert_eq!(frame.height(), 1);
            assert_eq!(column_names(&frame), ["key", "key_r"]);
        }
    }
}

#[test]
fn new_join_plans_own_inputs_and_computed_keys_do_not_replace_columns() {
    for kind in [2, 3, 4] {
        let result;
        // SAFETY: all borrowed owners are dropped before evaluating native output.
        unsafe {
            let left = df!("key" => &[1i64, 2], "leftrow" => &[10i64, 11])
                .unwrap()
                .lazy();
            let right = df!("key" => &[2i64], "rightrow" => &[20i64])
                .unwrap()
                .lazy();
            let left_keys = vec![col("key") + lit(1i64)];
            let right_keys = vec![col("key")];
            let suffix = String::from("_r");
            let mut out = ptr::null_mut();
            success(pldf_v1_join(
                &left,
                &right,
                &left_keys,
                &right_keys,
                kind,
                suffix.as_ptr(),
                suffix.len(),
                0,
                &mut out,
            ));
            result = *Box::from_raw(out);
        }
        let frame = collect_both(result);
        assert_eq!(
            rows(&frame, &["key"]),
            match kind {
                2 => vec![vec![Some(1)], vec![Some(2)]],
                3 => vec![vec![Some(1)]],
                4 => vec![vec![Some(2)]],
                _ => unreachable!(),
            }
        );
    }
}

#[test]
fn join_flags_keys_and_runtime_dtypes_are_checked() {
    let left = df!("key" => &[1i64]).unwrap().lazy();
    let right = df!("key" => &["one"]).unwrap().lazy();
    for kind in [2, 3, 4] {
        assert!(
            join(&left, &right, &["key"], kind, false, "_r")
                .collect()
                .is_err()
        );
        assert!(
            join(&left, &left, &["missing"], kind, false, "_r")
                .collect()
                .is_err()
        );
    }
    // SAFETY: deliberately invalid scalar/list contents remain valid borrowed owners.
    unsafe {
        for (left_keys, right_keys, kind, nulls) in [
            (vec![], vec![], 2, 0),
            (vec![col("key")], vec![col("key"), col("key")], 3, 0),
            (vec![col("key")], vec![col("key")], 4, 2),
            (vec![col("key")], vec![col("key")], 5, 0),
        ] {
            let mut out = ptr::null_mut();
            let error = pldf_v1_join(
                &left,
                &left,
                &left_keys,
                &right_keys,
                kind,
                b"_r".as_ptr(),
                2,
                nulls,
                &mut out,
            );
            assert!(!error.is_null());
            assert_eq!(Box::from_raw(error).kind, 1);
            assert!(out.is_null());
        }
    }
}
