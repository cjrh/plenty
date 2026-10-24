//! Failed checks reported through the runtime's `SIGILL` handler.
//!
//! In an executable, a check whose site is fixed at lowering time branches to
//! a block holding only a CLIF `trap`, so its code is a conditional jump to a
//! two-byte `ud2`, rather than to a block that loads the site and calls a
//! runtime helper. Each such block has its own trap code, numbered per
//! function. Cranelift reports where each code's `ud2` landed, and the object
//! gains one [`Table`] mapping those addresses to the check's kind and site.
//! `plenty_main` hands the table to the runtime, whose handler moves a
//! faulting thread into the helper the call would have reached, so messages
//! and exit status are unchanged (issue #425).
//!
//! The table refers to each listed function, so it would keep every function
//! with a check through the linker's `--gc-sections`, along with all that
//! function reaches. It lists only functions reachable from the object's
//! exported functions and `__del__` hooks through the calls and addresses in
//! placed code: the only references to an object's local function are from
//! code and from type metadata, so this covers whatever the linker keeps.
//!
//! Libraries and `--fuzz` builds keep the calls: a library leaves signal
//! dispositions to its host, and libFuzzer claims `SIGILL` itself.
//!
//! Layout, matching `Table` in the runtime's `traps.rs`: a `u64` entry count,
//! the address of the object's site table (or zero), each entry's `ud2`
//! address, then each entry's `u32` site record offset in the site table
//! ([`NO_SITE`] for none) with its [`kind`] in the low four bits, which a
//! record's sixteen-byte alignment leaves free. Twelve bytes per check, never
//! read unless a check fails.
use super::*;
use crate::hash::HashSet;
use cranelift_codegen::MachTrap;
use cranelift_codegen::ir::TrapCode;
use cranelift_module::{ModuleReloc, ModuleRelocTarget};

/// The first code handed out. Codes 1 to 3 mark the compiler's other traps,
/// which the runtime must not mistake for a check.
const FIRST_CODE: u8 = 4;
/// Cranelift reserves the codes above this.
const LAST_CODE: u8 = 250;
const HEADER: usize = 16;
pub(super) const NO_SITE: u32 = !KIND_MASK;
const KIND_MASK: u32 = 15;

/// The runtime helper a check's kind selects, as the runtime numbers them.
pub(super) fn kind(kind: TrapKind) -> u32 {
    match kind {
        TrapKind::Overflow => 0,
        TrapKind::DivZero => 1,
        TrapKind::Shift => 2,
        TrapKind::CastRange => 3,
        TrapKind::Unwrap { option: false } => 4,
        TrapKind::Unwrap { option: true } => 5,
    }
}

/// One function's checks, by trap code.
#[derive(Default)]
pub(super) struct Checks {
    /// `(kind, site record offset in the function's own records)`, for code
    /// `FIRST_CODE + index`.
    checks: Vec<(u32, Option<i64>)>,
}

impl Checks {
    /// A code for a check of `kind` at `site`, or `None` once the function
    /// has used every code; that check calls its helper instead.
    pub(super) fn code(&mut self, kind: u32, site: Option<i64>) -> Option<TrapCode> {
        let code = usize::from(FIRST_CODE) + self.checks.len();
        if code > usize::from(LAST_CODE) {
            return None;
        }
        self.checks.push((kind, site));
        Some(TrapCode::unwrap_user(code as u8))
    }

    /// The runtime helper the check with `code` reaches, for dumps.
    pub(super) fn helper(&self, code: u8) -> Option<&'static str> {
        const HELPERS: [&str; 6] = [
            "plenty_trap_overflow",
            "plenty_trap_div_zero",
            "plenty_trap_shift",
            "plenty_trap_cast_range",
            "plenty_trap_unwrap",
            "plenty_trap_unwrap",
        ];
        HELPERS.get(self.get(code)?.0 as usize).copied()
    }

    fn get(&self, code: u8) -> Option<(u32, Option<i64>)> {
        let index = usize::from(code.checked_sub(FIRST_CODE)?);
        self.checks.get(index).copied()
    }
}

/// `(offset, code)` of each of `traps` whose code may be a check's.
pub(crate) fn checks(traps: &[MachTrap]) -> Vec<(u32, u8)> {
    traps
        .iter()
        .map(|trap| (trap.offset, trap.code.as_raw().get()))
        .filter(|(_, code)| (FIRST_CODE..=LAST_CODE).contains(code))
        .collect()
}

/// The object's one table of every check's `ud2`.
pub(super) struct Table {
    data: DataId,
    /// `(function, ud2 offset in it, site offset in the site table | kind)`.
    entries: Vec<(FuncId, u32, u32)>,
    /// The functions each placed function's code refers to.
    references: HashMap<FuncId, Vec<FuncId>>,
    /// Functions reached from data: `__del__` hooks.
    roots: Vec<FuncId>,
}

impl Table {
    /// Declared before bodies are built, since `plenty_main` refers to it.
    pub(super) fn declare(module: &mut ObjectModule) -> Result<Self> {
        Ok(Self {
            data: module.declare_anonymous_data(false, false)?,
            entries: Vec::new(),
            references: HashMap::default(),
            roots: Vec::new(),
        })
    }

    pub(super) fn data(&self) -> DataId {
        self.data
    }

    /// Records that type metadata refers to `function`.
    pub(super) fn root(&mut self, function: FuncId) {
        self.roots.push(function);
    }

    /// Records the functions `function`'s placed code refers to.
    pub(super) fn refer(&mut self, function: FuncId, relocs: &[ModuleReloc]) {
        let targets: Vec<_> = relocs
            .iter()
            .filter_map(|reloc| match reloc.name {
                ModuleRelocTarget::User {
                    namespace: 0,
                    index,
                } => Some(FuncId::from_u32(index)),
                _ => None,
            })
            .collect();
        if !targets.is_empty() {
            self.references.insert(function, targets);
        }
    }

    /// The functions reachable from exported functions and the roots.
    fn reachable(&self, module: &ObjectModule) -> HashSet<FuncId> {
        let mut pending: Vec<_> = module
            .declarations()
            .get_functions()
            .filter(|(_, declaration)| declaration.linkage == Linkage::Export)
            .map(|(id, _)| id)
            .chain(self.roots.iter().copied())
            .collect();
        let mut reached = HashSet::default();
        while let Some(function) = pending.pop() {
            if reached.insert(function) {
                pending.extend(self.references.get(&function).into_iter().flatten());
            }
        }
        reached
    }

    /// Records the checks of `function`, whose compiled code has `traps`
    /// (`(offset, code)`) and whose site records start at `site_base` in the
    /// site table.
    pub(super) fn add(
        &mut self,
        function: FuncId,
        checks: &Checks,
        traps: &[(u32, u8)],
        site_base: i64,
    ) -> Result<()> {
        for &(offset, code) in traps {
            let (kind, site) = checks
                .get(code)
                .ok_or_else(|| format!("trap code {code} names no check"))?;
            let site = match site {
                Some(site) => {
                    let site = u32::try_from(site_base + site)?;
                    if site & KIND_MASK != 0 || site >= NO_SITE {
                        return Err(
                            format!("site offset {site} does not fit the trap table").into()
                        );
                    }
                    site
                }
                None => NO_SITE,
            };
            self.entries.push((function, offset, site | kind));
        }
        Ok(())
    }

    /// Defines the table, once every function is placed. `sites` is the
    /// object's site table, if it has one.
    pub(super) fn define(mut self, module: &mut ObjectModule, sites: Option<DataId>) -> Result<()> {
        let reached = self.reachable(module);
        self.entries
            .retain(|(function, _, _)| reached.contains(function));
        let mut bytes = Vec::with_capacity(HEADER + self.entries.len() * 12);
        bytes.extend((self.entries.len() as u64).to_ne_bytes());
        bytes.extend(0u64.to_ne_bytes());
        let mut description = DataDescription::new();
        if let Some(sites) = sites {
            let table = description.import_global_value(ModuleRelocTarget::user(1, sites.as_u32()));
            description.write_data_addr(8, table, 0);
        }
        for (i, &(function, offset, _)) in self.entries.iter().enumerate() {
            bytes.extend(0u64.to_ne_bytes());
            let target =
                description.import_global_value(ModuleRelocTarget::user(0, function.as_u32()));
            description.write_data_addr(u32::try_from(HEADER + i * 8)?, target, i64::from(offset));
        }
        for &(_, _, site) in &self.entries {
            bytes.extend(site.to_ne_bytes());
        }
        description.define(bytes.into_boxed_slice());
        description.set_align(8);
        module.define_data(self.data, &description)?;
        Ok(())
    }
}
