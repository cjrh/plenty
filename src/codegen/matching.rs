//! Bounded jump-table selection for integer literals and lowered enum tags,
//! and sorted-table dispatch for large string matches.
use super::*;
use cranelift_codegen::ir::JumpTableData;

// The B19 paired measurements found wins even for first-heavy inputs at 256
// cases, while smaller tables regressed for some distributions. This is a
// conservative policy, not a claim that every 256-case workload becomes faster.
const MIN_TABLE_CASES: usize = 256;

// Below this many distinct literals, a chain of `plenty_str_eq` calls stays.
// Issue #565 measured 2 to 597 arms: the chain wins on uniform inputs below
// about 12 arms and always wins when the first arm is hot, so the table starts
// where it clearly pays. See the profiling report's string match section.
const MIN_STR_TABLE_CASES: usize = 16;

/// Sorted literal tables, keyed by a match's leading string patterns in
/// source order. Equal matches, such as inlined copies, share one table.
pub(super) type StrTables = HashMap<Box<[StrId]>, StrTable>;

/// The read-only table one string match searches with `plenty_str_match`.
pub(super) struct StrTable {
    /// The distinct literals, ordered by byte length then bytes, in the
    /// layout `plenty_str_match` reads.
    data: DataId,
    /// For each table entry, the index of the first arm with that literal.
    arms: Box<[u32]>,
}

/// The string patterns a table could replace: every arm before the first
/// wildcard, which then becomes the fallback. Any other pattern, which a `str`
/// scrutinee never has, keeps the chain.
fn str_table_key(arms: &[MatchArm]) -> Option<Box<[StrId]>> {
    let mut ids = Vec::new();
    for arm in arms {
        match arm.pattern {
            Pattern::Str(id) => ids.push(id),
            Pattern::Wildcard => break,
            _ => return None,
        }
    }
    (ids.len() >= MIN_STR_TABLE_CASES).then(|| ids.into())
}

/// Declare a table for each string match with enough distinct literals.
/// `matches` must be in a deterministic order, since it numbers the symbols.
pub(super) fn declare_str_tables(
    matches: &[&[MatchArm]],
    heap: &Heap,
    module: &mut ObjectModule,
) -> Result<StrTables> {
    let mut tables = StrTables::default();
    for arms in matches {
        let Some(key) = str_table_key(arms) else {
            continue;
        };
        if tables.contains_key(&key) || u32::try_from(key.len()).is_err() {
            continue;
        }
        let mut entries: Vec<(&[u8], u32)> = (0u32..)
            .zip(key.iter())
            .map(|(arm, id)| (heap.data(*id).0, arm))
            .collect();
        // `plenty_str_match` relies on this order. The sort is stable, so of
        // equal literals the first arm in source order is kept, as the chain
        // would have matched it first.
        entries.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then_with(|| a.0.cmp(b.0)));
        entries.dedup_by(|later, kept| later.0 == kept.0);
        if entries.len() < MIN_STR_TABLE_CASES {
            continue;
        }
        // The layout `plenty_str_match` documents: offsets, then bytes.
        let mut bytes: Vec<u8> = Vec::new();
        let mut offsets = vec![0u32];
        for (literal, _) in &entries {
            bytes.extend_from_slice(literal);
            offsets.push(u32::try_from(bytes.len())?);
        }
        let mut blob: Vec<u8> = offsets.iter().flat_map(|o| o.to_ne_bytes()).collect();
        blob.extend_from_slice(&bytes);
        let name = format!("plenty_str_table_{}", tables.len());
        let data = module.declare_data(&name, Linkage::Local, false, false)?;
        let mut desc = DataDescription::new();
        desc.define(blob.into_boxed_slice());
        desc.set_align(4);
        module.define_data(data, &desc)?;
        let arms = entries.iter().map(|&(_, arm)| arm).collect();
        tables.insert(key, StrTable { data, arms });
    }
    Ok(tables)
}

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
            self.bcx.ins().iadd_imm_s(scrut, base.wrapping_neg())
        };
        // Check BEFORE narrowing to i32: a distant i64/u64 must not alias a
        // table entry. Use an inclusive upper bound so a full u8 table does
        // not truncate the length 256 to zero in an i8 comparison.
        let inside = self.bcx.ins().icmp_imm_s(
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

    /// Dispatch a large string match with one table search and a `br_table`
    /// over the found index, instead of one `plenty_str_eq` call per arm. The
    /// result `len` (no literal matched) takes the wildcard, or traps as the
    /// chain's impossible case does. The search is the scrutinee's last use, so
    /// this releases it once here rather than at the start of every arm.
    /// Returns false, emitting nothing, to keep the chain.
    pub(super) fn lower_str_table_dispatch(
        &mut self,
        scrut: cranelift_codegen::ir::Value,
        scrut_ty: &Ty,
        arms: &[MatchArm],
        arm_blocks: &[Block],
    ) -> bool {
        let Some(key) = str_table_key(arms) else {
            return false;
        };
        let Some(table) = self.str_data.tables.get(&key) else {
            return false;
        };
        let default = (key.len() < arms.len()).then(|| arm_blocks[key.len()]);
        let fallback = default.unwrap_or_else(|| self.bcx.create_block());
        let jump_table = JumpTableData::new(
            self.bcx.func.dfg.block_call(fallback, &[]),
            &table
                .arms
                .iter()
                .map(|&arm| self.bcx.func.dfg.block_call(arm_blocks[arm as usize], &[]))
                .collect::<Vec<_>>(),
        );
        let jump_table = self.bcx.create_jump_table(jump_table);
        let gv = self.module.declare_data_in_func(table.data, self.bcx.func);
        let address = self.bcx.ins().symbol_value(PTR_TY, gv);
        let len = self.bcx.ins().iconst(types::I64, table.arms.len() as i64);
        let search = self
            .module
            .declare_func_in_func(self.runtime.str_match, self.bcx.func);
        let call = self.bcx.ins().call(search, &[scrut, address, len]);
        // The index is at most `len`, which fits in u32.
        let index = self.bcx.inst_results(call)[0];
        let index = self.bcx.ins().ireduce(types::I32, index);
        self.release(scrut, scrut_ty);
        self.bcx.ins().br_table(index, jump_table);
        if default.is_none() {
            self.bcx.switch_to_block(fallback);
            self.bcx.seal_block(fallback);
            self.bcx.ins().trap(TrapCode::unwrap_user(1));
        }
        true
    }
}
