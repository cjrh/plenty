//! Identical code folding: one body for functions whose compiled code is the
//! same.
//!
//! Generic instances often compile to the same machine code. Quantity
//! instances that differ only in dimension are the common case: dimensions
//! are erased, so `total[Length]` and `total[Time]` have the same code unless
//! the body formats a value or builds a collection, whose type descriptor
//! names the dimension. See
//! book/src/design/toolchain/02-architecture/emission.md#identical-code-folding.
//!
//! A function is folded into an earlier one when everything that determines
//! its placed code is equal: the bytes, the relocations, the symbols the body
//! requested ([`Request`]) and its checks. Site records are compared by
//! content, so equal bodies share their positions too. Calls compare by
//! callee after folding, so callers of folded functions fold in turn when
//! the callee was placed first. The comparison uses a SHA-256 digest, as the
//! compiled-function cache does.
//!
//! A folded function still needs a definition: the object module asserts that
//! every local function is defined. It is defined empty, and once the module
//! is finished its symbol is moved to the earlier function's code
//! ([`Folds::resolve`]), so every reference to it reaches that code.
//! Exported functions, and every function under coverage, keep their own
//! bodies. So does every function used as a value: function values compare
//! code addresses, and two distinct functions must not compare equal. Other
//! functions can still fold into one, since nothing compares their addresses.

use std::hash::{Hash, Hasher};

use cranelift_module::{FuncId, ModuleReloc, ModuleRelocTarget};
use cranelift_object::ObjectProduct;
use sha2::{Digest, Sha256};

use super::Result;
use super::symbols::Request;
use super::traps::Checks;
use crate::hash::HashMap;

type Key = [u8; 32];

#[derive(Default)]
pub(super) struct Folds {
    /// Functions used as values: placed with their own code, never folded.
    addressed: crate::hash::HashSet<FuncId>,
    first: HashMap<Key, FuncId>,
    /// Each folded function and the function whose code it uses.
    folded: HashMap<FuncId, FuncId>,
    /// In placement order, so the object does not depend on map order.
    aliases: Vec<(FuncId, FuncId)>,
}

/// What determines a function's placed code.
pub(super) struct Body<'a> {
    pub alignment: u64,
    pub bytes: &'a [u8],
    pub relocs: &'a [ModuleReloc],
    pub traps: &'a [(u32, u8)],
    pub requests: &'a [Request],
    pub checks: &'a Checks,
}

impl Folds {
    /// The function `body` can share code with, else the key to record with
    /// [`Self::placed`] once `id` is placed.
    pub(super) fn find(&self, id: FuncId, body: &Body) -> std::result::Result<FuncId, Key> {
        let key = self.key(id, body);
        match self.first.get(&key) {
            Some(&into) if !self.addressed.contains(&id) => Ok(into),
            _ => Err(key),
        }
    }

    /// `id` is used as a function value, so keeps an address of its own.
    pub(super) fn address_taken(&mut self, id: FuncId) {
        self.addressed.insert(id);
    }

    pub(super) fn placed(&mut self, id: FuncId, key: Key) {
        self.first.entry(key).or_insert(id);
    }

    pub(super) fn fold(&mut self, id: FuncId, into: FuncId) {
        self.folded.insert(id, into);
        self.aliases.push((id, into));
    }

    fn canonical(&self, id: FuncId) -> FuncId {
        // `into` was placed with its own code, so it is never folded.
        self.folded.get(&id).copied().unwrap_or(id)
    }

    fn key(&self, id: FuncId, body: &Body) -> Key {
        let mut state = Sha256Hasher(Sha256::new());
        body.alignment.hash(&mut state);
        body.bytes.hash(&mut state);
        body.relocs.len().hash(&mut state);
        for reloc in body.relocs {
            reloc.offset.hash(&mut state);
            // `Reloc` has no `Hash`; its debug form names the variant.
            format!("{:?}", reloc.kind).hash(&mut state);
            reloc.addend.hash(&mut state);
            match &reloc.name {
                ModuleRelocTarget::User {
                    namespace: 0,
                    index,
                } => {
                    0u8.hash(&mut state);
                    let callee = FuncId::from_u32(*index);
                    // A recursive call is equal whichever function makes it.
                    if callee == id {
                        u32::MAX.hash(&mut state);
                    } else {
                        self.canonical(callee).as_u32().hash(&mut state);
                    }
                }
                ModuleRelocTarget::FunctionOffset(function, offset) => {
                    1u8.hash(&mut state);
                    (*function == id).hash(&mut state);
                    if *function != id {
                        self.canonical(*function).as_u32().hash(&mut state);
                    }
                    offset.hash(&mut state);
                }
                other => {
                    2u8.hash(&mut state);
                    other.to_string().hash(&mut state);
                }
            }
        }
        body.traps.hash(&mut state);
        body.requests.hash(&mut state);
        body.checks.hash(&mut state);
        state.0.finalize().into()
    }

    /// Point each folded function's symbol at the code it shares.
    pub(super) fn resolve(self, product: &mut ObjectProduct) -> Result<()> {
        use cranelift_object::object::{SymbolFlags, macho};
        let macho = product.object.format() == object::BinaryFormat::MachO;
        for (id, into) in self.aliases {
            let target = product.object.symbol(product.function_symbol(into));
            let (section, value, size) = (target.section, target.value, target.size);
            let symbol = product.object.symbol_mut(product.function_symbol(id));
            symbol.section = section;
            symbol.value = value;
            symbol.size = size;
            if macho {
                // The shared code is one linker atom, started by `into`'s
                // symbol; an alias must not split or start another.
                symbol.flags = SymbolFlags::MachO {
                    n_type: macho::N_SECT.into(),
                    n_desc: macho::N_ALT_ENTRY,
                };
            }
        }
        Ok(())
    }
}

/// Feeds `Hash` output to SHA-256.
struct Sha256Hasher(Sha256);

impl Hasher for Sha256Hasher {
    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
    fn finish(&self) -> u64 {
        unreachable!("read the digest instead")
    }
}
