//! Slice views, `&xs[a:b]` and `&mut xs[a:b]` (issue #581): borrowed runs
//! of list or byte rows, typed `&[T]` and `&mut [T]`. A view holds a loan of
//! its owner, so the owner cannot grow, shrink, or move while the view is
//! used. The loan path records the range, so views with provably disjoint
//! ranges coexist (issue #593, `ownership::paths`). Codegen addresses its
//! rows directly (src/codegen/views.rs). Views are local-only until stored
//! references land (backlog B11). See
//! book/src/design/language/04-expressions/slices.md.
use super::*;
use crate::collection_opcodes::{
    SLICE_START, SLICE_STOP, TEXT_SEARCH_ENDS, TEXT_SEARCH_FIND, TEXT_SEARCH_STARTS,
};
use crate::ownership::{Bound, range_step};
use crate::text::TextViewOp;

/// The operations a view supports, for diagnostics that reject others.
pub(super) const OPERATIONS: &str = "a view supports `len(v)`, `v[i]`, `&v[a:b]`, `==`, `for x in v`, `iter(v)`, and `print(v)`, and a `&mut [T]` view also `v[i] = x` and `v.swap(i, j)`";

/// The operations a `&substr` supports, for diagnostics that reject others.
pub(super) const TEXT_OPERATIONS: &str = "a `&substr` supports `len(v)`, `v.byte_len()`, `v[i]`, `&v[a:b]`, `v[a:b]?`, comparisons, `in`, `startswith`, `endswith`, `find`, `for c in v`, and `print(v)`; copy it to a `str` with `v[:]?` for other `str` methods";

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
        // `&b"abc"[1:]` views static data, as `&b"abc"` does at a `&[u8]`
        // parameter; `&mut` still reports that a literal is read-only.
        if !mutable
            && matches!(
                ungroup(base).kind,
                Expression::Bytes(_) | Expression::Text(_)
            )
        {
            let held = self.held_literal(base, ops)?;
            return self.slice_view(&held, bounds, mutable, ops);
        }
        let owner = self.place_type(base).map(classes::unbox);
        if matches!(owner, Some(Ty::Shared(_))) {
            if mutable {
                return Err(base.at.error(shared::MUTATION));
            }
            return self.slice_view(&shared::content(base), bounds, mutable, ops);
        }
        let bytes = owner.as_ref().and_then(byte_buffers::byte_buffer);
        let text = matches!(&owner, Some(Ty::Str)) || owner == Some(text_view());
        match &owner {
            Some(_) if text && mutable => {
                return Err(base.at.error(
                    "a `str` is immutable, so a substring view is shared only; write `&s[a:b]`",
                ));
            }
            Some(Ty::List(_) | Ty::View(..)) => {}
            Some(_) if bytes.is_some() => {}
            Some(Ty::ByteStr) if mutable => {
                return Err(base.at.error(
                    "a `bytes` literal is read-only; take an exclusive view of a `Bytes` buffer",
                ));
            }
            Some(Ty::ByteStr | Ty::Str) => {}
            Some(other) => {
                return Err(base.at.error(format!(
                    "{other} has no slice views; view a list, Bytes, bytes, or another view"
                )));
            }
            None => {
                return Err(base.at.error(
                    "a slice view borrows a named list, Bytes, bytes, or str value; bind the value to a name first",
                ));
            }
        }
        let mut flags = 0;
        let mut operands = Vec::new();
        let range = range_step(
            self.view_bound(&bounds.start),
            self.view_bound(&bounds.stop),
        );
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
        if text {
            // Bounds count scalars, so the runtime finds their byte offsets.
            let (source, loan) = self.borrow(base, false, ops)?;
            let operation = if owner == Some(Ty::Str) {
                self.unbox_reference(source, ops);
                TextViewOp::OfStr
            } else {
                TextViewOp::Subview
            };
            ops.extend(operands);
            ops.push(Op::PushInt(Value::I64(flags)));
            ops.push(Op::Collection(CollectionOp::TextView(operation)));
            return Ok((text_view(), loan));
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
        self.refine_loan(loan, range, ops);
        ops.extend(operands);
        ops.push(Op::PushInt(Value::I64(flags)));
        ops.push(Op::Collection(CollectionOp::ViewNew(source, mutable)));
        Ok((Ty::View(Arc::new(element), mutable), loan))
    }

    /// A view bound as its loan path records it (issue #593). A non-`mut`
    /// binding stands for its value: the checker rejects assigning it while
    /// a view it bounds is live.
    fn view_bound(&self, bound: &Option<Expr>) -> Bound {
        let Some(bound) = bound else {
            return Bound::Open;
        };
        if let Some(value) = slices::literal_bound(bound) {
            return Bound::Literal(value);
        }
        match &ungroup(bound).kind {
            Expression::Name(name) => match self.names.get(name) {
                Some(local) if local.ty == Ty::I64 && !local.mutable => Bound::Binding(local.slot),
                Some(local) if local.ty == Ty::I64 => Bound::Mutable(local.slot),
                _ => Bound::Computed,
            },
            _ => Bound::Computed,
        }
    }

    /// Narrow a precise loan to `step`, a place inside the one it covers.
    /// An imprecise loan keeps covering its whole origin.
    pub(super) fn refine_loan(&mut self, loan: usize, step: usize, ops: &mut [Op]) {
        if !self.loans[loan].precise {
            return;
        }
        self.loans[loan].fields.push(step);
        if let Some(Op::Loan(fact)) = ops
            .iter_mut()
            .rev()
            .find(|op| matches!(op, Op::Loan(l) if l.id == loan))
        {
            *fact = self.loans[loan].clone();
        }
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

    /// Whether `e` is a `&substr`: `&s[a:b]` of a `str` or `&substr`, or a
    /// view binding of that type.
    pub(super) fn is_text_view(&self, e: &Expr) -> bool {
        match &ungroup(e).kind {
            Expression::Unary(op, inner) if op == "&" || op == "&mut" => {
                match &ungroup(inner).kind {
                    Expression::Slice(owner, _) => matches!(
                        self.place_type(owner).map(classes::unbox),
                        Some(ty) if ty == Ty::Str || ty == text_view()
                    ),
                    _ => self.is_text_view(inner),
                }
            }
            _ => self.expression_type_hint(e) == Some(text_view()),
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

    /// The whole view, `&base[:]` or `&mut base[:]`, that `e` converts to
    /// where a view type is expected: a borrow of an owner (`&xs`, `&buf`,
    /// `&s`), a reference binding to one, or a `bytes` literal (a `str`
    /// literal when `text`). The conversion copies nothing, so it needs no
    /// syntax (consistency rule 3, backlog B90). A literal is first held in
    /// a hidden local, since a view borrows a place. `None` leaves `e` as
    /// written, so its own diagnostic reports anything else.
    pub(super) fn owner_view(
        &mut self,
        e: &Expr,
        mutable: bool,
        text: bool,
        ops: &mut Vec<Op>,
    ) -> Result<Option<Expr>> {
        let (base, mutable) = match &ungroup(e).kind {
            // `&b"..."` reads as the literal itself, as it did for the
            // `&S` parameters that `&[u8]` replaced.
            Expression::Unary(op, base)
                if op == "&" && !text && matches!(ungroup(base).kind, Expression::Bytes(_)) =>
            {
                (self.held_literal(base, ops)?, false)
            }
            Expression::Unary(op, base) if op == "&" || op == "&mut" => {
                (Expr::clone(base), op == "&mut")
            }
            Expression::Name(name) => match self.names.get(name) {
                Some(Local {
                    ty: Ty::Ref(..), ..
                }) => (e.clone(), mutable),
                _ => return Ok(None),
            },
            Expression::Bytes(_) if !text => (self.held_literal(e, ops)?, mutable),
            Expression::Text(_) if text => (self.held_literal(e, ops)?, mutable),
            _ => return Ok(None),
        };
        if matches!(ungroup(&base).kind, Expression::Slice(..)) {
            return Ok(None);
        }
        let viewable = |ty: &Ty| {
            matches!(ty, Ty::List(_) | Ty::Str | Ty::ByteStr)
                || byte_buffers::byte_buffer(ty).is_some()
        };
        let converts = match self.place_type(&base).map(classes::unbox) {
            Some(Ty::Shared(content)) => viewable(&classes::unbox(Ty::clone(&content))),
            Some(owner) => viewable(&owner),
            None => false,
        };
        if !converts {
            return Ok(None);
        }
        let whole = Expr {
            at: base.at.clone(),
            kind: Expression::Slice(
                Box::new(base),
                Box::new(SliceBounds {
                    start: None,
                    stop: None,
                    step: None,
                }),
            ),
        };
        Ok(Some(Expr {
            at: e.at.clone(),
            kind: Expression::Unary(if mutable { "&mut" } else { "&" }.into(), Box::new(whole)),
        }))
    }

    /// A `bytes` or `str` literal held in a hidden local, to be viewed. The
    /// literal is static data, so the local lives to the end of its scope
    /// like a written binding, and a view of it may be bound.
    fn held_literal(&mut self, e: &Expr, ops: &mut Vec<Op>) -> Result<Expr> {
        let ty = self
            .expr(e, ops)?
            .ok_or_else(|| e.at.error("expected a value, got ()"))?;
        let slot = self.slot(ty.clone(), &e.at)?;
        ops.push(Op::StoreLocal(slot));
        let name = format!("$viewed{slot}");
        self.bind(
            name.clone(),
            Local {
                slot,
                ty,
                mutable: false,
            },
        );
        Ok(Expr {
            at: e.at.clone(),
            kind: Expression::Name(name),
        })
    }

    /// `&f()?` or `&f()?[a:b]` at a shared view parameter: the owner the call
    /// returns is held until the end of the full expression, as a `&T`
    /// argument holds one, and `arg` is rewritten to view that holder. A view
    /// cannot outlive the call it is passed to, so the holder outlives it.
    /// `None` leaves `arg` as written.
    fn held_temporary(&mut self, arg: &Expr, ops: &mut Vec<Op>) -> Result<Option<Expr>> {
        let Expression::Unary(op, inner) = &ungroup(arg).kind else {
            return Ok(None);
        };
        if op != "&" {
            return Ok(None);
        }
        let (owner, bounds) = match &ungroup(inner).kind {
            Expression::Slice(base, bounds) => (ungroup(base), Some(bounds)),
            _ => (ungroup(inner), None),
        };
        let held = |kind: &Expression| {
            matches!(
                kind,
                Expression::Call(..)
                    | Expression::GenericCall(..)
                    | Expression::Method(..)
                    | Expression::GenericMethod(..)
                    | Expression::Invoke(..)
                    | Expression::Try(..)
            )
        };
        if !held(&owner.kind) || self.place_type(owner).is_some() {
            return Ok(None);
        }
        let ty = self
            .expr(owner, ops)?
            .ok_or_else(|| owner.at.error("expected a value, got ()"))?;
        let viewable = matches!(ty, Ty::List(_) | Ty::Str | Ty::ByteStr)
            || byte_buffers::byte_buffer(&ty).is_some();
        if !viewable {
            return Err(owner.at.error(format!(
                "this parameter takes a slice view of a list, Bytes, bytes, or str value, got {ty}"
            )));
        }
        let slot = self.slot(ty.clone(), &owner.at)?;
        ops.push(Op::StoreLocal(slot));
        self.expression_temps.push(slot);
        let name = format!("$held{slot}");
        self.bind(
            name.clone(),
            Local {
                slot,
                ty,
                mutable: false,
            },
        );
        let name = Expr {
            at: owner.at.clone(),
            kind: Expression::Name(name),
        };
        let viewed = match bounds {
            Some(bounds) => Expr {
                at: inner.at.clone(),
                kind: Expression::Slice(Box::new(name), bounds.clone()),
            },
            None => name,
        };
        Ok(Some(Expr {
            at: arg.at.clone(),
            kind: Expression::Unary("&".into(), Box::new(viewed)),
        }))
    }

    /// A `&[T]` or `&mut [T]` argument: `&xs[a:b]`, `&mut xs[a:b]`, a view
    /// binding, or an owner that [`Self::owner_view`] converts. `text`
    /// marks a `&substr`.
    pub(super) fn view_argument(
        &mut self,
        arg: &Expr,
        mutable: bool,
        text: bool,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, usize)> {
        // This intrinsic is argument-only: forward the substring's loan to
        // the consuming call instead of returning an untracked borrowed value.
        if let Expression::Call(name, args) = &ungroup(arg).kind
            && name == "__regex_text_bytes"
        {
            if mutable || text || args.len() != 1 {
                return Err(arg.at.error(
                    "__regex_text_bytes takes one text borrow at a shared byte-view parameter",
                ));
            }
            let (view, loan) = self.view_argument(&args[0], false, true, ops)?;
            self.same(Some(view), Some(text_view()), &arg.at)?;
            ops.push(Op::Collection(CollectionOp::Regex(
                crate::regex::RegexOp::TextBytes,
            )));
            return Ok((crate::bytes::byte_view(), loan));
        }
        let held = if mutable {
            None
        } else {
            self.held_temporary(arg, ops)?
        };
        let arg = held.as_ref().unwrap_or(arg);
        let converted = self.owner_view(arg, mutable, text, ops)?;
        let arg = converted.as_ref().unwrap_or(arg);
        let wanted = if mutable { "&mut xs[a:b]" } else { "&xs[a:b]" };
        if text && !self.is_view(arg) {
            return Err(arg.at.error(
                "this parameter takes a substring view: pass `&s` or a literal for a whole string, or `&s[a:b]` for part of it",
            ));
        }
        if !self.is_view(arg) {
            let whole = if mutable { "&mut xs" } else { "&xs" };
            return Err(arg.at.error(format!(
                "this parameter takes a slice view: pass `{whole}` for a whole list or buffer, or `{wanted}` for part of it"
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
        if **element == Ty::Text {
            return self.text_view_iteration(reverse, at, ops);
        }
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
            counter: (!reverse).then_some(index),
            exact_len: Some(vec![Op::LoadLocal(length)]),
            cleanup: vec![],
            condition,
            absent: vec![],
            body,
            step,
            target,
        })
    }

    /// The loop over the `&substr` on the stack: one-scalar `str` values, as
    /// `for c in s` yields, which are inline and so allocate nothing. The
    /// cursor is a byte offset that advances by each scalar's width.
    fn text_view_iteration(
        &mut self,
        reverse: bool,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<collections::Iteration> {
        let view = text_view();
        let source = self.slot(view.clone(), at)?;
        let length = self.slot(Ty::I64, at)?;
        let offset = self.slot(Ty::I64, at)?;
        let target = self.slot(Ty::Str, at)?;
        ops.extend([
            Op::StoreLocal(source),
            Op::LoadLocal(source),
            Op::Collection(CollectionOp::ViewLen(view.clone())),
            Op::StoreLocal(length),
        ]);
        let scalar = |operation| {
            [
                Op::LoadLocal(source),
                Op::LoadLocal(offset),
                Op::Collection(CollectionOp::TextView(operation)),
                Op::StoreLocal(target),
            ]
        };
        let width = [
            Op::LoadLocal(offset),
            Op::BorrowLocal(target, false),
            Op::Collection(CollectionOp::QueryRef(
                crate::collection::Query::TextByteLen,
            )),
        ];
        let mut body = Vec::new();
        let mut step = Vec::new();
        let condition = if reverse {
            ops.extend([Op::LoadLocal(length), Op::StoreLocal(offset)]);
            body.extend(scalar(TextViewOp::ScalarBefore));
            body.extend(width);
            body.extend([Op::Integer(IntegerOp::WrappingSub), Op::StoreLocal(offset)]);
            vec![Op::LoadLocal(offset), Op::PushInt(Value::I64(0)), Op::Gt]
        } else {
            ops.extend([Op::PushInt(Value::I64(0)), Op::StoreLocal(offset)]);
            body.extend(scalar(TextViewOp::Scalar));
            // `offset < length` bounds the offset, so it cannot overflow.
            step.extend(width);
            step.extend([Op::Integer(IntegerOp::WrappingAdd), Op::StoreLocal(offset)]);
            vec![Op::LoadLocal(offset), Op::LoadLocal(length), Op::Lt]
        };
        step.push(Op::DropLocal(target));
        Ok(collections::Iteration {
            counter: None,
            exact_len: None,
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
        if a == text_view() || b == text_view() {
            return self.text_comparison(op, (a, b), rhs, loans, at, ops);
        }
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

    /// `v.name(args)` on a `&substr`: the `str` searches that need no new
    /// string, plus `byte_len`.
    pub(super) fn text_view_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let mode = match name {
            "byte_len" => None,
            "startswith" => Some(TEXT_SEARCH_STARTS),
            "endswith" => Some(TEXT_SEARCH_ENDS),
            "find" => Some(TEXT_SEARCH_FIND),
            _ => {
                return Err(base.at.error(format!(
                    "a `&substr` has no method `{name}`; {TEXT_OPERATIONS}"
                )));
            }
        };
        let arity = usize::from(mode.is_some());
        if args.len() != arity {
            return Err(base.at.error(if arity == 0 {
                format!("{name} takes no arguments")
            } else {
                format!("{name} requires one argument")
            }));
        }
        let (_, loan) = self.view_source(base, ops)?;
        let Some(mode) = mode else {
            ops.push(Op::Collection(CollectionOp::ViewLen(text_view())));
            Self::end_reads(vec![loan], ops);
            return Ok(Some(Ty::I64));
        };
        let (needle, mut loans) = self.observe(&args[0], ops)?;
        let needle_text = match needle {
            Ty::Str => true,
            _ if needle == text_view() => false,
            _ => {
                return Err(args[0].at.error(format!(
                    "{name} on a `&substr` takes a `str` or `&substr`, not {needle}"
                )));
            }
        };
        ops.push(Op::PushInt(Value::I64(mode << 1 | i64::from(needle_text))));
        let operation = TextViewOp::Search {
            needle_text,
            find: mode == TEXT_SEARCH_FIND,
        };
        let (_, result) = operation.signature();
        ops.push(Op::Collection(CollectionOp::TextView(operation)));
        loans.push(loan);
        Self::end_reads(loans, ops);
        Ok(Some(result))
    }

    /// A comparison of a `&substr` with a `str` or another `&substr`, by
    /// Unicode scalars, as two strings compare.
    fn text_comparison(
        &mut self,
        op: &str,
        (a, b): (Ty, Ty),
        rhs: Vec<Op>,
        loans: Vec<usize>,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let compared = match op {
            "==" => Op::Eq,
            "!=" => Op::Ne,
            "<" => Op::Lt,
            "<=" => Op::Le,
            ">" => Op::Gt,
            ">=" => Op::Ge,
            _ => {
                return Err(at.error(format!(
                    "`&substr` supports comparisons, not `{op}`; copy the text with `v[:]?` to build a new string"
                )));
            }
        };
        let text = |ty: &Ty| -> Option<bool> {
            if *ty == Ty::Str {
                Some(true)
            } else {
                (*ty == text_view()).then_some(false)
            }
        };
        let (Some(a_text), Some(b_text)) = (text(&a), text(&b)) else {
            return Err(at.error(format!(
                "cannot compare {a} with {b}; a `&substr` compares with a `str` or another `&substr`"
            )));
        };
        ops.extend(rhs);
        let kinds = i64::from(a_text) | i64::from(b_text) << 1;
        ops.push(Op::PushInt(Value::I64(kinds)));
        ops.push(Op::Collection(CollectionOp::TextView(TextViewOp::Compare(
            a_text, b_text,
        ))));
        Self::end_reads(loans, ops);
        ops.extend([Op::PushInt(Value::I64(0)), compared]);
        Ok(Some(Ty::Bool))
    }

    /// `v.swap(i, j)` on an exclusive view: exchanges two rows in place with
    /// `list.swap`'s contract (issue #729). Both indices are evaluated before
    /// the view is borrowed, so they may read it, as `len(v) - 1` does.
    pub(super) fn view_swap(
        &mut self,
        base: &Expr,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let exclusive = match &ungroup(base).kind {
            Expression::Unary(op, _) => op == "&mut",
            _ => matches!(self.expression_type_hint(base), Some(Ty::View(_, true))),
        };
        if !exclusive {
            return Err(base.at.error(
                "a shared slice view is read-only, so it has no `swap`; take an exclusive view with `&mut xs[a:b]`",
            ));
        }
        if args.len() != 2 {
            return Err(base.at.error("swap requires two i64 indices"));
        }
        let mut slots = Vec::new();
        for argument in args {
            let (actual, loans) = self.observe_expected(argument, Some(Ty::I64), ops)?;
            self.same(Some(actual), Some(Ty::I64), &argument.at)?;
            let slot = self.slot(Ty::I64, &argument.at)?;
            ops.push(Op::StoreLocal(slot));
            Self::end_reads(loans, ops);
            slots.push(slot);
        }
        let (view, loan) = self.view_source(base, ops)?;
        ops.extend(slots.into_iter().map(Op::MoveLocal));
        self.mark(&base.at, ops);
        ops.push(Op::Collection(CollectionOp::ViewSwap(view)));
        ops.push(Op::Drop);
        ops.push(Op::UseLoan(loan));
        Ok(None)
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
        if **element == Ty::Text {
            // The scalar at a position, as `s[i]` reads it, which allocates
            // nothing.
            ops.push(Op::Collection(CollectionOp::TextView(TextViewOp::Index)));
            return Ok((Ty::Str, reads));
        }
        ops.push(self.index_op(CollectionOp::ViewRow(view.clone(), false)));
        ops.push(Op::ReadRef((**element).clone()));
        Ok(((**element).clone(), reads))
    }
}
