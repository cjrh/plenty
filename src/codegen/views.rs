//! Slice views `&xs[a:b]` and `&mut xs[a:b]`: the address of the first
//! selected row and a row count, packed as the view's two words. Rows are
//! addressed with the element's static row size, which the runtime derives
//! from the same descriptor (`list_layout::row_bytes`). The frontend holds the
//! owner's loan for the view's whole life, so its rows never move.
//! See book/src/design/language/04-expressions/slices.md.
use super::*;
use crate::collection::CollectionOp;
use crate::collection_opcodes::{SLICE_START, SLICE_STOP};
use cranelift_codegen::ir::Value as NativeValue;

impl Lowerer<'_, '_> {
    /// Lowers a slice-view operation; false for any other operation.
    pub(super) fn lower_view(&mut self, operation: &CollectionOp) -> Result<bool> {
        match operation {
            CollectionOp::ViewNew(source, mutable) => self.view_new(source, *mutable)?,
            CollectionOp::ViewLen(view) => {
                let (view, _) = self.pop_typed(view.clone())?;
                let (_, count) = self.bcx.ins().isplit(view);
                self.stack.push((count, Ty::I64));
            }
            CollectionOp::ViewRow(view, mutable) | CollectionOp::ViewItem(view, mutable) => {
                let element = view_element(view);
                let (index, _) = self.pop_typed(Ty::I64)?;
                let (value, _) = self.pop_typed(view.clone())?;
                let (rows, count) = self.bcx.ins().isplit(value);
                // Iteration stays below the count it read, so only indexing
                // checks.
                let index = match operation {
                    CollectionOp::ViewRow(..) => self.list_index(index, count)?,
                    _ => index,
                };
                let address = self.view_row(rows, index, &element);
                // A row is a top-level slot: no inline-sum tag is projected.
                let reference = self.bcx.ins().uextend(types::I128, address);
                self.stack
                    .push((reference, Ty::Ref(Arc::new(element), *mutable)));
            }
            CollectionOp::ViewEqual(view) => {
                let element = view_element(view);
                let (b, _) = self.pop_typed(view.clone())?;
                let (a, _) = self.pop_typed(view.clone())?;
                let (a_rows, a_count) = self.bcx.ins().isplit(a);
                let (b_rows, b_count) = self.bcx.ins().isplit(b);
                let id = self.module.metadata(&element);
                let gv = self.module.declare_data_in_func(id, self.bcx.func);
                let descriptor = self.bcx.ins().symbol_value(PTR_TY, gv);
                let helper = self
                    .module
                    .declare_func_in_func(self.runtime.view_equal, self.bcx.func);
                let call = self
                    .bcx
                    .ins()
                    .call(helper, &[a_rows, a_count, b_rows, b_count, descriptor]);
                let same = self.bcx.inst_results(call)[0];
                let same = self.bcx.ins().icmp_imm_u(IntCC::NotEqual, same, 0);
                self.stack.push((same, Ty::Bool));
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Pops the owner and the `start`, `stop` and flags operands and pushes the
    /// view of the clamped, contiguous selection. Clamping follows Python's
    /// slices with step 1, so the view is empty, never out of bounds.
    fn view_new(&mut self, source: &Ty, mutable: bool) -> Result<()> {
        let (flags, _) = self.pop_typed(Ty::I64)?;
        let (stop, _) = self.pop_typed(Ty::I64)?;
        let (start, _) = self.pop_typed(Ty::I64)?;
        let (inputs, output) = CollectionOp::ViewNew(source.clone(), mutable).signature();
        let element = view_element(&output);
        let (owner, _) = self.pop_typed(inputs[0].clone())?;
        let (rows, length) = match source {
            Ty::View(..) => self.bcx.ins().isplit(owner),
            Ty::List(_) => {
                let list = self.reference_word(owner);
                let flags = cranelift_codegen::ir::MemFlagsData::trusted();
                let rows = self
                    .bcx
                    .ins()
                    .load(PTR_TY, flags, list, crate::list_layout::ROWS);
                let length = if element.is_numeric() || element == Ty::Bool {
                    self.scalar_list_len(list, &element)
                } else {
                    let helper = self
                        .module
                        .declare_func_in_func(self.runtime.list_len, self.bcx.func);
                    let call = self.bcx.ins().call(helper, &[list]);
                    self.bcx.inst_results(call)[0]
                };
                (rows, length)
            }
            _ => {
                let bytes = self.reference_word(owner);
                let functions = [self.runtime.byte_view_rows, self.runtime.byte_view_len];
                let [rows, length] = functions.map(|function| {
                    let helper = self.module.declare_func_in_func(function, self.bcx.func);
                    let call = self.bcx.ins().call(helper, &[bytes]);
                    self.bcx.inst_results(call)[0]
                });
                (rows, length)
            }
        };
        let zero = self.bcx.ins().iconst(types::I64, 0);
        let first = self.view_bound(start, flags, SLICE_START, length, zero);
        let end = self.view_bound(stop, flags, SLICE_STOP, length, length);
        let count = self.bcx.ins().isub(end, first);
        let count = self.bcx.ins().smax(count, zero);
        let address = self.view_row(rows, first, &element);
        let view = self.bcx.ins().iconcat(address, count);
        self.stack.push((view, output));
        Ok(())
    }

    /// A bound clamped into `0..=length`, counting a negative bound from the
    /// end, or `omitted` when `flag` is clear. `bound + length` cannot
    /// overflow: the bound is negative and the length at most `isize::MAX`.
    fn view_bound(
        &mut self,
        bound: NativeValue,
        flags: NativeValue,
        flag: i64,
        length: NativeValue,
        omitted: NativeValue,
    ) -> NativeValue {
        let zero = self.bcx.ins().iconst(types::I64, 0);
        let from_end = self.bcx.ins().iadd(bound, length);
        let from_end = self.bcx.ins().smax(from_end, zero);
        let from_start = self.bcx.ins().smin(bound, length);
        let negative = self.bcx.ins().icmp_imm_s(IntCC::SignedLessThan, bound, 0);
        let clamped = self.bcx.ins().select(negative, from_end, from_start);
        let written = self.bcx.ins().band_imm_s(flags, flag);
        self.bcx.ins().select(written, clamped, omitted)
    }

    fn view_row(&mut self, rows: NativeValue, index: NativeValue, element: &Ty) -> NativeValue {
        let row_bytes = metadata::list_row_bytes(element) as i64;
        let offset = self.bcx.ins().imul_imm_s(index, row_bytes);
        self.bcx.ins().iadd(rows, offset)
    }
}

fn view_element(view: &Ty) -> Ty {
    let Ty::View(element, _) = view else {
        unreachable!("a slice view operand")
    };
    (**element).clone()
}
