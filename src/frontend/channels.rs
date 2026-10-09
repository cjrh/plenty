use super::*;
use crate::channel::ChannelOp;

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
        } else if !*sender && matches!(name, "recv" | "recv_nowait") && args.is_empty() {
            ChannelOp::Recv((**message).clone(), name == "recv_nowait")
        } else {
            return Err(base.at.error(if *sender {
                "Sender supports share(), send(value), and send_nowait(value)"
            } else {
                "Receiver supports share(), recv(), and recv_nowait()"
            }));
        };
        let output = op.signature().1;
        ops.push(Op::Channel(op));
        Self::end_reads(loans, ops);
        Ok(Some(output))
    }
}
