//! Coverage instrumentation for fuzzing builds (`plenty --fuzz`).
//!
//! Every block of a function increments its own byte counter when it runs,
//! the layout clang's `-fsanitize-coverage=inline-8bit-counters,pc-table`
//! produces, so libFuzzer can tell which inputs reach new code. The counters
//! of one function are a writable data object in section `__sancov_cntrs`;
//! beside it, in `__sancov_pcs`, is one sixteen-byte entry per counter: the
//! function's address and a flags word, 1 for the entry block. Both sections
//! are retained through `--gc-sections`, and the linker gathers each into one
//! array between `__start_` and `__stop_` symbols, in the same function
//! order, which the fuzz driver hands to libFuzzer.
//!
//! Cranelift does not expose block addresses, so every entry of a function
//! names the function itself: libFuzzer counts entries for its `cov:` figure
//! and prints them by function. Counting blocks rather than edges misses a
//! branch whose arms meet without running a block of their own; the hit-count
//! buckets of loops are unaffected.
use super::*;
use cranelift_codegen::cursor::{Cursor, FuncCursor};
use cranelift_codegen::ir::MemFlagsData;

pub(super) const COUNTERS: &str = "__sancov_cntrs";
pub(super) const PCS: &str = "__sancov_pcs";

/// One counter per block of `func`, incremented first thing in the block.
pub(super) fn instrument(func: &mut Function, symbols: &mut symbols::Symbols) {
    let blocks: Vec<Block> = func.layout.blocks().collect();
    if blocks.is_empty() {
        return;
    }
    let counters = symbols.coverage(blocks.len() as u32, func);
    let mut cursor = FuncCursor::new(func);
    for (index, block) in blocks.into_iter().enumerate() {
        cursor.goto_first_insertion_point(block);
        let base = cursor.ins().symbol_value(PTR_TY, counters);
        let offset = index as i32;
        // Wrapping and unsynchronized, as clang's counters are: a lost or
        // wrapped count only blurs libFuzzer's hit-count buckets.
        let count = cursor
            .ins()
            .load(types::I8, MemFlagsData::trusted(), base, offset);
        let count = cursor.ins().iadd_imm_s(count, 1);
        cursor
            .ins()
            .store(MemFlagsData::trusted(), count, base, offset);
    }
}

/// Define `function`'s `counters` counters and their PC table in `module`,
/// returning the counters.
pub(super) fn define(module: &mut ObjectModule, function: FuncId, counters: u32) -> Result<DataId> {
    let mut description = DataDescription::new();
    description.define(vec![0; counters as usize].into_boxed_slice());
    description.set_custom_section(COUNTERS);
    description.set_used(true);
    let data = module.declare_anonymous_data(true, false)?;
    module.define_data(data, &description)?;

    let mut table = DataDescription::new();
    let mut flags = vec![0; counters as usize * 16];
    flags[8] = 1;
    table.define(flags.into_boxed_slice());
    table.set_custom_section(PCS);
    table.set_align(8);
    table.set_used(true);
    let reference = table.import_function(cranelift_module::ModuleRelocTarget::user(
        0,
        function.as_u32(),
    ));
    for entry in 0..counters {
        table.write_function_addr(entry * 16, reference);
    }
    let pcs = module.declare_anonymous_data(false, false)?;
    module.define_data(pcs, &table)?;
    Ok(data)
}
