//! Intrinsic methods for the opaque, affine file owner.
use super::*;
use crate::nominal::Nominal;
use crate::record::ClassType;

/// A read-only byte operand supplied by a `Bytes` buffer or a `bytes` literal.
enum ByteSource {
    Buffer(Nominal<ClassType>),
    Literal,
}

// One list of modes serves this check and the runtime's `open`.
#[path = "../../plenty-runtime/src/open_modes.rs"]
mod open_modes;

impl Lower<'_> {
    pub(super) fn filesystem_call(
        &mut self,
        operation: crate::filesystem::FsOp,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if !operation.mutable() {
            return self.system_call(CollectionOp::Filesystem(operation), args, at, ops);
        }
        let [arg] = args else {
            return Err(at.error("directory operation takes one owner"));
        };
        let (_, loan) = self.borrow(arg, true, ops)?;
        let (inputs, output) = operation.signature();
        self.same(self.place_type(arg), Some(inputs[0].clone()), &arg.at)?;
        ops.push(Op::ReadRef(inputs[0].clone()));
        ops.push(Op::Collection(CollectionOp::Filesystem(operation)));
        ops.push(Op::UseLoan(loan));
        Ok(Some(output))
    }

    /// How the argument for a native's read-only byte operand is supplied,
    /// when it is a `Bytes` or `bytes` value or reference.
    fn byte_source(&self, arg: &Expr) -> Option<ByteSource> {
        let ty = match self.place_type(arg)? {
            Ty::Ref(inner, _) => (*inner).clone(),
            other => other,
        };
        if ty == Ty::ByteStr {
            return Some(ByteSource::Literal);
        }
        byte_buffers::byte_buffer(&ty).map(ByteSource::Buffer)
    }

    /// Whether the argument for `source` is a `bytes` literal, so that the
    /// native's literal variant reads it.
    pub(super) fn literal_source(&self, source: Option<usize>, args: &[Expr]) -> bool {
        source
            .and_then(|index| args.get(index))
            .is_some_and(|arg| matches!(self.byte_source(arg), Some(ByteSource::Literal)))
    }

    /// Observes one native operand. A `Bytes` buffer or reference given for
    /// a read-only source operand passes its storage, so std code generic
    /// over `std.bytes.ByteSource` reaches either form without a copy.
    fn operand(
        &mut self,
        arg: &Expr,
        expected: Ty,
        source: bool,
        ops: &mut Vec<Op>,
    ) -> Result<Vec<usize>> {
        if source && expected == Ty::ByteStorage {
            if let Some(ByteSource::Buffer(class)) = self.byte_source(arg) {
                let loan = self.byte_storage_place(&class, arg, false, ops)?;
                ops.push(Op::ReadRef(Ty::ByteStorage));
                return Ok(vec![loan]);
            }
            let other = match self.place_type(arg) {
                Some(Ty::Ref(inner, _)) => Some((*inner).clone()),
                other => other,
            };
            if let Some(other) = other.filter(|ty| *ty != Ty::ByteStorage) {
                return Err(arg.at.error(format!(
                    "bytes are read in place only from a `std.bytes.Bytes` buffer or a `bytes` literal, got {other}"
                )));
            }
        }
        let (actual, loans) = self.observe_expected(arg, Some(expected.clone()), ops)?;
        self.same(Some(actual), Some(expected), &arg.at)?;
        Ok(loans)
    }

    pub(super) fn bytes_call(
        &mut self,
        operation: crate::bytes::BytesOp,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let operation = match operation.literal() {
            Some(literal) if self.literal_source(operation.source(), args) => literal,
            _ => operation,
        };
        if !operation.mutable() {
            let source = operation.source();
            return self.exclusive_call(
                CollectionOp::Bytes(operation),
                &|_| false,
                source,
                args,
                at,
                ops,
            );
        }
        let (inputs, output) = operation.signature();
        if args.len() != inputs.len() {
            return Err(at.error(format!(
                "expected {} arguments, got {}",
                inputs.len(),
                args.len()
            )));
        }
        let (_, loan) = self.borrow(&args[0], true, ops)?;
        self.same(
            self.place_type(&args[0]),
            Some(inputs[0].clone()),
            &args[0].at,
        )?;
        ops.push(Op::ReadRef(inputs[0].clone()));
        let mut reads = Vec::new();
        for (index, (arg, expected)) in args.iter().zip(inputs).enumerate().skip(1) {
            reads.extend(self.operand(arg, expected, operation.source() == Some(index), ops)?);
        }
        ops.push(Op::Collection(CollectionOp::Bytes(operation)));
        ops.push(Op::UseLoan(loan));
        Self::end_reads(reads, ops);
        Ok(Some(output))
    }

    /// A native primitive whose `exclusive` operands are written through and
    /// need exclusive loans; the rest are observed. `source` is the operand
    /// only read for its bytes, if any.
    pub(super) fn exclusive_call(
        &mut self,
        operation: CollectionOp,
        exclusive: &dyn Fn(usize) -> bool,
        source: Option<usize>,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let (inputs, output) = operation.signature();
        if args.len() != inputs.len() {
            return Err(at.error(format!(
                "expected {} arguments, got {}",
                inputs.len(),
                args.len()
            )));
        }
        let (mut loans, mut reads) = (Vec::new(), Vec::new());
        for (index, (arg, expected)) in args.iter().zip(inputs).enumerate() {
            if exclusive(index) {
                let (_, loan) = self.borrow(arg, true, ops)?;
                self.same(self.place_type(arg), Some(expected.clone()), &arg.at)?;
                ops.push(Op::ReadRef(expected));
                loans.push(loan);
            } else {
                reads.extend(self.operand(arg, expected, source == Some(index), ops)?);
            }
        }
        ops.push(Op::Collection(operation));
        for loan in loans.into_iter().rev() {
            ops.push(Op::UseLoan(loan));
        }
        Self::end_reads(reads, ops);
        Ok(Some(output))
    }

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
