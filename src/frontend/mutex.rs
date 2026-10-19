//! `std.sync.Mutex` surface: `with mutex.lock() as value:` and `into_inner`.
//! A lock is a `with` manager only, so its unlock is ordinary context cleanup
//! on every exit and the guarded reference never outlives the block.
use super::*;
use crate::mutex::{MutexOp, STATE, VALUE};
use crate::nominal::Nominal;
use crate::record::{ClassOp, ClassType};
use contexts::{Context, Kind};

pub(super) const FORMAT: &str =
    "mutexes cannot be formatted; format a value read inside `with mutex.lock() as value:`";
const LOCK_FORM: &str = "lock() is valid only as a `with` manager: `with mutex.lock() as value:`";

/// `base.lock(args)`, before its receiver is known to be a mutex.
pub(super) fn lock_expression(manager: &Expr) -> Option<(&Expr, &[Expr])> {
    match &ungroup(manager).kind {
        Expression::Method(base, name, args, _) if name == "lock" => Some((base, args)),
        _ => None,
    }
}

/// The written place a lock names, compared to find a relock in its own block.
/// Dereferences are dropped: `*r` and `r` lock the same mutex. A computed
/// index is `[?]`, which matches no other place.
fn place_key(e: &Expr) -> Option<String> {
    match &ungroup(e).kind {
        Expression::Name(name) => Some(name.clone()),
        Expression::Member(base, field, _) => Some(format!("{}.{field}", place_key(base)?)),
        Expression::Index(base, index) => match &ungroup(index).kind {
            Expression::Number(n) => Some(format!("{}[{n}]", place_key(base)?)),
            _ => Some(format!("{}[?]", place_key(base)?)),
        },
        Expression::Unary(op, base) if op == "*" => place_key(base),
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
        "Mutex has no method `{name}`; it supports `with mutex.lock() as value:` and into_inner()"
    ))
}

impl Lower<'_> {
    pub(super) fn with_lock(
        &mut self,
        manager: &Expr,
        base: &Expr,
        args: &[Expr],
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
        if !args.is_empty() {
            return Err(manager.at.error("lock takes no arguments"));
        }
        if generators::yields(body) {
            return Err(manager.at.error(
                "yield inside a mutex lock block would keep the lock held while the generator is suspended; release the lock before yielding",
            ));
        }
        let place = place_key(base).ok_or_else(|| {
            base.at
                .error("lock() requires a named mutex or a field of one")
        })?;
        let element = crate::mutex::element(&class);
        for context in &self.contexts {
            let Kind::Lock(held, guarded) = &context.kind else {
                continue;
            };
            if *held == place && !place.contains("[?]") {
                return Err(manager.at.error(format!(
                    "`{place}` is already locked by an enclosing `with` block, so this lock would wait for itself forever; use the existing binding, or release the lock before relocking"
                )));
            }
            // Shared handles and references can name one mutex through
            // different places, so mutexes are told apart only by type.
            if *guarded == element {
                return Err(manager.at.error(format!(
                    "this locks `{place}` while an enclosing `with` block holds a lock on `{held}`; both are a `Mutex[{element}]` and may be one mutex reached through different handles, so this lock could wait for itself forever; lock them one at a time, or wrap one value in a distinct class"
                )));
            }
        }
        let saved = self.names.clone();
        let start = self.locals.len();
        let (reference, mutex_loan) = self.borrow(base, false, ops)?;
        let guard = self.slot(reference, &manager.at)?;
        ops.push(Op::StoreLocal(guard));
        ops.extend(state_word(&class, guard));
        ops.extend([Op::Mutex(MutexOp::Lock(guard)), Op::Drop]);
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
            kind: Kind::Lock(place, element.clone()),
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
