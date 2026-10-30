//! Slice syntax, `x[start:stop:step]`. Every slice copies the selected
//! elements into a new value and returns `Result[_, AllocError]`. Bounds
//! follow Python: negative bounds count from the end, out-of-range bounds
//! clamp, and omitted bounds depend on the step's sign. See
//! book/src/design/language/04-expressions/slices.md.
use super::*;
use crate::bytes::BytesOp;
use crate::clist::ClistOp;
use crate::collection_opcodes::{SLICE_START, SLICE_STOP};

/// The positions `[start:stop:step]` selects from `len` elements, as the
/// runtime's `slice_positions` computes them. Tuples use it at compile time.
fn positions(len: usize, start: Option<i64>, stop: Option<i64>, step: i64) -> Vec<usize> {
    let len = len as i128;
    let step = step as i128;
    let forward = step > 0;
    let (low, high) = if forward { (0, len) } else { (-1, len - 1) };
    let adjust = |bound: i64| {
        let bound = bound as i128;
        if bound < 0 {
            (bound + len).max(low)
        } else {
            bound.min(high)
        }
    };
    let first = start.map_or(if forward { 0 } else { len - 1 }, adjust);
    let end = stop.map_or(if forward { len } else { -1 }, adjust);
    let mut selected = Vec::new();
    let mut at = first;
    while (forward && at < end) || (!forward && at > end) {
        selected.push(at as usize);
        at += step;
    }
    selected
}

/// The value of an integer literal bound, possibly negated.
fn literal_bound(e: &Expr) -> Option<i64> {
    let (negative, inner) = match &ungroup(e).kind {
        Expression::Unary(sign, inner) if sign == "-" => (true, ungroup(inner)),
        _ => (false, ungroup(e)),
    };
    let Expression::Number(n) = &inner.kind else {
        return None;
    };
    let digits = n.replace('_', "");
    let value = digits.parse::<i128>().ok()?;
    i64::try_from(if negative { -value } else { value }).ok()
}

fn zero_step(bounds: &SliceBounds) -> bool {
    bounds
        .step
        .as_ref()
        .is_some_and(|step| literal_bound(step) == Some(0))
}

impl Lower<'_> {
    pub(super) fn slice(&mut self, e: &Expr, expected: Type, ops: &mut Vec<Op>) -> Result<Ty> {
        let Expression::Slice(base, bounds) = &e.kind else {
            unreachable!()
        };
        if let Some(step) = bounds.step.as_ref().filter(|_| zero_step(bounds)) {
            return Err(step.at.error("slice step cannot be zero"));
        }
        let ty = self.slice_value(e, base, bounds, ops)?;
        // A slice is never its own payload; name the `Result` as collection
        // literals do.
        if let (Some(expected), Ty::Enum(result)) = (&expected, &ty)
            && *expected == result.get().variants[0].fields[0]
        {
            return Err(e.at.error(format!(
                "slice returns {ty}; use `?` to propagate allocation failure"
            )));
        }
        Ok(ty)
    }

    fn slice_value(
        &mut self,
        e: &Expr,
        base: &Expr,
        bounds: &SliceBounds,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        let alloc = crate::sum::alloc_error;
        if let Some(ty @ Ty::Clist(_)) = self.place_type(base) {
            // Copying rows exposes every field, as `copy_row` does.
            self.check_clist_access(&ty, &base.at)?;
            let (_, loan) = self.borrow(base, false, ops)?;
            let reads = self.slice_operands(bounds, ops)?;
            self.mark(&e.at, ops);
            ops.push(Op::Collection(CollectionOp::Clist(ClistOp::Slice(
                ty.clone(),
            ))));
            Self::end_reads(reads, ops);
            Self::end_reads(vec![loan], ops);
            return Ok(crate::sum::result(ty, alloc()));
        }
        if self
            .place_type(base)
            .is_some_and(|ty| matches!(&ty, Ty::Enum(t) if t.tuple()))
        {
            return self.tuple_slice(base, bounds, ops);
        }
        let (ty, loans) = self.observe(base, ops)?;
        if let Ty::Enum(t) = &ty
            && t.tuple()
        {
            // Hold the temporary so each selected component reads the same
            // tuple.
            let slot = self.slot(ty.clone(), &base.at)?;
            ops.push(Op::StoreLocal(slot));
            if ty.managed() {
                self.expression_temps.push(slot);
            }
            let held = format!("$sliced{slot}");
            self.bind(
                held.clone(),
                Local {
                    slot,
                    ty: ty.clone(),
                    mutable: false,
                },
            );
            let held = Expr {
                at: base.at.clone(),
                kind: Expression::Name(held),
            };
            let out = self.tuple_slice(&held, bounds, ops)?;
            Self::end_reads(loans, ops);
            return Ok(out);
        }
        let (operation, result) = match &ty {
            Ty::List(element) => {
                self.check_slice_element(&ty, element, &base.at)?;
                (CollectionOp::ListSlice(ty.clone()), ty.clone())
            }
            Ty::Str => (CollectionOp::TextSlice, Ty::Str),
            Ty::ByteStr => (
                CollectionOp::Bytes(BytesOp::SliceLiteral),
                self.bytes_class(&e.at)?,
            ),
            Ty::Clist(_) => {
                return Err(base
                    .at
                    .error("slice a named clist; bind this clist to a name first"));
            }
            _ => match byte_buffers::byte_buffer(&ty) {
                Some(class) => {
                    ops.push(Op::Class(crate::record::ClassOp::Field(class, 0)));
                    (CollectionOp::Bytes(BytesOp::Slice), ty.clone())
                }
                None => return Err(base.at.error(unsliceable(&ty))),
            },
        };
        let reads = self.slice_operands(bounds, ops)?;
        self.mark(&e.at, ops);
        let output = operation.signature().1;
        ops.push(Op::Collection(operation));
        Self::end_reads(reads, ops);
        Self::end_reads(loans, ops);
        if let Some(class) = byte_buffers::byte_buffer(&result) {
            // Wrap the storage where `Bytes` is declared, which reaches its
            // private constructor.
            let declared = self
                .aliases
                .class_declaration(&class.name)
                .ok_or_else(|| e.at.error("`std.bytes.Bytes` is missing"))?;
            return Ok(self
                .rewrap(&class, &declared, Some(output), ops)?
                .expect("a rewrapped slice result"));
        }
        Ok(crate::sum::result(result, alloc()))
    }

    /// `std.bytes.Bytes`, which a `bytes` slice produces. The loader adds
    /// `std.bytes` to a program that might need it; see `Loader::slice_bytes`.
    fn bytes_class(&self, at: &Token) -> Result<Ty> {
        TypeRef {
            concrete: None,
            parameter: false,
            at: at.clone(),
            name: Some(byte_buffers::BYTES_CLASS.into()),
            args: Vec::new(),
        }
        .resolve(self.aliases)?
        .filter(|ty| byte_buffers::byte_buffer(ty).is_some())
        .ok_or_else(|| at.error("a `bytes` slice needs `std.bytes`, which is missing"))
    }

    /// Selected list elements are duplicated as `copy()` would, and `Shared`
    /// owners as `share()` would.
    fn check_slice_element(&self, list: &Ty, element: &Ty, at: &Token) -> Result<()> {
        if matches!(element, Ty::Shared(_)) {
            return Ok(());
        }
        let reason = if element.facts().shared {
            "copy() does not yet copy values containing Shared owners (issue #582)".to_owned()
        } else if element.recursive_data() {
            format!("{element} is recursive data, which copy() does not copy")
        } else if !element.can_copy() {
            format!("{element} cannot be copied")
        } else if element.layout_depth() >= 64 {
            return Err(at.error("type nesting exceeds the implementation limit of 64"));
        } else {
            return Ok(());
        };
        Err(at.error(format!(
            "slicing {list} copies each element, and {reason}; transfer elements out of an owned temporary with `.slice(start, stop)`, or build a new list from explicit copies"
        )))
    }

    /// Push start, stop, step and the flags saying which bounds were written,
    /// in that order. An omitted bound pushes a placeholder the flags mark.
    fn slice_operands(&mut self, bounds: &SliceBounds, ops: &mut Vec<Op>) -> Result<Vec<usize>> {
        let mut reads = Vec::new();
        let mut flags = 0;
        for (bound, flag) in [(&bounds.start, SLICE_START), (&bounds.stop, SLICE_STOP)] {
            let Some(bound) = bound else {
                ops.push(Op::PushInt(Value::I64(0)));
                continue;
            };
            let (got, bound_reads) = self.observe_expected(bound, Some(Ty::I64), ops)?;
            self.same(Some(got), Some(Ty::I64), &bound.at)?;
            reads.extend(bound_reads);
            flags |= flag;
        }
        match &bounds.step {
            Some(step) => self.nonzero_step("slice", step, ops)?,
            None => ops.push(Op::PushInt(Value::I64(1))),
        }
        ops.push(Op::PushInt(Value::I64(flags)));
        Ok(reads)
    }

    /// A tuple slice selects components at compile time, so its bounds are
    /// literals. `base` is a tuple place.
    fn tuple_slice(&mut self, base: &Expr, bounds: &SliceBounds, ops: &mut Vec<Op>) -> Result<Ty> {
        let Some(Ty::Enum(t)) = self.place_type(base) else {
            unreachable!("a tuple place")
        };
        let literal = |bound: &Option<Expr>| -> Result<Option<i64>> {
            bound
                .as_ref()
                .map(|bound| {
                    literal_bound(bound).ok_or_else(|| {
                        bound.at.error(
                            "tuple slice bounds must be integer literals; the result type depends on them",
                        )
                    })
                })
                .transpose()
        };
        let (start, stop) = (literal(&bounds.start)?, literal(&bounds.stop)?);
        let step = literal(&bounds.step)?.unwrap_or(1);
        let fields = t.get().variants[0].fields.clone();
        let selected = positions(fields.len(), start, stop, step);
        if let Some(&affine) = selected.iter().find(|&&i| fields[i].affine()) {
            return Err(base.at.error(format!(
                "tuple slices of affine components are not supported yet; component {affine} is {}, so unpack the tuple and copy the components",
                fields[affine]
            )));
        }
        let mut types = Vec::new();
        for i in selected {
            let component = Expr {
                at: base.at.clone(),
                kind: Expression::Index(
                    Box::new(base.clone()),
                    Box::new(Expr {
                        at: base.at.clone(),
                        kind: Expression::Number(i.to_string()),
                    }),
                ),
            };
            types.push(self.index(&component, ops)?);
        }
        let selected = if types.is_empty() {
            ops.push(Op::PushUnit);
            Ty::Unit
        } else {
            let tuple = crate::sum::tuple(types);
            let Ty::Enum(nominal) = &tuple else {
                unreachable!()
            };
            ops.push(Op::Enum(crate::sum::EnumOp::New(nominal.clone(), 0)));
            tuple
        };
        let result = crate::sum::result(selected, crate::sum::alloc_error());
        let Ty::Enum(nominal) = &result else {
            unreachable!()
        };
        ops.push(Op::Enum(crate::sum::EnumOp::New(nominal.clone(), 0)));
        Ok(result)
    }
}

fn unsliceable(ty: &Ty) -> String {
    match ty {
        Ty::Dict(..) => "dict does not support slicing".into(),
        Ty::Shared(_) => {
            format!("{ty} does not support slicing; index the handle, or copy its contents first")
        }
        _ => format!(
            "{ty} does not support slicing; slice a list, str, Bytes, bytes, tuple, or clist"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::positions;

    #[test]
    fn positions_follow_python() {
        let len = 5;
        assert_eq!(positions(len, None, None, 1), [0, 1, 2, 3, 4]);
        assert_eq!(positions(len, Some(1), Some(3), 1), [1, 2]);
        assert_eq!(positions(len, Some(-2), None, 1), [3, 4]);
        assert_eq!(positions(len, None, None, -1), [4, 3, 2, 1, 0]);
        assert_eq!(positions(len, None, None, 2), [0, 2, 4]);
        assert_eq!(positions(len, Some(10), None, -2), [4, 2, 0]);
        assert_eq!(positions(len, Some(-10), Some(10), 1), [0, 1, 2, 3, 4]);
        assert_eq!(positions(len, Some(3), Some(1), 1), [] as [usize; 0]);
        assert_eq!(positions(0, None, None, -1), [] as [usize; 0]);
    }
}
