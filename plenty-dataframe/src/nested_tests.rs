use super::*;

fn integers() -> Column {
    Series::new("source".into(), [Some(7i64), None, Some(9)]).into()
}

#[test]
fn list_parts_distinguish_null_empty_and_null_elements() {
    let list = list_column(
        "items",
        &integers(),
        &[0, 2, 2, 2, 3],
        &[true, false, true, true],
    )
    .unwrap();
    let (values, offsets, valid) = unpack_list(&list).unwrap();
    assert_eq!(values.name().as_str(), "item");
    assert_eq!(
        values.i64().unwrap().iter().collect::<Vec<_>>(),
        [Some(7), None, Some(9)]
    );
    assert_eq!(offsets, [0, 2, 2, 2, 3]);
    assert_eq!(valid, [1, 0, 1, 1]);
    let empty = list_column("empty", &integers().slice(0, 0), &[0], &[]).unwrap();
    assert_eq!(empty.dtype(), &DataType::List(Box::new(DataType::Int64)));
    let nulls = list_column(
        "nulls",
        &integers().slice(0, 0),
        &[0, 0, 0],
        &[false, false],
    )
    .unwrap();
    assert_eq!(nulls.null_count(), 2);
    assert_eq!(unpack_list(&nulls).unwrap().0.dtype(), &DataType::Int64);
}

#[test]
fn list_validation_rejects_malformed_offsets_and_hidden_children() {
    for (offsets, valid) in [
        (vec![], vec![]),
        (vec![1, 3], vec![true]),
        (vec![0, 4], vec![true]),
        (vec![0, -1, 3], vec![true, true]),
        (vec![0, 3, 2, 3], vec![true, true, true]),
        (vec![0, 3], vec![]),
        (vec![0, 3], vec![false]),
    ] {
        assert!(list_column("bad", &integers(), &offsets, &valid).is_err());
    }
}

#[test]
fn recursive_nested_columns_survive_parquet_and_preserve_schema() {
    let values = integers();
    let list = list_column("numbers", &values, &[0, 2, 2, 3], &[true, false, true]).unwrap();
    let fields = DataFrame::new(
        3,
        vec![list, Series::new("label".into(), ["a", "b", "c"]).into()],
    )
    .unwrap();
    let structure = StructChunked::from_columns("record".into(), 3, fields.columns())
        .unwrap()
        .with_outer_validity(Some([true, false, true].into_iter().collect()))
        .into_series();
    let outer = list_column("records", &structure.into(), &[0, 2, 3], &[true, true]).unwrap();
    let mut frame = DataFrame::new(2, vec![outer]).unwrap();
    let mut bytes = std::io::Cursor::new(Vec::new());
    ParquetWriter::new(&mut bytes).finish(&mut frame).unwrap();
    bytes.set_position(0);
    let restored = ParquetReader::new(bytes).finish().unwrap();
    assert!(frame.equals_missing(&restored));
    let tree = schema_tree(restored.schema());
    assert_eq!(tree.parents, [-1, 0, 1, 2, 1]);
    assert_eq!(
        tree.fields.0,
        [
            "records", "List", "item", "Struct", "numbers", "List", "item", "Int64", "label",
            "String"
        ]
        .map(|v| Some(v.to_owned()))
    );
    let result = restored
        .lazy()
        .select([
            col("records")
                .list()
                .len()
                .cast(DataType::Int64)
                .alias("len"),
            col("records")
                .list()
                .get(lit(-1i64), true)
                .struct_()
                .field_by_name("label")
                .alias("last"),
        ])
        .collect()
        .unwrap();
    assert_eq!(
        result
            .column("len")
            .unwrap()
            .i64()
            .unwrap()
            .iter()
            .collect::<Vec<_>>(),
        [Some(2), Some(1)]
    );
    assert_eq!(
        result
            .column("last")
            .unwrap()
            .str()
            .unwrap()
            .iter()
            .collect::<Vec<_>>(),
        [None, Some("c")]
    );
}

#[test]
fn ffi_nested_outputs_are_owned_and_validate_buffers() {
    unsafe {
        let child = integers();
        let offsets = [0i64, 2, 2, 3];
        let valid = [1u8, 0, 1];
        let mut out = ptr::null_mut();
        let error = pldf_v1_list(
            b"x".as_ptr(),
            1,
            &child,
            offsets.as_ptr().cast(),
            4,
            8,
            valid.as_ptr(),
            3,
            1,
            &mut out,
        );
        assert!(error.is_null());
        drop(child);
        let list = Box::from_raw(out);
        let mut copied = ptr::null_mut();
        let mut positions = [0i64; 4];
        let mut present = [0u8; 3];
        assert!(
            pldf_v1_list_parts(
                &*list,
                &mut copied,
                positions.as_mut_ptr().cast(),
                4,
                8,
                present.as_mut_ptr(),
                3,
                1
            )
            .is_null()
        );
        drop(list);
        let copied = Box::from_raw(copied);
        assert_eq!(copied.len(), 3);
        assert_eq!(positions, offsets);
        assert_eq!(present, valid);
        let fields = DataFrame::new(3, vec![*copied]).unwrap();
        let mut out = ptr::null_mut();
        assert!(
            pldf_v1_struct(b"s".as_ptr(), 1, &fields, valid.as_ptr(), 3, 1, &mut out).is_null()
        );
        drop(fields);
        let structure = Box::from_raw(out);
        let mut extracted = ptr::null_mut();
        assert!(
            pldf_v1_struct_parts(&*structure, &mut extracted, present.as_mut_ptr(), 3, 1).is_null()
        );
        drop(structure);
        let extracted = Box::from_raw(extracted);
        assert_eq!(extracted.height(), 3);
        assert_eq!(present, valid);
        let mut bad = ptr::null_mut();
        let error = pldf_v1_struct(
            b"s".as_ptr(),
            1,
            &*extracted,
            [2u8, 0, 1].as_ptr(),
            3,
            1,
            &mut bad,
        );
        assert!(!error.is_null());
        assert_eq!(Box::from_raw(error).kind, 1);
        assert!(bad.is_null());
    }
}

#[test]
fn list_slices_and_multiple_chunks_extract_canonical_offsets() {
    let one = list_column("v", &integers(), &[0, 2, 3], &[true, true]).unwrap();
    let mut series = one.as_materialized_series().clone();
    series.append(one.as_materialized_series()).unwrap();
    let sliced = series.slice(1, 2).into();
    let (child, offsets, valid) = unpack_list(&sliced).unwrap();
    assert_eq!(offsets, [0, 1, 3]);
    assert_eq!(valid, [1, 1]);
    assert_eq!(
        child.i64().unwrap().iter().collect::<Vec<_>>(),
        [Some(9), Some(7), None]
    );
}

#[test]
fn struct_parent_null_is_distinct_from_present_all_null_fields() {
    let child = Series::new("value".into(), [None::<i64>, None]);
    let structure = StructChunked::from_series("s".into(), 2, [&child].into_iter())
        .unwrap()
        .with_outer_validity(Some([true, false].into_iter().collect()));
    assert_eq!(structure.null_count(), 1);
    let column: Column = structure.into_series().into();
    let mut frame = DataFrame::new(2, vec![column]).unwrap();
    let mut bytes = std::io::Cursor::new(Vec::new());
    ParquetWriter::new(&mut bytes).finish(&mut frame).unwrap();
    bytes.set_position(0);
    let restored = ParquetReader::new(bytes).finish().unwrap();
    assert!(frame.equals_missing(&restored));
    let empty = child.slice(0, 0);
    assert_eq!(
        StructChunked::from_series("s".into(), 0, [&empty].into_iter())
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn extreme_list_indexes_are_null_instead_of_panicking() {
    let list = list_column("v", &integers(), &[0, 3], &[true]).unwrap();
    let frame = DataFrame::new(1, vec![list]).unwrap();
    for index in [i64::MIN, i64::MAX] {
        let expression = col("v");
        let mut out = ptr::null_mut();
        // SAFETY: the expression and output slot remain live throughout the call.
        unsafe {
            assert!(pldf_v1_list_get(&expression, index, &mut out).is_null());
            let expression = *Box::from_raw(out);
            let result = frame.clone().lazy().select([expression]).collect().unwrap();
            assert_eq!(result.column("v").unwrap().null_count(), 1);
        }
    }
}
