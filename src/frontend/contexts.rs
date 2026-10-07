//! Concrete context managers share ordinary lexical cleanup and early exits.
use super::*;
use crate::record::method;

pub(super) struct Context {
    pub slot: u8,
    pub exit: Vec<Op>,
    pub loan: Option<usize>,
}

impl Lower<'_> {
    pub(super) fn with_statement(
        &mut self,
        manager: &Expr,
        name: Option<&str>,
        body: &[Stmt],
        ops: &mut Vec<Op>,
    ) -> Result<BlockResult> {
        if generators::yields(body) {
            return Err(manager.at.error("yield inside with is not supported yet"));
        }
        let saved = self.names.clone();
        let start = self.locals.len();
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
        let owner = match &ty {
            Ty::Ref(inner, true) if loan.is_some() => (**inner).clone(),
            ty => ty.clone(),
        };
        if !matches!(owner, Ty::Class(_) | Ty::File) {
            return Err(manager
                .at
                .error("with requires an owned class instance or File, or explicit &mut borrow"));
        }
        let slot = self.slot(ty, &manager.at)?;
        ops.push(Op::StoreLocal(slot));
        let receiver = Ty::Ref(Rc::new(owner.clone()), true);
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
        let context = Context { slot, exit, loan };
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
            self.names.insert(
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
        let result = self.block_inner(body, ops, false)?;
        if matches!(result, BlockResult::Continues(_)) {
            self.cleanup(start, ops);
        }
        self.contexts.pop();
        self.names = saved;
        Ok(result)
    }

    pub(super) fn context_receiver(&self, context: &Context, ops: &mut Vec<Op>) {
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
