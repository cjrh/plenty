//! `std.atomic` surface: construction, methods on a borrowed place, `fence`,
//! and compile-time `Ordering` validation.
use super::*;
use crate::atomic::{AtomicOp, Modify, Ordering, OrderingUse, ORDERING};

const ELEMENTS: &str = "bool, i8, i16, i32, i64, u8, u16, u32, or u64";

pub(super) fn element_error(element: &Ty, at: &Token) -> Box<dyn Error> {
    at.error(format!(
        "Atomic[{element}] is not supported; the element type must be {ELEMENTS}"
    ))
}

/// The result type of a method on `Atomic[element]`, for type hints.
pub(super) fn method_output(element: &Ty, name: &str) -> Type {
    match name {
        "compare_exchange" => Some(crate::sum::result(element.clone(), element.clone())),
        "store" => None,
        "load" | "swap" | "into_inner" => Some(element.clone()),
        _ if name.starts_with("fetch_") => Some(element.clone()),
        _ => None,
    }
}

impl Lower<'_> {
    /// `Atomic[T](value)` names `T`; `Atomic(value)` takes it from the
    /// expected type or else from the value.
    pub(super) fn atomic_new(
        &mut self,
        types: Option<&[TypeRef]>,
        args: &[Expr],
        expected: Type,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let named = match types {
            Some([element]) => Some(
                element
                    .resolve(self.aliases)?
                    .ok_or_else(|| element_error(&Ty::Unit, &element.at))?,
            ),
            Some(_) => return Err(at.error("Atomic requires one element type")),
            None => match expected {
                Some(Ty::Atomic(element)) => Some((*element).clone()),
                _ => None,
            },
        };
        let [value] = args else {
            return Err(at.error("Atomic takes one initial value"));
        };
        let actual = self.expr_expected(value, named.clone(), ops)?;
        if named.is_some() {
            self.same(actual.clone(), named, &value.at)?;
        }
        let element = actual.ok_or_else(|| element_error(&Ty::Unit, &value.at))?;
        if !crate::atomic::supported(&element) {
            return Err(element_error(&element, &value.at));
        }
        let operation = AtomicOp::New(element);
        let output = operation.signature().1;
        ops.push(Op::Atomic(operation));
        Ok(Some(output))
    }

    pub(super) fn atomic_fence(
        &mut self,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let [order] = args else {
            return Err(at.error("fence takes one Ordering"));
        };
        self.ordering(order, "fence", OrderingUse::Fence)?;
        ops.push(Op::Atomic(AtomicOp::Fence));
        ops.push(Op::Drop);
        Ok(None)
    }

    /// Every operation except `into_inner` borrows the receiver's place, so a
    /// local stays in its frame slot and all threads address the same word.
    pub(super) fn atomic_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let Some(Ty::Atomic(element)) = self.place_type(base) else {
            unreachable!("caller checked the receiver type")
        };
        let element = (*element).clone();
        if name == "into_inner" {
            if !args.is_empty() {
                return Err(base.at.error("into_inner takes no arguments"));
            }
            if matches!(&ungroup(base).kind, Expression::Name(n)
                if self.names.get(n).is_some_and(|local| matches!(local.ty, Ty::Ref(..))))
            {
                return Err(base
                    .at
                    .error("into_inner consumes an owned atomic, not a reference"));
            }
            let actual = self.value(base, ops)?;
            self.same(
                Some(actual),
                Some(crate::atomic::ty(element.clone())),
                &base.at,
            )?;
            ops.push(Op::Atomic(AtomicOp::IntoInner(element.clone())));
            return Ok(Some(element));
        }
        let int = element.is_int();
        let (operation, values, orders): (AtomicOp, usize, &[OrderingUse]) = match name {
            "load" => (AtomicOp::Load(element.clone()), 0, &[OrderingUse::Load]),
            "store" => (AtomicOp::Store(element.clone()), 1, &[OrderingUse::Store]),
            "swap" => (
                AtomicOp::Modify(element.clone(), Modify::Swap),
                1,
                &[OrderingUse::Modify],
            ),
            "compare_exchange" => (
                AtomicOp::CompareExchange(element.clone()),
                2,
                &[OrderingUse::Modify, OrderingUse::Failure],
            ),
            "fetch_add" | "fetch_sub" if int => (
                AtomicOp::Checked(element.clone(), name == "fetch_sub"),
                1,
                &[OrderingUse::Modify],
            ),
            "fetch_not" if !int => (
                AtomicOp::Modify(element.clone(), Modify::Xor),
                0,
                &[OrderingUse::Modify],
            ),
            _ => {
                let modify = match name {
                    "fetch_wrapping_add" if int => Modify::WrappingAdd,
                    "fetch_wrapping_sub" if int => Modify::WrappingSub,
                    "fetch_max" if int => Modify::Max,
                    "fetch_min" if int => Modify::Min,
                    "fetch_and" => Modify::And,
                    "fetch_or" => Modify::Or,
                    "fetch_xor" => Modify::Xor,
                    "fetch_nand" => Modify::Nand,
                    _ => {
                        let methods = if int {
                            "load, store, swap, compare_exchange, fetch_add, fetch_sub, fetch_wrapping_add, fetch_wrapping_sub, fetch_and, fetch_or, fetch_xor, fetch_nand, fetch_max, fetch_min, and into_inner"
                        } else {
                            "load, store, swap, compare_exchange, fetch_and, fetch_or, fetch_xor, fetch_nand, fetch_not, and into_inner"
                        };
                        return Err(base.at.error(format!(
                            "Atomic[{element}] has no method `{name}`; it supports {methods}"
                        )));
                    }
                };
                (
                    AtomicOp::Modify(element.clone(), modify),
                    1,
                    &[OrderingUse::Modify],
                )
            }
        };
        if args.len() != values + orders.len() {
            let orderings = if orders.len() == 1 {
                "an Ordering".to_string()
            } else {
                "success and failure Orderings".to_string()
            };
            let what = match values {
                0 => orderings,
                1 => format!("a value and {orderings}"),
                _ => format!("current and new values and {orderings}"),
            };
            return Err(base.at.error(format!("{name} takes {what}")));
        }
        for (order, role) in args[values..].iter().zip(orders) {
            self.ordering(order, name, *role)?;
        }
        let place = match &ungroup(base).kind {
            Expression::Name(_) | Expression::Member(..) | Expression::Index(..) => true,
            Expression::Unary(op, _) => op == "*",
            _ => false,
        };
        if !place {
            return Err(base.at.error(
                "atomic operations need a place to update; bind the atomic to a name first",
            ));
        }
        let (_, loan) = self.borrow(base, false, ops)?;
        for value in &args[..values] {
            let actual = self.expr_expected(value, Some(element.clone()), ops)?;
            self.same(actual, Some(element.clone()), &value.at)?;
        }
        if name == "fetch_not" {
            ops.push(Op::PushBool(true));
        }
        let output = operation.signature().1;
        self.mark(&base.at, ops);
        ops.push(Op::Atomic(operation));
        Self::end_reads(vec![loan], ops);
        if output == Ty::Unit {
            ops.push(Op::Drop);
            Ok(None)
        } else {
            Ok(Some(output))
        }
    }

    /// Orderings are part of the program text, so invalid combinations are
    /// compile-time errors rather than runtime panics.
    fn ordering(&self, e: &Expr, operation: &str, role: OrderingUse) -> Result<()> {
        let written = match &ungroup(e).kind {
            Expression::Member(base, variant, _) => match self.qualified_type(base) {
                Ok(Some(Ty::Enum(t))) if &*t.name == ORDERING => {
                    Some(Ordering::named(variant).ok_or_else(|| {
                        e.at.error(format!(
                            "Ordering has no variant `{variant}`; use Relaxed, Release, Acquire, AcqRel, or SeqCst"
                        ))
                    })?)
                }
                _ => None,
            },
            _ => None,
        };
        let Some(order) = written else {
            return Err(e.at.error(format!(
                "{operation} requires its ordering written as `Ordering.<variant>` from std.atomic; orderings are fixed at compile time"
            )));
        };
        let allowed = role.allowed();
        if allowed.contains(&order) {
            return Ok(());
        }
        let names: Vec<_> = allowed.iter().map(|o| format!("Ordering.{o:?}")).collect();
        let (last, rest) = names.split_last().expect("every role allows an ordering");
        let subject = match role {
            OrderingUse::Failure => format!("{operation} failure ordering"),
            _ => operation.to_string(),
        };
        Err(e.at.error(format!(
            "{subject} does not accept Ordering.{order:?}; use {}, or {last}",
            rest.join(", ")
        )))
    }
}
