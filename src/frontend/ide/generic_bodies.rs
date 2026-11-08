//! Members named in generic bodies that lowering did not reach (issue #876).
//! Lowering records a member from the type it resolved, so it records nothing
//! for a generic function no call instantiates, or a method of a generic class
//! or enum no code uses. These bodies are walked from their declarations
//! instead, typing only what the declarations state: parameters, annotated
//! locals, fields, and method results. A receiver whose type is a
//! protocol-bounded type parameter refers to the protocol's requirement; one
//! of a declared class or enum refers to its member. Other receivers, such as
//! locals with inferred types, stay unresolved. A place lowering already
//! recorded keeps its record, so an instantiated body is unchanged.
use super::*;
use patterns::PatKind;

/// What the walk knows about the module declarations, and the generic bodies
/// to walk, copied before lowering consumes the resolved program.
pub(super) struct Bodies {
    classes: HashMap<String, Data>,
    enums: HashMap<String, Data>,
    /// Requirement names of each protocol.
    protocols: HashMap<String, HashSet<String>>,
    /// Each function's inputs, by symbol, for named arguments and results.
    functions: HashMap<String, Signature>,
    /// Generic bodies, with the type parameters of the declaration owning
    /// them (a class or enum) and that owner's name.
    bodies: Vec<(Function, Option<String>)>,
}

#[derive(Default)]
struct Data {
    type_params: TypeParams,
    fields: HashMap<String, TypeRef>,
    variants: HashSet<String>,
    methods: HashMap<String, Signature>,
    /// The parameter names `Class(...)` takes.
    constructor: Vec<String>,
}

struct Signature {
    inputs: Vec<String>,
    output: TypeRef,
    type_params: TypeParams,
}

type TypeParams = Vec<(String, Option<TypeRef>)>;

impl Signature {
    fn of(f: &Function) -> Self {
        Signature {
            inputs: f.inputs.iter().map(|(name, _)| name.clone()).collect(),
            output: f.output.clone(),
            type_params: f.type_params.clone(),
        }
    }
}

/// A receiver type the walk can resolve members of.
#[derive(Clone)]
enum Known {
    /// A declared class or enum, by symbol.
    Data(String),
    /// A type parameter bounded by this protocol.
    Bound(String),
}

impl Bodies {
    pub(super) fn of(resolved: &modules::Resolved) -> Self {
        let user = |at: &Token| {
            at.source()
                .is_some_and(|source| !resolved.standard_sources.contains(&source))
        };
        let mut bodies = Vec::new();
        let mut classes = HashMap::default();
        for c in &resolved.classes {
            let generic = !c.type_params.is_empty();
            let mut data = Data {
                type_params: c.type_params.clone(),
                fields: c.fields.iter().cloned().collect(),
                ..Data::default()
            };
            for f in &c.methods {
                data.methods.insert(f.name.clone(), Signature::of(f));
                if (generic || !f.type_params.is_empty()) && user(&f.at) {
                    bodies.push((f.clone(), Some(c.name.clone())));
                }
            }
            data.constructor = match c.methods.iter().find(|f| f.name == "__init__") {
                Some(init) => init.inputs.iter().skip(1).map(|(n, _)| n.clone()).collect(),
                None => c.fields.iter().map(|(n, _)| n.clone()).collect(),
            };
            classes.insert(c.name.clone(), data);
        }
        let mut enums = HashMap::default();
        for e in &resolved.enums {
            let generic = !e.type_params.is_empty();
            let mut data = Data {
                type_params: e.type_params.clone(),
                variants: e.variants.iter().map(|(v, _)| v.clone()).collect(),
                ..Data::default()
            };
            for f in &e.methods {
                data.methods.insert(f.name.clone(), Signature::of(f));
                if (generic || !f.type_params.is_empty()) && user(&f.at) {
                    bodies.push((f.clone(), Some(e.name.clone())));
                }
            }
            enums.insert(e.name.clone(), data);
        }
        let protocols = resolved
            .protocols
            .iter()
            .map(|p| {
                (
                    p.name.clone(),
                    p.methods.iter().map(|f| f.name.clone()).collect(),
                )
            })
            .collect();
        let mut functions = HashMap::default();
        for f in &resolved.functions {
            functions.insert(f.name.clone(), Signature::of(f));
            if !f.type_params.is_empty() && user(&f.at) {
                bodies.push((f.clone(), None));
            }
        }
        Bodies {
            classes,
            enums,
            protocols,
            functions,
            bodies,
        }
    }

    /// Record the members of every generic body at places lowering left
    /// unrecorded.
    pub(super) fn walk(&self) {
        let start = with(|r| r.references.len());
        for (f, owner) in &self.bodies {
            if skipped(&f.name) {
                continue;
            }
            let owner_params = owner
                .as_deref()
                .and_then(|o| self.data(o))
                .map(|d| d.type_params.as_slice())
                .unwrap_or_default();
            let walk = Walk {
                bodies: self,
                params: [owner_params, f.type_params.as_slice()].concat(),
            };
            let mut scope = HashMap::default();
            walk.inputs(f, owner.as_deref(), &mut scope);
            walk.block(&f.body, &mut scope);
        }
        with(|r| {
            r.inferred_from = Some(start);
            let added = r.references.split_off(start);
            let recorded: HashSet<Anchor> =
                r.references.iter().map(|(a, _, _)| a.clone()).collect();
            r.references.extend(
                added
                    .into_iter()
                    .filter(|(anchor, _, _)| !recorded.contains(anchor)),
            );
            // A place lowering resolved was checked in some instantiation.
            r.unresolved
                .retain(|(anchor, _)| !recorded.contains(anchor));
        });
    }

    fn data(&self, name: &str) -> Option<&Data> {
        self.classes.get(name).or_else(|| self.enums.get(name))
    }

    /// The callee and parameter names of a method of `known`.
    fn method(&self, known: &Known, name: &str) -> Option<(String, Option<&Signature>)> {
        match known {
            Known::Data(owner) => {
                let signature = self.data(owner)?.methods.get(name)?;
                Some((crate::record::method(owner, name), Some(signature)))
            }
            Known::Bound(protocol) => self
                .protocols
                .get(protocol)?
                .contains(name)
                .then(|| (format!("{protocol}.{name}"), None)),
        }
    }
}

type Scope = HashMap<String, Known>;

struct Walk<'a> {
    bodies: &'a Bodies,
    /// The type parameters in scope: the owner's, then the function's.
    params: TypeParams,
}

impl Walk<'_> {
    fn inputs(&self, f: &Function, owner: Option<&str>, scope: &mut Scope) {
        for (index, (name, ty)) in f.inputs.iter().enumerate() {
            let known = self.known(ty, &self.params).or_else(|| {
                (index == 0 && name == "self")
                    .then(|| owner.map(|o| Known::Data(o.to_owned())))
                    .flatten()
            });
            bind(scope, name, known);
        }
    }

    /// The type `ty` names, read with the type parameters `params`.
    fn known(&self, ty: &TypeRef, params: &[(String, Option<TypeRef>)]) -> Option<Known> {
        let name = ty.name.as_deref()?;
        if matches!(name, "&" | "&mut" | "Box") && ty.args.len() == 1 {
            return self.known(&ty.args[0], params);
        }
        if let Some((_, bound)) = params.iter().find(|(p, _)| p == name) {
            let protocol = bound.as_ref()?.name.as_deref()?;
            return self
                .bodies
                .protocols
                .contains_key(protocol)
                .then(|| Known::Bound(protocol.to_owned()));
        }
        self.bodies
            .data(name)
            .is_some()
            .then(|| Known::Data(name.to_owned()))
    }

    fn block(&self, body: &[Stmt], scope: &mut Scope) {
        for s in body {
            self.statement(s, scope);
        }
    }

    /// A nested block, whose bindings end with it.
    fn nested(&self, body: &[Stmt], scope: &Scope, shadowed: &[&String]) {
        let mut inner = scope.clone();
        for name in shadowed {
            inner.remove(*name);
        }
        self.block(body, &mut inner);
    }

    fn statement(&self, s: &Stmt, scope: &mut Scope) {
        match &s.kind {
            Statement::Unpack {
                names,
                targets,
                value,
                ..
            } => {
                self.expr(value, scope);
                for target in targets.iter().flatten() {
                    self.expr(target, scope);
                }
                for name in names {
                    scope.remove(name);
                }
            }
            Statement::With {
                manager,
                name,
                body,
            } => {
                self.expr(manager, scope);
                let shadowed: Vec<&String> = name.iter().collect();
                self.nested(body, scope, &shadowed);
            }
            Statement::Yield(e) | Statement::Expr(e) | Statement::Return(Some(e)) => {
                self.expr(e, scope)
            }
            Statement::Match { value, cases } => {
                self.expr(value, scope);
                self.cases(cases, scope);
            }
            Statement::Assign {
                name,
                annotation,
                value,
                ..
            } => {
                self.expr(value, scope);
                // Assigning an existing local keeps its type; only a new
                // binding can introduce one.
                if let Some(annotation) = annotation {
                    let known = self.known(annotation, &self.params);
                    bind(scope, name, known);
                } else if !scope.contains_key(name) {
                    let known = self.result(value, scope);
                    bind(scope, name, known);
                }
            }
            Statement::If {
                condition,
                yes,
                elifs,
                no,
            } => {
                self.expr(condition, scope);
                self.nested(yes, scope, &[]);
                for elif in elifs {
                    self.expr(&elif.condition, scope);
                    self.nested(&elif.body, scope, &[]);
                }
                self.nested(no, scope, &[]);
            }
            Statement::While { condition, body } => {
                self.expr(condition, scope);
                self.nested(body, scope, &[]);
            }
            Statement::For {
                name,
                iterable,
                body,
            } => {
                self.expr(iterable, scope);
                self.nested(body, scope, &name.iter().collect::<Vec<_>>());
            }
            Statement::SetIndex { target, value } | Statement::Augment { target, value, .. } => {
                self.expr(target, scope);
                self.expr(value, scope);
            }
            Statement::Return(None) | Statement::Pass | Statement::Break | Statement::Continue => {}
        }
    }

    fn cases(&self, cases: &[enums::Case], scope: &Scope) {
        for case in cases {
            self.pattern(&case.pattern);
            let mut inner = scope.clone();
            for name in case.pattern.bindings() {
                inner.remove(name);
            }
            self.member_bindings(&case.pattern, &mut inner);
            self.block(&case.body, &mut inner);
        }
    }

    /// `case name: Type` binds `name` with the written type.
    fn member_bindings(&self, pattern: &patterns::Pat, scope: &mut Scope) {
        match &pattern.kind {
            PatKind::Member(name, ty) => {
                let known = self.known(ty, &self.params);
                bind(scope, name, known);
            }
            PatKind::Or(alternatives) | PatKind::Tuple(alternatives) => {
                for p in alternatives {
                    self.member_bindings(p, scope);
                }
            }
            _ => {}
        }
    }

    fn pattern(&self, pattern: &patterns::Pat) {
        match &pattern.kind {
            PatKind::Variant(qualifier, name, payload, pos) => {
                if let Some(owner) = qualifier.as_ref().and_then(|q| q.name.as_deref())
                    && self
                        .bodies
                        .enums
                        .get(owner)
                        .is_some_and(|e| e.variants.contains(name))
                {
                    refer_member(&pattern.at, *pos, name, variant_key(owner, name));
                }
                for p in payload.iter().flatten() {
                    self.pattern(p);
                }
            }
            PatKind::Or(alternatives) | PatKind::Tuple(alternatives) => {
                for p in alternatives {
                    self.pattern(p);
                }
            }
            PatKind::Class { owner, fields, .. } => {
                let class = owner
                    .name
                    .as_deref()
                    .filter(|c| self.bodies.classes.contains_key(*c));
                for field in fields {
                    if let (Some(class), Some(label)) = (class, &field.name)
                        && self.bodies.classes[class].fields.contains_key(label)
                    {
                        refer(
                            &field.at,
                            Find::At(label.chars().count() as u32),
                            field_key(class, label),
                        );
                    }
                    self.pattern(&field.pattern);
                }
            }
            PatKind::Wildcard | PatKind::Binding(_) | PatKind::Member(..) | PatKind::Literal(_) => {
            }
        }
    }

    /// The type of `e`'s value, where the declarations state it.
    fn result(&self, e: &Expr, scope: &Scope) -> Option<Known> {
        match &e.kind {
            Expression::Name(name) => scope.get(name).cloned(),
            Expression::Group(inner) | Expression::Try(inner) => self.result(inner, scope),
            Expression::Member(base, name, _) => {
                let Known::Data(owner) = self.result(base, scope)? else {
                    return None;
                };
                let data = self.bodies.classes.get(&owner)?;
                self.known(data.fields.get(name)?, &data.type_params)
            }
            Expression::Method(..) | Expression::GenericMethod(..) => {
                let (base, name, _, _) = method_parts(e)?;
                let (_, signature) = self.bodies.method(&self.result(base, scope)?, name)?;
                let signature = signature?;
                let Known::Data(owner) = self.result(base, scope)? else {
                    return None;
                };
                let params = [
                    self.bodies.data(&owner)?.type_params.as_slice(),
                    &signature.type_params,
                ]
                .concat();
                self.known(&signature.output, &params)
            }
            Expression::Call(name, _) | Expression::GenericCall(name, _, _) => {
                if self.bodies.classes.contains_key(name) {
                    return Some(Known::Data(name.clone()));
                }
                let signature = self.bodies.functions.get(name)?;
                self.known(&signature.output, &signature.type_params)
            }
            _ => None,
        }
    }

    /// Named arguments of a call to `callee`, whose parameters are `inputs`.
    fn named(&self, args: &[Expr], callee: &str, inputs: &[String]) {
        for argument in args {
            if let Expression::NamedArgument(label, _) = &argument.kind
                && let Some(index) = inputs.iter().position(|p| p == label)
            {
                named_argument(&argument.at, label, callee, index);
            }
        }
    }

    fn exprs(&self, es: &[Expr], scope: &Scope) {
        for e in es {
            self.expr(e, scope);
        }
    }

    fn expr(&self, e: &Expr, scope: &Scope) {
        match &e.kind {
            Expression::NamedArgument(_, value) => self.expr(value, scope),
            Expression::Anonymous(f) => {
                let mut inner = scope.clone();
                for (name, ty) in &f.inputs {
                    let known = self.known(ty, &self.params);
                    bind(&mut inner, name, known);
                }
                self.block(&f.body, &mut inner);
            }
            Expression::Call(name, args) | Expression::GenericCall(name, _, args) => {
                if let Some(class) = self.bodies.classes.get(name) {
                    let new = crate::record::method(name, "new");
                    self.named(args, &new, &class.constructor);
                } else if let Some(signature) = self.bodies.functions.get(name) {
                    self.named(args, name, &signature.inputs);
                }
                self.exprs(args, scope);
            }
            Expression::Constructor(_, args)
            | Expression::Tuple(args)
            | Expression::Invoke(_, args) => {
                if let Expression::Invoke(callee, _) = &e.kind {
                    self.expr(callee, scope);
                }
                self.exprs(args, scope);
            }
            Expression::Member(base, name, pos) => {
                match self.result(base, scope) {
                    Some(Known::Data(owner))
                        if self
                            .bodies
                            .classes
                            .get(&owner)
                            .is_some_and(|c| c.fields.contains_key(name)) =>
                    {
                        refer_member(&e.at, *pos, name, field_key(&owner, name));
                    }
                    Some(_) => {}
                    None => {
                        if !self.qualified(e, base, name, *pos, scope) {
                            unresolved_member(&e.at, *pos, name);
                        }
                    }
                }
                self.expr(base, scope);
            }
            Expression::Method(..) | Expression::GenericMethod(..) => {
                let Some((base, name, args, pos)) = method_parts(e) else {
                    return;
                };
                if let Some(known) = self.result(base, scope)
                    && let Some((callee, signature)) = self.bodies.method(&known, name)
                {
                    refer_member(&e.at, pos, name, global(&callee));
                    if let Some(signature) = signature {
                        self.named(args, &callee, &signature.inputs);
                    }
                } else if self.result(base, scope).is_none()
                    && !self.qualified(e, base, name, pos, scope)
                {
                    unresolved_member(&e.at, pos, name);
                    for argument in args {
                        if let Expression::NamedArgument(label, _) = &argument.kind {
                            unresolved_label(&argument.at, label);
                        }
                    }
                }
                self.expr(base, scope);
                self.exprs(args, scope);
            }
            Expression::ClassReady(_, inner)
            | Expression::Try(inner)
            | Expression::Group(inner)
            | Expression::Unary(_, inner) => self.expr(inner, scope),
            Expression::Collection {
                entries, clauses, ..
            } => {
                let mut inner = scope.clone();
                for clause in clauses {
                    match clause {
                        Clause::For(names, source) => {
                            self.expr(source, &inner);
                            for name in names {
                                inner.remove(name);
                            }
                        }
                        Clause::Unpack(names, targets, source) => {
                            self.expr(source, &inner);
                            self.exprs(targets, &inner);
                            for name in names {
                                inner.remove(name);
                            }
                        }
                        Clause::If(condition) => self.expr(condition, &inner),
                    }
                }
                for (key, value) in entries {
                    self.expr(key, &inner);
                    if let Some(value) = value {
                        self.expr(value, &inner);
                    }
                }
            }
            Expression::Index(base, index) | Expression::Binary(_, base, index) => {
                self.expr(base, scope);
                self.expr(index, scope);
            }
            Expression::Slice(base, bounds) => {
                self.expr(base, scope);
                for bound in bounds.iter() {
                    self.expr(bound, scope);
                }
            }
            Expression::Conditional { condition, yes, no } => {
                self.expr(condition, scope);
                self.expr(yes, scope);
                self.expr(no, scope);
            }
            Expression::Match(value, cases) => {
                self.expr(value, scope);
                self.cases(cases, scope);
            }
            Expression::Checked(_, body) | Expression::If(body) => self.nested(body, scope, &[]),
            Expression::Ambiguous { generic, .. } => self.expr(generic, scope),
            Expression::GenericValue(..)
            | Expression::ClassNew(_)
            | Expression::Type(_)
            | Expression::Number(_)
            | Expression::Text(_)
            | Expression::Format(_)
            | Expression::Bytes(_)
            | Expression::Bool(_)
            | Expression::Unit
            | Expression::Name(_) => {}
        }
    }

    /// `Enum.Variant`, `Enum.Variant(...)` or `Type.method(...)`, written at
    /// `e`. False when `base` does not name a declared class or enum.
    fn qualified(&self, e: &Expr, base: &Expr, name: &str, pos: Pos, scope: &Scope) -> bool {
        let owner = match &base.kind {
            Expression::Name(owner) if !scope.contains_key(owner) => owner.as_str(),
            Expression::Type(ty) => match ty.name.as_deref() {
                Some(owner) => owner,
                None => return false,
            },
            _ => return false,
        };
        let Some(data) = self.bodies.data(owner) else {
            return false;
        };
        if data.variants.contains(name) {
            refer_member(&e.at, pos, name, variant_key(owner, name));
        } else if data.methods.contains_key(name) {
            let callee = crate::record::method(owner, name);
            refer_member(&e.at, pos, name, global(&callee));
        }
        true
    }
}

/// Bind `name`, shadowing any outer type it had.
fn bind(scope: &mut Scope, name: &str, known: Option<Known>) {
    match known {
        Some(known) => scope.insert(name.to_owned(), known),
        None => scope.remove(name),
    };
}

/// The receiver, name, arguments and name position of a method call.
fn method_parts(e: &Expr) -> Option<(&Expr, &str, &[Expr], Pos)> {
    match &e.kind {
        Expression::Method(base, name, args, pos) => Some((base, name, args, *pos)),
        Expression::GenericMethod(base, name, _, args, pos) => Some((base, name, args, *pos)),
        _ => None,
    }
}
