//! Local CSV I/O and explicit native streaming execution.
use super::*;
use polars::io::utils::file::Writable;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct CsvSchema(Schema);
destroy!(pldf_v1_csv_schema_drop, CsvSchema);
export!(pldf_v1_csv_schema_new(out: *mut *mut CsvSchema) { put(out, CsvSchema::default()) });
export!(pldf_v1_csv_schema_add(schema: *mut CsvSchema, name: *const u8, len: usize, dtype: i32) {
    let schema = schema.as_mut().ok_or_else(|| NativeError::new(1, "null CSV schema owner"))?;
    let name = text(name, len)?;
    let dtype = match dtype {
        0 => DataType::Int64, 1 => DataType::Float64,
        2 => DataType::Boolean, 3 => DataType::String,
        _ => return Err(NativeError::new(1, "unsupported CSV schema dtype")),
    };
    if schema.0.contains(name) { return Err(NativeError::new(1, format!("duplicate CSV schema override: {name}"))); }
    schema.0.insert(name.into(), dtype);
    Ok(())
});

fn boolean(value: u8) -> Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(NativeError::new(1, "CSV boolean option must be 0 or 1")),
    }
}
fn separator(value: &str) -> Result<u8> {
    let bytes = value.as_bytes();
    if bytes.len() != 1 || !bytes[0].is_ascii() || matches!(bytes[0], 0 | b'"' | b'\r' | b'\n') {
        return Err(NativeError::new(
            1,
            "CSV separator must be one ASCII byte other than NUL, quote, CR or LF",
        ));
    }
    Ok(bytes[0])
}

export!(pldf_v1_scan_csv(path: *const u8, path_len: usize, header: u8, sep: *const u8, sep_len: usize, infer_rows: i64, null: *const u8, null_len: usize, has_null: u8, schema: *const CsvSchema, out: *mut *mut LazyFrame) {
    let header = boolean(header)?;
    let separator = separator(text(sep, sep_len)?)?;
    let infer_rows = match infer_rows {
        -1 => None,
        n if n >= 0 => Some(usize::try_from(n).map_err(|_| NativeError::new(1, "CSV inference length exceeds addressable rows"))?),
        _ => return Err(NativeError::new(1, "CSV inference rows must be -1 (all), 0 (text), or positive")),
    };
    let null_value = if boolean(has_null)? { Some(NullValues::AllColumnsSingle(text(null, null_len)?.into())) } else { None };
    let overrides = get(schema)?.0.clone();
    let mut query = LazyCsvReader::new(PlRefPath::new(text(path, path_len)?))
        .with_glob(false)
        .with_has_header(header)
        .with_separator(separator)
        .with_infer_schema_length(infer_rows)
        .with_null_values(null_value)
        .with_missing_is_null(true)
        .with_ignore_errors(false)
        .with_dtype_overwrite(Some(Arc::new(overrides.clone())))
        .finish()?;
    let resolved = query.collect_schema()?;
    for name in overrides.iter_names() {
        if !resolved.contains(name.as_str()) {
            return Err(NativeError::new(1, format!("CSV schema override names missing column: {name}")));
        }
    }
    put(out, query)
});
export!(pldf_v1_write_csv(frame: *const DataFrame, path: *const u8, path_len: usize, header: u8, sep: *const u8, sep_len: usize, null: *const u8, null_len: usize) {
    let header = boolean(header)?;
    let separator = separator(text(sep, sep_len)?)?;
    let null = text(null, null_len)?;
    // Polars writes this token verbatim, not as a quoted string field.
    if null.bytes().any(|byte| byte == separator || matches!(byte, b'"' | b'\r' | b'\n')) {
        return Err(NativeError::new(1, "CSV output null token cannot contain the separator, quote, CR or LF"));
    }
    let frame = get(frame)?;
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(text(path, path_len)?)?;
    CsvWriter::new(file).include_header(header).with_separator(separator).with_null_value(null.into()).finish(&mut frame.clone())?;
    Ok(())
});
export!(pldf_v1_collect_streaming(query: *const LazyFrame, out: *mut *mut DataFrame) {
    // This materializes the complete result, even though upstream execution uses
    // the streaming engine. The facade cannot construct multiple output sinks.
    put(out, get(query)?.clone().collect_with_engine(Engine::Streaming)?.unwrap_single())
});
export!(pldf_v1_sink_parquet(query: *const LazyFrame, path: *const u8, path_len: usize) {
    let query = get(query)?.clone();
    let path = text(path, path_len)?;
    // Hold the exclusive create-new file descriptor through the native sink;
    // never reopen its path, which could truncate a lazy input or race a writer.
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    let target = SinkTarget::Dyn(SpecialEq::new(Arc::new(Mutex::new(Some(Writable::Local(file))))));
    query.sink(
        SinkDestination::File { target },
        FileWriteFormat::Parquet(Arc::new(ParquetWriteOptions::default())),
        UnifiedSinkArgs::default(),
    )?.collect_with_engine(Engine::Streaming)?;
    Ok(())
});

#[cfg(test)]
mod tests {
    use super::*;

    struct Directory(std::path::PathBuf);
    impl Directory {
        fn new(label: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("plenty-io-{}-{nanos}-{label}", std::process::id()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self, name: &str) -> String {
            self.0.join(name).to_str().unwrap().to_owned()
        }
        fn write(&self, name: &str, bytes: impl AsRef<[u8]>) -> String {
            let path = self.path(name);
            std::fs::write(&path, bytes).unwrap();
            path
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    unsafe fn success(error: *mut NativeError) {
        if !error.is_null() {
            panic!("{:?}", unsafe { Box::from_raw(error) });
        }
    }
    unsafe fn failure(error: *mut NativeError, kind: Option<i32>) {
        assert!(!error.is_null());
        let error = unsafe { Box::from_raw(error) };
        if let Some(kind) = kind {
            assert_eq!(error.kind, kind, "{error:?}");
        }
    }
    fn scan(
        path: &str,
        header: bool,
        sep: &str,
        infer: i64,
        null: Option<&str>,
        schema: &CsvSchema,
    ) -> LazyFrame {
        // SAFETY: views and schema remain borrowed only for this call; take unique output.
        unsafe {
            let token = null.unwrap_or("");
            let mut out = ptr::null_mut();
            success(pldf_v1_scan_csv(
                path.as_ptr(),
                path.len(),
                u8::from(header),
                sep.as_ptr(),
                sep.len(),
                infer,
                token.as_ptr(),
                token.len(),
                u8::from(null.is_some()),
                schema,
                &mut out,
            ));
            *Box::from_raw(out)
        }
    }
    fn streaming(query: &LazyFrame) -> DataFrame {
        // SAFETY: live borrowed query and unique output slot.
        unsafe {
            let mut out = ptr::null_mut();
            success(pldf_v1_collect_streaming(query, &mut out));
            *Box::from_raw(out)
        }
    }

    #[test]
    fn csv_quotes_unicode_empty_strings_and_nulls_are_distinct() {
        let dir = Directory::new("quotes");
        let path = dir.write(
            "literal[1].csv",
            "name,value\n\"é😀\",\"\"\n\"\",\n\"a,b\",\"line\nnext\"\n",
        );
        let query = scan(&path, true, ",", 0, None, &CsvSchema::default());
        let frame = streaming(&query);
        assert_eq!(
            frame
                .column("name")
                .unwrap()
                .str()
                .unwrap()
                .iter()
                .collect::<Vec<_>>(),
            [Some("é😀"), Some(""), Some("a,b")]
        );
        assert_eq!(
            frame
                .column("value")
                .unwrap()
                .str()
                .unwrap()
                .iter()
                .collect::<Vec<_>>(),
            [Some(""), None, Some("line\nnext")]
        );
        assert!(frame.equals_missing(&query.collect().unwrap()));
    }

    #[test]
    fn csv_headerless_overrides_null_tokens_and_nan_follow_native_rules() {
        let dir = Directory::new("types");
        let path = dir.write(
            "rows.csv",
            "001;3.5;true;NA\n002;;false;\"NA\"\n003;NaN;;\"\"\n",
        );
        let query;
        // SAFETY: schema owns names and is cloned by scan before its unique drop.
        unsafe {
            let mut schema = ptr::null_mut();
            success(pldf_v1_csv_schema_new(&mut schema));
            for (name, dtype) in [
                ("column_1", 3),
                ("column_2", 1),
                ("column_3", 2),
                ("column_4", 3),
            ] {
                success(pldf_v1_csv_schema_add(
                    schema,
                    name.as_ptr(),
                    name.len(),
                    dtype,
                ));
            }
            query = scan(&path, false, ";", 0, Some("NA"), &*schema);
            pldf_v1_csv_schema_drop(schema);
        }
        let frame = streaming(&query);
        assert_eq!(
            frame.column("column_1").unwrap().str().unwrap().get(0),
            Some("001")
        );
        let float = frame.column("column_2").unwrap().f64().unwrap();
        assert_eq!(float.get(0), Some(3.5));
        assert_eq!(float.get(1), None);
        assert!(float.get(2).unwrap().is_nan());
        assert_eq!(
            frame
                .column("column_3")
                .unwrap()
                .bool()
                .unwrap()
                .iter()
                .collect::<Vec<_>>(),
            [Some(true), Some(false), None]
        );
        assert_eq!(
            frame
                .column("column_4")
                .unwrap()
                .str()
                .unwrap()
                .iter()
                .collect::<Vec<_>>(),
            [None, None, Some("")]
        );
    }

    #[test]
    fn csv_inference_is_bounded_and_invalid_data_is_not_silently_dropped() {
        let dir = Directory::new("inference");
        let path = dir.write("rows.csv", "x\n1\nbad\n");
        let schema = CsvSchema::default();
        assert!(scan(&path, true, ",", 1, None, &schema).collect().is_err());
        for infer in [-1, 0] {
            let frame = streaming(&scan(&path, true, ",", infer, None, &schema));
            assert_eq!(
                frame
                    .column("x")
                    .unwrap()
                    .str()
                    .unwrap()
                    .iter()
                    .collect::<Vec<_>>(),
                [Some("1"), Some("bad")]
            );
        }
        let path = dir.write("header.csv", "x,y\n");
        let frame = streaming(&scan(&path, true, ",", 0, None, &schema));
        assert_eq!(frame.shape(), (0, 2));
        for (name, bytes) in [
            ("utf8.csv", b"x\n\xff\n".as_slice()),
            ("ragged.csv", b"x,y\n1,2,3\n".as_slice()),
            ("empty.csv", b"".as_slice()),
        ] {
            let path = dir.write(name, bytes);
            // SAFETY: malformed content can fail during scan or collection;
            // outputs and errors are still consumed once on either path.
            unsafe {
                let mut query = ptr::null_mut();
                let error = pldf_v1_scan_csv(
                    path.as_ptr(),
                    path.len(),
                    1,
                    b",".as_ptr(),
                    1,
                    100,
                    ptr::null(),
                    0,
                    0,
                    &schema,
                    &mut query,
                );
                if error.is_null() {
                    let query = Box::from_raw(query);
                    let mut frame = ptr::null_mut();
                    failure(pldf_v1_collect_streaming(&*query, &mut frame), None);
                    assert!(frame.is_null());
                } else {
                    failure(error, None);
                    assert!(query.is_null());
                }
            }
        }
    }

    #[test]
    fn csv_options_and_overrides_are_checked_before_output_side_effects() {
        let dir = Directory::new("invalid-options");
        let path = dir.write("input.csv", "x\n1\n");
        let output = dir.path("never.csv");
        let schema = CsvSchema::default();
        let frame = df!("x" => &[1i64]).unwrap();
        // SAFETY: every invalid option is scalar/text data with valid borrowed storage.
        unsafe {
            let mut query = ptr::null_mut();
            for sep in ["", "é", "ab", "\n", "\r", "\0", "\""] {
                failure(
                    pldf_v1_scan_csv(
                        path.as_ptr(),
                        path.len(),
                        1,
                        sep.as_ptr(),
                        sep.len(),
                        100,
                        ptr::null(),
                        0,
                        0,
                        &schema,
                        &mut query,
                    ),
                    Some(1),
                );
                failure(
                    pldf_v1_write_csv(
                        &frame,
                        output.as_ptr(),
                        output.len(),
                        1,
                        sep.as_ptr(),
                        sep.len(),
                        ptr::null(),
                        0,
                    ),
                    Some(1),
                );
                assert!(!std::path::Path::new(&output).exists());
            }
            for null in ["a,b", "a\"b", "a\rb", "a\nb"] {
                failure(
                    pldf_v1_write_csv(
                        &frame,
                        output.as_ptr(),
                        output.len(),
                        1,
                        b",".as_ptr(),
                        1,
                        null.as_ptr(),
                        null.len(),
                    ),
                    Some(1),
                );
                assert!(!std::path::Path::new(&output).exists());
            }
            failure(
                pldf_v1_scan_csv(
                    path.as_ptr(),
                    path.len(),
                    1,
                    b",".as_ptr(),
                    1,
                    -2,
                    ptr::null(),
                    0,
                    0,
                    &schema,
                    &mut query,
                ),
                Some(1),
            );
            failure(
                pldf_v1_scan_csv(
                    path.as_ptr(),
                    path.len(),
                    2,
                    b",".as_ptr(),
                    1,
                    100,
                    ptr::null(),
                    0,
                    0,
                    &schema,
                    &mut query,
                ),
                Some(1),
            );
            let mut override_schema = ptr::null_mut();
            success(pldf_v1_csv_schema_new(&mut override_schema));
            success(pldf_v1_csv_schema_add(
                override_schema,
                b"missing".as_ptr(),
                7,
                0,
            ));
            failure(
                pldf_v1_csv_schema_add(override_schema, b"missing".as_ptr(), 7, 0),
                Some(1),
            );
            failure(
                pldf_v1_csv_schema_add(override_schema, b"x".as_ptr(), 1, 4),
                Some(1),
            );
            failure(
                pldf_v1_scan_csv(
                    path.as_ptr(),
                    path.len(),
                    1,
                    b",".as_ptr(),
                    1,
                    100,
                    ptr::null(),
                    0,
                    0,
                    override_schema,
                    &mut query,
                ),
                Some(1),
            );
            pldf_v1_csv_schema_drop(override_schema);
            assert!(query.is_null());
        }
    }

    #[test]
    fn csv_writer_roundtrips_and_both_outputs_refuse_existing_aliases() {
        let dir = Directory::new("writer");
        let path = dir.path("out.csv");
        let frame = df!("text" => &[Some(""), None, Some("é,\"x\"\n😀")], "i" => &[Some(1i64), None, Some(3)], "b" => &[Some(true), None, Some(false)]).unwrap();
        let query = frame.clone().lazy();
        // SAFETY: borrowed frame/query owners and path bytes live for each call.
        unsafe {
            success(pldf_v1_write_csv(
                &frame,
                path.as_ptr(),
                path.len(),
                1,
                b",".as_ptr(),
                1,
                ptr::null(),
                0,
            ));
            let roundtrip = streaming(&scan(&path, true, ",", 100, None, &CsvSchema::default()));
            assert!(frame.equals_missing(&roundtrip));
            let original = std::fs::read(&path).unwrap();
            failure(
                pldf_v1_write_csv(
                    &frame,
                    path.as_ptr(),
                    path.len(),
                    1,
                    b",".as_ptr(),
                    1,
                    ptr::null(),
                    0,
                ),
                Some(3),
            );
            failure(
                pldf_v1_sink_parquet(&query, path.as_ptr(), path.len()),
                Some(3),
            );
            assert_eq!(std::fs::read(&path).unwrap(), original);
            #[cfg(unix)]
            {
                let alias = dir.path("alias.csv");
                std::os::unix::fs::symlink(&path, &alias).unwrap();
                failure(
                    pldf_v1_sink_parquet(&query, alias.as_ptr(), alias.len()),
                    Some(3),
                );
                failure(
                    pldf_v1_write_csv(
                        &frame,
                        alias.as_ptr(),
                        alias.len(),
                        1,
                        b",".as_ptr(),
                        1,
                        ptr::null(),
                        0,
                    ),
                    Some(3),
                );
                assert_eq!(std::fs::read(&path).unwrap(), original);
            }
        }
    }

    #[test]
    fn streaming_query_sinks_to_parquet_without_collecting_a_frame_first() {
        let dir = Directory::new("sink");
        let source = dir.write("source.csv", "key,value\na,1\nb,2\na,3\nb,4\n");
        let query = scan(&source, true, ",", 100, None, &CsvSchema::default())
            .filter(col("value").gt(lit(1i64)))
            .with_column((col("value") * lit(2i64)).alias("doubled"));
        let target = dir.path("out.parquet");
        // SAFETY: sink clones the plan; there is no collected Frame argument.
        unsafe {
            success(pldf_v1_sink_parquet(&query, target.as_ptr(), target.len()));
        }
        let written = LazyFrame::scan_parquet(PlRefPath::new(&target), Default::default())
            .unwrap()
            .collect()
            .unwrap();
        assert!(written.equals_missing(&streaming(&query)));
        assert_eq!(
            written
                .column("doubled")
                .unwrap()
                .i64()
                .unwrap()
                .iter()
                .collect::<Vec<_>>(),
            [Some(4), Some(6), Some(8)]
        );
        // A grouped/windowed query also runs in Streaming; this is correctness
        // coverage, not a promise that every operator retains bounded memory.
        let windowed = query.with_column(
            col("value")
                .sum()
                .over([col("key")])
                .unwrap()
                .alias("total"),
        );
        assert!(streaming(&windowed).equals_missing(&windowed.collect().unwrap()));
        let source_contents = std::fs::read(&source).unwrap();
        let source_query = scan(&source, true, ",", 100, None, &CsvSchema::default());
        unsafe {
            failure(
                pldf_v1_sink_parquet(&source_query, source.as_ptr(), source.len()),
                Some(3),
            );
        }
        assert_eq!(std::fs::read(source).unwrap(), source_contents);
    }

    #[test]
    fn csv_single_column_null_empty_and_typed_empty_roundtrips() {
        let dir = Directory::new("single");
        for (name, series) in [
            (
                "strings",
                Series::new("x".into(), &[Some(""), None, Some("text")]),
            ),
            ("all_null", Series::new("x".into(), &[None::<i64>, None])),
            ("empty", Series::new("x".into(), Vec::<i64>::new())),
        ] {
            let frame = DataFrame::new(series.len(), vec![series.into()]).unwrap();
            let schema = CsvSchema(frame.schema().as_ref().clone());
            let path = dir.path(name);
            // SAFETY: unique output path per case and borrowed frame/data views.
            unsafe {
                success(pldf_v1_write_csv(
                    &frame,
                    path.as_ptr(),
                    path.len(),
                    1,
                    b",".as_ptr(),
                    1,
                    ptr::null(),
                    0,
                ));
            }
            let result = streaming(&scan(&path, true, ",", 100, None, &schema));
            assert!(
                frame.equals_missing(&result),
                "roundtrip failed for {name}: {:?}",
                std::fs::read_to_string(path).unwrap()
            );
        }
    }
}
