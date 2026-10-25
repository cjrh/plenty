//! Concrete context managers share ordinary lexical cleanup and early exits.
use super::*;
use crate::record::method;

/// What a context's exit operates on.
pub(super) enum Kind {
    /// `__exit__` or an intrinsic exit receives the manager by `&mut`.
    Manager,
    /// The exit joins a scoped task.
    Thread,
    /// The exit unlocks the mutex at this written place, which guards a
    /// value of this type and may be aliasable (see `Lower::aliasable`).
    Lock(String, Ty, bool),
    /// The exit cancels the token share the slot holds.
    Cancel,
    /// The exit writes a lent pipeline's state back to its lender.
    Lent,
}

pub(super) struct Context {
    pub slot: u8,
    pub exit: Vec<Op>,
    pub loan: Option<usize>,
    pub kind: Kind,
    /// The `with` statement, reported when its exit keeps a borrow live.
    pub at: crate::ownership::Site,
}

impl Lower<'_> {
    pub(super) fn with_statement(
        &mut self,
        manager: &Expr,
        name: Option<&str>,
        body: &[Stmt],
        ops: &mut Vec<Op>,
    ) -> Result<BlockResult> {
        if let Some((base, args)) = mutex::lock_expression(manager) {
            if matches!(self.place_type(base), Some(Ty::Shared(_))) {
                let mut content = shared::content(base);
                while let Some(Ty::Shared(_)) = self.place_type(&content) {
                    content = shared::content(&content);
                }
                if self
                    .place_type(&content)
                    .as_ref()
                    .and_then(crate::mutex::class)
                    .is_some()
                {
                    return self.with_lock(manager, &content, args, name, body, ops);
                }
            }
            if self
                .place_type(base)
                .as_ref()
                .and_then(crate::mutex::class)
                .is_some()
            {
                return self.with_lock(manager, base, args, name, body, ops);
            }
            if self
                .expression_type_hint(base)
                .as_ref()
                .and_then(crate::mutex::class)
                .is_some()
            {
                return Err(base.at.error(
                    "lock() requires a named mutex or a field of one; bind the mutex to a name first",
                ));
            }
        }
        if generators::yields(body) {
            return Err(manager.at.error("yield inside with is not supported yet"));
        }
        if let Some((call, propagate, drop_job)) = threads::spawn_expression(manager) {
            return self.with_thread(call, propagate, drop_job, name, body, ops);
        }
        if let Expression::Method(base, method, args, _) = &ungroup(manager).kind
            && method == "cancel_on_exit"
            && self.token_place(base)
        {
            return self.with_cancel(manager, base, args, name, body, ops);
        }
        let saved = self.names.clone();
        let start = self.locals.len();
        let manager_start = ops.len();
        let (ty, loan) = if let Expression::Unary(op, base) = &ungroup(manager).kind {
            if op == "&mut" {
                let (ty, loan) = self.borrow(base, true, ops)?;
                (ty, Some(loan))
            } else {
                (self.value(manager, ops)?, None)
            }
        } else {
            (self.value(manager, ops)?, None)
        };
        self.building(&manager.at, manager_start, ops, false);
        let owner = match &ty {
            Ty::Ref(inner, true) if loan.is_some() => (**inner).clone(),
            ty => ty.clone(),
        };
        if matches!(ty, Ty::Channel(..) | Ty::Executor) {
            // An owned endpoint guard drops its handle in lexical cleanup order.
            // Placing it after a task manager disconnects its peers before join.
            let slot = self.slot(ty.clone(), &manager.at)?;
            ops.push(Op::StoreLocal(slot));
            // The guard is the moved binding's new owner; diagnostics keep its name.
            if let Expression::Name(source) = &ungroup(manager).kind {
                self.name_slot(slot, source);
            }
            if let Some(name) = name {
                if self.names.contains_key(name) {
                    return Err(manager.at.error(format!("duplicate binding `{name}`")));
                }
                let loan = self.new_loan(slot, false, None, ops);
                let reference = Ty::Ref(Arc::new(ty), false);
                ops.push(Op::BorrowLocal(slot, false));
                let target = self.slot(reference.clone(), &manager.at)?;
                ops.push(Op::StoreLocal(target));
                self.reference_locals.insert(target, loan);
                self.bind(
                    name.into(),
                    Local {
                        slot: target,
                        ty: reference,
                        mutable: false,
                    },
                );
            }
            let result = self.block_inner(body, ops, &Tail::Discard)?;
            if matches!(result, BlockResult::Continues(_)) {
                self.cleanup(start, ops);
            }
            self.names = saved;
            return Ok(result);
        }
        if !matches!(owner, Ty::Class(_) | Ty::File) {
            return Err(manager
                .at
                .error("with requires an owned class instance, File, or channel endpoint, or explicit &mut borrow"));
        }
        let slot = self.slot(ty, &manager.at)?;
        ops.push(Op::StoreLocal(slot));
        let receiver = Ty::Ref(Arc::new(owner.clone()), true);
        let (entry, enter, exit) = if let Ty::Class(class) = &owner {
            let mut entry = None;
            for name in ["__enter__", "__exit__"] {
                modules::check_member(self.access, &class.name, name, &manager.at)?;
                let callee = method(&class.name, name);
                let sig = self
                    .sigs
                    .get(&callee)
                    .ok_or_else(|| manager.at.error(format!("context manager requires {name}")))?;
                if sig.inputs.len() != 1 || sig.inputs[0].1 != receiver {
                    return Err(manager
                        .at
                        .error(format!("{name} requires only self: &mut {}", class.name)));
                }
                if name == "__exit__" && !sig.outputs.is_empty() {
                    return Err(manager.at.error("__exit__ must return ()"));
                }
                if name == "__enter__" {
                    entry = sig.outputs.first().cloned();
                }
            }
            (
                entry,
                vec![Op::Call(method(&class.name, "__enter__"))],
                vec![Op::Call(method(&class.name, "__exit__"))],
            )
        } else {
            // File's intrinsic entry returns its receiver. Exit closes without
            // replacing an in-flight body error; explicit close reports errors.
            (
                Some(receiver.clone()),
                vec![],
                vec![
                    Op::ReadRef(Ty::File),
                    Op::Collection(CollectionOp::FileClose),
                    Op::Drop,
                ],
            )
        };
        let context = Context {
            slot,
            exit,
            loan,
            kind: Kind::Manager,
            at: site(&manager.at, true),
        };
        let entry_loan = self.new_loan(
            loan.map(|id| self.loans[id].root).unwrap_or(slot),
            true,
            loan,
            ops,
        );
        if loan.is_some() {
            ops.push(Op::LoadLocal(slot));
        } else {
            ops.push(Op::BorrowLocal(slot, true));
        }
        ops.extend(enter);
        self.call_reference_result(
            "", // Context entry keeps the whole manager borrowed until exit.
            &FnSig {
                inputs: vec![("self".into(), receiver)],
                outputs: entry.clone().into_iter().collect(),
            },
            &[entry_loan],
            ops,
        );
        ops.push(Op::UseLoan(entry_loan));
        let result_loan = matches!(entry, Some(Ty::Ref(..))).then(|| self.loans.len() - 1);
        self.contexts.push(context);
        if let Some(name) = name {
            if self.names.contains_key(name) {
                return Err(manager.at.error(format!("duplicate binding `{name}`")));
            }
            let ty =
                entry.ok_or_else(|| manager.at.error("unit entry cannot have an as binding"))?;
            let target = self.slot(ty.clone(), &manager.at)?;
            ops.push(Op::StoreLocal(target));
            if let Some(loan) = result_loan {
                self.reference_locals.insert(target, loan);
            }
            self.bind(
                name.into(),
                Local {
                    slot: target,
                    ty,
                    mutable: false,
                },
            );
        } else if entry.is_some() {
            ops.push(Op::Drop);
        }
        let result = self.block_inner(body, ops, &Tail::Discard)?;
        if matches!(result, BlockResult::Continues(_)) {
            self.cleanup(start, ops);
        }
        self.contexts.pop();
        self.names = saved;
        Ok(result)
    }

    /// A token, or a reference to one, at a written place.
    pub(super) fn token_place(&self, base: &Expr) -> bool {
        let mut ty = self
            .place_type(base)
            .or_else(|| self.expression_type_hint(base));
        while let Some(Ty::Ref(inner, _)) = ty {
            ty = Some((*inner).clone());
        }
        ty == Some(Ty::CancellationToken)
    }

    /// `with token.cancel_on_exit():` holds a share of the token and cancels
    /// it on every exit from the block. A share copies the handle, so the
    /// guard allocates nothing (issue #355).
    fn with_cancel(
        &mut self,
        manager: &Expr,
        base: &Expr,
        args: &[Expr],
        name: Option<&str>,
        body: &[Stmt],
        ops: &mut Vec<Op>,
    ) -> Result<BlockResult> {
        if !args.is_empty() {
            return Err(manager.at.error("cancel_on_exit takes no arguments"));
        }
        if name.is_some() {
            return Err(manager
                .at
                .error("cancel_on_exit() has no value to bind; remove `as`"));
        }
        let saved = self.names.clone();
        let start = self.locals.len();
        self.control_method(base, "share", &[], ops)?;
        let slot = self.slot(Ty::CancellationToken, &manager.at)?;
        ops.push(Op::StoreLocal(slot));
        ops.push(Op::Tokens(crate::op::TokenMark::Guard(slot)));
        self.contexts.push(Context {
            slot,
            exit: vec![
                Op::Access(slot, false, None),
                Op::LoadLocal(slot),
                Op::Control(crate::control::ControlOp::Cancel),
                Op::Drop,
            ],
            loan: None,
            kind: Kind::Cancel,
            at: site(&manager.at, true),
        });
        let result = self.block_inner(body, ops, &Tail::Discard)?;
        if matches!(result, BlockResult::Continues(_)) {
            self.cleanup(start, ops);
        }
        self.contexts.pop();
        self.names = saved;
        Ok(result)
    }

    pub(super) fn context_receiver(&self, context: &Context, ops: &mut Vec<Op>) {
        if !matches!(context.kind, Kind::Manager) {
            return;
        }
        if let Some(loan) = context.loan {
            ops.push(Op::Access(self.loans[loan].root, true, Some(loan)));
            ops.push(Op::LoadLocal(context.slot));
            ops.push(Op::UseLoan(loan));
        } else {
            ops.push(Op::Access(context.slot, true, None));
            ops.push(Op::BorrowLocal(context.slot, true));
        }
    }
}
