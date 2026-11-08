//! Conservative, source-level proof that a parameter only supplies text reads.
//!
//! Unclassified uses suppress advice. In particular, an owning alias, an ordinary
//! reference, a capture, or a callable signature is never silently treated as a
//! text view. We inspect resolved declarations before lowering releases their ASTs.
use super::*;

pub(in crate::frontend) fn escapes(
    functions: &[Function],
    kept: &[&Function],
    standard: &HashSet<Arc<str>>,
) -> HashSet<String> {
    let candidates: HashSet<_> = functions
        .iter()
        .filter(|f| {
            !f.inputs.is_empty()
                && !crate::record::is_method(&f.name)
                && !f.name.starts_with("__plenty_")
                && !f
                    .at
                    .source()
                    .is_some_and(|source| standard.contains(&source))
        })
        .map(|f| f.name.as_str())
        .collect();
    let mut escaped = HashSet::default();
    if candidates.is_empty() {
        return escaped;
    }
    for f in functions.iter().chain(kept.iter().copied()) {
        if f.at
            .source()
            .is_some_and(|source| standard.contains(&source))
        {
            continue;
        }
        let mut visit = |node: Node<'_>| {
            if let Node::Expr(Expr {
                kind: Expression::Name(name) | Expression::GenericValue(name, _),
                ..
            }) = node
                && candidates.contains(name.as_str())
            {
                escaped.insert(name.clone());
            }
        };
        block(&f.body, &mut visit);
        for default in f.defaults.iter().flatten() {
            expression(default, &mut visit);
        }
    }
    escaped
}

pub(in crate::frontend) fn parameters(
    engine: &generics::Engine,
    sigs: &functions::Functions,
    escaped: &HashSet<String>,
) -> Vec<Warning> {
    let mut warnings = Vec::new();
    for (symbol, f) in &engine.pending {
        if f.foreign.is_some()
            || f.export.is_some()
            || f.name.starts_with("__plenty_")
            || crate::record::is_method(&f.name)
            || !f.type_params.is_empty()
            || engine.requirement(*symbol).is_some()
            || escaped.contains(&f.name)
            || f.defaults.iter().any(Option::is_some)
            || generators::yields(&f.body)
            || f.at
                .source()
                .is_some_and(|source| engine.standard_sources.contains(&source))
        {
            continue;
        }
        let Some(sig) = sigs.sig(*symbol) else {
            continue;
        };
        // Changing the first reference parameter can change a returned loan's
        // origin even when the text parameter itself only has reader uses.
        if sig.outputs.iter().any(Ty::carries_loan) {
            continue;
        }
        for ((name, ty), (_, written)) in sig.inputs.iter().zip(&f.inputs) {
            if !matches!(ty, Ty::Str) && !matches!(ty, Ty::Ref(t, false) if **t == Ty::Str) {
                continue;
            }
            let mut uses = HashSet::default();
            let mut reads = HashSet::default();
            let mut opaque = false;
            block(&f.body, &mut |node| {
                match node {
                    Node::Binding(binding) => opaque |= binding == name,
                    Node::Stmt(Stmt {
                        kind: Statement::For { iterable, .. },
                        ..
                    }) => {
                        read(iterable, name, &mut reads);
                    }
                    Node::Expr(e) => match &e.kind {
                        Expression::Name(used) if used == name => {
                            uses.insert(e.at.position());
                        }
                        // View capture is unsupported. Excluding the enclosing
                        // function also avoids conflating lifted/shadowed names.
                        Expression::Anonymous(_) => opaque = true,
                        Expression::Method(base, method, _, _) if text_method(method) => {
                            read(base, name, &mut reads);
                        }
                        Expression::Index(base, _) | Expression::Slice(base, _) => {
                            read(base, name, &mut reads);
                        }
                        Expression::Call(callee, args) => {
                            if matches!(callee.as_str(), "len" | "ord" | "print" | "str.repr") {
                                for arg in args {
                                    read(arg, name, &mut reads);
                                }
                            } else if let Some(callee) = sigs.get(callee) {
                                for (index, arg) in args.iter().enumerate() {
                                    let (input, value) = match &arg.kind {
                                        Expression::NamedArgument(label, value) => (
                                            callee.inputs.iter().find(|(n, _)| n == label),
                                            value.as_ref(),
                                        ),
                                        _ => (callee.inputs.get(index), arg),
                                    };
                                    if matches!(input, Some((_, Ty::View(t, false))) if **t == Ty::Text)
                                    {
                                        read(value, name, &mut reads);
                                    }
                                }
                            }
                        }
                        Expression::Format(format) => {
                            for arg in format.operands() {
                                read(arg, name, &mut reads);
                            }
                        }
                        Expression::Binary(op, a, b)
                            if matches!(
                                op.as_str(),
                                "==" | "!=" | "<" | "<=" | ">" | ">=" | "in" | "not in" | "+"
                            ) =>
                        {
                            // Do not mistake a dictionary key, list membership,
                            // or a generic comparison for a text operand.
                            if matches!(ungroup(b).kind, Expression::Text(_)) {
                                read(a, name, &mut reads);
                            }
                            if matches!(ungroup(a).kind, Expression::Text(_)) {
                                read(b, name, &mut reads);
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
            });
            if !opaque && !uses.is_empty() && uses.is_subset(&reads) {
                warnings.push(Warning::new(
                    Lint::TextViewParameter,
                    written.at.position(),
                    format!("parameter `{name}` only supplies text reads; consider `&strview` instead of `{}` to also accept substring views", type_name(Some(ty.clone()))),
                ));
            }
        }
    }
    warnings
}

fn read(e: &Expr, name: &str, reads: &mut HashSet<Position>) {
    let e = ungroup(e);
    if matches!(&e.kind, Expression::Name(used) if used == name) {
        reads.insert(e.at.position());
    }
}

fn text_method(name: &str) -> bool {
    matches!(
        name,
        "byte_len"
            | "as_bytes"
            | "startswith"
            | "endswith"
            | "find"
            | "rfind"
            | "count"
            | "get"
            | "isascii"
            | "isspace"
            | "isdigit"
            | "isalpha"
            | "isalnum"
            | "lower"
            | "upper"
            | "strip"
            | "lstrip"
            | "rstrip"
            | "concat"
            | "repeat"
            | "replace"
            | "removeprefix"
            | "removesuffix"
            | "split"
            | "splitlines"
            | "join"
            | "slice"
    )
}

/// A single exhaustive walk serves both callable escape detection and parameter
/// use classification, including scopes that shadow a parameter name.
enum Node<'a> {
    Stmt(&'a Stmt),
    Expr(&'a Expr),
    Binding(&'a str),
}

fn block(body: &[Stmt], visit: &mut impl FnMut(Node<'_>)) {
    for stmt in body {
        visit(Node::Stmt(stmt));
        match &stmt.kind {
            Statement::Assign { name, value, .. } => {
                visit(Node::Binding(name));
                expression(value, visit);
            }
            Statement::Unpack {
                names,
                targets,
                value,
                ..
            } => {
                for name in names {
                    visit(Node::Binding(name));
                }
                for target in targets.iter().flatten() {
                    expression(target, visit);
                }
                expression(value, visit);
            }
            Statement::Expr(e) | Statement::Yield(e) | Statement::Return(Some(e)) => {
                expression(e, visit)
            }
            Statement::SetIndex { target, value } | Statement::Augment { target, value, .. } => {
                expression(target, visit);
                expression(value, visit);
            }
            Statement::If {
                condition,
                yes,
                elifs,
                no,
            } => {
                expression(condition, visit);
                block(yes, visit);
                for arm in elifs {
                    expression(&arm.condition, visit);
                    block(&arm.body, visit);
                }
                block(no, visit);
            }
            Statement::While { condition, body } => {
                expression(condition, visit);
                block(body, visit);
            }
            Statement::For {
                name,
                iterable,
                body,
            } => {
                for name in name {
                    visit(Node::Binding(name));
                }
                expression(iterable, visit);
                block(body, visit);
            }
            Statement::With {
                manager,
                name,
                body,
            } => {
                if let Some(name) = name {
                    visit(Node::Binding(name));
                }
                expression(manager, visit);
                block(body, visit);
            }
            Statement::Match { value, cases } => matching(value, cases, visit),
            Statement::Return(None) | Statement::Pass | Statement::Break | Statement::Continue => {}
        }
    }
}

fn matching(value: &Expr, cases: &[enums::Case], visit: &mut impl FnMut(Node<'_>)) {
    expression(value, visit);
    for case in cases {
        pattern(&case.pattern, visit);
        block(&case.body, visit);
    }
}

fn pattern(p: &patterns::Pat, visit: &mut impl FnMut(Node<'_>)) {
    use patterns::PatKind;
    match &p.kind {
        PatKind::Binding(name) => visit(Node::Binding(name)),
        PatKind::Variant(_, _, Some(patterns), _)
        | PatKind::Or(patterns)
        | PatKind::Tuple(patterns) => {
            for p in patterns {
                pattern(p, visit);
            }
        }
        PatKind::Class { fields, .. } => {
            for field in fields {
                pattern(&field.pattern, visit);
            }
        }
        PatKind::Wildcard
        | PatKind::Member(..)
        | PatKind::Variant(_, _, None, _)
        | PatKind::Literal(_) => {}
    }
}

fn expression(e: &Expr, visit: &mut impl FnMut(Node<'_>)) {
    visit(Node::Expr(e));
    match &e.kind {
        Expression::Call(_, args)
        | Expression::GenericCall(_, _, args)
        | Expression::Tuple(args)
        | Expression::Constructor(_, args) => {
            for arg in args {
                expression(arg, visit);
            }
        }
        Expression::Method(base, _, args, _)
        | Expression::GenericMethod(base, _, _, args, _)
        | Expression::Invoke(base, args) => {
            expression(base, visit);
            for arg in args {
                expression(arg, visit);
            }
        }
        Expression::Member(base, _, _)
        | Expression::NamedArgument(_, base)
        | Expression::Unary(_, base)
        | Expression::Group(base)
        | Expression::Try(base)
        | Expression::ClassReady(_, base) => expression(base, visit),
        Expression::Index(a, b) | Expression::Binary(_, a, b) => {
            expression(a, visit);
            expression(b, visit);
        }
        Expression::Slice(base, bounds) => {
            expression(base, visit);
            for bound in bounds.iter() {
                expression(bound, visit);
            }
        }
        Expression::Conditional { condition, yes, no } => {
            expression(condition, visit);
            expression(yes, visit);
            expression(no, visit);
        }
        Expression::Match(value, cases) => matching(value, cases, visit),
        Expression::Checked(_, body) | Expression::If(body) => block(body, visit),
        Expression::Anonymous(f) => {
            for (name, _) in &f.inputs {
                visit(Node::Binding(name));
            }
            for capture in &f.captures {
                visit(Node::Binding(&capture.name));
            }
            for default in f.defaults.iter().flatten() {
                expression(default, visit);
            }
            block(&f.body, visit);
        }
        Expression::Format(format) => {
            for arg in format.operands() {
                expression(arg, visit);
            }
        }
        Expression::Collection {
            entries, clauses, ..
        } => {
            for clause in clauses {
                match clause {
                    Clause::For(names, e) => {
                        for name in names {
                            visit(Node::Binding(name));
                        }
                        expression(e, visit);
                    }
                    Clause::Unpack(names, _, e) => {
                        for name in names {
                            visit(Node::Binding(name));
                        }
                        expression(e, visit);
                    }
                    Clause::If(e) => expression(e, visit),
                }
            }
            for (k, v) in entries {
                expression(k, visit);
                if let Some(v) = v {
                    expression(v, visit);
                }
            }
        }
        Expression::Ambiguous { generic, index } => {
            expression(generic, visit);
            if let Ok(index) = index {
                expression(index, visit);
            }
        }
        Expression::Name(_)
        | Expression::GenericValue(..)
        | Expression::Type(_)
        | Expression::Number(_)
        | Expression::Text(_)
        | Expression::Bytes(_)
        | Expression::Bool(_)
        | Expression::Unit
        | Expression::ClassNew(..) => {}
    }
}
