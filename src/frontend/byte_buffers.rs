//! Index syntax and iteration for `std.bytes.Bytes`. The class keeps one
//! private `ByteStorage` field, so `b[i]`, `b[i] = v`, and `for x in b` lower
//! straight to byte-storage primitives with the list index rules: negative
//! indices count from the end and a missing index traps at the source position.
//! There are no element references: bytes are not addressable values.
use super::collections::{self, Iteration};
use super::*;
use crate::bytes::BytesOp;
use crate::nominal::Nominal;
use crate::record::{ClassOp, ClassType};

pub(super) const BYTES_CLASS: &str = "std.bytes.Bytes";

/// The `Bytes` class when `ty` is a byte buffer value.
pub(super) fn byte_buffer(ty: &Ty) -> Option<Nominal<ClassType>> {
    match ty {
        Ty::Class(class)
            if class.name == BYTES_CLASS
                && class.get().fields.first().map(|f| &f.1) == Some(&Ty::ByteStorage) =>
        {
            Some(class.clone())
        }
        _ => None,
    }
}

/// Ops reading a byte buffer's storage from the `Bytes` value in `slot`.
fn storage_of(class: &Nominal<ClassType>, slot: u8) -> [Op; 2] {
    [
        Op::LoadLocal(slot),
        Op::Class(ClassOp::Field(class.clone(), 0)),
    ]
}

impl Lower<'_> {
    /// Iterate the buffer on the stack by index. The source is owned by the
    /// loop or held under a shared loan, so its length is read once and every
    /// read stays in range.
    pub(super) fn byte_iteration(
        &mut self,
        class: Nominal<ClassType>,
        ty: Ty,
        reverse: bool,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        let source = self.slot(ty, at)?;
        let target = self.slot(Ty::U8, at)?;
        let index = self.slot(Ty::I64, at)?;
        let end = self.slot(Ty::I64, at)?;
        ops.push(Op::StoreLocal(source));
        ops.extend(storage_of(&class, source));
        ops.extend([
            Op::Collection(CollectionOp::Bytes(BytesOp::Len)),
            Op::StoreLocal(end),
        ]);
        let mut body = Vec::new();
        let mut step = Vec::new();
        let condition = if reverse {
            ops.extend([Op::LoadLocal(end), Op::StoreLocal(index)]);
            collections::decrement(index, &mut body);
            vec![Op::LoadLocal(index), Op::PushInt(Value::I64(0)), Op::Gt]
        } else {
            ops.extend([Op::PushInt(Value::I64(0)), Op::StoreLocal(index)]);
            collections::increment(index, &mut step);
            vec![Op::LoadLocal(index), Op::LoadLocal(end), Op::Lt]
        };
        body.extend([
            Op::BorrowLocal(source, false),
            Op::Class(ClassOp::FieldRef(class, 0, false)),
            Op::LoadLocal(index),
            Op::Collection(CollectionOp::Bytes(BytesOp::Item)),
            Op::StoreLocal(target),
        ]);
        Ok(Iteration {
            condition,
            absent: vec![],
            body,
            step,
            target,
        })
    }

    /// `base[index]` for a named buffer place: the storage is read through a
    /// shared field reference under a loan the caller ends with the reads.
    pub(super) fn byte_item(
        &mut self,
        class: &Nominal<ClassType>,
        base: &Expr,
        index: &Expr,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Vec<usize>> {
        let loan = self.byte_storage_place(class, base, false, ops)?;
        let (got, mut reads) = self.observe_expected(index, Some(Ty::I64), ops)?;
        self.same(Some(got), Some(Ty::I64), &index.at)?;
        self.mark(at, ops);
        ops.push(Op::Collection(CollectionOp::Bytes(BytesOp::Item)));
        reads.push(loan);
        Ok(reads)
    }

    /// `base[index]` once a temporary `Bytes` value is on the stack.
    pub(super) fn byte_value_item(
        &mut self,
        class: &Nominal<ClassType>,
        index: &Expr,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Vec<usize>> {
        ops.push(Op::Class(ClassOp::Field(class.clone(), 0)));
        let (got, reads) = self.observe_expected(index, Some(Ty::I64), ops)?;
        self.same(Some(got), Some(Ty::I64), &index.at)?;
        self.mark(at, ops);
        ops.push(Op::Collection(CollectionOp::Bytes(BytesOp::ValueItem)));
        Ok(reads)
    }

    /// A reference to the storage of the named buffer `base`, under a loan
    /// of the whole buffer that the caller ends.
    fn byte_storage_place(
        &mut self,
        class: &Nominal<ClassType>,
        base: &Expr,
        mutable: bool,
        ops: &mut Vec<Op>,
    ) -> Result<usize> {
        let (reference, loan) = self.borrow(base, mutable, ops)?;
        self.unbox_reference(reference, ops);
        ops.push(Op::Class(ClassOp::FieldRef(class.clone(), 0, mutable)));
        Ok(loan)
    }

    /// `base[index] = value`, in the order assignment documents: the value,
    /// then the index, then the exclusive loan and the checked write.
    pub(super) fn set_byte(
        &mut self,
        class: &Nominal<ClassType>,
        base: &Expr,
        index: &Expr,
        value: &Expr,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let actual = self.expr_expected(value, Some(Ty::U8), ops)?;
        self.same(actual, Some(Ty::U8), &value.at)?;
        let value_slot = self.slot(Ty::U8, &value.at)?;
        ops.push(Op::StoreLocal(value_slot));
        let index_slot = self.byte_index(index, ops)?;
        self.write_byte(class, base, index_slot, value_slot, at, ops)
    }

    /// `base[index] op= value`: the index once, a read under a shared loan
    /// that lasts through the right side, then a checked write.
    pub(super) fn augment_byte(
        &mut self,
        class: &Nominal<ClassType>,
        (base, index): (&Expr, &Expr),
        op: &str,
        value: &Expr,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let index_slot = self.byte_index(index, ops)?;
        let read = self.byte_storage_place(class, base, false, ops)?;
        ops.push(Op::LoadLocal(index_slot));
        self.mark(at, ops);
        ops.push(Op::Collection(CollectionOp::Bytes(BytesOp::Item)));
        let actual = self.expr_expected(value, Some(Ty::U8), ops)?;
        self.same(actual, Some(Ty::U8), &value.at)?;
        self.mark_arithmetic(op, &Ty::U8, at, ops);
        Self::numeric_operation(op, Ty::U8, at, ops)?;
        let result = self.slot(Ty::U8, at)?;
        ops.push(Op::StoreLocal(result));
        ops.push(Op::UseLoan(read));
        self.write_byte(class, base, index_slot, result, at, ops)
    }

    fn byte_index(&mut self, index: &Expr, ops: &mut Vec<Op>) -> Result<u8> {
        let actual = self.expr_expected(index, Some(Ty::I64), ops)?;
        self.same(actual, Some(Ty::I64), &index.at)?;
        let slot = self.slot(Ty::I64, &index.at)?;
        ops.push(Op::StoreLocal(slot));
        Ok(slot)
    }

    fn write_byte(
        &mut self,
        class: &Nominal<ClassType>,
        base: &Expr,
        index: u8,
        value: u8,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let loan = self.byte_storage_place(class, base, true, ops)?;
        ops.extend([Op::LoadLocal(index), Op::LoadLocal(value)]);
        self.mark(at, ops);
        ops.extend([
            Op::Collection(CollectionOp::Bytes(BytesOp::SetItem)),
            Op::Drop,
            Op::UseLoan(loan),
        ]);
        Ok(())
    }
}
