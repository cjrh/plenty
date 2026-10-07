use super::*;

pub(super) fn constructor(name: &str) -> String {
    format!("__plenty_try_generator_{name}")
}

pub(super) fn yields(body: &[Stmt]) -> bool {
    body.iter().any(|stmt| match &stmt.kind {
        Statement::Yield(_) => true,
        Statement::If { yes, no, .. } => yields(yes) || yields(no),
        Statement::For { body, .. }
        | Statement::While { body, .. }
        | Statement::With { body, .. } => yields(body),
        Statement::Match { cases, .. } => cases.iter().any(|case| yields(&case.body)),
        _ => false,
    })
}

impl Lower<'_> {
    pub(super) fn cleanup(&self, start: usize, ops: &mut Vec<Op>) {
        for i in (start..self.locals.len()).rev() {
            let slot = (self.parameters + i) as u8;
            if let Some(context) = self.contexts.iter().find(|context| context.slot == slot) {
                self.context_receiver(context, ops);
                ops.extend(context.exit.iter().cloned());
            }
            if self.locals[i].managed() {
                ops.push(Op::DropLocal(slot));
            }
        }
    }
    pub(super) fn next(&mut self, args: &[Expr], at: &Token, ops: &mut Vec<Op>) -> Result<Type> {
        let [Expr {
            kind: Expression::Name(name),
            ..
        }] = args
        else {
            return Err(at.error("next requires one named mutable generator binding"));
        };
        let local = self
            .names
            .get(name)
            .ok_or_else(|| at.error(format!("unknown binding `{name}`")))?
            .clone();
        if let Ty::Ref(inner, true) = &local.ty {
            let Ty::Generator(element) = inner.as_ref() else {
                return Err(at.error("next requires a generator"));
            };
            let output = crate::sum::option((**element).clone());
            let (ty, loan) = self.read_place(&local, ops);
            ops.push(Op::Collection(CollectionOp::Next(ty)));
            ops.push(Op::UseLoan(loan));
            return Ok(Some(output));
        }
        let Ty::Generator(element) = &local.ty else {
            return Err(at.error("next requires a generator"));
        };
        if !local.mutable {
            return Err(
                at.error("next requires a mutable generator binding; declare it with `mut`")
            );
        }
        ops.push(Op::Next(
            local.slot,
            format!("{}:{}: `{name}`", at.line, at.column),
        ));
        Ok(Some(crate::sum::option((**element).clone())))
    }
}
