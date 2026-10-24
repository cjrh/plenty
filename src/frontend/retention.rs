//! Releases generic function templates during lowering.
//!
//! An ordinary function's tree is released once it is lowered, but a template
//! is instantiated by any later call, so its tree used to stay alive until
//! lowering ended: nearly twice the checking memory of the same program
//! without type parameters (issue #434).
//!
//! A template is instantiated only from a call or function value naming it.
//! Every body that is still to be lowered, or that can itself still be
//! instantiated, holds the templates it names. A body leaves the count when
//! its function is lowered and its tree released, or when its template is
//! released; a template held by nothing is removed in turn, since nothing
//! left can name it. Only free functions of user modules are tracked. The
//! standard library and generated code reach templates by synthesized names,
//! and methods are reached through a receiver type, so those templates stay.
//! Templates named by a body that is never counted, such as a default
//! expression, a call wrapper or a generic class's methods, stay as well. Counting a body that is
//! never lowered only keeps a template, never releases one early.
use super::*;

#[derive(Default)]
pub(super) struct Retention {
    /// Tracked templates by name. Empty when nothing can be released.
    ids: HashMap<Arc<str>, u32>,
    templates: Vec<Tracked>,
    /// The templates each recorded, not yet released function names.
    held: HashMap<functions::Symbol, Box<[u32]>>,
}

struct Tracked {
    name: Arc<str>,
    /// Bodies naming this template that may still be lowered or instantiated.
    holders: u32,
    /// Named by a body that is never released.
    pinned: bool,
    released: bool,
    /// The templates this template's body names, other than itself.
    names: Box<[u32]>,
    /// Whether this template's body names itself.
    recursive: bool,
}

impl Retention {
    /// Tracks the free generic functions of user modules among `functions`.
    pub(super) fn new(functions: &[Function], aliases: &TypeAliases) -> Self {
        let mut retention = Self::default();
        if ide::active() {
            // Editor queries read declarations after lowering.
            return retention;
        }
        for f in functions {
            if !f.type_params.is_empty()
                && !crate::record::is_method(&f.name)
                && !f.name.starts_with("std.")
                && !aliases.data.contains(&f.name)
            {
                let name: Arc<str> = f.name.as_str().into();
                let id =
                    u32::try_from(retention.templates.len()).expect("fewer than 2^32 templates");
                if retention.ids.insert(Arc::clone(&name), id).is_none() {
                    retention.templates.push(Tracked {
                        name,
                        holders: 0,
                        pinned: false,
                        released: false,
                        names: Box::default(),
                        recursive: false,
                    });
                }
            }
        }
        retention
    }

    fn active(&self) -> bool {
        !self.ids.is_empty()
    }

    /// The tracked templates `body` and `defaults` name, once each.
    fn named(&self, body: &[Stmt], defaults: &[Option<Expr>]) -> Vec<u32> {
        let mut named = Vec::new();
        let mut each = |name: &str| {
            if let Some(&id) = self.ids.get(name) {
                named.push(id);
            }
        };
        names_in_block(body, &mut each);
        for default in defaults.iter().flatten() {
            names_in_expr(default, &mut each);
        }
        named.sort_unstable();
        named.dedup();
        named
    }

    /// Counts the templates a template's body names. Untracked templates keep
    /// their bodies, so what they name is pinned.
    pub(super) fn template(&mut self, f: &Function) {
        if !self.active() {
            return;
        }
        let mut named = self.named(&f.body, &[]);
        match self.ids.get(f.name.as_str()).copied() {
            Some(own) => {
                // A recursive call is reached only through another holder.
                let recursive = named.contains(&own);
                named.retain(|&id| id != own);
                for &id in &named {
                    self.templates[id as usize].holders += 1;
                }
                let tracked = &mut self.templates[own as usize];
                tracked.names = named.into();
                tracked.recursive = recursive;
            }
            None => self.pin(&named),
        }
    }

    /// Pins the templates a body that is never released names.
    pub(super) fn keep(&mut self, body: &[Stmt]) {
        if self.active() {
            let named = self.named(body, &[]);
            self.pin(&named);
        }
    }

    /// Pins the templates default expressions name: they are lowered at
    /// each call that omits the argument.
    pub(super) fn keep_defaults(&mut self, defaults: &[Option<Expr>]) {
        if self.active() {
            let named = self.named(&[], defaults);
            self.pin(&named);
        }
    }

    fn pin(&mut self, named: &[u32]) {
        for &id in named {
            self.templates[id as usize].pinned = true;
        }
    }

    /// Counts the templates a recorded function names until it is released.
    pub(super) fn record(&mut self, symbol: functions::Symbol, f: &Function) {
        if !self.active() {
            return;
        }
        let named = self.named(&f.body, &[]);
        self.hold(symbol, named);
    }

    /// Counts the templates a new instance of `template` names until it is
    /// released. Substitution replaces only types, so the instance names
    /// what its template names; a tracked template's are already known.
    pub(super) fn instance(&mut self, symbol: functions::Symbol, template: &Function) {
        if !self.active() {
            return;
        }
        let named = match self.ids.get(template.name.as_str()) {
            Some(&own) => {
                let tracked = &self.templates[own as usize];
                let mut named = tracked.names.to_vec();
                if tracked.recursive {
                    named.push(own);
                }
                named
            }
            None => self.named(&template.body, &[]),
        };
        self.hold(symbol, named);
    }

    fn hold(&mut self, symbol: functions::Symbol, named: Vec<u32>) {
        if named.is_empty() {
            return;
        }
        for &id in &named {
            self.templates[id as usize].holders += 1;
        }
        match self.held.entry(symbol) {
            Entry::Vacant(entry) => {
                entry.insert(named.into());
            }
            // A declaration replaced before it was lowered keeps both counts.
            Entry::Occupied(mut entry) => {
                let mut both = entry.get().to_vec();
                both.extend(named);
                entry.insert(both.into());
            }
        }
    }

    /// Whether `name` is a template that has been released.
    pub(super) fn released(&self, name: &str) -> bool {
        self.ids
            .get(name)
            .is_some_and(|&id| self.templates[id as usize].released)
    }

    /// Drops what `symbol`'s released tree held, removing the templates
    /// that nothing holds any longer.
    pub(super) fn complete(
        &mut self,
        symbol: functions::Symbol,
        templates: &mut HashMap<String, Arc<Function>>,
    ) {
        let Some(named) = self.held.remove(&symbol) else {
            return;
        };
        let mut work = named.into_vec();
        while let Some(id) = work.pop() {
            let tracked = &mut self.templates[id as usize];
            tracked.holders -= 1;
            if tracked.holders > 0 || tracked.pinned || tracked.released {
                continue;
            }
            tracked.released = true;
            work.extend(std::mem::take(&mut tracked.names));
            templates.remove(&*tracked.name);
        }
    }
}

/// Calls `each` with every function name `body` can call or take as a value.
/// Module name resolution has made these names canonical symbols.
fn names_in_block(body: &[Stmt], each: &mut impl FnMut(&str)) {
    for stmt in body {
        match &stmt.kind {
            Statement::Assign { value: e, .. }
            | Statement::Expr(e)
            | Statement::Yield(e)
            | Statement::Unpack { value: e, .. }
            | Statement::Return(Some(e)) => names_in_expr(e, each),
            Statement::SetIndex { target, value } | Statement::Augment { target, value, .. } => {
                names_in_expr(target, each);
                names_in_expr(value, each);
            }
            Statement::If {
                condition,
                yes,
                elifs,
                no,
            } => {
                names_in_expr(condition, each);
                names_in_block(yes, each);
                for elif in elifs {
                    names_in_expr(&elif.condition, each);
                    names_in_block(&elif.body, each);
                }
                names_in_block(no, each);
            }
            Statement::While { condition: e, body }
            | Statement::For {
                iterable: e, body, ..
            }
            | Statement::With {
                manager: e, body, ..
            } => {
                names_in_expr(e, each);
                names_in_block(body, each);
            }
            Statement::Match { value, cases } => names_in_match(value, cases, each),
            Statement::Return(None) | Statement::Pass | Statement::Break | Statement::Continue => {}
        }
    }
}

fn names_in_match(value: &Expr, cases: &[enums::Case], each: &mut impl FnMut(&str)) {
    names_in_expr(value, each);
    for case in cases {
        names_in_block(&case.body, each);
    }
}

fn names_in_expr(e: &Expr, each: &mut impl FnMut(&str)) {
    match &e.kind {
        Expression::Name(name) | Expression::GenericValue(name, _) => each(name),
        Expression::Call(name, args) | Expression::GenericCall(name, _, args) => {
            each(name);
            for arg in args {
                names_in_expr(arg, each);
            }
        }
        Expression::Match(value, cases) => names_in_match(value, cases, each),
        Expression::Anonymous(function) => {
            names_in_block(&function.body, each);
            for default in function.defaults.iter().flatten() {
                names_in_expr(default, each);
            }
        }
        Expression::Tuple(args) | Expression::Constructor(_, args) => {
            for arg in args {
                names_in_expr(arg, each);
            }
        }
        Expression::Format(format) => {
            for arg in format.operands() {
                names_in_expr(arg, each);
            }
        }
        Expression::Method(base, _, args, _)
        | Expression::GenericMethod(base, _, _, args, _)
        | Expression::Invoke(base, args) => {
            names_in_expr(base, each);
            for arg in args {
                names_in_expr(arg, each);
            }
        }
        Expression::Member(base, _, _)
        | Expression::NamedArgument(_, base)
        | Expression::Unary(_, base)
        | Expression::Group(base)
        | Expression::Try(base)
        | Expression::ClassReady(_, base) => names_in_expr(base, each),
        Expression::Index(a, b) | Expression::Binary(_, a, b) => {
            names_in_expr(a, each);
            names_in_expr(b, each);
        }
        Expression::Conditional { condition, yes, no } => {
            names_in_expr(condition, each);
            names_in_expr(yes, each);
            names_in_expr(no, each);
        }
        Expression::Collection {
            entries, clauses, ..
        } => {
            for clause in clauses {
                match clause {
                    Clause::For(_, e) | Clause::If(e) => names_in_expr(e, each),
                }
            }
            for (k, v) in entries {
                names_in_expr(k, each);
                if let Some(v) = v {
                    names_in_expr(v, each);
                }
            }
        }
        // Both readings, should one survive module name resolution.
        Expression::Ambiguous { generic, index } => {
            names_in_expr(generic, each);
            names_in_expr(index, each);
        }
        Expression::Type(_)
        | Expression::Number(_)
        | Expression::Text(_)
        | Expression::Bytes(_)
        | Expression::Bool(_)
        | Expression::Unit
        | Expression::ClassNew(..) => {}
    }
}
