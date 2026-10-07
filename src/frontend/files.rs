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
        let operation = match name {
            "close" => CollectionOp::FileClose,
            "read" => CollectionOp::FileRead,
            "write" => CollectionOp::FileWrite,
            "flush" => CollectionOp::FileFlush,
            "sync" => CollectionOp::FileSync,
            _ => return Err(base.at.error(format!("unknown File method `{name}`"))),
        };
        let count = usize::from(name == "write");
        if args.len() != count {
            return Err(base.at.error(if count == 0 {
                format!("{name} takes no arguments")
            } else {
                "write takes one string argument".into()
            }));
        }
        let mut reads = Vec::new();
        if let Some(arg) = args.first() {
            let (ty, loans) = self.observe(arg, ops)?;
            self.same(Some(ty), Some(Ty::Str), &arg.at)?;
            reads = loans;
        }
        let (_, loan) = self.borrow(base, true, ops)?;
        ops.push(Op::ReadRef(Ty::File));
        if count == 1 {
            ops.push(Op::Swap);
        }
        let output = operation.signature().1;
        ops.push(Op::Collection(operation));
        ops.push(Op::UseLoan(loan));
        Self::end_reads(reads, ops);
        Ok(Some(output))
    }
}
