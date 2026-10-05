//! Fixed-layout records, method expansion, and constructor initialization checks.
use super::*;
use crate::record::{method, ClassOp, ClassType};

pub(super) struct ClassDecl {
    pub(super) at: Token,
    pub(super) name: String,
    pub(super) fields: Vec<(String, TypeRef)>,
    pub(super) methods: Vec<Function>,
    pub(super) public_members: HashSet<String>,
}

impl Parser {
    pub(super) fn class_decl(&mut self) -> Result<ClassDecl> {
        let at = self.take();
        let name = self.name()?;
        self.expect(":")?;
        self.kind(Kind::Newline, "a newline after `:`")?;
        self.kind(Kind::Indent, "an indented class declaration")?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut names = HashSet::new();
        let mut public_members = HashSet::new();
        while !matches!(self.peek().kind, Kind::Dedent | Kind::Eof) {
            let public = self.eat("pub");
            let member = if self.peek().is("def") {
                let f = self.function_in(Some(&name))?;
                let member = f.name.clone();
                methods.push(f);
                member
            } else if self.eat("pass") {
                if public {
                    return Err(at.error("pub requires a field or method declaration"));
                }
                self.kind(Kind::Newline, "the end of pass")?;
                continue;
            } else {
                let field = self.name()?;
                if matches!(field.as_str(), "__init__" | "__del__") {
                    return Err(at.error("lifecycle names must declare methods"));
                }
                self.expect(":")?;
                let ty = self.ty()?;
                self.kind(Kind::Newline, "the end of the field declaration")?;
                fields.push((field.clone(), ty));
                field
            };
            if !names.insert(member.clone()) {
                return Err(at.error(format!("duplicate class member `{member}`")));
            }
            if public {
                public_members.insert(member.clone());
            }
            if member == "__new__" || member == "self" {
                return Err(at.error(format!("reserved class member `{member}`")));
            }
        }
        self.kind(Kind::Dedent, "the end of the class declaration")?;
        Ok(ClassDecl {
            at,
            name,
            fields,
            methods,
            public_members,
        })
    }
}

impl ClassDecl {
    pub(super) fn resolve(&self, aliases: &TypeAliases) -> Result<Ty> {
        let fields = self
            .fields
            .iter()
            .map(|(n, t)| {
                let ty = t
                    .resolve(aliases)?
                    .ok_or_else(|| t.at.error("class fields cannot be unit"))?;
                if ty.restricted_storage() {
                    return Err(t
                        .at
                        .error("references and generators cannot be stored in class fields"));
                }
                Ok((n.clone(), ty))
            })
            .collect::<Result<Vec<_>>>()?;
        let destructor = self
            .methods
            .iter()
            .any(|f| f.name == "__del__")
            .then(|| method(&self.name, "__del__"));
        Ok(Ty::Class(Rc::new(ClassType {
            name: self.name.clone(),
            depth: 1 + fields
                .iter()
                .map(|(_, t)| t.layout_depth())
                .max()
                .unwrap_or(0),
            copyable: destructor.is_none() && fields.iter().all(|(_, t)| t.can_copy()),
            has_destructor: destructor.is_some() || fields.iter().any(|(_, t)| t.has_destructor()),
            fields,
            destructor,
        })))
    }
}

fn type_ref(at: &Token, name: &str) -> TypeRef {
    TypeRef {
        at: at.clone(),
        name: Some(name.into()),
        args: vec![],
    }
}
fn expression(at: &Token, kind: Expression) -> Expr {
    Expr {
        at: at.clone(),
        kind,
    }
}
fn name(at: &Token, n: &str) -> Expr {
    expression(at, Expression::Name(n.into()))
}
fn statement(at: &Token, kind: Statement) -> Stmt {
    Stmt {
        at: at.clone(),
        kind,
    }
}

pub(super) fn expand(classes: Vec<ClassDecl>, aliases: &TypeAliases) -> Result<Vec<Function>> {
    let mut functions = Vec::new();
    for class in classes {
        let Some(Ty::Class(ty)) = &aliases[&class.name] else {
            unreachable!()
        };
        let at = &class.at;
        let mut methods = class.methods;
        if !methods.iter().any(|m| m.name == "__init__") {
            let mut inputs = vec![(
                "self".into(),
                TypeRef {
                    at: at.clone(),
                    name: Some("&mut".into()),
                    args: vec![type_ref(at, &class.name)],
                },
            )];
            inputs.extend(class.fields.iter().cloned());
            let body = class
                .fields
                .iter()
                .map(|(field, _)| {
                    statement(
                        at,
                        Statement::SetIndex {
                            target: expression(
                                at,
                                Expression::Member(Box::new(name(at, "self")), field.clone()),
                            ),
                            value: name(at, field),
                        },
                    )
                })
                .collect();
            methods.push(Function {
                name: "__init__".into(),
                at: at.clone(),
                inputs,
                output: TypeRef {
                    at: at.clone(),
                    name: None,
                    args: vec![],
                },
                doc: String::new(),
                body,
            });
        }
        let mut constructor_inputs = Vec::new();
        for f in &mut methods {
            let special = matches!(f.name.as_str(), "__init__" | "__del__");
            let Some((receiver, receiver_type)) = f.inputs.first() else {
                return Err(f.at.error("methods require a self receiver"));
            };
            let resolved = receiver_type.resolve(aliases)?;
            if receiver != "self"
                || !matches!(&resolved, Some(Ty::Ref(t, m)) if **t == Ty::Class(ty.clone()) && (!special || *m))
            {
                return Err(f.at.error("method receiver must be self: &Class (or self: &mut Class); lifecycle methods require &mut"));
            }
            if special && (f.output.resolve(aliases)?.is_some() || generators::yields(&f.body)) {
                return Err(f
                    .at
                    .error("__init__ and __del__ must return () and cannot yield"));
            }
            if f.name == "__del__" && f.inputs.len() != 1 {
                return Err(f.at.error("__del__ takes only self"));
            }
            if f.name == "__init__" {
                validate_init(f, ty)?;
                constructor_inputs = f.inputs[1..].to_vec();
            }
            f.name = method(&class.name, &f.name);
        }
        let instance = "__plenty_instance";
        let mut args = vec![expression(
            at,
            Expression::Unary("&mut".into(), Box::new(name(at, instance))),
        )];
        args.extend(constructor_inputs.iter().map(|(n, _)| name(at, n)));
        functions.push(Function {
            name: method(&class.name, "__new__"),
            at: at.clone(),
            inputs: constructor_inputs,
            output: type_ref(at, &class.name),
            doc: String::new(),
            body: vec![
                statement(
                    at,
                    Statement::Assign {
                        name: instance.into(),
                        mutable: true,
                        annotation: None,
                        value: expression(at, Expression::ClassNew(ty.clone())),
                    },
                ),
                statement(
                    at,
                    Statement::Expr(expression(
                        at,
                        Expression::Call(method(&class.name, "__init__"), args),
                    )),
                ),
                statement(at, Statement::Expr(name(at, instance))),
            ],
        });
        functions.extend(methods);
    }
    Ok(functions)
}

/// Definite initialization is deliberately local: a partially initialized self
/// may only be accessed through already initialized fields, never passed away.
fn validate_init(f: &Function, ty: &ClassType) -> Result<()> {
    struct Check<'a> {
        ty: &'a ClassType,
    }
    impl Check<'_> {
        fn complete(&self, initialized: &HashSet<String>, at: &Token) -> Result<()> {
            let missing: Vec<_> = self
                .ty
                .fields
                .iter()
                .filter(|(n, _)| !initialized.contains(n))
                .map(|(n, _)| n.as_str())
                .collect();
            if missing.is_empty() {
                Ok(())
            } else {
                Err(at.error(format!("fields not initialized: {}", missing.join(", "))))
            }
        }
        fn expr(&self, e: &Expr, initialized: &HashSet<String>) -> Result<()> {
            match &e.kind {
                Expression::Name(n) if n == "self" => self.complete(initialized, &e.at)?,
                Expression::Member(base, field) if matches!(&ungroup(base).kind, Expression::Name(n) if n == "self") => {
                    if !initialized.contains(field) {
                        return Err(e.at.error(format!("field `{field}` is not initialized")));
                    }
                }
                Expression::Member(a, _) | Expression::Group(a) | Expression::Unary(_, a) => {
                    self.expr(a, initialized)?
                }
                Expression::Method(a, _, args) => {
                    self.expr(a, initialized)?;
                    for arg in args {
                        self.expr(arg, initialized)?;
                    }
                }
                Expression::Call(_, args) | Expression::Constructor(_, args) => {
                    for arg in args {
                        self.expr(arg, initialized)?;
                    }
                }
                Expression::Binary(_, a, b) | Expression::Index(a, b) => {
                    self.expr(a, initialized)?;
                    self.expr(b, initialized)?;
                }
                Expression::Conditional { condition, yes, no } => {
                    self.expr(condition, initialized)?;
                    self.expr(yes, initialized)?;
                    self.expr(no, initialized)?;
                }
                Expression::Collection {
                    entries, clauses, ..
                } => {
                    for clause in clauses {
                        match clause {
                            Clause::For(n, e) => {
                                self.binding(n, &e.at)?;
                                self.expr(e, initialized)?;
                            }
                            Clause::If(e) => self.expr(e, initialized)?,
                        }
                    }
                    for (a, b) in entries {
                        self.expr(a, initialized)?;
                        if let Some(b) = b {
                            self.expr(b, initialized)?;
                        }
                    }
                }
                _ => {}
            }
            Ok(())
        }
        fn binding(&self, n: &str, at: &Token) -> Result<()> {
            if n == "self" {
                Err(at.error("cannot rebind self in a constructor"))
            } else {
                Ok(())
            }
        }
        fn block(
            &self,
            body: &[Stmt],
            mut set: HashSet<String>,
        ) -> Result<Option<HashSet<String>>> {
            for stmt in body {
                match &stmt.kind {
                    Statement::SetIndex { target, value } => {
                        self.expr(value, &set)?;
                        if let Expression::Member(base, field) = &target.kind {
                            if matches!(&ungroup(base).kind, Expression::Name(n) if n == "self") {
                                if !self.ty.fields.iter().any(|(n, _)| n == field) {
                                    return Err(target
                                        .at
                                        .error(format!("unknown field `{field}`")));
                                }
                                set.insert(field.clone());
                                continue;
                            }
                        }
                        self.expr(target, &set)?;
                    }
                    Statement::Assign { name, value, .. } => {
                        self.binding(name, &stmt.at)?;
                        self.expr(value, &set)?;
                    }
                    Statement::Expr(e) => self.expr(e, &set)?,
                    Statement::Return(e) => {
                        if let Some(e) = e {
                            self.expr(e, &set)?;
                        }
                        self.complete(&set, &stmt.at)?;
                        return Ok(None);
                    }
                    Statement::If { condition, yes, no } => {
                        self.expr(condition, &set)?;
                        let a = self.block(yes, set.clone())?;
                        let b = self.block(no, set.clone())?;
                        match (a, b) {
                            (Some(a), Some(b)) => set = a.intersection(&b).cloned().collect(),
                            (Some(a), None) | (None, Some(a)) => set = a,
                            (None, None) => return Ok(None),
                        }
                    }
                    Statement::While { condition, body } => {
                        self.expr(condition, &set)?;
                        self.block(body, set.clone())?;
                    }
                    Statement::For {
                        name,
                        iterable,
                        body,
                    } => {
                        self.binding(name, &stmt.at)?;
                        self.expr(iterable, &set)?;
                        self.block(body, set.clone())?;
                    }
                    Statement::Match { value, cases } => {
                        self.expr(value, &set)?;
                        let mut continuing = Vec::new();
                        for case in cases {
                            if let Some((_, _, Some(names))) = &case.pattern {
                                for n in names {
                                    self.binding(n, &stmt.at)?;
                                }
                            }
                            if let Some(s) = self.block(&case.body, set.clone())? {
                                continuing.push(s);
                            }
                        }
                        let Some(mut merged) = continuing.pop() else {
                            return Ok(None);
                        };
                        for s in continuing {
                            merged.retain(|n| s.contains(n));
                        }
                        set = merged;
                    }
                    Statement::Break | Statement::Continue => return Ok(None),
                    Statement::Yield { .. } => {
                        return Err(stmt.at.error("constructors cannot yield"))
                    }
                    Statement::Pass => {}
                }
            }
            Ok(Some(set))
        }
    }
    let check = Check { ty };
    if let Some(set) = check.block(&f.body, HashSet::new())? {
        check.complete(&set, &f.at)?;
    }
    Ok(())
}

impl Lower<'_> {
    pub(super) fn finish_temporaries(&mut self, start: usize, ops: &mut Vec<Op>) {
        for slot in self.expression_temps.drain(start..).rev() {
            ops.push(Op::DropLocal(slot));
        }
    }
    pub(super) fn field(&mut self, e: &Expr, ops: &mut Vec<Op>) -> Result<(Ty, Vec<usize>)> {
        let Expression::Member(base, name) = &e.kind else {
            unreachable!()
        };
        let (ty, loans) = self.observe(base, ops)?;
        let Ty::Class(class) = ty else {
            return Err(e.at.error("field access requires a class instance"));
        };
        let index = field_index(&class, name, &e.at)?;
        modules::check_member(self.access, &class.name, name, &e.at)?;
        let ty = class.fields[index].1.clone();
        ops.push(Op::Class(ClassOp::Field(class, index)));
        Ok((ty, loans))
    }
    pub(super) fn set_field(
        &mut self,
        target: &Expr,
        value: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        // Resolve the target without emitting its borrow until after evaluating the RHS.
        let mut address = Vec::new();
        let (reference, loan) = self.borrow(target, true, &mut address)?;
        let Ty::Ref(ty, _) = reference else {
            unreachable!()
        };
        let actual = self.expr_expected(value, Some((*ty).clone()), ops)?;
        self.same(actual, Some((*ty).clone()), &value.at)?;
        ops.extend(address);
        ops.push(Op::WriteRef((*ty).clone()));
        ops.push(Op::UseLoan(loan));
        Ok(())
    }
    pub(super) fn place_type(&self, e: &Expr) -> Option<Ty> {
        match &ungroup(e).kind {
            Expression::Name(n) => self.names.get(n).map(|l| match &l.ty {
                Ty::Ref(t, _) => (**t).clone(),
                t => t.clone(),
            }),
            Expression::Member(base, n) => {
                let Ty::Class(t) = self.place_type(base)? else {
                    return None;
                };
                t.fields
                    .iter()
                    .find(|(f, _)| f == n)
                    .map(|(_, t)| t.clone())
            }
            _ => None,
        }
    }
    pub(super) fn class_method(
        &mut self,
        base: &Expr,
        class: Rc<ClassType>,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if matches!(name, "__init__" | "__del__" | "__new__") {
            return Err(base.at.error("lifecycle methods cannot be called directly"));
        }
        let callee = method(&class.name, name);
        modules::check_member(self.access, &class.name, name, &base.at)?;
        let sig = self
            .sigs
            .get(&callee)
            .ok_or_else(|| {
                base.at
                    .error(format!("unknown method `{name}` on {}", class.name))
            })?
            .clone();
        if sig.inputs.len() != args.len() + 1 {
            return Err(base.at.error(format!(
                "method `{name}` expects {} arguments",
                sig.inputs.len() - 1
            )));
        }
        let Ty::Ref(_, mutable) = &sig.inputs[0].1 else {
            unreachable!()
        };
        let (_, loan) = self.borrow(base, *mutable, ops)?;
        let mut loans = vec![loan];
        loans.extend(self.call_arguments(args, &sig.inputs[1..], ops)?);
        ops.push(Op::Call(callee));
        Self::end_reads(loans, ops);
        Ok(sig.outputs.first().cloned())
    }
}

pub(super) fn field_index(class: &ClassType, name: &str, at: &Token) -> Result<usize> {
    class
        .fields
        .iter()
        .position(|(n, _)| n == name)
        .ok_or_else(|| at.error(format!("unknown field `{name}` on {}", class.name)))
}

/// Observable cleanup belongs after the callee returns. A normal call preserves
/// that order; numeric functions without resource locals retain tail calls.
pub(super) fn preserve_drop_order(body: &mut Vec<Op>) {
    let mut out = Vec::with_capacity(body.len());
    for op in body.drain(..) {
        match op {
            Op::TailCall(name) => {
                out.push(Op::Call(name));
                out.push(Op::Return);
            }
            Op::Match(arms) => {
                let arms = arms
                    .iter()
                    .map(|arm| {
                        let mut body = arm.body.to_vec();
                        preserve_drop_order(&mut body);
                        MatchArm {
                            pattern: arm.pattern,
                            body: body.into(),
                        }
                    })
                    .collect::<Vec<_>>();
                out.push(Op::Match(arms.into()));
            }
            Op::Loop { condition, body } => {
                let mut body = body.to_vec();
                preserve_drop_order(&mut body);
                out.push(Op::Loop {
                    condition,
                    body: body.into(),
                });
            }
            op => out.push(op),
        }
    }
    *body = out;
}
