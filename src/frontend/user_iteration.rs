//! Structural user iterators call ordinary exclusive methods. Borrowed results
//! retain the method receiver loan; advancing again is an ordinary conflicting
//! exclusive borrow while an earlier item is live.
use super::*;
use collections::Iteration;

impl Lower<'_> {
    pub(super) fn strict_streaming_origin(&self, origin: usize) -> bool {
        let mut pending = vec![origin];
        let mut seen = HashSet::default();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            if self.pipeline.streaming_items.contains(&id) {
                return true;
            }
            let loan = &self.loans[id];
            pending.extend(loan.parent);
            pending.extend(loan.dependencies.iter().copied());
        }
        false
    }

    fn user_iterator_method(&mut self, ty: &Ty, at: &Token) -> Result<Option<String>> {
        let Ty::Class(class) = ty else {
            return Ok(None);
        };
        self.sync_data()?;
        let name = crate::record::method(&class.name, "next");
        let valid = if let Some(template) = self.generics.templates.get(&name) {
            template.inputs.len() == 1 && template.inputs[0].1.name.as_deref() == Some("&mut")
        } else if let Some(sig) = self.sigs.get(&name) {
            sig.inputs.len() == 1 && matches!(&sig.inputs[0].1, Ty::Ref(_, true))
        } else {
            return Ok(None);
        };
        if !valid {
            return Err(at.error("a user iterator requires next(self: &mut Self) -> Option[T] with no other parameters"));
        }
        Ok(Some(class.name.to_string()))
    }

    fn user_iterator_item(&self, output: Option<Ty>, at: &Token) -> Result<(Ty, Ty)> {
        let Some(option @ Ty::Enum(_)) = output else {
            return Err(at.error("a user iterator's next method must return Option[T]"));
        };
        let Ty::Enum(sum) = &option else {
            unreachable!()
        };
        if !sum.is_option() {
            return Err(at.error("a user iterator's next method must return Option[T]"));
        }
        let item = sum.get().variants[1].fields[0].clone();
        Ok((option, item))
    }

    pub(super) fn user_next(
        &mut self,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Option<Ty>> {
        let [argument] = args else { return Ok(None) };
        let base = match &ungroup(argument).kind {
            Expression::Unary(op, base) if op == "&mut" => &**base,
            _ => argument,
        };
        let Some(ty) = self.place_type(base) else {
            return Ok(None);
        };
        let Some(owner) = self.user_iterator_method(&ty, at)? else {
            return Ok(None);
        };
        let output = self.class_method(base, &owner, "next", None, &[], ops)?;
        let (output, _) = self.user_iterator_item(output, at)?;
        Ok(Some(output))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn user_iteration(
        &mut self,
        names: &[String],
        iterable: &Expr,
        ty: &Ty,
        borrowed: bool,
        mutable: bool,
        loans: &[usize],
        reverse: bool,
        ops: &mut Vec<Op>,
    ) -> Result<Option<Iteration>> {
        let Some(owner) = self.user_iterator_method(ty, &iterable.at)? else {
            return Ok(None);
        };
        if borrowed && !mutable {
            return Err(iterable
                .at
                .error("a user iterator requires an exclusive source; use &mut iterator"));
        }
        if reverse {
            return Err(iterable
                .at
                .error("reversed is not supported for user iterators"));
        }
        let stored = if borrowed {
            Ty::Ref(Arc::new(ty.clone()), true)
        } else {
            ty.clone()
        };
        let slot = self.slot(stored.clone(), &iterable.at)?;
        ops.push(Op::StoreLocal(slot));
        if let Some(&loan) = loans.first() {
            // Register a carried record just like an ordinary binding, so
            // suspension sees the view class rather than only its source.
            self.bind_reference(slot, loan, false, ops);
        }
        let name = format!("<user iterator {slot}>");
        self.bind(
            name.clone(),
            Local {
                slot,
                ty: stored,
                mutable: true,
            },
        );
        let base = Expr {
            at: iterable.at.clone(),
            kind: Expression::Name(name.clone()),
        };
        let mut condition = Vec::new();
        let output = self.class_method(&base, &owner, "next", None, &[], &mut condition)?;
        let (option, item) = self.user_iterator_item(output, &iterable.at)?;
        let target = self.slot(item.clone(), &iterable.at)?;
        if item.carries_loan() {
            let call = Expr {
                at: iterable.at.clone(),
                kind: Expression::Method(Box::new(base), "next".into(), vec![], Pos::default()),
            };
            let loan = self.reference_origin(&call, &condition)?;
            self.bind_reference(target, loan, false, &mut condition);
            self.pipeline.streaming_items.insert(loan);
        }
        let pending = self.slot(option.clone(), &iterable.at)?;
        let Ty::Enum(sum) = option else {
            unreachable!()
        };
        condition.extend([
            Op::StoreLocal(pending),
            Op::LoadLocal(pending),
            Op::Enum(crate::sum::EnumOp::Tag(sum.clone())),
            Op::PushInt(Value::I64(1)),
            Op::Eq,
        ]);
        condition.extend(loans.iter().copied().map(Op::UseLoan));
        let mut body = vec![
            Op::MoveLocal(pending),
            Op::Enum(crate::sum::EnumOp::Take(sum.clone(), 1, 0)),
            Op::StoreLocal(target),
            Op::DropLocal(pending),
        ];
        body.extend(loans.iter().copied().map(Op::UseLoan));
        let item_name = if names.len() == 1 {
            names[0].clone()
        } else {
            format!("<user iterator item {target}>")
        };
        self.bind(
            item_name.clone(),
            Local {
                slot: target,
                ty: item,
                mutable: false,
            },
        );
        if names.len() > 1 {
            for name in names {
                self.names.remove(name);
            }
            self.unpack_tuple(
                names,
                false,
                &Expr {
                    at: iterable.at.clone(),
                    kind: Expression::Name(item_name.clone()),
                },
                &mut body,
            )?;
            self.names.remove(&item_name);
        }
        self.names.remove(&name);
        // A short-circuiting adapter can skip this poll entirely. No item
        // originates on that path; stop its condition-only loan facts from
        // flowing backwards through the skipped call into source setup.
        let mut absent: Vec<_> = condition
            .iter()
            .filter_map(|op| match op {
                Op::Loan(loan) => Some(Op::EndLoan(loan.id)),
                _ => None,
            })
            .collect();
        absent.extend([
            Op::Enum(crate::sum::EnumOp::New(sum, 0)),
            Op::StoreLocal(pending),
        ]);
        Ok(Some(Iteration {
            exact_len: None,
            counter: None,
            // An unmanaged record still has a lexical lifetime. In particular,
            // its strict receiver borrow must end when an owning loop exits.
            cleanup: if borrowed {
                vec![]
            } else {
                vec![Op::Access(slot, true, None)]
            },
            condition,
            body,
            step: vec![Op::DropLocal(target)],
            target,
            absent,
        }))
    }
}
