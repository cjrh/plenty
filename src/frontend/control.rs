use super::*;
use crate::control::ControlOp;

impl Lower<'_> {
    pub(super) fn control_new(
        &mut self,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if !args.is_empty() {
            return Err(at.error("CancellationToken() takes no arguments"));
        }
        ops.push(Op::Control(ControlOp::New));
        Ok(Some(ControlOp::New.signature().1))
    }

    pub(super) fn control_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let observed = self.observe(base, ops)?;
        self.observed_control_method(base, name, args, observed, ops)
    }

    pub(super) fn observed_control_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        observed: (Ty, Vec<usize>),
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let (ty, loans) = observed;
        if name == "share" && args.is_empty() {
            Self::end_reads(loans, ops);
            return Ok(Some(ty));
        }
        let op = match (name, args) {
            ("cancel", []) => ControlOp::Cancel,
            ("is_cancelled", []) => ControlOp::IsCancelled,
            ("wait", []) => ControlOp::Wait,
            ("wait_timeout", [timeout]) => {
                let actual = self.expr_expected(timeout, Some(Ty::U64), ops)?;
                self.same(actual, Some(Ty::U64), &timeout.at)?;
                ControlOp::WaitTimeout
            }
            _ => return Err(base.at.error("CancellationToken supports share(), cancel(), is_cancelled(), wait(), and wait_timeout(timeout_ms)")),
        };
        let output = op.signature().1;
        ops.push(Op::Control(op));
        Self::end_reads(loans, ops);
        if output == Ty::Unit {
            ops.push(Op::Drop);
            Ok(None)
        } else {
            Ok(Some(output))
        }
    }
}
