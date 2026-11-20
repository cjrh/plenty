//! A selected allocation retains provider state that its public type doesn't
//! mention. Keep this join analysis separate from structural worker eligibility.
use crate::clist::ClistOp;
use crate::collection::CollectionOp;
use crate::collection_opcodes::{
    ALLOCATOR_CLIST_SLICE, ALLOCATOR_TEXT_CASE, ALLOCATOR_TEXT_CONCAT, ALLOCATOR_TEXT_FINISH,
    ALLOCATOR_TEXT_JOIN, ALLOCATOR_TEXT_QUOTE, ALLOCATOR_TEXT_REPEAT, ALLOCATOR_TEXT_REPLACE,
    ALLOCATOR_TEXT_SLICE, ALLOCATOR_TEXT_SPLIT, ALLOCATOR_TEXT_SPLIT_WHITESPACE,
    ALLOCATOR_TEXT_SPLITLINES, ALLOCATOR_TEXT_STRIP,
};
use crate::hash::HashSet;
use crate::op::{CallableSig, Op, Ty};
use crate::threading::{self, CallableTargets, ThreadOp};

/// Per-kind worker contract: a producer, the public owner it returns, and the
/// operations on an existing owner which can call back without destroying it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner {
    Clist,
    Text,
}

impl Owner {
    fn producer(op: &Op) -> Option<Self> {
        match crate::foreign::allocator_opcode(op)? {
            ALLOCATOR_TEXT_FINISH
            | ALLOCATOR_TEXT_JOIN
            | ALLOCATOR_TEXT_CONCAT
            | ALLOCATOR_TEXT_SPLIT
            | ALLOCATOR_TEXT_SPLIT_WHITESPACE
            | ALLOCATOR_TEXT_SPLITLINES
            | ALLOCATOR_TEXT_CASE
            | ALLOCATOR_TEXT_QUOTE
            | ALLOCATOR_TEXT_STRIP
            | ALLOCATOR_TEXT_REPLACE
            | ALLOCATOR_TEXT_REPEAT
            | ALLOCATOR_TEXT_SLICE => Some(Self::Text),
            ALLOCATOR_CLIST_SLICE => Some(Self::Clist),
            _ => None,
        }
    }

    fn of(ty: &Ty) -> Option<Self> {
        match ty {
            Ty::Str => Some(Self::Text),
            Ty::Clist(_) => Some(Self::Clist),
            _ => None,
        }
    }

    fn growth(op: &Op) -> Option<Self> {
        match op {
            Op::Collection(CollectionOp::Clist(ClistOp::Reserve(_) | ClistOp::Append(_))) => {
                Some(Self::Clist)
            }
            _ => None,
        }
    }
}

#[derive(Default)]
pub(super) struct Providers {
    owners: Vec<Owner>,
    state_owns_selected: bool,
    mutexes: Vec<Ty>,
    targets: CallableTargets,
}

/// Which worker inputs can carry a caller's erased provider state.
#[derive(Default)]
pub(super) struct Sources {
    allocator: bool,
    releasable_allocator: bool,
    owners: Vec<Owner>,
}

impl Sources {
    pub fn any(&self) -> bool {
        self.allocator || !self.owners.is_empty()
    }
}

/// A callback is always a potential wait. It can lock a *caller-reachable*
/// mutex only if its allocator (at construction) or owner came in as input.
#[derive(Default, Clone, Copy)]
pub(super) struct WorkerEffects {
    pub waits: bool,
    pub provider_lock: bool,
}

impl Providers {
    pub fn new(ops: &[Op]) -> Self {
        // Checking sees even unused monomorphic std method bodies. Importing
        // std.alloc alone must not count as constructing a selected owner.
        let mut registered = false;
        threading::walk(ops, &mut |op| {
            registered |= matches!(op, Op::Thread(ThreadOp::AllocatorProvider(_)));
        });
        if !registered {
            return Self::default();
        }
        let dead = crate::op::reachability::dead_definitions(ops, &[], false);
        let mut owners = Vec::new();
        let mut states = Vec::new();
        let mut addresses = Vec::new();
        for op in ops {
            if matches!(op, Op::DefineFn(name, _) if dead.contains(name)) {
                continue;
            }
            threading::walk(std::slice::from_ref(op), &mut |op| {
                if let Op::FunctionAddress(name, sig) = op {
                    addresses.push((name.as_str(), sig));
                }
                if let Some(kind) = Owner::producer(op)
                    && !owners.contains(&kind)
                {
                    owners.push(kind);
                }
                if let Op::Thread(ThreadOp::AllocatorProvider(provider)) = op {
                    states.push(provider.state.clone());
                }
            });
        }
        if states.is_empty() || owners.is_empty() {
            return Self::default();
        }
        // A last Allocator drop destroys its erased state. The owner and the
        // callback mutex can belong to different registrations: a selected str
        // or clist retains its inner allocator. Bound both by emitted types.
        let mut state_owns_selected = false;
        visit_types(&states, Ty::managed, |ty| {
            state_owns_selected |= Owner::of(ty).is_some_and(|kind| owners.contains(&kind));
        });
        let mutexes = threading::mutexes(states);
        Self {
            owners,
            state_owns_selected,
            targets: if mutexes.is_empty() {
                CallableTargets::default()
            } else {
                CallableTargets::new(addresses)
            },
            mutexes,
        }
    }

    pub fn sources(&self, inputs: &[Ty]) -> Sources {
        let mut sources = Sources::default();
        if self.mutexes.is_empty() {
            return sources;
        }
        visit_types(
            inputs,
            |_| true,
            |ty| {
                sources.allocator |= is_allocator(ty);
                if let Some(kind) = Owner::of(ty)
                    && self.owners.contains(&kind)
                    && !sources.owners.contains(&kind)
                {
                    sources.owners.push(kind);
                }
            },
        );
        if self.state_owns_selected && sources.allocator {
            // Even through a shared loan, a receiver can yield a message and a
            // lock permits replacing its guarded value. A sender cannot yield
            // or drain a message: stop at that edge in both ownership walks.
            let mut transferable = inputs.to_vec();
            visit_types(
                inputs,
                |ty| !matches!(ty, Ty::Channel(_, true)),
                |ty| {
                    if matches!(ty, Ty::Channel(_, false)) || crate::mutex::class(ty).is_some() {
                        threading::contained(ty, &mut transferable);
                    }
                },
            );
            visit_types(
                &transferable,
                |ty| {
                    !matches!(
                        ty,
                        Ty::Ref(_, false) | Ty::View(_, false) | Ty::Channel(_, true)
                    )
                },
                |ty| {
                    sources.releasable_allocator |= is_allocator(ty);
                },
            );
        }
        sources
    }

    /// The data-only Callable exception for pool shutdown cannot apply to an
    /// owning argument/result that can carry a caller's lock into a job. A
    /// shared loan doesn't transfer the owner; an exclusive loan allows
    /// replacement or growth. Plain Allocator itself already
    /// has a destructor and so is not certified as data-only.
    pub fn signature_carries_lock(&self, sig: &CallableSig) -> bool {
        if self.mutexes.is_empty() {
            return false;
        }
        let roots = sig
            .inputs
            .iter()
            .chain(&sig.output)
            .cloned()
            .collect::<Vec<_>>();
        let mut carries = false;
        visit_types(
            &roots,
            |ty| !matches!(ty, Ty::Ref(_, false) | Ty::View(_, false)),
            |ty| {
                carries |= Owner::of(ty).is_some_and(|owner| self.owners.contains(&owner));
            },
        );
        carries
    }

    pub fn extend_mutexes(&self, locks: &mut Vec<Ty>) {
        for ty in &self.mutexes {
            if !locks.contains(ty) {
                locks.push(ty.clone());
            }
        }
    }

    pub fn operation(
        &self,
        op: &Op,
        sources: &Sources,
        effects: &mut WorkerEffects,
        functions: &mut Vec<String>,
    ) {
        if self.owners.is_empty() {
            return;
        }
        // An indirect call is already a wait. To learn whether it reaches
        // an allocator callback, use the same closed targets as certification.
        if sources.any()
            && let Op::CallIndirect(sig) | Op::TailCallIndirect(sig) = op
            && let Some(names) = self.targets.names(sig)
        {
            functions.extend(names.iter().cloned());
        }
        if let Some(kind) = Owner::producer(op)
            && self.owners.contains(&kind)
        {
            effects.waits = true;
            effects.provider_lock |= sources.allocator;
        }
        if let Some(kind) = Owner::growth(op)
            && self.owners.contains(&kind)
        {
            effects.waits = true;
            effects.provider_lock |= sources.owners.contains(&kind);
        }
    }

    /// Generated drops needn't appear as a call (or even a DropLocal). Cover
    /// parameters, locals, owned operands/results and nested payloads; a loan
    /// does not destroy the referent. Growth through a loan is checked above.
    pub fn cleanup(&self, roots: &[Ty], sources: &Sources, effects: &mut WorkerEffects) {
        if !self.owners.is_empty() {
            visit_types(roots, Ty::managed, |ty| {
                if self.state_owns_selected && is_allocator(ty) {
                    effects.waits = true;
                    // A share made from &Allocator cannot be the last owner.
                    effects.provider_lock |= sources.releasable_allocator;
                }
                if let Some(kind) = Owner::of(ty)
                    && self.owners.contains(&kind)
                {
                    effects.waits = true;
                    effects.provider_lock |= sources.owners.contains(&kind);
                }
            });
        }
    }
}

fn is_allocator(ty: &Ty) -> bool {
    matches!(ty, Ty::Class(c) if c.name.as_ref() == "std.alloc.Allocator")
}

#[expect(
    clippy::mutable_key_type,
    reason = "nominal identities exclude definition cells"
)]
fn visit_types(roots: &[Ty], through: impl Fn(&Ty) -> bool, mut visit: impl FnMut(&Ty)) {
    let mut pending = roots.to_vec();
    let mut seen = HashSet::default();
    while let Some(ty) = pending.pop() {
        if !through(&ty) || !seen.insert(ty.clone()) {
            continue;
        }
        visit(&ty);
        threading::contained(&ty, &mut pending);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pruned_std_bodies_do_not_supply_either_hidden_cleanup_fact() {
        // Model both operations in pre-lowered std bodies. clist's real method
        // is generic, so an unused import alone wouldn't exercise pruning.
        let source = r#"import std.alloc as alloc
class Row:
    n: i64
pub class Holding:
    rows: clist[Row]
    pub def allocate(self: &mut Holding, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        alloc.system().block(request)
    pub def release(self: &mut Holding, storage: alloc.ByteBlock) -> ():
        drop(storage)
def register_owner() -> ():
    root = alloc.system()
    drop(alloc.allocator(&root, Holding(clist[Row]())))
def produce() -> ():
    rows = clist[Row]()
    drop(alloc.system().slice_clist(&rows))
def main() -> ():
    pass
"#;
        let program = crate::frontend::compile(source, &mut Default::default()).unwrap();
        for produce in [false, true] {
            for register in [false, true] {
                let mut ops = program.ops.clone();
                for op in &mut ops {
                    if let Op::DefineFn(name, f) = op {
                        std::sync::Arc::make_mut(&mut f.origins).standard_library = name != "main";
                    }
                }
                if produce {
                    ops.push(Op::Call("produce".into()));
                }
                if register {
                    ops.push(Op::Call("register_owner".into()));
                }
                assert_eq!(
                    Providers::new(&ops).state_owns_selected,
                    produce && register,
                    "produce={produce}, register={register}"
                );
            }
        }
    }
}
