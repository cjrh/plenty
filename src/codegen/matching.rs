//! Bounded jump-table selection for integer literals and lowered enum tags.
use super::*;
use cranelift_codegen::ir::JumpTableData;

// The B19 paired measurements found wins even for first-heavy inputs at 256
// cases, while smaller tables regressed for some distributions. This is a
// conservative policy, not a claim that every 256-case workload becomes faster.
const MIN_TABLE_CASES: usize = 256;

/// Order by mathematical value, so negative-to-positive ranges stay contiguous.
/// The checker has already checked each literal against the scrutinee's type.
fn key(value: Value) -> i128 {
    match value {
        Value::I8(n) => i128::from(n),
        Value::I16(n) => i128::from(n),
        Value::I32(n) => i128::from(n),
        Value::I64(n) => i128::from(n),
        Value::U8(n) => i128::from(n),
        Value::U16(n) => i128::from(n),
        Value::U32(n) => i128::from(n),
        Value::U64(n) => i128::from(n),
    }
}

impl Lowerer<'_, '_> {
    /// Emit only dispatch; arm bodies, ownership cleanup and joins remain shared
    /// with chain lowering. A final wildcard is the default; without one this
    /// emits the same defensive impossible-case trap as chain lowering.
    pub(super) fn lower_dense_dispatch(
        &mut self,
        scrut: cranelift_codegen::ir::Value,
        scrut_ty: &Ty,
        arms: &[MatchArm],
        arm_blocks: &[Block],
    ) -> bool {
        if !scrut_ty.is_int() || arms.len() < MIN_TABLE_CASES {
            return false;
        }
        let mut entries = Vec::with_capacity(arms.len());
        let mut default = None;
        for (i, arm) in arms.iter().enumerate() {
            match arm.pattern {
                Pattern::Int { value, .. } => entries.push((key(value), arm_blocks[i])),
                Pattern::Wildcard if i + 1 == arms.len() => default = Some(arm_blocks[i]),
                // Keep chains for other patterns and for early wildcards. In
                // particular, never reorder tests past a wildcard.
                _ => return false,
            }
        }
        if entries.len() < MIN_TABLE_CASES || u32::try_from(entries.len()).is_err() {
            return false;
        }
        entries.sort_unstable_by_key(|&(value, _)| value);
        if !entries.windows(2).all(|pair| pair[1].0 == pair[0].0 + 1) {
            // Sparse and duplicate keys retain source-order chain behavior.
            // Table storage is proportional to cases, never the key span.
            return false;
        }

        let fallback = default.unwrap_or_else(|| self.bcx.create_block());
        let table = JumpTableData::new(
            self.bcx.func.dfg.block_call(fallback, &[]),
            &entries
                .iter()
                .map(|&(_, block)| self.bcx.func.dfg.block_call(block, &[]))
                .collect::<Vec<_>>(),
        );
        let table = self.bcx.create_jump_table(table);
        // Subtract in the scrutinee's width, wrapping deliberately. This maps
        // signed ranges across zero, and ranges next to u64::MAX, to 0..len.
        let base = entries[0].0 as i64;
        let index = if base == 0 {
            scrut
        } else {
            self.bcx.ins().iadd_imm(scrut, base.wrapping_neg())
        };
        // Check BEFORE narrowing to i32: a distant i64/u64 must not alias a
        // table entry. Use an inclusive upper bound so a full u8 table does
        // not truncate the length 256 to zero in an i8 comparison.
        let inside = self.bcx.ins().icmp_imm(
            IntCC::UnsignedLessThanOrEqual,
            index,
            (entries.len() - 1) as i64,
        );
        let dispatch = self.bcx.create_block();
        self.bcx.ins().brif(inside, dispatch, &[], fallback, &[]);
        self.bcx.switch_to_block(dispatch);
        self.bcx.seal_block(dispatch);
        let index = match clif_type(scrut_ty.clone()).bits() {
            bits if bits < 32 => self.bcx.ins().uextend(types::I32, index),
            bits if bits > 32 => self.bcx.ins().ireduce(types::I32, index),
            _ => index,
        };
        self.bcx.ins().br_table(index, table);
        if default.is_none() {
            self.bcx.switch_to_block(fallback);
            self.bcx.seal_block(fallback);
            self.bcx.ins().trap(TrapCode::unwrap_user(1));
        }
        true
    }
}
