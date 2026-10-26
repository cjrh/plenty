//! Column-oriented nested transfers. Parent validity is separate from child nulls.
use super::*;
use polars::chunked_array::builder::get_list_builder;

unsafe fn parent_validity(
    data: *const u8,
    len: usize,
    stride: usize,
    rows: usize,
) -> Result<Vec<bool>> {
    if len != rows {
        return Err(NativeError::new(
            1,
            "parent validity length does not match row count",
        ));
    }
    unsafe { values::<u8>(data, len, stride)? }
        .into_iter()
        .map(|v| match v {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(NativeError::new(
                1,
                "parent validity must contain only 0 or 1",
            )),
        })
        .collect()
}

fn list_column(name: &str, child: &Column, offsets: &[i64], valid: &[bool]) -> Result<Column> {
    if offsets.len() != valid.len().saturating_add(1)
        || offsets.first() != Some(&0)
        || offsets.last().copied() != Some(child.len() as i64)
        || offsets.windows(2).any(|p| p[0] < 0 || p[0] > p[1])
    {
        return Err(NativeError::new(
            1,
            "list offsets must start at zero, be ordered, and end at child length",
        ));
    }
    let mut builder = get_list_builder(child.dtype(), child.len(), valid.len(), name.into());
    for (range, present) in offsets.windows(2).zip(valid) {
        if *present {
            builder.append_series(
                &child
                    .as_materialized_series()
                    .slice(range[0], (range[1] - range[0]) as usize),
            )?;
        } else {
            if range[0] != range[1] {
                return Err(NativeError::new(
                    1,
                    "null list rows must have empty offset ranges",
                ));
            }
            builder.append_null();
        }
    }
    Ok(builder.finish().into_series().into())
}

export!(pldf_v1_list(name: *const u8, name_len: usize, child: *const Column, offsets: *const u8, offsets_len: usize, offsets_stride: usize, valid: *const u8, valid_len: usize, valid_stride: usize, out: *mut *mut Column) {
    let offsets = values::<i64>(offsets, offsets_len, offsets_stride)?;
    let valid = parent_validity(valid, valid_len, valid_stride, offsets.len().saturating_sub(1))?;
    put(out, list_column(text(name, name_len)?, get(child)?, &offsets, &valid)?)
});

export!(pldf_v1_struct(name: *const u8, name_len: usize, fields: *const DataFrame, valid: *const u8, valid_len: usize, valid_stride: usize, out: *mut *mut Column) {
    let fields = get(fields)?;
    if fields.width() == 0 { return Err(NativeError::new(1, "struct columns require at least one field")); }
    let valid = parent_validity(valid, valid_len, valid_stride, fields.height())?;
    let result = StructChunked::from_columns(text(name, name_len)?.into(), fields.height(), fields.columns())?
        .with_outer_validity(Some(valid.into_iter().collect()));
    put(out, result.into_series().into())
});

export!(pldf_v1_frame_column(frame: *const DataFrame, name: *const u8, name_len: usize, out: *mut *mut Column) {
    put(out, get(frame)?.column(text(name, name_len)?)?.clone())
});
export!(pldf_v1_column_frame(column: *const Column, out: *mut *mut DataFrame) {
    let column = get(column)?;
    put(out, DataFrame::new(column.len(), vec![column.clone()])?)
});
export!(pldf_v1_column_len(column: *const Column, out: *mut i64) {
    let target = out.as_mut().ok_or_else(|| NativeError::new(1, "null length output"))?;
    *target = get(column)?.len() as i64;
    Ok(())
});

fn unpack_list(column: &Column) -> Result<(Column, Vec<i64>, Vec<u8>)> {
    let list = column.list()?;
    let mut child = Series::new_empty("item".into(), list.inner_dtype());
    let mut offsets = Vec::with_capacity(list.len().saturating_add(1));
    let mut valid = Vec::with_capacity(list.len());
    offsets.push(0);
    for value in list.series_iter() {
        valid.push(u8::from(value.is_some()));
        if let Some(value) = value {
            child.append(&value)?;
        }
        if child.len() > u32::MAX as usize {
            return Err(NativeError::new(
                1,
                "flattened list exceeds the engine's 32-bit row limit",
            ));
        }
        offsets.push(child.len() as i64);
    }
    Ok((child.rechunk().into(), offsets, valid))
}

export!(pldf_v1_list_parts(column: *const Column, child: *mut *mut Column, offsets: *mut u8, offsets_len: usize, offsets_stride: usize, valid: *mut u8, valid_len: usize, valid_stride: usize) {
    let column = get(column)?;
    check_view::<i64>(offsets, offsets_len, offsets_stride)?;
    check_view::<u8>(valid, valid_len, valid_stride)?;
    if offsets_len != column.len().saturating_add(1) || valid_len != column.len() {
        return Err(NativeError::new(1, "list output buffers have incorrect lengths"));
    }
    let (values, positions, present) = unpack_list(column)?;
    put(child, values)?;
    for (i, position) in positions.into_iter().enumerate() { offsets.add(i * offsets_stride).cast::<i64>().write_unaligned(position); }
    for (i, present) in present.into_iter().enumerate() { valid.add(i * valid_stride).write(present); }
    Ok(())
});

export!(pldf_v1_struct_parts(column: *const Column, fields: *mut *mut DataFrame, valid: *mut u8, valid_len: usize, valid_stride: usize) {
    let column = get(column)?;
    let structure = column.struct_()?;
    check_view::<u8>(valid, valid_len, valid_stride)?;
    if valid_len != column.len() { return Err(NativeError::new(1, "struct output validity has incorrect length")); }
    let children = structure.fields_as_series().into_iter().map(Column::from).collect();
    put(fields, DataFrame::new(column.len(), children)?)?;
    for (i, present) in column.is_not_null().iter().enumerate() {
        valid.add(i * valid_stride).write(u8::from(present == Some(true)));
    }
    Ok(())
});

pub struct NativeSchemaTree {
    fields: NativeStrings,
    parents: Vec<i64>,
}
destroy!(pldf_v1_schema_tree_drop, NativeSchemaTree);

fn schema_tree(schema: &Schema) -> NativeSchemaTree {
    let mut pending = schema
        .iter()
        .map(|(name, dtype)| (-1, name.as_str(), dtype))
        .collect::<Vec<_>>();
    pending.reverse();
    let mut fields = Vec::new();
    let mut parents = Vec::new();
    while let Some((parent, name, dtype)) = pending.pop() {
        let index = parents.len() as i64;
        parents.push(parent);
        fields.push(Some(name.to_owned()));
        fields.push(Some(super::temporal::dtype_descriptor(dtype)));
        match dtype {
            DataType::List(inner) => pending.push((index, "item", inner)),
            DataType::Struct(children) => {
                pending.extend(
                    children
                        .iter()
                        .rev()
                        .map(|field| (index, field.name().as_str(), field.dtype())),
                );
            }
            _ => {}
        }
    }
    NativeSchemaTree {
        fields: NativeStrings(fields),
        parents,
    }
}
export!(pldf_v1_frame_schema_tree(frame: *const DataFrame, out: *mut *mut NativeSchemaTree) {
    put(out, schema_tree(get(frame)?.schema()))
});
export!(pldf_v1_query_schema_tree(query: *const LazyFrame, out: *mut *mut NativeSchemaTree) {
    let schema = get(query)?.clone().collect_schema()?;
    put(out, schema_tree(&schema))
});
export!(pldf_v1_schema_tree_len(tree: *const NativeSchemaTree, out: *mut i64) {
    *out.as_mut().ok_or_else(|| NativeError::new(1, "null schema length output"))? = get(tree)?.parents.len() as i64;
    Ok(())
});
export!(pldf_v1_schema_tree_copy(tree: *const NativeSchemaTree, parents: *mut u8, len: usize, stride: usize, fields: *mut *mut NativeStrings) {
    let tree = get(tree)?;
    check_view::<i64>(parents, len, stride)?;
    if len != tree.parents.len() { return Err(NativeError::new(1, "schema parent buffer has incorrect length")); }
    put(fields, NativeStrings(tree.fields.0.clone()))?;
    for (i, parent) in tree.parents.iter().enumerate() { parents.add(i * stride).cast::<i64>().write_unaligned(*parent); }
    Ok(())
});

export!(pldf_v1_list_len(expr: *const Expr, out: *mut *mut Expr) {
    put(out, get(expr)?.clone().list().len().cast(DataType::Int64))
});
export!(pldf_v1_list_get(expr: *const Expr, index: i64, out: *mut *mut Expr) {
    // Polars' negative-index conversion takes abs(), which cannot represent
    // i64::MIN. Both values are out of bounds under this engine's u32 row limit.
    let index = index.max(-i64::MAX);
    put(out, get(expr)?.clone().list().get(lit(index), true))
});
export!(pldf_v1_struct_field(expr: *const Expr, name: *const u8, name_len: usize, out: *mut *mut Expr) {
    put(out, get(expr)?.clone().struct_().field_by_name(text(name, name_len)?))
});

#[cfg(test)]
#[path = "nested_tests.rs"]
mod tests;
