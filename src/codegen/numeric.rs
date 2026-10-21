//! Numeric built-ins. Integer results that the type cannot hold trap as
//! arithmetic overflow does; float operations follow IEEE 754 and never trap.
//! See `book/src/design/facilities/01-builtins/numbers.md`.
use super::*;
use crate::numeric::{FloatOp, NumericOp};
use cranelift_codegen::ir::Value as NativeValue;
use cranelift_codegen::ir::immediates::{Ieee32, Ieee64};
use cranelift_codegen::ir::{ConstantData, InstructionData, MemFlags, Opcode, ValueDef};

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
                let result = match kind {
                    FloatOp::Abs => self.float_abs(x),
                    FloatOp::Copysign => self.float_copysign(x, y),
                    FloatOp::Floor | FloatOp::Ceil | FloatOp::Trunc | FloatOp::Round => {
                        self.float_round(*kind, x)
                    }
                    _ => self.float_math(*kind, x, y, &ty)?,
                };
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

    /// A float constant used as the left operand of `fsub` or `fdiv`, read
    /// from the constant pool. Cranelift rematerializes an `f64const` in each
    /// block that uses it, and the x86-64 backend builds a left operand with
    /// `movabs` + `movq`; a right operand is a constant-pool memory operand
    /// already. A `vconst` lowers to one constant-pool load and uses no
    /// general-purpose register. Lane 0 of the vector holds the same
    /// bits, so the operation is unchanged (issue #380). Zero stays an
    /// `f64const`, which lowers to a register-clearing `xorpd`, and so does
    /// a constant whose `right` operand is also constant, which Cranelift
    /// folds.
    pub(super) fn float_left_operand(
        &mut self,
        value: NativeValue,
        right: NativeValue,
    ) -> NativeValue {
        let dfg = &self.bcx.func.dfg;
        let Some(bits) = known::known_float_bits(dfg, value).filter(|bits| *bits != 0) else {
            return value;
        };
        if known::known_float_bits(dfg, right).is_some() {
            return value;
        }
        let (vector, width) = if dfg.value_type(value) == types::F32 {
            (types::F32X4, 4)
        } else {
            (types::F64X2, 8)
        };
        let mut bytes = [0u8; 16];
        bytes[..width].copy_from_slice(&bits.to_le_bytes()[..width]);
        let constant = self
            .bcx
            .func
            .dfg
            .constants
            .insert(ConstantData::from(&bytes[..]));
        let vector = self.bcx.ins().vconst(vector, constant);
        self.bcx.ins().extractlane(vector, 0)
    }

    /// The sign bit and the remaining magnitude bits of a float type.
    fn float_masks(&self, value: NativeValue) -> (u64, u64) {
        if self.bcx.func.dfg.value_type(value) == types::F32 {
            (1 << 31, (1 << 31) - 1)
        } else {
            (1 << 63, (1 << 63) - 1)
        }
    }

    fn float_bits(&mut self, value: NativeValue, bits: u64) -> NativeValue {
        if self.bcx.func.dfg.value_type(value) == types::F32 {
            self.bcx.ins().f32const(Ieee32::with_bits(bits as u32))
        } else {
            self.bcx.ins().f64const(Ieee64::with_bits(bits))
        }
    }

    /// `x.fabs()` clears the sign bit, as Rust's `abs` does, NaN payload
    /// included. The mask is the right operand of `band`, so x86-64 reads it
    /// from the constant pool; Cranelift's `fabs` builds it with `movabs`.
    fn float_abs(&mut self, x: NativeValue) -> NativeValue {
        let (_, magnitude) = self.float_masks(x);
        if let Some(bits) = known::known_float_bits(&self.bcx.func.dfg, x) {
            return self.float_bits(x, bits & magnitude);
        }
        let mask = self.float_bits(x, magnitude);
        self.bcx.ins().band(x, mask)
    }

    /// `-x` flips the sign bit, NaN payload included, as Cranelift's `fneg`
    /// does. The mask is the right operand of `bxor`, so x86-64 reads it from
    /// the constant pool; `fneg` builds it with `movabs` + `movq` at each use
    /// (issue #385). A constant folds here, and so does `-(-x)`, which
    /// Cranelift's `fneg(fneg x)` rule no longer sees.
    pub(super) fn float_neg(&mut self, x: NativeValue) -> NativeValue {
        let (sign, _) = self.float_masks(x);
        let dfg = &self.bcx.func.dfg;
        if let Some(bits) = known::known_float_bits(dfg, x) {
            return self.float_bits(x, bits ^ sign);
        }
        if let ValueDef::Result(inst, _) = dfg.value_def(dfg.resolve_aliases(x))
            && let InstructionData::Binary {
                opcode: Opcode::Bxor,
                args: [inner, mask],
            } = dfg.insts[inst]
            && known::known_float_bits(dfg, mask) == Some(sign)
        {
            return inner;
        }
        let mask = self.float_bits(x, sign);
        self.bcx.ins().bxor(x, mask)
    }

    /// `x.copysign(y)`: the magnitude bits of `x` with the sign bit of `y`,
    /// as Rust's `copysign`, NaN payload included. Each mask or constant is
    /// a right operand, read from the constant pool on x86-64; Cranelift's
    /// `fcopysign` builds its mask and a constant `x` in registers.
    fn float_copysign(&mut self, x: NativeValue, y: NativeValue) -> NativeValue {
        let (sign, magnitude) = self.float_masks(x);
        let dfg = &self.bcx.func.dfg;
        let known_x = known::known_float_bits(dfg, x);
        let known_y = known::known_float_bits(dfg, y);
        match (known_x, known_y) {
            (Some(x_bits), Some(y_bits)) => {
                self.float_bits(x, (x_bits & magnitude) | (y_bits & sign))
            }
            (Some(x_bits), None) => {
                let mask = self.float_bits(y, sign);
                let sign_of_y = self.bcx.ins().band(y, mask);
                if x_bits & magnitude == 0 {
                    return sign_of_y;
                }
                let magnitude_of_x = self.float_bits(x, x_bits & magnitude);
                self.bcx.ins().bor(sign_of_y, magnitude_of_x)
            }
            (None, Some(y_bits)) => {
                let mask = self.float_bits(x, magnitude);
                let magnitude_of_x = self.bcx.ins().band(x, mask);
                if y_bits & sign == 0 {
                    magnitude_of_x
                } else {
                    let sign = self.float_bits(x, sign);
                    self.bcx.ins().bor(magnitude_of_x, sign)
                }
            }
            (None, None) => {
                let mask = self.float_bits(x, magnitude);
                let magnitude_of_x = self.bcx.ins().band(x, mask);
                let mask = self.float_bits(y, sign);
                let sign_of_y = self.bcx.ins().band(y, mask);
                self.bcx.ins().bor(magnitude_of_x, sign_of_y)
            }
        }
    }

    /// `floor`, `ceil`, `trunc` and `round` (ties to even) with SSE2
    /// arithmetic, bit for bit as Rust's `floor`, `ceil`, `trunc` and
    /// `round_ties_even` (issue #386). Generated code targets the SSE2
    /// baseline, where Cranelift's rounding instructions are libcalls, not
    /// SSE4.1 `roundsd`.
    ///
    /// For a magnitude `a` below 2^52 (2^23 for `f32`), `(a + 2^52) - 2^52`
    /// is `a` rounded to a whole number, ties to even: the sum lies in
    /// `[2^52, 2^53)`, whose spacing is 1. A result one past `x` in the
    /// wrong direction steps back by one. The sign of `x` is ORed into the
    /// result, which gives `-0.0` where the result is zero and `x` negative,
    /// as the IEEE functions do; a nonzero result already has the sign of
    /// `x`. A larger magnitude or an infinity is already whole, so `x` is
    /// returned. A NaN goes through the arithmetic, which quiets it and
    /// keeps its sign and payload, as the library functions do.
    fn float_round(&mut self, kind: FloatOp, x: NativeValue) -> NativeValue {
        let f32 = self.bcx.func.dfg.value_type(x) == types::F32;
        if let Some(bits) = known::known_float_bits(&self.bcx.func.dfg, x) {
            let bits = if f32 {
                let x = f32::from_bits(bits as u32);
                let whole = match kind {
                    FloatOp::Floor => x.floor(),
                    FloatOp::Ceil => x.ceil(),
                    FloatOp::Trunc => x.trunc(),
                    _ => x.round_ties_even(),
                };
                u64::from(whole.to_bits())
            } else {
                let x = f64::from_bits(bits);
                let whole = match kind {
                    FloatOp::Floor => x.floor(),
                    FloatOp::Ceil => x.ceil(),
                    FloatOp::Trunc => x.trunc(),
                    _ => x.round_ties_even(),
                };
                whole.to_bits()
            };
            return self.float_bits(x, bits);
        }
        let (sign_mask, magnitude_mask) = self.float_masks(x);
        let (big, one) = if f32 {
            (u64::from(8_388_608f32.to_bits()), u64::from(1f32.to_bits()))
        } else {
            (4_503_599_627_370_496f64.to_bits(), 1f64.to_bits())
        };
        let mask = self.float_bits(x, magnitude_mask);
        let a = self.bcx.ins().band(x, mask);
        let mask = self.float_bits(x, sign_mask);
        let sign = self.bcx.ins().band(x, mask);
        let shift = self.float_bits(x, big);
        let shifted = self.bcx.ins().fadd(a, shift);
        let nearest = self.bcx.ins().fsub(shifted, shift);
        let whole = match kind {
            FloatOp::Round => nearest,
            // Rounding the magnitude to nearest overshoots by one at most.
            FloatOp::Trunc => self.float_step(nearest, a, FloatCC::GreaterThan, one),
            _ => {
                let signed = self.bcx.ins().bor(nearest, sign);
                if kind == FloatOp::Floor {
                    self.float_step(signed, x, FloatCC::GreaterThan, one)
                } else {
                    self.float_step(signed, x, FloatCC::LessThan, one)
                }
            }
        };
        let result = self.bcx.ins().bor(whole, sign);
        // `a >= 2^52` keeps the constant a right operand, which x86-64 reads
        // from the constant pool; `a < 2^52` swaps it into a register.
        let large = self.bcx.ins().fcmp(FloatCC::GreaterThanOrEqual, a, shift);
        self.bcx.ins().select(large, x, result)
    }

    /// `value` moved one toward `target` when `value cc target` holds:
    /// down for `GreaterThan`, up for `LessThan`. The compare runs on
    /// vectors so that its result is a lane mask (`cmppd`) selecting the
    /// constant 1, without a branch. A scalar `select` lowers to a
    /// conditional jump, which mispredicts on every other value of
    /// irregular input. Only lane 0 is used.
    fn float_step(
        &mut self,
        value: NativeValue,
        target: NativeValue,
        cc: FloatCC,
        one: u64,
    ) -> NativeValue {
        let (vector, width) = if self.bcx.func.dfg.value_type(value) == types::F32 {
            (types::F32X4, 4)
        } else {
            (types::F64X2, 8)
        };
        let mut bytes = [0u8; 16];
        for lane in bytes.chunks_mut(width) {
            lane.copy_from_slice(&one.to_le_bytes()[..width]);
        }
        let constant = self
            .bcx
            .func
            .dfg
            .constants
            .insert(ConstantData::from(&bytes[..]));
        let ones = self.bcx.ins().vconst(vector, constant);
        let value_lanes = self.bcx.ins().scalar_to_vector(vector, value);
        let target_lanes = self.bcx.ins().scalar_to_vector(vector, target);
        let past = self.bcx.ins().fcmp(cc, value_lanes, target_lanes);
        let past = self.bcx.ins().bitcast(vector, MemFlags::new(), past);
        let step = self.bcx.ins().band(past, ones);
        let moved = if cc == FloatCC::GreaterThan {
            self.bcx.ins().fsub(value_lanes, step)
        } else {
            self.bcx.ins().fadd(value_lanes, step)
        };
        self.bcx.ins().extractlane(moved, 0)
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
