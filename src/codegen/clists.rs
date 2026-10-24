//! Direct access to a checked primitive clist schema. No metadata is cached
//! across source operations: mutation/growth observes the current owner.
use super::*;
use crate::clist::{self, ClistOp};
use crate::clist_layout;
use cranelift_codegen::ir::{MemFlagsData, Value as NativeValue};

impl Lowerer<'_, '_> {
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
