//! Direct access to a checked primitive clist schema. `Get` and `Set` read
//! the owner's header at each access. `GetView` and `SetView` use a row count
//! and column address an enclosing loop read once, which the frontend allows
//! only for loops that cannot resize, replace or move the clist.
use super::*;
use crate::clist::{self, ClistOp};
use crate::clist_layout;
use cranelift_codegen::ir::{MemFlagsData, Value as NativeValue};

/// The row size under which [`super::induction::Checked`] records a clist's
/// checked row index: the recorded value is that index, normalized, not a row
/// address, and no list row has a negative size.
const CLIST_ROW: i64 = -1;

impl Lowerer<'_, '_> {
    /// `Len`, `Column`, `GetView` and `SetView`, inline: none calls the
    /// runtime.
    pub(super) fn lower_clist_inline(&mut self, op: &ClistOp) -> Result<()> {
        let (inputs, output) = op.signature();
        let mut args = Vec::with_capacity(inputs.len());
        for ty in inputs.iter().rev() {
            args.push(self.pop_typed(ty.clone())?.0);
        }
        args.reverse();
        let Ty::Class(row) = clist::row(op.owner()) else {
            unreachable!()
        };
        let fields = &row.get().fields;
        let prefix =
            |field: usize| -> usize { fields[..field].iter().map(|(_, t)| clist::width(t)).sum() };
        let result = match op {
            ClistOp::Len(_) | ClistOp::Column(..) => {
                // An empty clist has no allocation: its count and column
                // address are zero, as the runtime reports them.
                let owner = self.reference_word(args[0]);
                let present = self.bcx.create_block();
                let done = self.bcx.create_block();
                let zero = self.bcx.ins().iconst(types::I64, 0);
                let result = self.bcx.append_block_param(done, types::I64);
                self.bcx
                    .ins()
                    .brif(owner, present, &[], done, &[zero.into()]);
                self.bcx.switch_to_block(present);
                self.bcx.seal_block(present);
                let value = match op {
                    ClistOp::Len(_) => self.bcx.ins().load(
                        types::I64,
                        MemFlagsData::trusted(),
                        owner,
                        clist_layout::LEN,
                    ),
                    ClistOp::Column(_, field) => {
                        let capacity = self.bcx.ins().load(
                            types::I64,
                            MemFlagsData::trusted(),
                            owner,
                            clist_layout::CAPACITY,
                        );
                        self.clist_column_address(owner, capacity, prefix(*field))
                    }
                    _ => unreachable!(),
                };
                self.bcx.ins().jump(done, &[value.into()]);
                self.bcx.switch_to_block(done);
                self.bcx.seal_block(done);
                result
            }
            ClistOp::GetView(_, field) | ClistOp::SetView(_, field) => {
                let [column, length] = [args[args.len() - 1], args[args.len() - 2]];
                let index = self.checked_clist_row(length, args[1])?;
                let ty = &fields[*field].1;
                let offset = self.bcx.ins().imul_imm_s(index, clist::width(ty) as i64);
                let address = self.bcx.ins().iadd(column, offset);
                if let ClistOp::GetView(..) = op {
                    self.bcx
                        .ins()
                        .load(clif_type(ty.clone()), MemFlagsData::trusted(), address, 0)
                } else {
                    self.bcx
                        .ins()
                        .store(MemFlagsData::trusted(), args[2], address, 0);
                    self.bcx.ins().iconst(clif_type(Ty::Unit), 0)
                }
            }
            _ => unreachable!("inline clist operation"),
        };
        self.stack.push((result, output));
        Ok(())
    }

    /// Row `index` of a clist with `length` rows, normalized from the end
    /// when negative; an index out of range traps as `Get` does. The check
    /// is shared by every field of the row: one done in this block or a
    /// dominating one for the same length and index values is reused, as
    /// [`Self::checked_row`] reuses list checks. A row count is at most
    /// isize::MAX, so the normalization cannot overflow.
    fn checked_clist_row(
        &mut self,
        length: NativeValue,
        index: NativeValue,
    ) -> Result<NativeValue> {
        let dfg = &self.bcx.func.dfg;
        let known = super::known::known_signed(dfg, index);
        let length_key = dfg.resolve_aliases(length);
        let key = super::induction::RowKey {
            rows: length_key,
            length: length_key,
            index: known.ok_or_else(|| dfg.resolve_aliases(index)),
        };
        let block = self.bcx.current_block();
        if let Some(row) = self.counters.checked.find(block, key, CLIST_ROW) {
            return Ok(row);
        }
        let nonnegative =
            known.map_or_else(|| self.nonnegative_operand(index), |constant| constant >= 0);
        if (nonnegative
            && self
                .counters
                .bounds
                .below(&self.bcx.func.dfg, index, length))
            || self.versioned_index(index, length)
        {
            return Ok(index);
        }
        let failure = self.bcx.create_block();
        let valid = self.bcx.create_block();
        let row = if nonnegative {
            let in_range = self.bcx.ins().icmp(IntCC::UnsignedLessThan, index, length);
            self.bcx.ins().brif(in_range, valid, &[], failure, &[]);
            index
        } else {
            let joined = self.bcx.append_block_param(valid, types::I64);
            let negative = self.index_hot_test(index, length, valid, &[index.into()]);
            let (adjusted, in_range) = self.index_adjust(index, length, negative);
            self.bcx
                .ins()
                .brif(in_range, valid, &[adjusted.into()], failure, &[]);
            joined
        };
        self.bcx.switch_to_block(failure);
        self.bcx.set_cold_block(failure);
        self.bcx.seal_block(failure);
        let site = self.trap_site()?;
        let helper = self
            .module
            .declare_func_in_func(self.runtime.trap_clist_index, self.bcx.func);
        self.bcx.ins().call(helper, &[site]);
        self.bcx.ins().trap(TrapCode::unwrap_user(3));
        self.bcx.switch_to_block(valid);
        self.bcx.seal_block(valid);
        self.counters
            .checked
            .record(block, valid, key, CLIST_ROW, row);
        Ok(row)
    }

    fn clist_column_address(
        &mut self,
        owner: NativeValue,
        capacity: NativeValue,
        prefix: usize,
    ) -> NativeValue {
        // Reserve checks the total byte count and rounds capacity to eight;
        // every primitive column is naturally aligned, even after byte fields.
        let offset = self.bcx.ins().imul_imm_s(capacity, prefix as i64);
        let base = self.bcx.ins().iadd(owner, offset);
        self.bcx
            .ins()
            .iadd_imm_s(base, i64::from(clist_layout::DATA))
    }

    fn clist_field_address(
        &mut self,
        owner: NativeValue,
        capacity: NativeValue,
        prefix: usize,
        index: NativeValue,
        ty: &Ty,
    ) -> NativeValue {
        let base = self.clist_column_address(owner, capacity, prefix);
        let offset = self.bcx.ins().imul_imm_s(index, clist::width(ty) as i64);
        self.bcx.ins().iadd(base, offset)
    }

    pub(super) fn lower_clist_access(&mut self, op: &ClistOp) -> Result<()> {
        let (inputs, output) = op.signature();
        let mut args = Vec::with_capacity(inputs.len());
        for ty in inputs.iter().rev() {
            args.push(self.pop_typed(ty.clone())?.0);
        }
        args.reverse();
        let owner = self.reference_word(args[0]);
        let present = self.bcx.create_block();
        let missing = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.append_block_param(done, clif_type(output.clone()));
        self.bcx.ins().brif(owner, present, &[], missing, &[]);
        self.bcx.switch_to_block(present);
        self.bcx.seal_block(present);

        let Ty::Class(row) = clist::row(op.owner()) else {
            unreachable!()
        };
        let fields = &row.get().fields;
        let prefix = |field: usize| fields[..field].iter().map(|(_, t)| clist::width(t)).sum();
        let result = match op {
            ClistOp::Get(..) | ClistOp::Set(..) | ClistOp::CopyRow(_) => {
                let length = self.bcx.ins().load(
                    types::I64,
                    MemFlagsData::trusted(),
                    owner,
                    clist_layout::LEN,
                );
                let index = args[1];
                // Length <= isize::MAX; adding it to a negative index cannot
                // overflow. One unsigned comparison also rejects i64::MIN.
                let negative = self.bcx.ins().icmp_imm_s(IntCC::SignedLessThan, index, 0);
                let adjusted = self.bcx.ins().iadd(index, length);
                let index = self.bcx.ins().select(negative, adjusted, index);
                let valid = self.bcx.ins().icmp(IntCC::UnsignedLessThan, index, length);
                let access = self.bcx.create_block();
                self.bcx.ins().brif(valid, access, &[], missing, &[]);
                self.bcx.switch_to_block(access);
                self.bcx.seal_block(access);
                let capacity = self.bcx.ins().load(
                    types::I64,
                    MemFlagsData::trusted(),
                    owner,
                    clist_layout::CAPACITY,
                );
                match op {
                    ClistOp::Get(_, field) | ClistOp::Set(_, field) => {
                        let ty = &fields[*field].1;
                        let address =
                            self.clist_field_address(owner, capacity, prefix(*field), index, ty);
                        if matches!(op, ClistOp::Get(..)) {
                            self.bcx.ins().load(
                                clif_type(ty.clone()),
                                MemFlagsData::trusted(),
                                address,
                                0,
                            )
                        } else {
                            self.bcx
                                .ins()
                                .store(MemFlagsData::trusted(), args[2], address, 0);
                            self.bcx.ins().iconst(clif_type(Ty::Unit), 0)
                        }
                    }
                    ClistOp::CopyRow(_) if row.get().newtype_field().is_some() => {
                        // A one-field row is its scalar: `Some(row)` is that
                        // scalar's bits under the option's tag.
                        let ty = &fields[0].1;
                        let address = self.clist_field_address(owner, capacity, 0, index, ty);
                        let value = self.bcx.ins().load(
                            clif_type(ty.clone()),
                            MemFlagsData::trusted(),
                            address,
                            0,
                        );
                        let packed = self.pack(value, ty);
                        let word = self.raw_word(packed);
                        let some = self.bcx.ins().iconst(types::I64, 1);
                        self.bcx.ins().iconcat(word, some)
                    }
                    ClistOp::CopyRow(_) => {
                        // Fresh destination is already a snapshot. Store tagged
                        // row slots once, preserving the primitive's exact bits.
                        let storage = self.inline_storage(output.inline_bytes());
                        let mut prefix = 0;
                        for (field, (_, ty)) in fields.iter().enumerate() {
                            let address =
                                self.clist_field_address(owner, capacity, prefix, index, ty);
                            let value = self.bcx.ins().load(
                                clif_type(ty.clone()),
                                MemFlagsData::trusted(),
                                address,
                                0,
                            );
                            let packed = self.pack(value, ty);
                            self.bcx.ins().store(
                                MemFlagsData::trusted(),
                                packed,
                                storage,
                                (field * 16) as i32,
                            );
                            prefix += clist::width(ty);
                        }
                        let some = self.bcx.ins().iconst(types::I64, 1);
                        self.bcx.ins().iconcat(storage, some)
                    }
                    _ => unreachable!(),
                }
            }
            _ => unreachable!("direct clist access"),
        };
        self.bcx.ins().jump(done, &[result.into()]);
        self.bcx.switch_to_block(missing);
        self.bcx.seal_block(missing);
        if op.traps() {
            // An invalid index needs only the source site, not a descriptor
            // and argument block for the generic collection dispatcher.
            self.bcx.set_cold_block(missing);
            let site = self.trap_site()?;
            let helper = self
                .module
                .declare_func_in_func(self.runtime.trap_clist_index, self.bcx.func);
            self.bcx.ins().call(helper, &[site]);
            self.bcx.ins().trap(TrapCode::unwrap_user(3));
        } else {
            // A missing row returns Nothing without touching any row storage.
            // Only `copy_row` reaches here; its `Option` is an I128 slot, and
            // `iconst` accepts at most I64.
            debug_assert_eq!(clif_type(output.clone()), types::I128);
            let zero = self.bcx.ins().iconst(types::I64, 0);
            let zero = self.bcx.ins().uextend(types::I128, zero);
            self.bcx.ins().jump(done, &[zero.into()]);
        }
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
        let result = self.bcx.block_params(done)[0];
        self.stack.push((result, output));
        Ok(())
    }
}
