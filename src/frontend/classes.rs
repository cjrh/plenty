//! Fixed-layout records, method expansion, and constructor initialization checks.
use super::*;
use crate::record::{method, ClassOp, ClassType};

#[derive(Clone)]
pub(super) struct ClassDecl {
    pub(super) at: Token,
    pub(super) name: String,
    pub(super) type_params: Vec<(String, Option<TypeRef>)>,
    pub(super) fields: Vec<(String, TypeRef)>,
    pub(super) methods: Vec<Function>,
    pub(super) public_members: HashSet<String>,
}

impl Parser {
    pub(super) fn class_decl(&mut self) -> Result<ClassDecl> {
        let at = self.take();
        let name = self.name()?;
        let type_params = self.type_parameters()?;
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
                let mut f = self.function_in(Some(&name))?;
                if f.type_params
                    .iter()
                    .any(|(n, _)| type_params.iter().any(|(owner, _)| owner == n))
                {
                    return Err(f
                        .at
                        .error("method type parameters cannot shadow class type parameters"));
                }
                if let Some((_, receiver)) = f.inputs.first_mut() {
                    if let Some(owner) = receiver
                        .args
                        .first_mut()
                        .filter(|t| t.name.as_ref() == Some(&name) && t.args.is_empty())
                    {
                        owner.args = type_params.iter().map(|(n, _)| type_ref(&at, n)).collect();
                    }
                }
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
            if matches!(member.as_str(), "__new__" | "new" | "self") {
                return Err(at.error(format!("reserved class member `{member}`")));
            }
        }
        self.kind(Kind::Dedent, "the end of the class declaration")?;
        Ok(ClassDecl {
            at,
            name,
            type_params,
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
        Ok(Ty::Class(crate::nominal::Nominal::new(ClassType {
            name: self.name.clone(),
            fallible_init: match self.methods.iter().find(|f| f.name == "__init__") {
                Some(f) => f.output.resolve(aliases)?.is_some(),
                None => false,
            },
            fields,
            destructor,
        })))
    }
}

fn type_ref(at: &Token, name: &str) -> TypeRef {
    TypeRef {
        concrete: None,
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
        let Some(Some(Ty::Class(ty))) = lookup_type(&class.name, aliases) else {
            unreachable!()
        };
        let at = &class.at;
        let mut methods = class.methods;
        if !methods.iter().any(|m| m.name == "__init__") {
            let mut inputs = vec![(
                "self".into(),
                TypeRef {
                    concrete: None,
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
                once: false,
                captures: vec![],
                foreign: None,
                export: None,
                type_params: vec![],
                name: "__init__".into(),
                at: at.clone(),
                inputs,
                output: TypeRef {
                    concrete: None,
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
            let output = if f.type_params.is_empty() {
                f.output.resolve(aliases)?
            } else {
                None
            };
            if special
                && (generators::yields(&f.body)
                    || (output.is_some()
                        && !(f.name == "__init__"
                            && output == Some(crate::sum::allocation_result()))))
            {
                return Err(f
                    .at
                    .error("__del__ must return (); __init__ must return () or Result[(), AllocError]; neither can yield"));
            }
            if f.name == "__del__" && f.inputs.len() != 1 {
                return Err(f.at.error("__del__ takes only self"));
            }
            if f.name == "__init__" {
                validate_init(f, &ty)?;
                constructor_inputs = f.inputs[1..].to_vec();
            }
            f.name = method(&class.name, &f.name);
        }
        let instance = "__plenty_instance";
        // Unused depth-64 declarations remain valid; constructing one would
        // exceed the checked constructor's Result nesting limit.
        if Ty::Class(ty.clone()).layout_depth() < 64 {
            let mut args = vec![expression(
                at,
                Expression::Unary("&mut".into(), Box::new(name(at, instance))),
            )];
            args.extend(constructor_inputs.iter().map(|(n, _)| name(at, n)));
            let init = expression(at, Expression::Call(method(&class.name, "__init__"), args));
            let allocation = expression(at, Expression::ClassNew(ty.clone()));
            let output = TypeRef {
                concrete: None,
                at: at.clone(),
                name: Some("Result".into()),
                args: vec![type_ref(at, &class.name), type_ref(at, "AllocError")],
            };
            functions.push(Function {
                once: false,
                captures: vec![],
                foreign: None,
                export: None,
                type_params: vec![],
                name: method(&class.name, "new"),
                at: at.clone(),
                inputs: constructor_inputs.clone(),
                output,
                doc: String::new(),
                body: vec![
                    statement(
                        at,
                        Statement::Assign {
                            name: instance.into(),
                            mutable: true,
                            annotation: None,
                            value: expression(at, Expression::Try(Box::new(allocation))),
                        },
                    ),
                    statement(
                        at,
                        Statement::Expr(if ty.get().fallible_init {
                            expression(at, Expression::Try(Box::new(init)))
                        } else {
                            init
                        }),
                    ),
                    statement(
                        at,
                        Statement::Expr(expression(
                            at,
                            Expression::ClassReady(
                                ty.clone(),
                                Box::new(expression(
                                    at,
                                    Expression::Unary("&mut".into(), Box::new(name(at, instance))),
                                )),
                            ),
                        )),
                    ),
                    statement(
                        at,
                        Statement::Expr(expression(
                            at,
                            Expression::Call("Ok".into(), vec![name(at, instance)]),
                        )),
                    ),
                ],
            });
        }
        functions.extend(methods);
    }
    Ok(functions)
}

/// Definite initialization is deliberately local: a partially initialized self
/// may only be accessed through already initialized fields, never passed away.
fn validate_init(f: &Function, ty: &crate::nominal::Nominal<ClassType>) -> Result<()> {
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
                Expression::Member(a, _)
                | Expression::Group(a)
                | Expression::Unary(_, a)
                | Expression::Try(a) => self.expr(a, initialized)?,
                Expression::Method(a, _, args)
                | Expression::GenericMethod(a, _, _, args)
                | Expression::Invoke(a, args) => {
                    self.expr(a, initialized)?;
                    for arg in args {
                        self.expr(arg, initialized)?;
                    }
                }
                Expression::Call(_, args)
                | Expression::GenericCall(_, _, args)
                | Expression::Constructor(_, args)
                | Expression::Tuple(args, _) => {
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
                                for name in n {
                                    self.binding(name, &e.at)?;
                                }
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
                    Statement::With {
                        manager,
                        name,
                        body,
                    } => {
                        self.expr(manager, &set)?;
                        if let Some(name) = name {
                            self.binding(name, &stmt.at)?;
                        }
                        if let Some(next) = self.block(body, set.clone())? {
                            set = next;
                        } else {
                            return Ok(None);
                        }
                    }
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
                    Statement::Unpack { names, value, .. } => {
                        for name in names {
                            self.binding(name, &stmt.at)?;
                        }
                        self.expr(value, &set)?;
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
                        let error = e.as_ref().is_some_and(
                            |e| matches!(&ungroup(e).kind, Expression::Call(n, _) if n == "Err"),
                        );
                        if !error {
                            self.complete(&set, &stmt.at)?;
                        }
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
                        for name in name {
                            self.binding(name, &stmt.at)?;
                        }
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
    let definition = ty.get();
    let check = Check { ty: &definition };
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
        if let Expression::Member(base, name) = &e.kind {
            if self.place_type(base) == Some(Ty::File) {
                if name != "closed" {
                    return Err(e.at.error(format!("unknown File property `{name}`")));
                }
                let (_, loans) = self.observe(base, ops)?;
                ops.push(Op::Collection(CollectionOp::FileClosed));
                return Ok((Ty::Bool, loans));
            }
        }
        if self.place_type(e).is_some() {
            let (reference, loan) = self.borrow(e, false, ops)?;
            let Ty::Ref(ty, _) = reference else {
                unreachable!()
            };
            ops.push(Op::ReadRef((*ty).clone()));
            return Ok(((*ty).clone(), vec![loan]));
        }
        let Expression::Member(base, name) = &e.kind else {
            unreachable!()
        };
        let (ty, loans) = self.observe(base, ops)?;
        let Ty::Class(class) = ty else {
            return Err(e.at.error("field access requires a class instance"));
        };
        let index = field_index(&class, name, &e.at)?;
        modules::check_member(self.access, &class.name, name, &e.at)?;
        let ty = class.get().fields[index].1.clone();
        ops.push(Op::Class(ClassOp::Field(class, index)));
        Ok((ty, loans))
    }
    pub(super) fn set_field(
        &mut self,
        target: &Expr,
        value: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let ty = self.place_type(target).ok_or_else(|| {
            target
                .at
                .error("assignment requires a named binding, field, or collection element")
        })?;
        // Evaluate the RHS before borrowing the destination. Keep its owner in a
        // local until all indices succeed, so `?` also cleans it up on failure.
        let actual = self.expr_expected(value, Some(ty.clone()), ops)?;
        self.same(actual, Some(ty.clone()), &value.at)?;
        let temp = self.slot(ty.clone(), &value.at)?;
        ops.push(Op::StoreLocal(temp));
        let mut indices = Vec::new();
        self.assignment_indices(target, &mut indices, ops)?;
        // Resolve addresses only once all user code has finished evaluating.
        let (_, loan) = self.borrow_with_indices(target, true, &mut Some(indices.iter()), ops)?;
        ops.push(Op::MoveLocal(temp, "assignment value".into()));
        ops.push(Op::Swap);
        ops.push(Op::WriteRef(ty));
        ops.push(Op::UseLoan(loan));
        Ok(())
    }
    pub(super) fn place_type(&self, e: &Expr) -> Option<Ty> {
        match &ungroup(e).kind {
            Expression::Index(base, _) => match self.place_type(base)? {
                Ty::List(element) | Ty::Dict(_, element) => Some((*element).clone()),
                _ => None,
            },
            Expression::Name(n) => self.names.get(n).map(|l| match &l.ty {
                Ty::Ref(t, _) => (**t).clone(),
                t => t.clone(),
            }),
            Expression::Member(base, n) => {
                let Ty::Class(t) = self.place_type(base)? else {
                    return None;
                };
                t.get()
                    .fields
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
        class: crate::nominal::Nominal<ClassType>,
        name: &str,
        types: Option<&[TypeRef]>,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if matches!(name, "__init__" | "__del__" | "__new__") {
            return Err(base.at.error("lifecycle methods cannot be called directly"));
        }
        let callee = method(&class.name, name);
        self.sync_data()?;
        modules::check_member(self.access, &class.name, name, &base.at)?;
        if let Some(template) = self.generics.templates.get(&callee) {
            let receiver = Expr {
                at: base.at.clone(),
                kind: Expression::Unary(
                    template.inputs[0].1.name.clone().unwrap(),
                    Box::new(base.clone()),
                ),
            };
            let arguments: Vec<_> = std::iter::once(receiver)
                .chain(args.iter().cloned())
                .collect();
            return self.generic_call(&callee, types, &arguments, &base.at, ops);
        }
        if types.is_some() {
            return Err(base.at.error(format!("`{name}` is not a generic method")));
        }
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
        ops.push(Op::Call(callee.clone()));
        self.call_reference_result(&callee, &sig, &loans, ops);
        Self::end_reads(loans, ops);
        Ok(sig.outputs.first().cloned())
    }

    pub(super) fn generic_method(
        &mut self,
        base: &Expr,
        name: &str,
        types: &[TypeRef],
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if let Some(Ty::Class(class)) = self.place_type(base) {
            return self.class_method(base, class, name, Some(types), args, ops);
        }
        let observed = self.observe(base, ops)?;
        self.temporary_class_method(base, name, Some(types), args, observed, ops)
    }

    pub(super) fn temporary_class_method(
        &mut self,
        base: &Expr,
        name: &str,
        types: Option<&[TypeRef]>,
        args: &[Expr],
        observed: (Ty, Vec<usize>),
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let (ty, loans) = observed;
        let Ty::Class(class) = &ty else {
            return Err(base.at.error("generic methods require a class receiver"));
        };
        let slot = self.slot(ty.clone(), &base.at)?;
        ops.push(Op::StoreLocal(slot));
        let receiver = format!("__plenty_receiver_{slot}");
        self.names.insert(
            receiver.clone(),
            Local {
                slot,
                ty: ty.clone(),
                mutable: loans.is_empty(),
            },
        );
        let expr = Expr {
            at: base.at.clone(),
            kind: Expression::Name(receiver.clone()),
        };
        let result = self.class_method(&expr, class.clone(), name, types, args, ops)?;
        if matches!(result, Some(Ty::Ref(..))) {
            return Err(base
                .at
                .error("reference-returning methods require a named receiver or class field"));
        }
        self.names.remove(&receiver);
        self.expression_temps.push(slot);
        Self::end_reads(loans, ops);
        Ok(result)
    }
}

pub(super) fn field_index(
    class: &crate::nominal::Nominal<ClassType>,
    name: &str,
    at: &Token,
) -> Result<usize> {
    class
        .get()
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
            Op::TailCallIndirect(signature) => {
                out.push(Op::CallIndirect(signature));
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
