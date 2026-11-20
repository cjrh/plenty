//! Native calls and generated cleanup can enter Plenty provider code without a Call op.
use crate::collection::CollectionOp as C;
use crate::collection_opcodes::*;
use crate::hash::HashSet;
use crate::op::{CompiledFn, Op, Ty};
use crate::threading::{self, AllocatorProvider, ThreadOp};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct Callbacks {
    providers: Vec<Arc<AllocatorProvider>>,
    producers: HashSet<i64>,
    targets: threading::CallableTargets,
}

impl Callbacks {
    pub fn new(ops: &[Op]) -> Self {
        let mut result = Self::default();
        let mut addresses = Vec::new();
        let dead = crate::op::reachability::dead_definitions(ops, &[], false);
        for op in ops {
            if matches!(op, Op::DefineFn(name, _) if dead.contains(name)) {
                continue;
            }
            threading::walk(std::slice::from_ref(op), &mut |op| {
                if let Op::Thread(ThreadOp::AllocatorProvider(p)) = op {
                    result.providers.push(p.clone());
                }
                if let Op::FunctionAddress(name, sig) = op {
                    addresses.push((name.as_str(), sig));
                }
                if let Some(code) = crate::foreign::allocator_opcode(op) {
                    result.producers.insert(code);
                }
            });
        }
        result.targets = threading::CallableTargets::new(addresses);
        result
    }

    /// Stored allocator identity is erased. A descendant holding a selected
    /// owner can reach coordination state through its provider.
    #[expect(
        clippy::mutable_key_type,
        reason = "nominal identities exclude definition cells"
    )]
    pub fn extend_child_inputs(&self, inputs: &mut Vec<Ty>) {
        if self.providers.is_empty() {
            return;
        }
        let mut pending = inputs.clone();
        let mut seen = HashSet::default();
        while let Some(ty) = pending.pop() {
            if !seen.insert(ty.clone()) {
                continue;
            }
            if self.selected(&ty)
                || matches!(&ty, Ty::Class(c) if c.name.as_ref()=="std.alloc.Allocator")
            {
                inputs.extend(self.providers.iter().map(|p| p.state.clone()));
                break;
            }
            threading::contained(&ty, &mut pending);
        }
    }

    fn selected(&self, ty: &Ty) -> bool {
        let has = |op| self.producers.contains(&op);
        match ty {
            Ty::Clist(_) => has(ALLOCATOR_CLIST_SLICE),
            Ty::Str => {
                has(ALLOCATOR_TEXT_SLICE)
                    || has(ALLOCATOR_TEXT_CONCAT)
                    || has(ALLOCATOR_TEXT_JOIN)
                    || has(ALLOCATOR_TEXT_FINISH)
                    || has(ALLOCATOR_TEXT_REPEAT)
                    || has(ALLOCATOR_TEXT_REPLACE)
                    || has(ALLOCATOR_TEXT_STRIP)
                    || has(ALLOCATOR_TEXT_CASE)
            }
            Ty::ByteStorage => has(ALLOCATOR_BYTES_SLICE),
            Ty::Box(_) => has(ALLOCATOR_BOX) || has(ALLOCATOR_COPY) || has(ALLOCATOR_LIST_SLICE),
            Ty::List(_) | Ty::Dict(..) | Ty::Set(_) => self.producers.iter().any(|&c| {
                matches!(
                    c,
                    ALLOCATOR_LIST | ALLOCATOR_DICT_SET..=ALLOCATOR_LIST_SLICE
                )
            }),
            Ty::Class(c) => c.name.as_ref() == "std.alloc.ByteBlock",
            _ => false,
        }
    }

    /// Visit types that can actually be destroyed, not the referent of a loan.
    /// A final handle destroys its registered state, possibly another owner.
    #[expect(
        clippy::mutable_key_type,
        reason = "nominal identities exclude definition cells"
    )]
    pub fn edges(&self, function: &CompiledFn, pending: &mut Vec<String>) {
        if self.providers.is_empty() {
            return;
        }
        let mut types = Vec::new();
        let _ = threading::inspect(function, &mut types, &mut Vec::new());
        let mut alloc = false;
        let mut release = false;
        let mut final_handle = false;
        threading::walk(&function.body, &mut |op| {
            if let Op::CallIndirect(sig) | Op::TailCallIndirect(sig) = op
                && let Some(names) = self.targets.names(sig)
            {
                pending.extend(names.iter().cloned());
            }
            if let Some(code) = crate::foreign::allocator_opcode(op) {
                match code {
                    ALLOCATOR_RELEASE => final_handle = true,
                    ALLOCATOR_BLOCK_RELEASE => release = true,
                    ALLOCATOR_LIST | ALLOCATOR_BLOCK | ALLOCATOR_REGISTER..=ALLOCATOR_TEXT_CASE => {
                        alloc = true;
                        release = true;
                    }
                    _ => {}
                }
            }
            if let Op::ReadRef(t) | Op::ReplaceRef(t) | Op::WriteRef(t) = op {
                types.push(t.clone());
            }
            if let Op::Collection(op) = op {
                let growth = match op {
                    C::TryReserve(t)
                    | C::TryInsert(t)
                    | C::TryExtend(t)
                    | C::ListTryInsert(t)
                    | C::SetTryUpdate(t)
                    | C::DictTryUpdate(t)
                    | C::DictGetOrInsert(t)
                    | C::DictLazyFinish(t) => self.selected(t),
                    C::Clist(
                        crate::clist::ClistOp::Reserve(_) | crate::clist::ClistOp::Append(_),
                    ) => self.producers.contains(&ALLOCATOR_CLIST_SLICE),
                    C::Bytes(b) => b.may_grow() && self.producers.contains(&ALLOCATOR_BYTES_SLICE),
                    C::Net(crate::net::NetOp::ReceiveFrom) => {
                        self.producers.contains(&ALLOCATOR_BYTES_SLICE)
                    }
                    _ => false,
                };
                alloc |= growth;
                release |= growth;
            }
        });
        let mut seen = HashSet::default();
        let mut state_queued = false;
        loop {
            if !state_queued && (release || final_handle) {
                types.extend(self.providers.iter().map(|p| p.state.clone()));
                state_queued = true;
            }
            let Some(ty) = types.pop() else { break };
            if !seen.insert(ty.clone()) || !ty.managed() {
                continue;
            }
            if let Ty::Class(c) = &ty {
                pending.extend(c.get().destructor.iter().cloned());
                final_handle |= c.name.as_ref() == "std.alloc.Allocator";
            }
            if self.selected(&ty) && !release {
                release = true;
            }
            if !matches!(ty, Ty::Channel(_, true)) {
                threading::contained(&ty, &mut types);
            }
        }
        for p in &self.providers {
            if alloc {
                pending.push(p.allocate.clone());
            }
            if release {
                pending.push(p.release.clone());
            }
        }
    }
}
