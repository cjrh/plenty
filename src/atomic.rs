//! `std.atomic`: inline atomic integers and booleans. Every operation lowers
//! to one native atomic instruction (or a compare-exchange loop) on the
//! payload word of the value's slot; none allocates or calls the runtime.
//! See book/src/design/stdlib/atomic.md.
use crate::op::Ty;
use std::sync::Arc;

/// Resolved spellings of the module's compiler-provided exports. They contain
/// dots, so source code reaches them only through an import of `std.atomic`.
pub const ATOMIC: &str = "std.atomic.Atomic";
pub const FENCE: &str = "std.atomic.fence";
/// The `Ordering` enum declared in stdlib/std/atomic.plenty.
pub const ORDERING: &str = "std.atomic.Ordering";

/// Element types an `Atomic[T]` may hold: those with a native atomic width.
pub fn supported(ty: &Ty) -> bool {
    ty.is_int() || *ty == Ty::Bool
}

pub fn ty(element: Ty) -> Ty {
    Ty::Atomic(Arc::new(element))
}

/// One `Ordering` variant. Lowering currently strengthens every ordering to
/// sequential consistency, so only validation reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ordering {
    Relaxed,
    Release,
    Acquire,
    AcqRel,
    SeqCst,
}

impl Ordering {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "Relaxed" => Self::Relaxed,
            "Release" => Self::Release,
            "Acquire" => Self::Acquire,
            "AcqRel" => Self::AcqRel,
            "SeqCst" => Self::SeqCst,
            _ => return None,
        })
    }
}

/// Where an ordering argument appears, which fixes the orderings it allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderingUse {
    Load,
    Store,
    Modify,
    Failure,
    Fence,
}

impl OrderingUse {
    pub fn allowed(self) -> &'static [Ordering] {
        use Ordering::*;
        match self {
            Self::Load | Self::Failure => &[Relaxed, Acquire, SeqCst],
            Self::Store => &[Relaxed, Release, SeqCst],
            Self::Modify => &[Relaxed, Release, Acquire, AcqRel, SeqCst],
            Self::Fence => &[Release, Acquire, AcqRel, SeqCst],
        }
    }
}

/// Read-modify-write operations with one native instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modify {
    Swap,
    WrappingAdd,
    WrappingSub,
    And,
    Or,
    Xor,
    Nand,
    Max,
    Min,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AtomicOp {
    New(Ty),
    IntoInner(Ty),
    Load(Ty),
    Store(Ty),
    Modify(Ty, Modify),
    /// `fetch_add` (false) or `fetch_sub` (true): traps on overflow before
    /// anything is stored, so no thread observes a wrapped value.
    Checked(Ty, bool),
    CompareExchange(Ty),
    Fence,
}

impl AtomicOp {
    pub fn signature(&self) -> (Vec<Ty>, Ty) {
        let shared = |t: &Ty| Ty::Ref(Arc::new(ty(t.clone())), false);
        match self {
            Self::New(t) => (vec![t.clone()], ty(t.clone())),
            Self::IntoInner(t) => (vec![ty(t.clone())], t.clone()),
            Self::Load(t) => (vec![shared(t)], t.clone()),
            Self::Store(t) => (vec![shared(t), t.clone()], Ty::Unit),
            Self::Modify(t, _) | Self::Checked(t, _) => (vec![shared(t), t.clone()], t.clone()),
            Self::CompareExchange(t) => (
                vec![shared(t), t.clone(), t.clone()],
                crate::sum::result(t.clone(), t.clone()),
            ),
            Self::Fence => (vec![], Ty::Unit),
        }
    }
}
