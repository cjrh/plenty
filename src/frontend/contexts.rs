//! Concrete context managers share ordinary lexical cleanup and early exits.
use super::*;
use crate::record::method;

pub(super) struct Context {
    pub slot: u8,
    pub exit: String,
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
        let Ty::Class(class) = self.value(manager, ops)? else {
            return Err(manager.at.error("with requires an owned class instance"));
        };
        let slot = self.slot(Ty::Class(class.clone()), &manager.at)?;
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
        ops.push(Op::BorrowLocal(slot, true));
        ops.push(Op::Call(method(&class.name, "__enter__")));
        self.contexts.push(Context {
            slot,
            exit: method(&class.name, "__exit__"),
        });
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
}
