//! Read, owned-copy and fixed-length operations on list slice views,
//! `&[T]` and `&mut [T]` (issue #866). Each specializes an ordinary Plenty
//! helper from `view_methods.plenty`, so arguments, loans and element
//! equality and copying follow the same checks as user code. Byte views
//! reach their binary string methods first (`byte_methods`), and `&strview`
//! has its own operations (`text_methods`). See
//! book/src/design/language/04-expressions/slices.md#operations-on-views.
use super::*;

const HELPERS: &str = include_str!("view_methods.plenty");

/// List methods that change the list's length. A view's length is fixed, so
/// they are named with that reason rather than as unknown methods.
const LENGTH_CHANGING: &[&str] = &["append", "extend", "insert", "pop", "clear", "reserve"];

impl Lower<'_> {
    /// The list or byte view type of `e`, `&[T]` or `&mut [T]`; `None` for
    /// a `&strview` and for anything that is not a view.
    pub(super) fn list_view_type(&self, e: &Expr) -> Option<Ty> {
        let view = match &ungroup(e).kind {
            Expression::Unary(op, inner) if op == "&" || op == "&mut" => {
                match &ungroup(inner).kind {
                    Expression::Slice(owner, _) => self.slice_view_type(owner, op == "&mut")?,
                    // `&v` of a view is the same view, shared unless both
                    // borrows are exclusive.
                    _ => match self.list_view_type(inner)? {
                        Ty::View(element, mutable) => Ty::View(element, mutable && op == "&mut"),
                        _ => return None,
                    },
                }
            }
            _ => self.expression_type_hint(e)?,
        };
        (matches!(view, Ty::View(..)) && view != text_view()).then_some(view)
    }

    /// `&base[:]` when `base` is a list whose elements the runtime list
    /// search does not compare: anything but numbers, `bool`, and `str`. A
    /// list's `count`, `find`, and `rfind` then search its whole view, so
    /// lists and views accept the same element types.
    pub(super) fn whole_list_view(&self, base: &Expr) -> Option<Expr> {
        let mut ty = self.expression_type_hint(base)?;
        while let Ty::Ref(inner, _) = ty {
            ty = (*inner).clone();
        }
        let Ty::List(element) = ty else {
            return None;
        };
        if element.is_int() || element.is_float() || matches!(*element, Ty::Bool | Ty::Str) {
            return None;
        }
        let bounds = SliceBounds {
            start: None,
            stop: None,
            step: None,
        };
        Some(Expr {
            kind: Expression::Unary(
                "&".into(),
                Box::new(Expr {
                    kind: Expression::Slice(Box::new(base.clone()), Box::new(bounds)),
                    at: base.at.clone(),
                }),
            ),
            at: base.at.clone(),
        })
    }

    /// Calls helper `name` from `view_methods.plenty`, specialized for
    /// `element`, with `args` in source order.
    fn view_helper(
        &mut self,
        name: &str,
        element: &Ty,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let helper = format!("__plenty_{name}");
        if !self.generics.templates.contains_key(&helper) {
            for mut function in modules::generated_interface(HELPERS)?.functions {
                function.name = format!("__plenty_{}", function.name);
                self.generics.validate_template(&function, self.aliases)?;
                self.generics.retention.keep(&function.body);
                self.generics
                    .templates
                    .insert(function.name.clone(), Arc::new(function));
            }
        }
        let symbol = self.specialize(&helper, vec![Argument::Type(element.clone())], at)?;
        self.call_named(&symbol, args, at, ops)
    }

    /// `item in view` and `item not in view`: a linear search with the
    /// element type's `==`. The item is evaluated first and lent, as a shared
    /// argument is; nothing is allocated.
    pub(super) fn view_membership(
        &mut self,
        op: &str,
        item: &Expr,
        view: &Expr,
        element: &Ty,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        Self::membership_equality(element, at)?;
        self.view_helper(
            "view_contains",
            element,
            &[item.clone(), view.clone()],
            at,
            ops,
        )?;
        if op == "not in" {
            ops.push(Op::Not);
        }
        Ok(Some(Ty::Bool))
    }

    /// A method of a list slice view other than `swap`.
    pub(super) fn view_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let Some(Ty::View(element, exclusive)) = self.list_view_type(base) else {
            return Err(base.at.error(format!(
                "a slice view has no method `{name}`; {}",
                views::OPERATIONS
            )));
        };
        if let Some(named) = args
            .iter()
            .find(|arg| matches!(arg.kind, Expression::NamedArgument(..)))
        {
            return Err(named.at.error(
                "named arguments require a statically resolved function, method, or class constructor",
            ));
        }
        match name {
            "get" => {
                let [index] = args else {
                    return Err(base.at.error("get requires one index argument"));
                };
                if element.affine() {
                    return Err(base.at.error(format!(
                        "get cannot return owned {element} values from a view; borrow with `&v[index]`, which traps when absent, or copy with `copy(&v[index])?`"
                    )));
                }
                self.view_helper(
                    "view_get",
                    &element,
                    &[base.clone(), index.clone()],
                    &base.at,
                    ops,
                )
            }
            "count" | "find" | "rfind" => {
                let [item] = args else {
                    return Err(base.at.error(format!(
                        "view.{name} takes one element to search for"
                    )));
                };
                Self::membership_equality(&element, &base.at)?;
                let helper = format!("view_{name}");
                self.view_helper(&helper, &element, &[base.clone(), item.clone()], &base.at, ops)
            }
            "reverse" => {
                if !exclusive {
                    return Err(base.at.error(
                        "a shared slice view is read-only, so it has no `reverse`; take an exclusive view with `&mut xs[a:b]`",
                    ));
                }
                if !args.is_empty() {
                    return Err(base.at.error("view.reverse takes no arguments"));
                }
                self.view_helper("view_reverse", &element, std::slice::from_ref(base), &base.at, ops)
            }
            "get_ref" | "get_mut" => Err(base.at.error(format!(
                "a slice view cannot `{name}` yet (issue #{LOOKUP_ISSUE}); borrow with `&v[i]` after checking `i < len(v)`, or copy a non-affine element with `v.get(i)`"
            ))),
            "sort" | "sort_by_key" | "sort_stable_by_key" => Err(base.at.error(format!(
                "a slice view cannot `{name}` yet (issue #{SORT_ISSUE}); sort the whole list, or reorder the view's elements with `swap`"
            ))),
            _ if LENGTH_CHANGING.contains(&name) => Err(base.at.error(format!(
                "`{name}` changes a list's length, and a slice view's length is fixed; call it on the list after the view's last use"
            ))),
            _ => Err(base.at.error(format!(
                "a slice view has no method `{name}`; {}",
                views::OPERATIONS
            ))),
        }
    }

    /// An owned list of copies of every element of `view`, as `list(v)?` and
    /// the copying slice `v[a:b]?` produce. Elements are copied as `copy()`
    /// copies them and `Shared` handles shared, as a list's copying slice
    /// does; storage for every element is reserved first.
    pub(super) fn view_copy(
        &mut self,
        view: &Expr,
        element: &Ty,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        if let Some(reason) = Self::uncopyable_element(element, at)? {
            return Err(at.error(format!(
                "copying a view of {element} copies each element, and {reason}; borrow the elements instead, or build a list from explicit copies"
            )));
        }
        let copied = match element {
            Ty::Shared(content) => {
                self.view_helper("view_share", content, std::slice::from_ref(view), at, ops)?
            }
            _ => self.view_helper("view_copy", element, std::slice::from_ref(view), at, ops)?,
        };
        Ok(copied.expect("a view copy returns its Result"))
    }
}

/// The follow-up issue for sorting slice views in place.
const SORT_ISSUE: u32 = 886;

/// The follow-up issue for optional borrowed lookup on slice views.
const LOOKUP_ISSUE: u32 = 890;
