//! `std.sync.Mutex[T]`: a value and a futex state word, entered with
//! `with mutex.lock() as value:`. The type is the generic class declared in
//! stdlib/std/sync.plenty, so layout, moves, destruction, and worker
//! eligibility are ordinary class behavior. See
//! book/src/design/facilities/03-concurrency/mutex.md.
use crate::nominal::Nominal;
use crate::op::Ty;
use crate::record::ClassType;
use std::sync::Arc;

/// The generic class in stdlib/std/sync.plenty; its instances are named
/// `std.sync.Mutex[T]`.
pub const CLASS: &str = "std.sync.Mutex";
const PREFIX: &str = "std.sync.Mutex[";
/// Field order of that class.
pub const STATE: usize = 0;
pub const VALUE: usize = 1;

/// The class of a `Mutex[T]`, if `ty` is one.
pub fn class(ty: &Ty) -> Option<&Nominal<ClassType>> {
    match ty {
        Ty::Class(class) if class.name.starts_with(PREFIX) => Some(class),
        _ => None,
    }
}

/// The guarded type `T` of a `Mutex[T]` class.
pub fn element(class: &Nominal<ClassType>) -> Ty {
    class.get().fields[VALUE].1.clone()
}

fn state() -> Ty {
    Ty::Ref(Arc::new(crate::atomic::ty(Ty::U32)), false)
}

/// Lock and unlock address the state word through a shared reference; the
/// slot is the guard local that holds the mutex reference while locked, so
/// the ownership checker can see which locks a join would wait under.
#[derive(Clone, Debug, PartialEq)]
pub enum MutexOp {
    /// Acquire: an inline compare-exchange from 0 to 1, else the runtime's
    /// futex wait loop.
    Lock(u8),
    /// Release: an inline exchange to 0, then a futex wake if the previous
    /// state recorded waiters.
    Unlock(u8),
    /// The guarded value's shared reference, made exclusive by the lock.
    Exclusive(Ty),
    /// Move the value out of an owned mutex. Unlike a field read, nothing is
    /// released: the value keeps its own cleanup, and the state word has none.
    IntoInner(Nominal<ClassType>),
}

impl MutexOp {
    pub fn signature(&self) -> (Vec<Ty>, Ty) {
        match self {
            Self::Lock(_) | Self::Unlock(_) => (vec![state()], Ty::Unit),
            Self::Exclusive(t) => (
                vec![Ty::Ref(Arc::new(t.clone()), false)],
                Ty::Ref(Arc::new(t.clone()), true),
            ),
            Self::IntoInner(class) => (vec![Ty::Class(class.clone())], element(class)),
        }
    }
}
