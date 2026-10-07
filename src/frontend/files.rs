//! Intrinsic methods for the opaque, affine file owner.
use super::*;

impl Lower<'_> {
    pub(super) fn open_file(
        &mut self,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if !(1..=2).contains(&args.len()) {
            return Err(at.error("open takes a path and optional mode string"));
        }
        let mut loans = Vec::new();
        for arg in args {
            let (ty, read) = self.observe(arg, ops)?;
            self.same(Some(ty), Some(Ty::Str), &arg.at)?;
            loans.extend(read);
        }
        if args.len() == 1 {
            ops.push(Op::PushStr(self.heap.add_str("r".into())));
        }
        ops.push(Op::Collection(CollectionOp::OpenFile));
        Self::end_reads(loans, ops);
        Ok(Some(CollectionOp::OpenFile.signature().1))
    }

    pub(super) fn file_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if name != "close" {
            return Err(base.at.error(format!("unknown File method `{name}`")));
        }
        if !args.is_empty() {
            return Err(base.at.error("close takes no arguments"));
        }
        let (_, loan) = self.borrow(base, true, ops)?;
        ops.push(Op::ReadRef(Ty::File));
        ops.push(Op::Collection(CollectionOp::FileClose));
        ops.push(Op::UseLoan(loan));
        Ok(Some(CollectionOp::FileClose.signature().1))
    }
}
