//! A selected allocation retains provider state that its public type doesn't
//! mention. Keep this join analysis separate from structural worker eligibility.
use crate::clist::ClistOp;
use crate::collection::CollectionOp;
use crate::collection_opcodes::ALLOCATOR_CLIST_SLICE;
use crate::hash::HashSet;
use crate::op::{CallableSig, Op, Ty};
use crate::threading::{self, CallableTargets, ThreadOp};

/// Per-kind worker contract: a producer, the public owner it returns, and the
/// operations on an existing owner which can call back without destroying it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner {
    Clist,
}

impl Owner {
    fn producer(op: &Op) -> Option<Self> {
        match crate::foreign::allocator_opcode(op)? {
            ALLOCATOR_CLIST_SLICE => Some(Self::Clist),
            _ => None,
        }
    }

    fn of(ty: &Ty) -> Option<Self> {
        match ty {
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
    mutexes: Vec<Ty>,
    targets: CallableTargets,
}

/// Which worker inputs can carry a caller's erased provider state.
#[derive(Default)]
pub(super) struct Sources {
    allocator: bool,
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
        let mut owners = Vec::new();
        let mut states = Vec::new();
        let mut addresses = Vec::new();
        threading::walk(ops, &mut |op| {
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
        if states.is_empty() || owners.is_empty() {
            return Self::default();
        }
        let mutexes = threading::mutexes(states);
        Self {
            owners,
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
        if !self.mutexes.is_empty() {
            visit_types(
                inputs,
                |_| true,
                |ty| {
                    sources.allocator |=
                        matches!(ty, Ty::Class(c) if c.name.as_ref() == "std.alloc.Allocator");
                    if let Some(kind) = Owner::of(ty)
                        && self.owners.contains(&kind)
                        && !sources.owners.contains(&kind)
                    {
                        sources.owners.push(kind);
                    }
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
