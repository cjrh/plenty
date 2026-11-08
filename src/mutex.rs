//! `std.sync.Mutex[T]`: a value and a futex state word, entered with
//! `with mutex.lock() as value:`, without waiting by
//! `with mutex.lock_now()? as value:`, or once a predicate holds by
//! `with mutex.lock_when(&ready, &deadline, &cancel)? as value:`. The type is the generic class declared in
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
/// Field order of that class. The sequence word counts unlocks that found
/// condition waiters; `lock_when` sleeps on it.
pub const STATE: usize = 0;
pub const SEQUENCE: usize = 1;
pub const VALUE: usize = 2;

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

fn word() -> Ty {
    Ty::Ref(Arc::new(crate::atomic::ty(Ty::U32)), false)
}

/// Lock and unlock address the state word, and unlock and wait the sequence
/// word, through shared references; the slot is the guard local that holds
/// the mutex reference while locked, so the ownership checker can see which
/// locks a join would wait under. State bits 0-1 are 0 unlocked, 1 locked
/// and 2 locked with possible lock waiters; bit 2 records condition waiters.
#[derive(Clone, Debug, PartialEq)]
pub enum MutexOp {
    /// Acquire: an inline compare-exchange from 0 to 1, else the runtime's
    /// futex wait loop.
    Lock(u8),
    /// `lock_now()`: the same compare-exchange, without waiting. Pushes
    /// whether it acquired the lock; the frontend turns that into a
    /// `Result[(), LockError]` and propagates it with `Try`, so the guard
    /// holds a lock only on success.
    LockNow(u8),
    /// Release: an inline exchange to 0, then the runtime's wake if the
    /// previous state recorded lock or condition waiters. Condition waiters
    /// are woken by every such unlock, so a change made under the lock
    /// notifies them without a separate call.
    Unlock(u8),
    /// One `lock_when` wait, with the lock held: pops the state and sequence
    /// words and the deadline's nanoseconds. Pushes true without waiting if
    /// the deadline has passed. Otherwise releases the lock, sleeps until an
    /// unlock notifies condition waiters or at most 10 ms pass, relocks, and
    /// pushes false. The name is the predicate's function, whose body the
    /// ownership checker searches for waits; `None` when it has no visible
    /// body.
    Wait(u8, Option<Arc<str>>),
    /// The guarded value's shared reference, made exclusive by the lock.
    Exclusive(Ty),
    /// Move the value out of an owned mutex. Unlike a field read, nothing is
    /// released: the value keeps its own cleanup, and the state word has none.
    IntoInner(Nominal<ClassType>),
}

impl MutexOp {
    pub fn signature(&self) -> (Vec<Ty>, Ty) {
        match self {
            Self::Lock(_) => (vec![word()], Ty::Unit),
            Self::Unlock(_) => (vec![word(), word()], Ty::Unit),
            Self::LockNow(_) => (vec![word()], Ty::Bool),
            Self::Wait(..) => (vec![word(), word(), Ty::U64], Ty::Bool),
            Self::Exclusive(t) => (
                vec![Ty::Ref(Arc::new(t.clone()), false)],
                Ty::Ref(Arc::new(t.clone()), true),
            ),
            Self::IntoInner(class) => (vec![Ty::Class(class.clone())], element(class)),
        }
    }
}
