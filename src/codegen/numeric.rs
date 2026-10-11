//! Numeric built-ins. Integer results that the type cannot hold trap as
//! arithmetic overflow does; float operations follow IEEE 754 and never trap.
//! See `book/src/design/facilities/01-builtins/numbers.md`.
use super::*;
use crate::numeric::{FloatOp, NumericOp};
use cranelift_codegen::ir::Value as NativeValue;

impl Lowerer<'_, '_> {
    pub(super) fn lower_numeric(&mut self, operation: &NumericOp) -> Result<()> {
        match operation {
            NumericOp::ListSum { element, borrowed } => self.lower_list_sum(element, *borrowed),
            NumericOp::Abs => {
                let (value, ty) = self.stack.pop().ok_or("AOT: stack underflow on abs")?;
                let result = if ty.is_float() {
                    self.bcx.ins().fabs(value)
                } else if is_signed(ty.clone()) {
                    let (min, _) = ty.int_range().expect("integer type");
                    let is_min = self.bcx.ins().icmp_imm(IntCC::Equal, value, min as i64);
                    self.trap_if(is_min, TrapKind::Overflow)?;
                    let negative = self.bcx.ins().icmp_imm(IntCC::SignedLessThan, value, 0);
                    let negated = self.bcx.ins().ineg(value);
                    self.bcx.ins().select(negative, negated, value)
                } else {
                    value
                };
                self.stack.push((result, ty));
                Ok(())
            }
            NumericOp::Min | NumericOp::Max => {
                let (a, b, ty) = self.pop_pair()?;
                let min = *operation == NumericOp::Min;
                let result = if ty.is_float() {
                    // Both propagate NaN and order -0.0 below 0.0.
                    if min {
                        self.bcx.ins().fmin(a, b)
                    } else {
                        self.bcx.ins().fmax(a, b)
                    }
                } else {
                    let condition = match (min, is_signed(ty.clone())) {
                        (true, true) => IntCC::SignedLessThan,
                        (true, false) => IntCC::UnsignedLessThan,
                        (false, true) => IntCC::SignedGreaterThan,
                        (false, false) => IntCC::UnsignedGreaterThan,
                    };
                    let first = self.bcx.ins().icmp(condition, a, b);
                    self.bcx.ins().select(first, a, b)
                };
                self.stack.push((result, ty));
                Ok(())
            }
            NumericOp::Sqrt => {
                let (value, ty) = self.stack.pop().ok_or("AOT: stack underflow on sqrt")?;
                let result = self.bcx.ins().sqrt(value);
                self.stack.push((result, ty));
                Ok(())
            }
            NumericOp::Float(kind) => {
                let (x, y, ty) = if matches!(kind, FloatOp::Pow | FloatOp::Fmod) {
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

    /// The loop `total = 0; for x in xs: total = total + x`, reading each
    /// element through the same helper as indexing.
    fn lower_list_sum(&mut self, element: &Ty, borrowed: bool) -> Result<()> {
        let (receiver, list) = self.stack.pop().ok_or("AOT: stack underflow on sum")?;
        let owner = if borrowed {
            let packed = self.read_reference(receiver);
            self.raw_word(packed)
        } else {
            receiver
        };
        let native = clif_type(element.clone());
        let length = self
            .module
            .declare_func_in_func(self.runtime.list_len, self.bcx.func);
        let get = self
            .module
            .declare_func_in_func(self.runtime.list_scalar_get, self.bcx.func);
        let call = self.bcx.ins().call(length, &[owner]);
        let length = self.bcx.inst_results(call)[0];

        let header = self.bcx.create_block();
        let body = self.bcx.create_block();
        let exit = self.bcx.create_block();
        self.bcx.append_block_param(header, types::I64);
        self.bcx.append_block_param(header, native);
        let index = self.bcx.ins().iconst(types::I64, 0);
        let total = if *element == Ty::F32 {
            self.bcx.ins().f32const(0.0)
        } else if *element == Ty::F64 {
            self.bcx.ins().f64const(0.0)
        } else {
            self.bcx.ins().iconst(native, 0)
        };
        self.bcx
            .ins()
            .jump(header, &[BlockArg::Value(index), BlockArg::Value(total)]);

        self.bcx.switch_to_block(header);
        let index = self.bcx.block_params(header)[0];
        let total = self.bcx.block_params(header)[1];
        let more = self.bcx.ins().icmp(IntCC::UnsignedLessThan, index, length);
        self.bcx.ins().brif(more, body, &[], exit, &[]);

        self.bcx.switch_to_block(body);
        self.bcx.seal_block(body);
        // The index is in bounds, so the access itself needs no site.
        let no_site = self.bcx.ins().iconst(PTR_TY, 0);
        let call = self.bcx.ins().call(get, &[owner, index, no_site]);
        let bits = self.bcx.inst_results(call)[0];
        let value = self.unpack(bits, element);
        let next = if element.is_float() {
            self.bcx.ins().fadd(total, value)
        } else {
            let (next, overflow) = if is_signed(element.clone()) {
                self.bcx.ins().sadd_overflow(total, value)
            } else {
                self.bcx.ins().uadd_overflow(total, value)
            };
            self.trap_if(overflow, TrapKind::Overflow)?;
            next
        };
        let index = self.bcx.ins().iadd_imm(index, 1);
        self.bcx
            .ins()
            .jump(header, &[BlockArg::Value(index), BlockArg::Value(next)]);
        self.bcx.seal_block(header);

        self.bcx.switch_to_block(exit);
        self.bcx.seal_block(exit);
        if !borrowed {
            self.release(receiver, &list);
        }
        self.stack.push((total, element.clone()));
        Ok(())
    }
}
