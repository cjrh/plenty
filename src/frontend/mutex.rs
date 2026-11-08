//! `std.sync.Mutex` surface: `with mutex.lock() as value:`,
//! `with mutex.lock_now()? as value:`,
//! `with mutex.lock_when(ready, &deadline, &cancel)? as value:`, and
//! `into_inner`. A lock is a `with`
//! manager only, so its unlock is ordinary context cleanup on every exit and
//! the guarded reference never outlives the block.
use super::*;
use crate::mutex::{MutexOp, SEQUENCE, STATE, VALUE};
use crate::nominal::Nominal;
use crate::record::{ClassOp, ClassType};
use contexts::{Context, Kind};

pub(super) const FORMAT: &str =
    "mutexes cannot be formatted; format a value read inside `with mutex.lock() as value:`";
const LOCK_FORM: &str = "lock() is valid only as a `with` manager: `with mutex.lock() as value:`";
const LOCK_NOW_FORM: &str =
    "lock_now() is valid only as a propagated `with` manager: `with mutex.lock_now()? as value:`";
const LOCK_WHEN_FORM: &str = "lock_when() is valid only as a propagated `with` manager: `with mutex.lock_when(ready, &deadline, &cancel)? as value:`";
const LOCK_ERROR: &str = "std.sync.LockError";
const DEADLINE: &str = "std.time.Deadline";
/// The binding for a `lock_when` value written without `as`; source names
/// cannot contain a space.
const HIDDEN_VALUE: &str = "lock value";

/// How a lock manager acquires its mutex.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Acquire {
    /// `lock()` waits for the lock.
    Wait,
    /// `lock_now()?` fails with `LockError.WouldBlock` instead of waiting.
    Now,
    /// `lock_when(ready, &deadline, &cancel)?` waits for the lock and then
    /// for `ready(value)`, failing with `LockError.TimedOut` or `Cancelled`.
    When,
}

impl Acquire {
    pub(super) fn method(self) -> &'static str {
        match self {
            Self::Wait => "lock",
            Self::Now => "lock_now",
            Self::When => "lock_when",
        }
    }
}

/// The arguments and acquisition of a lock manager.
#[derive(Clone, Copy)]
pub(super) struct LockCall<'a> {
    pub(super) args: &'a [Expr],
    pub(super) acquire: Acquire,
}

/// `base.lock(args)`, `base.lock_now(args)?`, or `base.lock_when(args)?`,
/// before its receiver is known to be a mutex.
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
            Expression::Method(base, name, args, _) if name == "lock_when" => {
                Some((base, call(args, Acquire::When)))
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
        "Mutex has no method `{name}`; it supports `with mutex.lock() as value:`, `with mutex.lock_now()? as value:`, `with mutex.lock_when(ready, &deadline, &cancel)? as value:`, and into_inner()"
    ))
}

/// The arguments of `lock_when(ready, &deadline, &cancel)`.
struct When<'a> {
    /// The predicate's name: a function, or a local callable or reference
    /// to one.
    ready: &'a Expr,
    /// The places of the deadline and token, without their `&`.
    deadline: &'a Expr,
    cancel: &'a Expr,
}

/// `e` without one leading shared borrow.
fn unborrowed(e: &Expr) -> &Expr {
    match &ungroup(e).kind {
        Expression::Unary(op, inner) if op == "&" => inner,
        _ => e,
    }
}

/// An `Err` of `LockError.variant`, propagated with `?`.
struct Failure {
    error: Nominal<crate::sum::EnumType>,
    variant: usize,
    source: Nominal<crate::sum::EnumType>,
    target: Nominal<crate::sum::EnumType>,
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
        let when = match acquire {
            Acquire::When => Some(self.when_arguments(manager, args)?),
            _ if !args.is_empty() => {
                return Err(manager.at.error(format!("{method} takes no arguments")));
            }
            _ => None,
        };
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
        if acquire == Acquire::When
            && let Some(Kind::Lock(held, ..)) = self
                .contexts
                .iter()
                .map(|context| &context.kind)
                .find(|kind| matches!(kind, Kind::Lock(..)))
        {
            return Err(manager.at.error(format!(
                "lock_when() would wait while this function holds a lock on `{held}`, keeping that mutex locked for as long as it waits; release it first, or wait for one mutex that guards both values"
            )));
        }
        for context in &self.contexts {
            let Kind::Lock(held, guarded, other) = &context.kind else {
                continue;
            };
            if *held == place && !place.contains("[?]") {
                let outcome = match acquire {
                    Acquire::Wait | Acquire::When => "this lock would wait for itself forever",
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
        let deadline = match &when {
            Some(when) => Some(self.deadline_nanoseconds(when.deadline, ops)?),
            None => None,
        };
        ops.extend(state_word(&class, guard));
        match acquire {
            Acquire::Wait | Acquire::When => {
                ops.extend([Op::Mutex(MutexOp::Lock(guard)), Op::Drop]);
            }
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
        let name = name.or(when.as_ref().map(|_| HIDDEN_VALUE));
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
        exit.extend(sequence_word(&class, guard));
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
        if let (Some(when), Some(deadline), Some(name)) = (&when, deadline, name) {
            self.wait_until_ready(manager, &class, guard, deadline, when, name, ops)?;
        }
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
        let failure = self.failure(manager, "WouldBlock")?;
        ops.push(Op::Mutex(MutexOp::LockNow(guard)));
        ops.push(branch(
            vec![
                Op::PushUnit,
                Op::Enum(EnumOp::New(failure.source.clone(), 0)),
            ],
            vec![
                Op::Enum(EnumOp::New(failure.error.clone(), failure.variant)),
                Op::Enum(EnumOp::New(failure.source.clone(), 1)),
            ],
        ));
        self.propagate_lock_error(failure, ops);
        Ok(())
    }

    /// The `LockError` variant `variant` and the `?` that propagates it from
    /// the function containing `manager`.
    fn failure(&mut self, manager: &Expr, variant: &str) -> Result<Failure> {
        let target = self.propagation_target(manager)?;
        let Some(Some(Ty::Enum(error))) = lookup_type(LOCK_ERROR, self.aliases) else {
            unreachable!("std.sync declares LockError beside Mutex")
        };
        let index = error
            .get()
            .variants
            .iter()
            .position(|found| found.name == variant)
            .expect("LockError declares every lock failure");
        let Ty::Enum(source) = crate::sum::result(Ty::Unit, Ty::Enum(error.clone())) else {
            unreachable!("a Result is an enum")
        };
        let target = self.check_propagation(manager, &source, target, None)?;
        Ok(Failure {
            error,
            variant: index,
            source,
            target,
        })
    }

    /// `?` on the `Result[(), LockError]` on the stack, unwinding every
    /// context in force.
    fn propagate_lock_error(&mut self, failure: Failure, ops: &mut Vec<Op>) {
        let mut cleanup = Vec::new();
        if !self.contexts.is_empty() {
            self.cleanup(0, &mut cleanup);
        }
        ops.extend([
            Op::Try {
                source: Box::new(failure.source),
                target: Box::new(failure.target),
                cleanup: cleanup.into(),
            },
            Op::Drop,
        ]);
    }

    /// Fail with `variant`, then leave `false` for the enclosing loop
    /// condition; the failure always propagates, so the `false` is not read.
    fn fail(&mut self, failure: Failure, ops: &mut Vec<Op>) {
        use crate::sum::EnumOp;
        ops.extend([
            Op::Enum(EnumOp::New(failure.error.clone(), failure.variant)),
            Op::Enum(EnumOp::New(failure.source.clone(), 1)),
        ]);
        self.propagate_lock_error(failure, ops);
        ops.push(Op::PushBool(false));
    }

    /// Check `lock_when`'s arguments before anything is evaluated.
    fn when_arguments<'a>(&self, manager: &Expr, args: &'a [Expr]) -> Result<When<'a>> {
        let [ready, deadline, cancel] = args else {
            return Err(manager
                .at
                .error("lock_when takes three arguments: `lock_when(ready, &deadline, &cancel)`"));
        };
        if let Some(named) = args
            .iter()
            .find(|arg| matches!(arg.kind, Expression::NamedArgument(..)))
        {
            return Err(named.at.error("lock_when takes positional arguments only"));
        }
        let ready = unborrowed(ready);
        if !matches!(ungroup(ready).kind, Expression::Name(_)) {
            return Err(ready.at.error(
                "lock_when's predicate must be named: pass a function, or a callable bound to a local, so its body can be checked not to wait",
            ));
        }
        // A predicate that mutated the value would not advance the sequence
        // word, so no waiter would wake to see its change.
        let hint = match self.expression_type_hint(ready) {
            Some(Ty::Ref(inner, _)) => Some((*inner).clone()),
            other => other,
        };
        if let Some(Ty::Ref(_, true)) = hint
            .as_ref()
            .and_then(super::callables::signature)
            .and_then(|sig| sig.inputs.first())
        {
            return Err(ready.at.error(
                "lock_when's predicate must take the value as `&T`, not `&mut T`: it only reads the value, and a change it made would not wake other waiters",
            ));
        }
        let deadline = unborrowed(deadline);
        if !matches!(self.place_type(deadline), Some(Ty::Class(c)) if c.get().name == DEADLINE) {
            return Err(deadline.at.error(
                "lock_when's second argument must name a std.time `Deadline`: `&deadline`",
            ));
        }
        let cancel = unborrowed(cancel);
        if !matches!(self.place_type(cancel), Some(Ty::CancellationToken)) {
            return Err(cancel
                .at
                .error("lock_when's third argument must name a `CancellationToken`: `&cancel`"));
        }
        Ok(When {
            ready,
            deadline,
            cancel,
        })
    }

    /// Read the deadline once, into a new local holding its monotonic
    /// nanoseconds. `Deadline` keeps them private; the runtime compares them
    /// with the clock that `monotonic_now()` reads.
    fn deadline_nanoseconds(&mut self, deadline: &Expr, ops: &mut Vec<Op>) -> Result<u8> {
        let Some(Ty::Class(class)) = self.place_type(deadline) else {
            unreachable!("when_arguments checked the deadline")
        };
        let (_, loan) = self.borrow(deadline, false, ops)?;
        let slot = self.slot(Ty::U64, &deadline.at)?;
        ops.extend([
            Op::Class(ClassOp::FieldRef(class, 0, false)),
            Op::ReadRef(Ty::U64),
            Op::UseLoan(loan),
            Op::StoreLocal(slot),
        ]);
        Ok(slot)
    }

    /// Wait, with the lock held and its context pushed, until `ready(value)`
    /// holds. Cancellation is checked first, then the predicate, so a
    /// predicate that holds at the deadline still succeeds. The `?` of each
    /// failure unlocks through the block's own exit.
    #[expect(
        clippy::too_many_arguments,
        reason = "the parts of one lock_when manager"
    )]
    fn wait_until_ready(
        &mut self,
        manager: &Expr,
        class: &Nominal<ClassType>,
        guard: u8,
        deadline: u8,
        when: &When<'_>,
        value: &str,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let at = &manager.at;
        let expr = |kind| Expr {
            at: at.clone(),
            kind,
        };
        let cancelled = expr(Expression::Method(
            Box::new(when.cancel.clone()),
            "is_cancelled".into(),
            Vec::new(),
            Default::default(),
        ));
        let Expression::Name(ready) = &ungroup(when.ready).kind else {
            unreachable!("when_arguments checked the predicate")
        };
        let call = Expr {
            at: when.ready.at.clone(),
            kind: Expression::Call(ready.clone(), vec![expr(Expression::Name(value.into()))]),
        };
        let mut check = Vec::new();
        let ty = self.expr(&call, &mut check)?;
        if ty != Some(Ty::Bool) {
            return Err(when.ready.at.error(format!(
                "lock_when's predicate must return bool, not {}",
                ty.map_or("()".into(), |ty| ty.to_string())
            )));
        }
        let predicate = check.iter().rev().find_map(|op| match op {
            Op::Call(name) => Some(Arc::from(name.as_str())),
            Op::ClosureCall(closure) if !closure.name.is_empty() => {
                Some(Arc::from(closure.name.as_str()))
            }
            _ => None,
        });
        let mut condition = Vec::new();
        self.value(&cancelled, &mut condition)?;
        let mut stop = Vec::new();
        let failure = self.failure(manager, "Cancelled")?;
        self.fail(failure, &mut stop);
        let mut timed_out = Vec::new();
        let failure = self.failure(manager, "TimedOut")?;
        self.fail(failure, &mut timed_out);
        let mut wait = Vec::from(state_word(class, guard));
        wait.extend(sequence_word(class, guard));
        wait.extend([
            Op::LoadLocal(deadline),
            Op::Mutex(MutexOp::Wait(guard, predicate)),
            branch(timed_out, vec![Op::PushBool(true)]),
        ]);
        check.push(branch(vec![Op::PushBool(false)], wait));
        condition.push(branch(stop, check));
        ops.push(Op::Loop {
            condition: condition.into(),
            body: Arc::from([]),
        });
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
            "lock_when" => Err(base.at.error(LOCK_WHEN_FORM)),
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
            "lock_when" => Err(base.at.error(LOCK_WHEN_FORM)),
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

/// The sequence word's shared reference, which `lock_when` waiters sleep on.
fn sequence_word(class: &Nominal<ClassType>, guard: u8) -> [Op; 2] {
    [
        Op::LoadLocal(guard),
        Op::Class(ClassOp::FieldRef(class.clone(), SEQUENCE, false)),
    ]
}

/// Ownership of the mutex proves no lock is held, so its value moves out
/// without one.
fn into_inner(class: &Nominal<ClassType>, ops: &mut Vec<Op>) -> Ty {
    ops.push(Op::Mutex(MutexOp::IntoInner(class.clone())));
    crate::mutex::element(class)
}
