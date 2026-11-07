//! `std.sync.Mutex` surface: `with mutex.lock() as value:`,
//! `with mutex.lock_now()? as value:`, and `into_inner`. A lock is a `with`
//! manager only, so its unlock is ordinary context cleanup on every exit and
//! the guarded reference never outlives the block.
use super::*;
use crate::mutex::{MutexOp, STATE, VALUE};
use crate::nominal::Nominal;
use crate::record::{ClassOp, ClassType};
use contexts::{Context, Kind};

pub(super) const FORMAT: &str =
    "mutexes cannot be formatted; format a value read inside `with mutex.lock() as value:`";
const LOCK_FORM: &str = "lock() is valid only as a `with` manager: `with mutex.lock() as value:`";
const LOCK_NOW_FORM: &str =
    "lock_now() is valid only as a propagated `with` manager: `with mutex.lock_now()? as value:`";
const LOCK_ERROR: &str = "std.sync.LockError";

/// How a lock manager acquires its mutex.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Acquire {
    /// `lock()` waits for the lock.
    Wait,
    /// `lock_now()?` fails with `LockError.WouldBlock` instead of waiting.
    Now,
}

impl Acquire {
    pub(super) fn method(self) -> &'static str {
        match self {
            Self::Wait => "lock",
            Self::Now => "lock_now",
        }
    }
}

/// The arguments and acquisition of a lock manager.
#[derive(Clone, Copy)]
pub(super) struct LockCall<'a> {
    pub(super) args: &'a [Expr],
    pub(super) acquire: Acquire,
}

/// `base.lock(args)` or `base.lock_now(args)?`, before its receiver is known
/// to be a mutex.
pub(super) fn lock_expression(manager: &Expr) -> Option<(&Expr, LockCall<'_>)> {
    let call = |args, acquire| LockCall { args, acquire };
    match &ungroup(manager).kind {
        Expression::Method(base, name, args, _) if name == "lock" => {
            Some((base, call(args, Acquire::Wait)))
        }
        Expression::Try(inner) => match &ungroup(inner).kind {
            Expression::Method(base, name, args, _) if name == "lock_now" => {
                Some((base, call(args, Acquire::Now)))
            }
            _ => None,
        },
        _ => None,
    }
}

/// The written place a lock names, compared to find a relock in its own block.
/// Dereferences are dropped: `*r` and `r` lock the same mutex. A computed
/// index is `[?]`, which matches no other place; a tuple index is its resolved
/// position, so `t[-1]` and `t[1]` of a pair match.
fn place_key(lower: &Lower<'_>, e: &Expr) -> Option<String> {
    match &ungroup(e).kind {
        Expression::Name(name) => Some(name.clone()),
        Expression::Member(base, field, _) => Some(format!("{}.{field}", place_key(lower, base)?)),
        Expression::Index(base, index) => {
            let tuple = match lower.place_type(base) {
                Some(Ty::Enum(t)) if t.tuple() => {
                    slices::tuple_position(index, t.get().variants[0].fields.len()).ok()
                }
                _ => None,
            };
            match (tuple, &ungroup(index).kind) {
                (Some(i), _) => Some(format!("{}[{i}]", place_key(lower, base)?)),
                (None, Expression::Number(n)) => Some(format!("{}[{n}]", place_key(lower, base)?)),
                _ => Some(format!("{}[?]", place_key(lower, base)?)),
            }
        }
        Expression::Unary(op, base) if op == "*" => place_key(lower, base),
        _ => None,
    }
}

/// Mutexes reach an atomic state word, so they share `Facts::atomic` with
/// atomics; diagnostics name whichever the value holds.
pub(super) fn holds_mutex(ty: &Ty) -> bool {
    !crate::threading::mutexes([ty.clone()]).is_empty()
}

fn other_method(name: &str, at: &Token) -> Box<dyn Error> {
    at.error(format!(
        "Mutex has no method `{name}`; it supports `with mutex.lock() as value:`, `with mutex.lock_now()? as value:`, and into_inner()"
    ))
}

impl Lower<'_> {
    /// Whether the mutex at `e` could also be named by a different written
    /// place: it is reached through a reference, a shared handle, or a
    /// computed index.
    fn aliasable(&self, e: &Expr) -> bool {
        match &ungroup(e).kind {
            Expression::Name(name) => self
                .names
                .get(name)
                .is_none_or(|local| matches!(local.ty, Ty::Ref(..) | Ty::Shared(_))),
            Expression::Member(base, _, _) => self.aliasable(base),
            Expression::Index(base, index) => {
                let literal = match self.place_type(base) {
                    Some(Ty::Enum(t)) if t.tuple() => true,
                    _ => matches!(ungroup(index).kind, Expression::Number(_)),
                };
                !literal || self.aliasable(base)
            }
            Expression::Unary(op, base) if op == "*" => {
                matches!(self.place_type(base), Some(Ty::Shared(_))) || self.aliasable(base)
            }
            _ => true,
        }
    }

    pub(super) fn with_lock(
        &mut self,
        manager: &Expr,
        base: &Expr,
        LockCall { args, acquire }: LockCall<'_>,
        name: Option<&str>,
        body: &[Stmt],
        ops: &mut Vec<Op>,
    ) -> Result<BlockResult> {
        let Some(class) = self
            .place_type(base)
            .as_ref()
            .and_then(crate::mutex::class)
            .cloned()
        else {
            unreachable!("caller checked the receiver type")
        };
        let method = acquire.method();
        if !args.is_empty() {
            return Err(manager.at.error(format!("{method} takes no arguments")));
        }
        if generators::yields(body) {
            return Err(manager.at.error(
                "yield inside a mutex lock block would keep the lock held while the generator is suspended; release the lock before yielding",
            ));
        }
        let place = place_key(self, base).ok_or_else(|| {
            base.at.error(format!(
                "{method}() requires a named mutex or a field of one"
            ))
        })?;
        let element = crate::mutex::element(&class);
        let aliasable = self.aliasable(base);
        for context in &self.contexts {
            let Kind::Lock(held, guarded, other) = &context.kind else {
                continue;
            };
            if *held == place && !place.contains("[?]") {
                let outcome = match acquire {
                    Acquire::Wait => "this lock would wait for itself forever",
                    Acquire::Now => "this lock_now() always fails",
                };
                return Err(manager.at.error(format!(
                    "`{place}` is already locked by an enclosing `with` block, so {outcome}; use the existing binding, or release the lock before relocking"
                )));
            }
            // References, shared handles, and computed indices can name one
            // mutex through different places; there, mutexes are told apart
            // only by type. Distinct owned places are distinct mutexes. A
            // `lock_now()` cannot wait, so it may name the held mutex: it
            // then fails with `WouldBlock`.
            if acquire == Acquire::Wait && *guarded == element && (aliasable || *other) {
                return Err(manager.at.error(format!(
                    "this locks `{place}` while an enclosing `with` block holds a lock on `{held}`; both are a `Mutex[{element}]`, and a reference, shared handle, or computed index can make them one mutex, so this lock could wait for itself forever; lock them one at a time, or wrap one value in a distinct class"
                )));
            }
        }
        let saved = self.names.clone();
        let start = self.locals.len();
        let place_start = ops.len();
        let (reference, mutex_loan) = self.borrow(base, false, ops)?;
        // A computed index evaluates like any manager expression: an endpoint
        // it moves before a nested wait is held through that wait (issue #350).
        self.building(&manager.at, place_start, ops, false);
        let guard = self.slot(reference, &manager.at)?;
        ops.push(Op::StoreLocal(guard));
        ops.extend(state_word(&class, guard));
        match acquire {
            Acquire::Wait => ops.extend([Op::Mutex(MutexOp::Lock(guard)), Op::Drop]),
            Acquire::Now => self.lock_now(manager, guard, ops)?,
        }
        // The value's exclusive loan is rooted at the guard, not the mutex,
        // which this block and the workers keep borrowed shared.
        let value_loan = self.new_loan(guard, true, None, ops);
        ops.extend([
            Op::LoadLocal(guard),
            Op::Class(ClassOp::FieldRef(class.clone(), VALUE, false)),
            Op::Mutex(MutexOp::Exclusive(element.clone())),
        ]);
        if let Some(name) = name {
            if self.names.contains_key(name) {
                return Err(manager.at.error(format!("duplicate binding `{name}`")));
            }
            let ty = Ty::Ref(Arc::new(element.clone()), true);
            let target = self.slot(ty.clone(), &manager.at)?;
            ops.push(Op::StoreLocal(target));
            self.bind_reference(target, value_loan, false, ops);
            self.bind(
                name.into(),
                Local {
                    slot: target,
                    ty,
                    mutable: false,
                },
            );
        } else {
            ops.push(Op::Drop);
        }
        let mut exit = vec![Op::UseLoan(value_loan)];
        exit.extend(state_word(&class, guard));
        exit.extend([
            Op::Mutex(MutexOp::Unlock(guard)),
            Op::Drop,
            Op::UseLoan(mutex_loan),
        ]);
        self.contexts.push(Context {
            slot: guard,
            exit,
            loan: None,
            kind: Kind::Lock(place, element.clone(), aliasable),
            at: site(&manager.at, true),
        });
        let result = self.block_inner(body, ops, &Tail::Discard)?;
        if matches!(result, BlockResult::Continues(_)) {
            self.cleanup(start, ops);
        }
        self.contexts.pop();
        self.names = saved;
        Ok(result)
    }

    /// Attempt the lock whose state word is on the stack, and propagate
    /// `LockError.WouldBlock` with `?` when another holder has it. The block's
    /// context is not pushed yet, so a failure unwinds only enclosing ones.
    /// Ownership recognises this `LockNow`, branch, `Try` sequence to know
    /// that the guard holds no lock on the failing path.
    fn lock_now(&mut self, manager: &Expr, guard: u8, ops: &mut Vec<Op>) -> Result<()> {
        use crate::sum::EnumOp;
        let target = self.propagation_target(manager)?;
        let Some(Some(Ty::Enum(error))) = lookup_type(LOCK_ERROR, self.aliases) else {
            unreachable!("std.sync declares LockError beside Mutex")
        };
        let would_block = error
            .get()
            .variants
            .iter()
            .position(|variant| variant.name == "WouldBlock")
            .expect("LockError has WouldBlock");
        let Ty::Enum(source) = crate::sum::result(Ty::Unit, Ty::Enum(error.clone())) else {
            unreachable!("a Result is an enum")
        };
        let target = self.check_propagation(manager, &source, target, None)?;
        ops.push(Op::Mutex(MutexOp::LockNow(guard)));
        ops.push(branch(
            vec![Op::PushUnit, Op::Enum(EnumOp::New(source.clone(), 0))],
            vec![
                Op::Enum(EnumOp::New(error, would_block)),
                Op::Enum(EnumOp::New(source.clone(), 1)),
            ],
        ));
        let mut cleanup = Vec::new();
        if !self.contexts.is_empty() {
            self.cleanup(0, &mut cleanup);
        }
        ops.extend([
            Op::Try {
                source: Box::new(source),
                target: Box::new(target),
                cleanup: cleanup.into(),
            },
            Op::Drop,
        ]);
        Ok(())
    }

    /// A method on a mutex place.
    pub(super) fn mutex_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let Some(class) = self
            .place_type(base)
            .as_ref()
            .and_then(crate::mutex::class)
            .cloned()
        else {
            unreachable!("caller checked the receiver type")
        };
        match name {
            "lock" => Err(base.at.error(LOCK_FORM)),
            "lock_now" => Err(base.at.error(LOCK_NOW_FORM)),
            "into_inner" => {
                if !args.is_empty() {
                    return Err(base.at.error("into_inner takes no arguments"));
                }
                if matches!(&ungroup(base).kind, Expression::Name(n)
                    if self.names.get(n).is_some_and(|local| matches!(local.ty, Ty::Ref(..))))
                {
                    return Err(base
                        .at
                        .error("into_inner consumes an owned mutex, not a reference"));
                }
                let actual = self.value(base, ops)?;
                self.same(Some(actual), Some(Ty::Class(class.clone())), &base.at)?;
                Ok(Some(into_inner(&class, ops)))
            }
            _ => Err(other_method(name, &base.at)),
        }
    }

    /// A method on an owned temporary mutex, already on the stack.
    pub(super) fn temporary_mutex_method(
        &mut self,
        base: &Expr,
        class: &Nominal<ClassType>,
        name: &str,
        args: &[Expr],
        loans: &[usize],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        match name {
            "lock" => Err(base.at.error(LOCK_FORM)),
            "lock_now" => Err(base.at.error(LOCK_NOW_FORM)),
            "into_inner" if args.is_empty() && loans.is_empty() => Ok(Some(into_inner(class, ops))),
            "into_inner" if !args.is_empty() => Err(base.at.error("into_inner takes no arguments")),
            "into_inner" => Err(base
                .at
                .error("into_inner consumes an owned mutex, not a reference")),
            _ => Err(other_method(name, &base.at)),
        }
    }
}

/// The state word's shared reference, from the mutex reference in `guard`.
fn state_word(class: &Nominal<ClassType>, guard: u8) -> [Op; 2] {
    [
        Op::LoadLocal(guard),
        Op::Class(ClassOp::FieldRef(class.clone(), STATE, false)),
    ]
}

/// Ownership of the mutex proves no lock is held, so its value moves out
/// without one.
fn into_inner(class: &Nominal<ClassType>, ops: &mut Vec<Op>) -> Ty {
    ops.push(Op::Mutex(MutexOp::IntoInner(class.clone())));
    crate::mutex::element(class)
}
