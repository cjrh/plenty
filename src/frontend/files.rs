//! Intrinsic methods for the opaque, affine file owner.
use super::*;

// One list of modes serves this check and the runtime's `open`.
#[path = "../../plenty-runtime/src/open_modes.rs"]
mod open_modes;

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
        if let Some(Expression::Text(mode)) = args.get(1).map(|mode| &ungroup(mode).kind) {
            if open_modes::OpenMode::parse(mode).is_none() {
                return Err(args[1].at.error(format!(
                    "unknown open mode `{mode}`; use one of {}",
                    open_modes::NAMES
                )));
            }
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
            "read" if !args.is_empty() => CollectionOp::FileReadSized,
            "read" => CollectionOp::FileRead,
            "readline" if !args.is_empty() => CollectionOp::FileReadLineSized,
            "readline" => CollectionOp::FileReadLine,
            "write" => CollectionOp::FileWrite,
            "flush" => CollectionOp::FileFlush,
            "sync" => CollectionOp::FileSync,
            "readable" => CollectionOp::FileReadable,
            "writable" => CollectionOp::FileWritable,
            "tell" => CollectionOp::FileTell,
            "readlines" => CollectionOp::FileReadLines,
            "writelines" => CollectionOp::FileWriteLines,
            "seek" => CollectionOp::FileSeek,
            "truncate" if args.is_empty() => CollectionOp::FileTruncate,
            "truncate" => CollectionOp::FileTruncateSized,
            _ => return Err(base.at.error(format!("unknown File method `{name}`"))),
        };
        let (inputs, output) = operation.signature();
        let count = inputs.len() - 1;
        if args.len() != count {
            return Err(base.at.error(if count == 0 {
                format!("{name} takes no arguments")
            } else {
                format!("{name} takes one argument")
            }));
        }
        let mut reads = Vec::new();
        if let Some(arg) = args.first() {
            let expected = inputs[1].clone();
            let (ty, loans) = self.observe_expected(arg, Some(expected.clone()), ops)?;
            self.same(Some(ty), Some(expected), &arg.at)?;
            reads = loans;
        }
        let (_, loan) = self.borrow(base, !matches!(name, "readable" | "writable"), ops)?;
        ops.push(Op::ReadRef(Ty::File));
        if count == 1 {
            ops.push(Op::Swap);
        }
        ops.push(Op::Collection(operation));
        ops.push(Op::UseLoan(loan));
        Self::end_reads(reads, ops);
        Ok(Some(output))
    }
}
