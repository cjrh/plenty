//! Coverage instrumentation for fuzzing builds (`plenty --fuzz`).
//!
//! Every block of a function increments its own byte counter when it runs,
//! the layout clang's `-fsanitize-coverage=inline-8bit-counters,pc-table`
//! produces, so libFuzzer can tell which inputs reach new code. The counters
//! of all functions share a writable array in section `__sancov_cntrs`;
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

/// Coverage is placed in function order, independent of worker completion.
/// Keeping exactly two sections also avoids Mach-O's 255-section limit.
#[derive(Default)]
pub(super) struct Table {
    functions: Vec<(DataId, FuncId, u32)>,
}

impl Table {
    /// Each function receives an exact address, without GOT relocation addends.
    pub(super) fn append(
        &mut self,
        module: &mut ObjectModule,
        function: FuncId,
        counters: u32,
    ) -> Result<DataId> {
        if counters == 0 {
            return Err("a coverage function needs at least one counter".into());
        }
        let data = module.declare_data(
            &format!("__plenty_coverage_{}", function.as_u32()),
            Linkage::Import,
            true,
            false,
        )?;
        self.functions.push((data, function, counters));
        Ok(data)
    }

    pub(super) fn define(self, module: &mut ObjectModule) -> Result<Placed> {
        if self.functions.is_empty() {
            return Ok(Placed {
                table: None,
                aliases: Vec::new(),
            });
        }
        let count = self
            .functions
            .iter()
            .try_fold(0usize, |total, &(_, _, count)| {
                total
                    .checked_add(count as usize)
                    .ok_or("too many coverage counters")
            })?;
        let bytes = count
            .checked_mul(16)
            .ok_or("coverage PC table is too large")?;
        u32::try_from(bytes)?;
        let mut description = DataDescription::new();
        description.define(vec![0; count].into_boxed_slice());
        description.set_custom_section(&section_name(module, COUNTERS));
        description.set_used(true);
        let data = module.declare_anonymous_data(true, false)?;
        module.define_data(data, &description)?;

        let mut table = DataDescription::new();
        let mut flags = vec![0; bytes];
        let mut first = 0usize;
        let mut aliases = Vec::with_capacity(self.functions.len());
        for (alias, function, counters) in self.functions {
            flags[first * 16 + 8] = 1;
            let reference = table.import_function(cranelift_module::ModuleRelocTarget::user(
                0,
                function.as_u32(),
            ));
            for entry in first..first + counters as usize {
                table.write_function_addr(u32::try_from(entry * 16)?, reference);
            }
            aliases.push((alias, first as u64, u64::from(counters)));
            first += counters as usize;
        }
        table.define(flags.into_boxed_slice());
        table.set_custom_section(&section_name(module, PCS));
        table.set_align(8);
        table.set_used(true);
        let pcs = module.declare_anonymous_data(false, false)?;
        module.define_data(pcs, &table)?;
        Ok(Placed {
            table: Some(data),
            aliases,
        })
    }
}

pub(super) struct Placed {
    table: Option<DataId>,
    aliases: Vec<(DataId, u64, u64)>,
}

impl Placed {
    pub(super) fn resolve(self, product: &mut cranelift_object::ObjectProduct) -> Result<()> {
        use cranelift_object::object::{SymbolScope, macho, write::SymbolSection};
        let Some(table) = self.table else {
            return Ok(());
        };
        let table = product.object.symbol(product.data_symbol(table));
        let SymbolSection::Section(section) = table.section else {
            return Err("the coverage counter table has no section".into());
        };
        let base = table.value;
        for (alias, offset, size) in self.aliases {
            let alias = product.data_symbol(alias);
            let symbol = product.object.symbol_mut(alias);
            symbol.section = SymbolSection::Section(section);
            symbol.value = base + offset;
            symbol.size = size;
            symbol.scope = SymbolScope::Compilation;
            // These addresses designate slices of one array. They must not
            // create independently movable/dead-strippable Mach-O atoms.
            if let object::SymbolFlags::MachO { n_desc, .. } =
                product.object.symbol_flags_mut(alias)
            {
                *n_desc |= macho::N_ALT_ENTRY;
            }
        }
        Ok(())
    }
}

fn section_name(module: &ObjectModule, name: &str) -> String {
    if module.isa().triple().binary_format == target_lexicon::BinaryFormat::Macho {
        format!("__DATA,{name}")
    } else {
        name.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift_codegen::ir::{Function, InstBuilder, UserFuncName};
    use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
    use object::{Object, ObjectSection, ObjectSymbol};

    #[test]
    fn many_functions_share_ordered_counter_and_pc_arrays_on_every_native_target() {
        const FUNCTIONS: usize = 300;
        const COUNTERS_PER_FUNCTION: u32 = 3;
        for target in crate::target::NATIVE_TARGETS {
            let isa =
                crate::codegen::baseline_isa(target.triple, "backtracking", "none", false).unwrap();
            let mut builder = cranelift_object::ObjectBuilder::new(
                isa,
                "coverage-arrays",
                cranelift_module::default_libcall_names(),
            )
            .unwrap();
            builder
                .per_function_section(true)
                .per_data_object_section(true);
            let mut module = ObjectModule::new(builder);
            let mut table = Table::default();
            let mut scratch = FunctionBuilderContext::new();
            for index in 0..FUNCTIONS {
                let signature = module.make_signature();
                let function = module
                    .declare_function(&format!("covered_{index}"), Linkage::Local, &signature)
                    .unwrap();
                let counters = table
                    .append(&mut module, function, COUNTERS_PER_FUNCTION)
                    .unwrap();
                let mut context = module.make_context();
                context.func = Function::with_name_signature(
                    UserFuncName::user(0, function.as_u32()),
                    signature,
                );
                let counter = module.declare_data_in_func(counters, &mut context.func);
                let mut body = FunctionBuilder::new(&mut context.func, &mut scratch);
                let block = body.create_block();
                body.switch_to_block(block);
                body.seal_block(block);
                let address = body.ins().symbol_value(PTR_TY, counter);
                let value = body
                    .ins()
                    .load(types::I8, MemFlagsData::trusted(), address, 0);
                let value = body.ins().iadd_imm_s(value, 1);
                body.ins().store(MemFlagsData::trusted(), value, address, 0);
                body.ins().return_(&[]);
                body.finalize(crate::codegen::FRONTEND);
                module.define_function(function, &mut context).unwrap();
            }
            let placed = table.define(&mut module).unwrap();
            let mut product = module.finish();
            placed.resolve(&mut product).unwrap();
            let bytes = product.emit().unwrap();
            let object = object::File::parse(bytes.as_slice()).unwrap();
            let counters: Vec<_> = object
                .sections()
                .filter(|section| section.name().unwrap() == COUNTERS)
                .collect();
            let pcs: Vec<_> = object
                .sections()
                .filter(|section| section.name().unwrap() == PCS)
                .collect();
            assert_eq!(counters.len(), 1, "{}", target.triple);
            assert_eq!(pcs.len(), 1, "{}", target.triple);
            let counters = &counters[0];
            let pcs = &pcs[0];
            let count = FUNCTIONS * COUNTERS_PER_FUNCTION as usize;
            assert_eq!(counters.data().unwrap(), vec![0; count]);
            assert_eq!(pcs.data().unwrap().len(), count * 16);
            assert_eq!(pcs.relocations().count(), count);
            for (index, entry) in pcs.data().unwrap().as_chunks::<16>().0.iter().enumerate() {
                let flags = u64::from_le_bytes(entry[8..16].try_into().unwrap());
                assert_eq!(
                    flags,
                    u64::from(index % COUNTERS_PER_FUNCTION as usize == 0)
                );
            }
            for index in 0..FUNCTIONS {
                let name = format!("__plenty_coverage_{index}");
                let name = if target.object_format == object::BinaryFormat::MachO {
                    format!("_{name}")
                } else {
                    name
                };
                let symbol = object
                    .symbols()
                    .find(|symbol| symbol.name().unwrap() == name)
                    .unwrap();
                assert_eq!(symbol.section_index(), Some(counters.index()));
                assert_eq!(
                    symbol.address() - counters.address(),
                    index as u64 * u64::from(COUNTERS_PER_FUNCTION)
                );
                if let object::SymbolFlags::MachO { n_desc, .. } = symbol.flags() {
                    assert!(n_desc.contains(object::macho::N_ALT_ENTRY));
                }
            }
            if target.object_format == object::BinaryFormat::MachO {
                assert!(object.sections().count() <= 4);
            }
        }
    }
}
