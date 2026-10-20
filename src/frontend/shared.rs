//! `Shared[T]`: construction, `share()`, and shared access through a named
//! handle. Every access borrows the content through `*handle`, so its loan is
//! rooted at the handle binding. See
//! book/src/design/facilities/01-builtins/shared.md.
use super::*;
use crate::boxed::BoxOp;
use crate::hash::HashSet;

pub(super) const EQUALITY: &str =
    "values containing Shared owners do not support equality; compare values read through the handles";
pub(super) const FORMAT: &str =
    "Shared owners cannot be formatted; format values read through the handle";
pub(super) const MUTATION: &str = "a Shared owner gives shared access only; mutate its content through an Atomic or a Mutex inside it";

/// `*base`, the content place of a shared handle.
pub(super) fn content(base: &Expr) -> Expr {
    Expr {
        at: base.at.clone(),
        kind: Expression::Unary("*".into(), Box::new(base.clone())),
    }
}

/// Why `Shared[content]` is rejected, if it is: it can reach a pool, or a
/// value could own a share of itself through state that changes after
/// construction.
#[expect(
    clippy::mutable_key_type,
    reason = "nominal identities exclude definition cells"
)]
fn rejection(content: &Ty) -> Option<String> {
    let shared = Ty::Shared(Arc::new(content.clone()));
    // The mutex class's qualified name is internal; source spells `Mutex[T]`.
    let shown = |ty: &Ty| ty.to_string().replace("std.sync.Mutex[", "Mutex[");
    // A type is visited at most twice: before and after a mutable edge.
    let mut pending = vec![(content.clone(), None::<String>)];
    let mut seen = HashSet::default();
    while let Some((ty, through)) = pending.pop() {
        if !seen.insert((ty.clone(), through.is_some())) {
            continue;
        }
        match &ty {
            Ty::Executor | Ty::Future(_) => {
                let (shared, ty) = (shown(&shared), shown(&ty));
                return Some(format!(
                    "{shared} cannot hold {ty}: its last owner could drop on a pool worker, which would wait for its own pool; keep executors and futures on the owning thread"
                ));
            }
            Ty::Shared(_) if ty == shared => {
                if let Some(through) = &through {
                    let shared = shown(&shared);
                    return Some(format!(
                        "{shared} can reach itself through {through}, so a value could hold a share of itself and never be released; link back with an index into a list that owns the values instead"
                    ));
                }
            }
            // Erased captures could hold a share of the value.
            Ty::BoxedCallable(_) => {
                if let Some(through) = &through {
                    let (shared, ty) = (shown(&shared), shown(&ty));
                    return Some(format!(
                        "{shared} can store a {ty} through {through}, and its captures cannot be checked for a share of the value; store a closure type or a named function instead"
                    ));
                }
            }
            _ => {}
        }
        // A mutex value and a channel queue can change after construction,
        // so a share stored there can close a cycle. Immutable contents
        // cannot: a value exists before any share of it.
        let edge = match crate::mutex::class(&ty) {
            Some(class) => Some(format!("Mutex[{}]", crate::mutex::element(class))),
            None if matches!(ty, Ty::Channel(..)) => Some(shown(&ty)),
            None => None,
        };
        let next = if edge.is_some() {
            through.clone().or(edge)
        } else {
            through.clone()
        };
        let mut children = Vec::new();
        crate::threading::contained(&ty, &mut children);
        pending.extend(children.into_iter().map(|child| (child, next.clone())));
    }
    None
}

impl Lower<'_> {
    /// `Shared[T](value)` names `T`; `Shared(value)` takes it from the value.
    pub(super) fn shared_new(
        &mut self,
        types: Option<&[TypeRef]>,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let named = match types {
            Some([content]) => Some(
                content
                    .resolve(self.aliases)?
                    .ok_or_else(|| content.at.error("a shared owner cannot hold unit"))?,
            ),
            Some(_) => return Err(at.error("Shared requires 1 type argument")),
            None => None,
        };
        let [value] = args else {
            return Err(at.error("Shared takes one value"));
        };
        let actual = self.expr_expected(value, named.clone(), ops)?;
        if named.is_some() {
            self.same(actual.clone(), named, &value.at)?;
        }
        let content = actual.ok_or_else(|| value.at.error("a shared owner cannot hold unit"))?;
        if let Some(barrier) = content.storage_barrier() {
            return Err(value.at.error(barrier.message("a shared owner")));
        }
        if let Some(reason) = rejection(&content) {
            return Err(at.error(reason));
        }
        let operation = BoxOp::Share(content);
        let output = operation.signature().1;
        ops.push(Op::Box(operation));
        Ok(Some(output))
    }

    /// A method on a shared handle place. Only `share` belongs to the handle;
    /// every other method is the content's, called through `*handle`.
    pub(super) fn shared_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if name != "share" {
            return self.method(&content(base), name, args, ops);
        }
        if !args.is_empty() {
            return Err(base.at.error("share takes no arguments"));
        }
        // observe already retained one owner of the same allocation.
        let (ty, loans) = self.observe(base, ops)?;
        Self::end_reads(loans, ops);
        Ok(Some(ty))
    }

    /// `*handle` read as a value: a copy of copyable content. Affine content
    /// stays with the other owners.
    pub(super) fn shared_read(&mut self, e: &Expr, ops: &mut Vec<Op>) -> Result<Type> {
        let Expression::Unary(_, base) = &ungroup(e).kind else {
            unreachable!("caller matched a dereference")
        };
        let Some(Ty::Shared(content)) = self.place_type(base) else {
            unreachable!("caller checked the handle type")
        };
        if content.affine() {
            let content = content.to_string().replace("std.sync.Mutex[", "Mutex[");
            return Err(e.at.error(format!(
                "cannot move out of a Shared owner; other owners may still use its {content}; borrow it with `&*handle`, or call `share()` for another owner"
            )));
        }
        let (reference, loan) = self.borrow(e, false, ops)?;
        let Ty::Ref(ty, _) = reference else {
            unreachable!()
        };
        ops.push(Op::ReadRef((*ty).clone()));
        Self::end_reads(vec![loan], ops);
        Ok(Some((*ty).clone()))
    }
}
