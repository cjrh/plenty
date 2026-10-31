//! Slice views, `&xs[a:b]` and `&mut xs[a:b]` (issue #581): borrowed runs
//! of list or byte rows, typed `&[T]` and `&mut [T]`. A view holds a loan of
//! its whole owner, so the owner cannot grow, shrink, or move while the view
//! is used; codegen addresses its rows directly (src/codegen/views.rs). Views
//! are local-only until stored references land (backlog B11). See
//! book/src/design/language/04-expressions/slices.md.
use super::*;
use crate::collection_opcodes::{SLICE_START, SLICE_STOP};

/// The operations a view supports, for diagnostics that reject others.
pub(super) const OPERATIONS: &str =
    "a view supports `len(v)`, `v[i]`, `&v[a:b]`, `==`, `for x in v`, `iter(v)`, and `print(v)`";

pub(super) const RETURN: &str = "a function cannot return a slice view yet (backlog B11): a view borrows its owner only while the caller holds it; return the bounds, or an owned copy `xs[a:b]?`";

impl Lower<'_> {
    /// `&base[start:stop]` or `&mut base[start:stop]` (issue #581): a view of
    /// the selected rows of a named list, `Bytes`, `bytes`, or view, in
    /// place. The view holds a loan of the whole owner, so no row moves
    /// while it is used. The bounds are evaluated before that loan begins,
    /// so they may read the owner that an exclusive view borrows.
    pub(super) fn slice_view(
        &mut self,
        base: &Expr,
        bounds: &SliceBounds,
        mutable: bool,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, usize)> {
        if let Some(step) = &bounds.step {
            return Err(step.at.error(
                "a slice view selects adjacent elements, so it takes no step; copy every k-th element with `xs[a:b:k]?`",
            ));
        }
        let owner = self.place_type(base).map(classes::unbox);
        if matches!(owner, Some(Ty::Shared(_))) {
            if mutable {
                return Err(base.at.error(shared::MUTATION));
            }
            return self.slice_view(&shared::content(base), bounds, mutable, ops);
        }
        let bytes = owner.as_ref().and_then(byte_buffers::byte_buffer);
        match &owner {
            Some(Ty::List(_) | Ty::View(..)) => {}
            Some(_) if bytes.is_some() => {}
            Some(Ty::ByteStr) if mutable => {
                return Err(base.at.error(
                    "a `bytes` literal is read-only; take an exclusive view of a `Bytes` buffer",
                ));
            }
            Some(Ty::ByteStr) => {}
            Some(Ty::Str) => {
                return Err(base.at.error(
                    "str has no slice views yet: a `&str` refers to a whole string value; copy the part with `s[a:b]?`",
                ));
            }
            Some(other) => {
                return Err(base.at.error(format!(
                    "{other} has no slice views; view a list, Bytes, bytes, or another view"
                )));
            }
            None => {
                return Err(base.at.error(
                    "a slice view borrows a named list, Bytes, or bytes value; bind the value to a name first",
                ));
            }
        }
        let mut flags = 0;
        let mut operands = Vec::new();
        for (bound, flag) in [(&bounds.start, SLICE_START), (&bounds.stop, SLICE_STOP)] {
            let Some(bound) = bound else {
                operands.push(Op::PushInt(Value::I64(0)));
                continue;
            };
            flags |= flag;
            if let Some(value) = slices::literal_bound(bound) {
                operands.push(Op::PushInt(Value::I64(value)));
                continue;
            }
            let (got, reads) = self.observe_expected(bound, Some(Ty::I64), ops)?;
            self.same(Some(got), Some(Ty::I64), &bound.at)?;
            let slot = self.slot(Ty::I64, &bound.at)?;
            ops.push(Op::StoreLocal(slot));
            Self::end_reads(reads, ops);
            operands.push(Op::LoadLocal(slot));
        }
        let (source, element, loan) = match (owner, bytes) {
            (_, Some(class)) => {
                let loan = self.byte_storage_place(&class, base, mutable, ops)?;
                (Ty::ByteStorage, Ty::U8, loan)
            }
            (Some(Ty::View(element, _)), _) => {
                let (view, loan) = self.borrow(base, mutable, ops)?;
                (view, (*element).clone(), loan)
            }
            (Some(owner), _) => {
                let (reference, loan) = self.borrow(base, mutable, ops)?;
                self.unbox_reference(reference, ops);
                let element = owner.element().filter(|_| owner != Ty::ByteStr);
                (owner, element.unwrap_or(Ty::U8), loan)
            }
            (None, None) => unreachable!("checked owner"),
        };
        ops.extend(operands);
        ops.push(Op::PushInt(Value::I64(flags)));
        ops.push(Op::Collection(CollectionOp::ViewNew(source, mutable)));
        Ok((Ty::View(Arc::new(element), mutable), loan))
    }

    /// Whether `e` is a slice view, `&xs[a:b]`, a view binding, or a borrow
    /// of one, judged without lowering it.
    pub(super) fn is_view(&self, e: &Expr) -> bool {
        match &ungroup(e).kind {
            Expression::Unary(op, inner) if op == "&" || op == "&mut" => {
                matches!(ungroup(inner).kind, Expression::Slice(..)) || self.is_view(inner)
            }
            _ => matches!(self.expression_type_hint(e), Some(Ty::View(..))),
        }
    }

    /// The named `Bytes` buffer that a loop over `&buffer`, `&mut buffer`, or
    /// a `&Bytes` binding borrows, and whether the borrow is exclusive. A
    /// temporary buffer is not a place, so it has no view.
    pub(super) fn borrowed_byte_buffer<'e>(&self, e: &'e Expr) -> Option<(&'e Expr, bool)> {
        let (base, mutable) = match &ungroup(e).kind {
            Expression::Unary(op, inner) if op == "&" || op == "&mut" => (&**inner, op == "&mut"),
            Expression::Name(n) => match self.names.get(n)?.ty {
                Ty::Ref(_, mutable) => (e, mutable),
                _ => return None,
            },
            _ => return None,
        };
        let owner = self.place_type(base).map(classes::unbox)?;
        byte_buffers::byte_buffer(&owner).map(|_| (base, mutable))
    }

    /// Lowers a view operand `e` that [`Self::is_view`] accepts. A written
    /// `&` or `&mut` sets the permission; a view binding keeps its own.
    pub(super) fn view_source(&mut self, e: &Expr, ops: &mut Vec<Op>) -> Result<(Ty, usize)> {
        match &ungroup(e).kind {
            Expression::Unary(op, inner) => self.borrow(inner, op == "&mut", ops),
            _ => {
                let mutable = matches!(self.expression_type_hint(e), Some(Ty::View(_, true)));
                self.borrow(e, mutable, ops)
            }
        }
    }

    /// A `&[T]` or `&mut [T]` argument: `&xs[a:b]`, `&mut xs[a:b]`, or a view
    /// binding. A list or other value is not converted implicitly, so every
    /// view a call receives is written as one.
    pub(super) fn view_argument(
        &mut self,
        arg: &Expr,
        mutable: bool,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, usize)> {
        let wanted = if mutable { "&mut xs[a:b]" } else { "&xs[a:b]" };
        if !self.is_view(arg) {
            let whole = if mutable { "&mut xs[:]" } else { "&xs[:]" };
            return Err(arg.at.error(format!(
                "this parameter takes a slice view, `{wanted}`; pass a whole list or buffer as `{whole}`"
            )));
        }
        match &ungroup(arg).kind {
            Expression::Unary(op, _) if (op == "&mut") != mutable => Err(arg.at.error(format!(
                "this parameter takes {}, `{wanted}`",
                if mutable {
                    "an exclusive slice view"
                } else {
                    "a shared slice view"
                }
            ))),
            Expression::Unary(_, inner) => self.borrow(inner, mutable, ops),
            _ => self.borrow(arg, mutable, ops),
        }
    }

    /// The loop over the view on the stack: a `&T` or `&mut T` per row, as
    /// `for x in &xs` yields. The caller keeps the view's loan live through
    /// the loop, so the count read once stays valid.
    pub(super) fn view_iteration(
        &mut self,
        view: Ty,
        reverse: bool,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<collections::Iteration> {
        let Ty::View(element, mutable) = &view else {
            unreachable!("a slice view")
        };
        let source = self.slot(view.clone(), at)?;
        let length = self.slot(Ty::I64, at)?;
        let index = self.slot(Ty::I64, at)?;
        let target = self.slot(Ty::Ref(element.clone(), *mutable), at)?;
        ops.extend([
            Op::StoreLocal(source),
            Op::LoadLocal(source),
            Op::Collection(CollectionOp::ViewLen(view.clone())),
            Op::StoreLocal(length),
        ]);
        let mut body = Vec::new();
        let mut step = Vec::new();
        let condition = if reverse {
            ops.extend([Op::LoadLocal(length), Op::StoreLocal(index)]);
            body.extend([
                Op::LoadLocal(index),
                Op::PushInt(Value::I64(1)),
                Op::Integer(IntegerOp::WrappingSub),
                Op::StoreLocal(index),
            ]);
            vec![Op::LoadLocal(index), Op::PushInt(Value::I64(0)), Op::Gt]
        } else {
            ops.extend([Op::PushInt(Value::I64(0)), Op::StoreLocal(index)]);
            // `index < length` bounds the index, so it cannot overflow.
            step.extend([
                Op::LoadLocal(index),
                Op::PushInt(Value::I64(1)),
                Op::Integer(IntegerOp::WrappingAdd),
                Op::StoreLocal(index),
            ]);
            vec![Op::LoadLocal(index), Op::LoadLocal(length), Op::Lt]
        };
        body.extend([
            Op::LoadLocal(source),
            Op::LoadLocal(index),
            Op::Collection(CollectionOp::ViewItem(view.clone(), *mutable)),
            Op::StoreLocal(target),
        ]);
        Ok(collections::Iteration {
            exact_len: Some(vec![Op::LoadLocal(length)]),
            cleanup: vec![],
            condition,
            absent: vec![],
            body,
            step,
            target,
        })
    }

    /// `a == b` or `a != b` for two views of one element type, with `a` on
    /// the stack and `rhs` lowering `b`. Elements compare as list elements do.
    pub(super) fn view_equality(
        &mut self,
        op: &str,
        (a, b): (Ty, Ty),
        rhs: Vec<Op>,
        loans: Vec<usize>,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if !matches!(op, "==" | "!=") {
            return Err(at.error(format!(
                "slice views support `==` and `!=`, not `{op}`; compare their elements"
            )));
        }
        let (Ty::View(element, a_mutable), Ty::View(other, b_mutable)) = (&a, &b) else {
            return Err(at.error(format!(
                "cannot compare {a} with {b}; a slice view equals another view, so view both sides, as `&xs[:]`"
            )));
        };
        if element != other {
            return Err(at.error(format!("cannot compare {a} with {b}")));
        }
        let facts = element.facts();
        if facts.closure
            || facts.concurrent_handle
            || facts.shared
            || facts.atomic
            || element.recursive_data()
        {
            return Err(at.error(format!(
                "{element} elements do not support automatic equality, so neither does {a}"
            )));
        }
        let shared = Ty::View(element.clone(), false);
        if *a_mutable {
            ops.push(Op::Reborrow(shared.clone()));
        }
        ops.extend(rhs);
        if *b_mutable {
            ops.push(Op::Reborrow(shared.clone()));
        }
        ops.push(Op::Collection(CollectionOp::ViewEqual(shared)));
        Self::end_reads(loans, ops);
        if op == "!=" {
            ops.push(Op::Not);
        }
        Ok(Some(Ty::Bool))
    }

    /// Reads element `index` of the view of type `view` on the stack, in
    /// place: the element stays in its row, as an observed list element does.
    /// Returns the element type and the loans the index read.
    pub(super) fn view_get(
        &mut self,
        view: &Ty,
        index: &Expr,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, Vec<usize>)> {
        let Ty::View(element, _) = view else {
            unreachable!("a slice view")
        };
        let (got, reads) = self.observe_expected(index, Some(Ty::I64), ops)?;
        self.same(Some(got), Some(Ty::I64), &index.at)?;
        self.mark(at, ops);
        ops.push(Op::Collection(CollectionOp::ViewRow(view.clone(), false)));
        ops.push(Op::ReadRef((**element).clone()));
        Ok(((**element).clone(), reads))
    }
}
