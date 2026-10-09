use super::*;
use crate::channel::{ChannelOp, SelectMode};

impl Lower<'_> {
    pub(super) fn channel_new(
        &mut self,
        ty: &TypeRef,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if ty.args.len() != 1 || args.len() != 1 {
            return Err(ty.at.error("use channel[T](positive_capacity)"));
        }
        let message = ty.args[0]
            .resolve(self.aliases)?
            .ok_or_else(|| ty.at.error("channel messages cannot be unit"))?;
        if !message.heap_storable() {
            return Err(ty.at.error("channel messages require a concrete wholly owned type without references or generator frames"));
        }
        let actual = self.expr_expected(&args[0], Some(Ty::U64), ops)?;
        self.same(actual, Some(Ty::U64), &args[0].at)?;
        let op = ChannelOp::New(message);
        let output = op.signature().1;
        ops.push(Op::Channel(op));
        Ok(Some(output))
    }

    pub(super) fn channel_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let (ty, loans) = self.observe(base, ops)?;
        let Ty::Channel(message, sender) = &ty else {
            unreachable!()
        };
        if name == "share" && args.is_empty() {
            // observe already retained one owned endpoint. Unlike copying a
            // container, explicit sharing preserves this queue's identity.
            Self::end_reads(loans, ops);
            return Ok(Some(ty));
        }
        let op = if *sender && matches!(name, "send" | "send_nowait") && args.len() == 1 {
            let actual = self.expr_expected(&args[0], Some((**message).clone()), ops)?;
            self.same(actual, Some((**message).clone()), &args[0].at)?;
            ChannelOp::Send((**message).clone(), name == "send_nowait")
        } else if *sender && name == "send_timeout" && args.len() == 2 {
            let actual = self.expr_expected(&args[0], Some((**message).clone()), ops)?;
            self.same(actual, Some((**message).clone()), &args[0].at)?;
            let actual = self.expr_expected(&args[1], Some(Ty::U64), ops)?;
            self.same(actual, Some(Ty::U64), &args[1].at)?;
            ChannelOp::SendTimeout((**message).clone())
        } else if !*sender && name == "recv_timeout" && args.len() == 1 {
            let actual = self.expr_expected(&args[0], Some(Ty::U64), ops)?;
            self.same(actual, Some(Ty::U64), &args[0].at)?;
            ChannelOp::RecvTimeout((**message).clone())
        } else if !*sender && matches!(name, "recv" | "recv_nowait") && args.is_empty() {
            ChannelOp::Recv((**message).clone(), name == "recv_nowait")
        } else {
            return Err(base.at.error(if *sender {
                "Sender supports share(), send(value), send_nowait(value), and send_timeout(value, timeout_ms)"
            } else {
                "Receiver supports share(), recv(), recv_nowait(), and recv_timeout(timeout_ms)"
            }));
        };
        let output = op.signature().1;
        ops.push(Op::Channel(op));
        Self::end_reads(loans, ops);
        Ok(Some(output))
    }

    pub(super) fn channel_select(
        &mut self,
        name: &str,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let mode = match name {
            "select_recv" => SelectMode::Wait,
            "select_recv_nowait" => SelectMode::Nowait,
            _ => SelectMode::Timeout,
        };
        let count = if mode == SelectMode::Timeout { 3 } else { 2 };
        if args.len() != count {
            return Err(at.error(format!(
                "{name} requires two borrowed receivers{}",
                if count == 3 {
                    " and a timeout in milliseconds"
                } else {
                    ""
                }
            )));
        }
        let mut messages = Vec::new();
        let mut loans = Vec::new();
        for arg in &args[..2] {
            // Public functions borrow explicitly; observing an existing
            // reference creates a child loan and retains its endpoint handle.
            let reference = matches!(&ungroup(arg).kind, Expression::Unary(op, _) if op == "&" || op == "&mut")
                || matches!(self.expression_type_hint(arg), Some(Ty::Ref(..)));
            if !reference {
                return Err(arg.at.error("selection borrows receivers; pass &receiver"));
            }
            let (ty, reads) = self.observe(arg, ops)?;
            let Ty::Channel(message, false) = ty else {
                return Err(arg.at.error("selection requires a borrowed Receiver"));
            };
            messages.push((*message).clone());
            loans.extend(reads);
        }
        if let Some(timeout) = args.get(2) {
            let actual = self.expr_expected(timeout, Some(Ty::U64), ops)?;
            self.same(actual, Some(Ty::U64), &timeout.at)?;
        }
        let op = ChannelOp::Select(messages.remove(0), messages.remove(0), mode);
        let output = op.signature().1;
        ops.push(Op::Channel(op));
        Self::end_reads(loans, ops);
        Ok(Some(output))
    }
}
