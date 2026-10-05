use super::*;
use crate::sum::EnumOp;

/// All runtime slots carry 64 raw bits, including float fields and frame locals.
pub(super) fn pack_value(
    b: &mut FunctionBuilder<'_>,
    value: cranelift_codegen::ir::Value,
    ty: &Ty,
) -> cranelift_codegen::ir::Value {
    if ty.is_float() {
        let bits = b.ins().bitcast(
            if *ty == Ty::F32 {
                types::I32
            } else {
                types::I64
            },
            cranelift_codegen::ir::MemFlags::new(),
            value,
        );
        if *ty == Ty::F32 {
            b.ins().uextend(types::I64, bits)
        } else {
            bits
        }
    } else if clif_type(ty.clone()) == types::I64 {
        value
    } else if is_signed(ty.clone()) {
        b.ins().sextend(types::I64, value)
    } else {
        b.ins().uextend(types::I64, value)
    }
}

impl Lowerer<'_, '_> {
    pub(super) fn lower_class(&mut self, op: &crate::record::ClassOp) -> Result<()> {
        use crate::record::ClassOp;
        let (inputs, output) = op.signature().ok_or("invalid class operation")?;
        let result = match op {
            ClassOp::New(t) => {
                let callback = if let Some(name) = &t.destructor {
                    let id = self.user_fns[name]
                        .drop_callback
                        .ok_or("missing destructor adapter")?;
                    let reference = self.module.declare_func_in_func(id, self.bcx.func);
                    self.bcx.ins().func_addr(PTR_TY, reference)
                } else {
                    self.bcx.ins().iconst(PTR_TY, 0)
                };
                self.collection_call(30, &[callback], Some(&output))?
            }
            ClassOp::Field(_, i) => {
                let (owner, _) = self.pop_typed(inputs[0].clone())?;
                let index = self.bcx.ins().iconst(types::I64, *i as i64);
                let value = self.collection_call(31, &[owner, index], None)?;
                self.release(owner, &inputs[0]);
                self.unpack(value, &output)
            }
            ClassOp::FieldRef(_, i, _) => {
                let (address, _) = self.pop_typed(inputs[0].clone())?;
                let owner = self.bcx.ins().load(
                    PTR_TY,
                    cranelift_codegen::ir::MemFlags::trusted(),
                    address,
                    0,
                );
                self.bcx.ins().iadd_imm(owner, 32 + (*i as i64) * 8)
            }
        };
        self.stack.push((result, output));
        Ok(())
    }
    pub(super) fn lower_enum(&mut self, op: &EnumOp) -> Result<()> {
        let (inputs, output) = op.signature().ok_or("invalid enum operation")?;
        let mut values = Vec::new();
        for ty in inputs.iter().rev() {
            let (value, _) = self.pop_typed(ty.clone())?;
            values.push(self.pack(value, ty));
        }
        values.reverse();
        let result = match op {
            EnumOp::New(t, tag) => {
                let tag = self.bcx.ins().iconst(types::I64, *tag as i64);
                let result = self.collection_call(20, &[tag], Some(&Ty::Enum(t.clone())))?;
                for (i, value) in values.iter().enumerate() {
                    let field = self.bcx.ins().iconst(types::I64, i as i64);
                    self.collection_call(21, &[result, field, *value], None)?;
                }
                result
            }
            EnumOp::Tag(_) => self.collection_call(22, &values, None)?,
            EnumOp::Field(_, tag, field) | EnumOp::Take(_, tag, field) => {
                let tag = self.bcx.ins().iconst(types::I64, *tag as i64);
                let field = self.bcx.ins().iconst(types::I64, *field as i64);
                self.collection_call(
                    if matches!(op, EnumOp::Take(..)) {
                        25
                    } else {
                        23
                    },
                    &[values[0], field, tag],
                    None,
                )?
            }
        };
        for (value, ty) in values.iter().zip(&inputs) {
            self.release(*value, ty);
        }
        let result = self.unpack(result, &output);
        self.stack.push((result, output));
        Ok(())
    }

    pub(super) fn pack(
        &mut self,
        value: cranelift_codegen::ir::Value,
        ty: &Ty,
    ) -> cranelift_codegen::ir::Value {
        pack_value(self.bcx, value, ty)
    }
    pub(super) fn unpack(
        &mut self,
        value: cranelift_codegen::ir::Value,
        ty: &Ty,
    ) -> cranelift_codegen::ir::Value {
        let target = clif_type(ty.clone());
        if ty.is_float() {
            let bits = if *ty == Ty::F32 {
                self.bcx.ins().ireduce(types::I32, value)
            } else {
                value
            };
            return self
                .bcx
                .ins()
                .bitcast(target, cranelift_codegen::ir::MemFlags::new(), bits);
        }
        if target == types::I64 {
            value
        } else {
            self.bcx.ins().ireduce(target, value)
        }
    }
}
