use super::*;
use crate::sum::EnumOp;

impl Lowerer<'_, '_> {
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
            EnumOp::Field(_, tag, field) => {
                let tag = self.bcx.ins().iconst(types::I64, *tag as i64);
                let field = self.bcx.ins().iconst(types::I64, *field as i64);
                self.collection_call(23, &[values[0], field, tag], None)?
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
        if clif_type(ty.clone()) == types::I64 {
            value
        } else if is_signed(ty.clone()) {
            self.bcx.ins().sextend(types::I64, value)
        } else {
            self.bcx.ins().uextend(types::I64, value)
        }
    }
    pub(super) fn unpack(
        &mut self,
        value: cranelift_codegen::ir::Value,
        ty: &Ty,
    ) -> cranelift_codegen::ir::Value {
        let target = clif_type(ty.clone());
        if target == types::I64 {
            value
        } else {
            self.bcx.ins().ireduce(target, value)
        }
    }
}
