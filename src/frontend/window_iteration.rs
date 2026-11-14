//! Shared windows/chunks are loop-only sources, not stored user iterators.
//! The source loan protects both the count and the yielded view descriptors.
use super::*;
use crate::text::TextViewOp;

fn both(first: Vec<Op>, second: Vec<Op>) -> Vec<Op> {
    let mut result = first;
    result.push(Op::If {
        then: second.into(),
        elifs: Arc::from([]),
        otherwise: Arc::from([Op::PushBool(false)]),
    });
    result
}

impl Lower<'_> {
    pub(super) fn window_iteration(
        &mut self,
        names: &[String],
        iterable: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<Option<collections::Iteration>> {
        let Expression::Method(base, method, args, _) = &ungroup(iterable).kind else {
            return Ok(None);
        };
        if !matches!(method.as_str(), "windows" | "chunks") {
            return Ok(None);
        }
        let Some(text) = self.window_source(base) else {
            return Ok(None); // Preserve ordinary user methods.
        };
        if names.len() != 1 || args.len() != 1 {
            return Err(iterable
                .at
                .error("windows/chunks require one width and one view binding"));
        }
        let at = &iterable.at;
        let temporary_start = self.expression_temps.len();
        let (view, loan) = self.view_argument(base, false, text, ops)?;
        let source = self.slot(view.clone(), at)?;
        ops.push(Op::StoreLocal(source));
        let width = self.slot(Ty::I64, at)?;
        let (actual, reads) = self.observe_expected(&args[0], Some(Ty::I64), ops)?;
        self.same(Some(actual), Some(Ty::I64), &args[0].at)?;
        ops.push(Op::StoreLocal(width));
        Self::end_reads(reads, ops);
        let length = self.slot(Ty::I64, at)?;
        let start = self.slot(Ty::I64, at)?;
        let count = self.slot(Ty::I64, at)?;
        let target = self.slot(view.clone(), at)?;
        ops.extend([
            Op::LoadLocal(source),
            Op::Collection(CollectionOp::ViewLen(view.clone())),
            Op::StoreLocal(length),
            Op::PushInt(Value::I64(0)),
            Op::StoreLocal(start),
        ]);
        let chunks = method == "chunks";
        let (condition, body, step) = if view == text_view() {
            self.text_windows(
                (source, width, length, start, count, target),
                chunks,
                at,
                ops,
            )?
        } else {
            // Compare width with remaining length before adding it to start.
            // Hence start + count <= length <= i64.MAX, even for huge widths.
            let remaining = vec![
                Op::LoadLocal(length),
                Op::LoadLocal(start),
                Op::Integer(IntegerOp::WrappingSub),
            ];
            let mut available = remaining.clone();
            available.extend(if chunks {
                vec![Op::PushInt(Value::I64(0)), Op::Gt]
            } else {
                vec![Op::LoadLocal(width), Op::Ge]
            });
            let condition = both(
                vec![Op::LoadLocal(width), Op::PushInt(Value::I64(0)), Op::Gt],
                available,
            );
            let mut body = vec![Op::LoadLocal(width)];
            if chunks {
                body.extend(remaining.clone());
                body.push(Op::Le);
                body.push(Op::If {
                    then: Arc::from([Op::LoadLocal(width)]),
                    elifs: Arc::from([]),
                    otherwise: remaining.into(),
                });
            }
            body.extend([
                Op::StoreLocal(count),
                Op::LoadLocal(source),
                Op::LoadLocal(start),
                Op::LoadLocal(count),
                Op::Collection(CollectionOp::ViewSpan(view.clone())),
                Op::StoreLocal(target),
            ]);
            let step = vec![
                Op::LoadLocal(start),
                if chunks {
                    Op::LoadLocal(count)
                } else {
                    Op::PushInt(Value::I64(1))
                },
                Op::Integer(IntegerOp::WrappingAdd),
                Op::StoreLocal(start),
            ];
            (condition, body, step)
        };
        // A shared window may overlap another. None can authorize a write to
        // its source; the same origin remains live through every backedge.
        self.reference_locals.insert(target, loan);
        self.bind(
            names[0].clone(),
            Local {
                slot: target,
                ty: view,
                mutable: false,
            },
        );
        self.expression_temps.truncate(temporary_start);
        let mut condition = condition;
        condition.push(Op::UseLoan(loan));
        let mut body = body;
        body.push(Op::UseLoan(loan));
        Ok(Some(collections::Iteration {
            exact_len: None,
            cleanup: vec![],
            condition,
            absent: vec![],
            body,
            step,
            target,
            counter: None,
        }))
    }

    /// Only builtin owners/views are intrinsic sources. This also recognizes
    /// fallible list displays, whose element type is inferred when lowered.
    pub(super) fn window_source(&self, base: &Expr) -> Option<bool> {
        let hint = self.slice_view_type(base, false);
        let owner = match &ungroup(base).kind {
            Expression::Try(inner) => Some(ungroup(inner)),
            Expression::Method(inner, name, args, _) if name == "unwrap" && args.is_empty() => {
                Some(ungroup(inner))
            }
            _ => None,
        };
        let list_literal = owner.is_some_and(|owner| {
            matches!(
                &owner.kind, Expression::Collection { kind, .. } if kind == "list"
            )
        });
        (hint.is_some() || self.is_view(base) || list_literal).then_some(hint == Some(text_view()))
    }

    /// UTF-8 windows keep two byte cursors. Initial width scanning happens
    /// once; each overlapping advance decodes at most two scalars, avoiding
    /// a scalar-index-to-byte scan from the beginning for every window.
    fn text_windows(
        &mut self,
        (source, width, length, start, count, target): (u8, u8, u8, u8, u8, u8),
        chunks: bool,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<(Vec<Op>, Vec<Op>, Vec<Op>)> {
        let end = self.slot(Ty::I64, at)?;
        let scalar = self.slot(Ty::Str, at)?;
        let taken = self.slot(Ty::I64, at)?;
        let advance = |cursor| {
            vec![
                Op::LoadLocal(source),
                Op::LoadLocal(cursor),
                Op::Collection(CollectionOp::TextView(TextViewOp::Scalar)),
                Op::StoreLocal(scalar),
                Op::LoadLocal(cursor),
                Op::BorrowLocal(scalar, false),
                Op::Collection(CollectionOp::QueryRef(
                    crate::collection::Query::TextByteLen,
                )),
                Op::Integer(IntegerOp::WrappingAdd),
                Op::StoreLocal(cursor),
                Op::DropLocal(scalar),
            ]
        };
        let scan_condition = both(
            vec![Op::LoadLocal(end), Op::LoadLocal(length), Op::Lt],
            vec![Op::LoadLocal(taken), Op::LoadLocal(width), Op::Lt],
        );
        let mut scan_body = advance(end);
        scan_body.extend([
            Op::LoadLocal(taken),
            Op::PushInt(Value::I64(1)),
            Op::Integer(IntegerOp::WrappingAdd),
            Op::StoreLocal(taken),
        ]);
        let scan = vec![
            Op::PushInt(Value::I64(0)),
            Op::StoreLocal(taken),
            Op::Loop {
                condition: scan_condition.into(),
                body: scan_body.into(),
            },
        ];
        ops.extend([Op::PushInt(Value::I64(0)), Op::StoreLocal(end)]);
        ops.extend(scan.clone());
        let available = if chunks {
            vec![Op::LoadLocal(start), Op::LoadLocal(length), Op::Lt]
        } else {
            vec![Op::LoadLocal(taken), Op::LoadLocal(width), Op::Eq]
        };
        let condition = both(
            vec![Op::LoadLocal(width), Op::PushInt(Value::I64(0)), Op::Gt],
            available,
        );
        let body = vec![
            Op::LoadLocal(end),
            Op::LoadLocal(start),
            Op::Integer(IntegerOp::WrappingSub),
            Op::StoreLocal(count),
            Op::LoadLocal(source),
            Op::LoadLocal(start),
            Op::LoadLocal(count),
            Op::Collection(CollectionOp::ViewSpan(text_view())),
            Op::StoreLocal(target),
        ];
        let step = if chunks {
            let mut step = vec![Op::LoadLocal(end), Op::StoreLocal(start)];
            step.extend(scan);
            step
        } else {
            let mut step = advance(start);
            // At the final window end == length. Decrease taken to terminate
            // without reading a scalar beyond the allocation.
            step.extend([
                Op::LoadLocal(end),
                Op::LoadLocal(length),
                Op::Lt,
                Op::If {
                    then: advance(end).into(),
                    elifs: Arc::from([]),
                    otherwise: Arc::from([
                        Op::LoadLocal(taken),
                        Op::PushInt(Value::I64(1)),
                        Op::Integer(IntegerOp::WrappingSub),
                        Op::StoreLocal(taken),
                    ]),
                },
            ]);
            step
        };
        Ok((condition, body, step))
    }
}
