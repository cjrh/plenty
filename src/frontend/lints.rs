//! Lints: reports about code the checker accepts. See
//! book/src/design/toolchain/01-building/warnings.md.

use super::*;
use crate::diagnostic::{Lint, Warning};

mod text_parameters;
pub(super) use text_parameters::{
    escapes as text_view_escapes, parameters as text_view_parameters,
};

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
    ///   value, or `(*r)` when `try_expr` is the operand of `.` or `[`
    ///   (see `note_postfix_operand`); or a closure capture, suggested as
    ///   itself. A `&mut` reference, a dereference or a call is left alone;
    /// - the body is ordinary code written in the program: a function,
    ///   method, anonymous function or closure, not `std` or a generated
    ///   helper. An instance of generic code is checked per type argument,
    ///   and another instance may need the copy.
    pub(super) fn redundant_copy(&mut self, try_expr: &Expr, value: &Expr, payload: &Ty) {
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
            || !written_body(&self.function_name)
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
            // The payload checks above make `*name` a copy of a copyable value.
            // A capture is a reference only internally; see `copied_place`.
            Expression::Name(name)
                if borrow.is_empty()
                    && !self.captures.contains(name)
                    && matches!(
                        self.names.get(name),
                        Some(Local {
                            ty: Ty::Ref(_, false),
                            ..
                        })
                    ) =>
            {
                let replacement = if self.postfix_tries.contains(&try_expr.at.position()) {
                    format!("(*{name})")
                } else {
                    format!("*{name}")
                };
                Some((name.clone(), replacement))
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

    /// Records a `?` written directly as the operand of `e`'s `.` or `[`, as
    /// in `copy(r)?.x`: `*r.x` would dereference the field, so
    /// `redundant_copy` suggests `(*r)` there. Called by both expression
    /// entry points before lowering `e`, which lowers the operand.
    pub(super) fn note_postfix_operand(&mut self, e: &Expr) {
        let (Expression::Member(base, ..)
        | Expression::Method(base, ..)
        | Expression::GenericMethod(base, ..)
        | Expression::Index(base, _)
        | Expression::Slice(base, _)) = &e.kind
        else {
            return;
        };
        if matches!(base.kind, Expression::Try(_)) {
            self.postfix_tries.insert(base.at.position());
        }
    }

    /// `Some` when `e` is an argument whose plain use is the copy: the
    /// spelling to suggest when it is a name or field path, else `None`.
    fn copied_place(&self, e: &Expr) -> Option<Option<String>> {
        match &e.kind {
            Expression::Text(_)
            | Expression::Number(_)
            | Expression::Bool(_)
            | Expression::Bytes(_) => Some(None),
            // A closure reads a capture through a hidden reference, but its
            // body names the value, which the payload checks make copyable.
            Expression::Name(name) if self.captures.contains(name) => Some(Some(name.clone())),
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

/// Whether a body named `symbol` is one its author wrote: a function, a
/// method, or an anonymous function or closure. Other `__plenty_` bodies are
/// generated, and a generic instance is excluded by its requirement instead,
/// including methods of generic classes and enums and closures lifted from
/// generic code (issue #846).
fn written_body(symbol: &str) -> bool {
    !symbol.starts_with("__plenty_")
        || crate::record::is_method(symbol)
        || symbol.starts_with("__plenty_anonymous_")
        || symbol.starts_with("__plenty_closure_")
}

/// `a.b.c` for a field path from a name, as written.
fn field_path(e: &Expr) -> Option<String> {
    match &e.kind {
        Expression::Name(name) => Some(name.clone()),
        Expression::Member(base, field, _) => Some(format!("{}.{field}", field_path(base)?)),
        _ => None,
    }
}
