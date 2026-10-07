//! Concrete context managers share ordinary lexical cleanup and early exits.
use super::*;
use crate::record::method;

pub(super) struct Context {
    pub slot: u8,
    pub exit: String,
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
        let class = match &ty {
            Ty::Class(class) => class.clone(),
            Ty::Ref(inner, true) if loan.is_some() => {
                let Ty::Class(class) = inner.as_ref() else {
                    return Err(manager.at.error("with requires a class instance"));
                };
                class.clone()
            }
            _ => {
                return Err(manager
                    .at
                    .error("with requires an owned class instance or explicit &mut borrow"))
            }
        };
        let slot = self.slot(ty, &manager.at)?;
        ops.push(Op::StoreLocal(slot));
        let mut entry = None;
        for name in ["__enter__", "__exit__"] {
            modules::check_member(self.access, &class.name, name, &manager.at)?;
            let callee = method(&class.name, name);
            let sig = self
                .sigs
                .get(&callee)
                .ok_or_else(|| manager.at.error(format!("context manager requires {name}")))?;
            if sig.inputs.len() != 1
                || sig.inputs[0].1 != Ty::Ref(Rc::new(Ty::Class(class.clone())), true)
            {
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
        let context = Context {
            slot,
            exit: method(&class.name, "__exit__"),
            loan,
        };
        self.context_receiver(&context, ops);
        ops.push(Op::Call(method(&class.name, "__enter__")));
        self.contexts.push(context);
        if let Some(name) = name {
            if self.names.contains_key(name) {
                return Err(manager.at.error(format!("duplicate binding `{name}`")));
            }
            let ty =
                entry.ok_or_else(|| manager.at.error("unit entry cannot have an as binding"))?;
            let target = self.slot(ty.clone(), &manager.at)?;
            ops.push(Op::StoreLocal(target));
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
