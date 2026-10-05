use super::*;

pub(super) fn yields(body: &[Stmt]) -> bool {
    body.iter().any(|stmt| match &stmt.kind {
        Statement::Yield(_) => true,
        Statement::If { yes, no, .. } => yields(yes) || yields(no),
        Statement::For { body, .. } | Statement::While { body, .. } => yields(body),
        Statement::Match { cases, .. } => cases.iter().any(|case| yields(&case.body)),
        _ => false,
    })
}

impl Lower<'_> {
    pub(super) fn cleanup(&self, start: usize, ops: &mut Vec<Op>) {
        for i in (start..self.locals.len()).rev() {
            if self.locals[i].managed() {
                ops.push(Op::DropLocal((self.parameters + i) as u8));
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
            .ok_or_else(|| at.error(format!("unknown binding `{name}`")))?;
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
