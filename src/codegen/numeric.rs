//! Numeric built-ins. Integer results that the type cannot hold trap as
//! arithmetic overflow does; float operations follow IEEE 754 and never trap.
//! See `book/src/design/facilities/01-builtins/numbers.md`.
use super::*;
use crate::numeric::{FloatOp, NumericOp};
use cranelift_codegen::ir::Value as NativeValue;

impl Lowerer<'_, '_> {
    pub(super) fn lower_numeric(&mut self, operation: &NumericOp) -> Result<()> {
        match operation {
            NumericOp::Sqrt => {
                let (value, ty) = self.stack.pop().ok_or("AOT: stack underflow on sqrt")?;
                let result = self.bcx.ins().sqrt(value);
                self.stack.push((result, ty));
                Ok(())
            }
            NumericOp::Float(kind) => {
                let (x, y, ty) = if kind.is_binary() {
                    self.pop_pair()?
                } else {
                    let (x, ty) = self.stack.pop().ok_or("AOT: stack underflow on float")?;
                    (x, x, ty)
                };
                let result = self.float_math(*kind, x, y, &ty)?;
                self.stack.push((result, ty));
                Ok(())
            }
            NumericOp::Pow => {
                let (base, exponent, ty) = self.pop_pair()?;
                let result = if ty.is_float() {
                    self.float_math(FloatOp::Pow, base, exponent, &ty)?
                } else {
                    self.int_pow(base, exponent, &ty)?
                };
                self.stack.push((result, ty));
                Ok(())
            }
        }
    }

    /// `plenty_f64_math(kind, x, y) -> f64`, or the `f32` twin. Unary
    /// functions ignore `y`.
    fn float_math(
        &mut self,
        kind: FloatOp,
        x: NativeValue,
        y: NativeValue,
        ty: &Ty,
    ) -> Result<NativeValue> {
        let float = clif_type(ty.clone());
        let name = if *ty == Ty::F32 {
            "plenty_f32_math"
        } else {
            "plenty_f64_math"
        };
        let mut signature = self.module.make_signature();
        signature.call_conv = CallConv::SystemV;
        signature.params.extend([
            AbiParam::new(types::I64),
            AbiParam::new(float),
            AbiParam::new(float),
        ]);
        signature.returns.push(AbiParam::new(float));
        let helper = self
            .module
            .declare_function(name, Linkage::Import, &signature)?;
        let helper = self.module.declare_func_in_func(helper, self.bcx.func);
        let kind = self.bcx.ins().iconst(types::I64, kind as i64);
        let call = self.bcx.ins().call(helper, &[kind, x, y]);
        Ok(self.bcx.inst_results(call)[0])
    }

    /// `plenty_int_pow(base, exponent, kind, site) -> i64` receives both
    /// operands extended to 64 bits and traps instead of returning a value
    /// outside the type.
    fn int_pow(
        &mut self,
        base: NativeValue,
        exponent: NativeValue,
        ty: &Ty,
    ) -> Result<NativeValue> {
        let wide = if is_signed(ty.clone()) {
            Ty::I64
        } else {
            Ty::U64
        };
        let base = self.cast(base, ty.clone(), wide.clone());
        let exponent = self.cast(exponent, ty.clone(), wide.clone());
        let mut signature = self.module.make_signature();
        signature.call_conv = CallConv::SystemV;
        signature.params.extend([
            AbiParam::new(types::I64),
            AbiParam::new(types::I64),
            AbiParam::new(types::I64),
            AbiParam::new(PTR_TY),
        ]);
        signature.returns.push(AbiParam::new(types::I64));
        let helper = self
            .module
            .declare_function("plenty_int_pow", Linkage::Import, &signature)?;
        let helper = self.module.declare_func_in_func(helper, self.bcx.func);
        let kind = self
            .bcx
            .ins()
            .iconst(types::I64, crate::numeric::int_kind(ty));
        let site = self.trap_site()?;
        let call = self.bcx.ins().call(helper, &[base, exponent, kind, site]);
        let result = self.bcx.inst_results(call)[0];
        Ok(self.cast(result, wide, ty.clone()))
    }
}
