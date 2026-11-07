//! Lints: reports about code the checker accepts. See
//! book/src/design/toolchain/01-building/warnings.md.

use super::*;
use crate::diagnostic::{Lint, Warning};

impl Lower<'_> {
    /// Reports `copy(argument)?` whose `payload` type copies on every use, so
    /// the argument itself is the same value (issue #819). The rule is the
    /// checker's own instantiated copyability, `Ty::affine`, applied only
    /// where replacing the call by its argument keeps the type, evaluates the
    /// same expression once, and transfers no ownership:
    ///
    /// - the operand is the builtin call, propagated directly by `?`;
    ///   `copy(x)` used as a `Result` is left alone;
    /// - the argument is an owned local, a field path or element of one, or a
    ///   literal, suggested as itself; a shared borrow `&place` of such a
    ///   local, field path or element, suggested as `place`; or a local
    ///   shared reference `r`, suggested as `*r`, which reads the copyable
    ///   value. A `&mut` reference, a dereference or a call is left alone;
    /// - the body is ordinary code: an instance of generic code is checked
    ///   per type argument, and another instance may need the copy.
    pub(super) fn redundant_copy(&mut self, value: &Expr, payload: &Ty) {
        let Expression::Call(name, args) = &ungroup(value).kind else {
            return;
        };
        let (true, [argument]) = (name == "copy", args.as_slice()) else {
            return;
        };
        if payload.affine()
            || payload.restricted_storage()
            || payload.has_destructor()
            || self.generics.requirement(self.symbol).is_some()
            || self.function_name.starts_with("__plenty_")
        {
            return;
        }
        if let Some(source) = value.at.source()
            && self.generics.source_facts(&source).1
        {
            return;
        }
        let argument = ungroup(argument);
        let (borrow, place) = match &argument.kind {
            Expression::Unary(op, place) if op == "&" => ("&", ungroup(place)),
            _ => ("", argument),
        };
        // (as written, replacement) for a name or field path.
        let spelled = match &place.kind {
            Expression::Name(name) if borrow.is_empty() && self.shared_reference(name) => {
                Some((name.clone(), format!("*{name}")))
            }
            // A borrowed literal is left alone: `&b"..."` is a view, not bytes.
            Expression::Text(_)
            | Expression::Number(_)
            | Expression::Bool(_)
            | Expression::Bytes(_)
                if !borrow.is_empty() =>
            {
                return;
            }
            _ => {
                let Some(text) = self.copied_place(place) else {
                    return;
                };
                text.map(|text| (text.clone(), text))
            }
        };
        let ty = type_name(Some(payload.clone()));
        let reason = format!("`{ty}` copies on assignment and when passed or returned");
        let message = match spelled {
            Some((written, replacement)) => {
                format!("`copy({borrow}{written})?` is redundant: {reason}; write `{replacement}`")
            }
            None => {
                let what = if borrow.is_empty() {
                    "argument"
                } else {
                    "borrowed place"
                };
                format!(
                    "this `copy({borrow}...)?` is redundant: {reason}; write the {what} without `copy({borrow}...)?`"
                )
            }
        };
        self.generics.warnings.push(Warning::new(
            Lint::RedundantCopy,
            value.at.position(),
            message,
        ));
    }

    /// Whether `name` is a local `&T` reference to a type that copies, so
    /// `*name` reads the same value `copy(name)?` makes.
    fn shared_reference(&self, name: &str) -> bool {
        matches!(self.names.get(name), Some(Local { ty: Ty::Ref(t, false), .. })
            if !t.affine() && !t.restricted_storage())
    }

    /// `Some` when `e` is an argument whose plain use is the copy: the
    /// spelling to suggest when it is a name or field path, else `None`.
    fn copied_place(&self, e: &Expr) -> Option<Option<String>> {
        match &e.kind {
            Expression::Text(_)
            | Expression::Number(_)
            | Expression::Bool(_)
            | Expression::Bytes(_) => Some(None),
            Expression::Name(name) => {
                let local = self.names.get(name)?;
                (!local.ty.affine() && !local.ty.restricted_storage()).then(|| Some(name.clone()))
            }
            Expression::Member(..) => {
                self.place_type(e)?;
                Some(field_path(e))
            }
            Expression::Index(base, _) => {
                let owner = self.place_type(base)?;
                let indexed = matches!(owner, Ty::List(_) | Ty::Dict(..))
                    || matches!(&owner, Ty::Enum(t) if t.tuple());
                indexed.then_some(None)
            }
            _ => None,
        }
    }
}

/// `a.b.c` for a field path from a name, as written.
fn field_path(e: &Expr) -> Option<String> {
    match &e.kind {
        Expression::Name(name) => Some(name.clone()),
        Expression::Member(base, field, _) => Some(format!("{}.{field}", field_path(base)?)),
        _ => None,
    }
}
