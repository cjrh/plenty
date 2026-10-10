use super::*;
use crate::sum::EnumOp;

#[derive(Clone)]
pub(super) struct EnumDecl {
    pub(super) at: Token,
    pub(super) name: String,
    pub(super) type_params: Vec<(String, Option<TypeRef>)>,
    pub(super) variants: Vec<(String, Vec<TypeRef>)>,
}
#[derive(Clone)]
pub(super) struct Case {
    at: Token,
    pub(super) pattern: Option<(Option<TypeRef>, String, Option<Vec<String>>)>,
    pub(super) body: Vec<Stmt>,
}

impl Parser {
    pub(super) fn enum_decl(&mut self) -> Result<EnumDecl> {
        let at = self.take();
        let name = self.name()?;
        let type_params = self.type_parameters()?;
        self.expect(":")?;
        self.kind(Kind::Newline, "a newline after `:`")?;
        self.kind(Kind::Indent, "an indented enum declaration")?;
        let mut variants = Vec::new();
        while !matches!(self.peek().kind, Kind::Dedent | Kind::Eof) {
            let variant = self.name()?;
            if variants.iter().any(|(name, _)| name == &variant) {
                return Err(self.peek().error(format!("duplicate variant `{variant}`")));
            }
            let mut fields = Vec::new();
            if self.eat("(") {
                while !self.eat(")") {
                    fields.push(self.ty()?);
                    if self.eat(")") {
                        break;
                    }
                    self.expect(",")?;
                }
                if fields.is_empty() {
                    return Err(at.error("declare a nullary variant without parentheses"));
                }
            }
            self.kind(Kind::Newline, "the end of the variant declaration")?;
            variants.push((variant, fields));
        }
        self.kind(Kind::Dedent, "the end of the enum declaration")?;
        if variants.is_empty() {
            return Err(at.error("an enum needs at least one variant"));
        }
        Ok(EnumDecl {
            at,
            name,
            type_params,
            variants,
        })
    }

    pub(super) fn match_statement(&mut self) -> Result<Stmt> {
        let at = self.take();
        let value = self.expr(0)?;
        self.expect(":")?;
        self.kind(Kind::Newline, "a newline after `:`")?;
        self.kind(Kind::Indent, "indented case arms")?;
        let mut cases = Vec::new();
        while self.peek().is("case") {
            let at = self.take();
            let pattern = if self.eat("_") {
                None
            } else {
                let mut ty = self.ty()?;
                let (owner, variant) = if self.eat(".") {
                    (Some(ty), self.name()?)
                } else if ty.args.is_empty() && ty.name.as_ref().is_some_and(|n| n.contains('.')) {
                    let name = ty.name.take().unwrap();
                    let (owner, variant) = name.rsplit_once('.').unwrap();
                    ty.name = Some(owner.into());
                    (Some(ty), variant.into())
                } else if ty.args.is_empty() && ty.name.as_deref().is_some_and(prelude_variant) {
                    (None, ty.name.unwrap())
                } else {
                    return Err(at.error("qualify user-defined variants with their enum type"));
                };
                let bindings = if self.eat("(") {
                    let mut bindings = Vec::new();
                    while !self.eat(")") {
                        bindings.push(self.name()?);
                        if self.eat(")") {
                            break;
                        }
                        self.expect(",")?;
                    }
                    Some(bindings)
                } else {
                    None
                };
                Some((owner, variant, bindings))
            };
            let body = self.suite()?;
            cases.push(Case { at, pattern, body });
        }
        self.kind(Kind::Dedent, "the end of the match cases")?;
        if cases.is_empty() {
            return Err(at.error("match requires case arms"));
        }
        Ok(Stmt {
            at,
            kind: Statement::Match { value, cases },
        })
    }
}

/// Reserve nominal identities before resolving transparent aliases. Definitions
/// are finalized from a work queue after every alias is available.
pub(super) fn resolve_types(
    aliases: &[TypeAlias],
    enums: &[EnumDecl],
    classes: &[classes::ClassDecl],
) -> Result<TypeAliases> {
    enum Decl<'a> {
        Alias(&'a TypeAlias),
        Enum(&'a EnumDecl),
        Class(&'a classes::ClassDecl),
    }
    impl Decl<'_> {
        fn name(&self) -> &str {
            match self {
                Self::Alias(a) => &a.name,
                Self::Enum(e) => &e.name,
                Self::Class(c) => &c.name,
            }
        }
        fn at(&self) -> &Token {
            match self {
                Self::Alias(a) => &a.at,
                Self::Enum(e) => &e.at,
                Self::Class(c) => &c.at,
            }
        }
        fn refs(&self) -> Vec<&TypeRef> {
            match self {
                Self::Alias(a) => vec![&a.target],
                Self::Enum(e) => e.variants.iter().flat_map(|(_, f)| f.iter()).collect(),
                Self::Class(c) => c.fields.iter().map(|(_, t)| t).collect(),
            }
        }
    }
    let declarations: Vec<_> = aliases
        .iter()
        .map(Decl::Alias)
        .chain(enums.iter().map(Decl::Enum))
        .chain(classes.iter().map(Decl::Class))
        .collect();
    let mut names = HashMap::new();
    for (i, declaration) in declarations.iter().enumerate() {
        let name = declaration.name();
        if builtin(name) {
            return Err(declaration
                .at()
                .error(format!("cannot redefine builtin `{name}` as a type")));
        }
        if names.insert(name, i).is_some() {
            return Err(declaration
                .at()
                .error(format!("type `{name}` is already defined")));
        }
    }
    let mut resolved = TypeAliases::with_data(enums, classes)?;
    for declaration in &declarations {
        let generic = match declaration {
            Decl::Alias(_) => continue,
            Decl::Enum(e) => !e.type_params.is_empty(),
            Decl::Class(c) => !c.type_params.is_empty(),
        };
        if !generic {
            let ty = resolved.instantiate(&TypeRef {
                concrete: None,
                at: declaration.at().clone(),
                name: Some(declaration.name().into()),
                args: vec![],
            })?;
            resolved.insert(declaration.name().into(), Some(ty));
        }
    }
    let mut active = HashSet::new();
    let mut path = Vec::new();
    let mut completed = HashSet::new();
    for root in 0..declarations.len() {
        if !matches!(declarations[root], Decl::Alias(_)) {
            continue;
        }
        let mut work = vec![(root, false)];
        while let Some((i, finish)) = work.pop() {
            let declaration = &declarations[i];
            let name = declaration.name();
            if completed.contains(&i) {
                continue;
            }
            if finish {
                completed.insert(i);
                active.remove(&i);
                debug_assert_eq!(path.pop(), Some(i));
                let Decl::Alias(alias) = declaration else {
                    unreachable!()
                };
                let ty = alias.target.resolve(&resolved)?;
                if ty.as_ref().is_some_and(|t| t.layout_depth() > 64) {
                    return Err(declaration
                        .at()
                        .error("type nesting exceeds the implementation limit of 64"));
                }
                resolved.insert(name.to_owned(), ty);
                continue;
            }
            if !active.insert(i) {
                let start = path.iter().position(|&node| node == i).unwrap();
                let cycle = &path[start..];
                let mut steps: Vec<_> = cycle
                    .iter()
                    .take(16)
                    .map(|&node| declarations[node].name().to_owned())
                    .collect();
                if cycle.len() > 16 {
                    steps.push(format!("... ({} declarations omitted)", cycle.len() - 16));
                }
                steps.push(name.to_owned());
                return Err(declaration
                    .at()
                    .error(format!("cyclic type alias: {}", steps.join(" -> "))));
            }
            path.push(i);
            work.push((i, true));
            let mut refs = declaration.refs();
            while let Some(t) = refs.pop() {
                refs.extend(&t.args);
                if let Some(name) = &t.name {
                    if let Some(&dep) = names.get(name.as_str()) {
                        if matches!(declarations[dep], Decl::Alias(_)) && !completed.contains(&dep)
                        {
                            work.push((dep, false));
                        }
                    }
                }
            }
        }
    }
    resolved.finish_types()?;
    Ok(resolved)
}

impl Lower<'_> {
    pub(super) fn prelude_constructor(
        &mut self,
        name: &str,
        args: Option<&[Expr]>,
        expected: Type,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if name == "Nothing" && args.is_some() {
            return Err(at.error("nullary variants do not take parentheses"));
        }
        if name != "Nothing" && !args.is_some_and(|a| a.len() == 1) {
            return Err(at.error(format!("variant `{name}` requires 1 payload argument")));
        }
        if let Some(ty) = expected {
            if !prelude_owner(name, &ty) {
                return Err(at.error(format!(
                    "`{name}` requires a {} context",
                    prelude_family(name)
                )));
            }
            return self.variant(ty, name, args, at, ops);
        }
        if name == "Some" && args.is_some_and(|a| a.len() == 1) {
            let arg = &args.unwrap()[0];
            let ty = self.expr(arg, ops)?.unwrap_or(Ty::Unit);
            if ty.contains_reference() {
                return Err(at.error("references cannot be stored in enum payloads"));
            }
            if ty == Ty::Unit {
                ops.push(Op::PushUnit);
            }
            if ty.layout_depth() >= 64 {
                return Err(at.error("type nesting exceeds the implementation limit of 64"));
            }
            let Ty::Enum(t) = crate::sum::option(ty) else {
                unreachable!()
            };
            ops.push(Op::Enum(EnumOp::New(t.clone(), 1)));
            return Ok(Some(Ty::Enum(t)));
        }
        Err(at.error(format!(
            "cannot infer `{name}`; provide an {} annotation, parameter, or return type",
            prelude_family(name)
        )))
    }
    pub(super) fn qualified_type(&self, base: &Expr) -> Result<Type> {
        match &base.kind {
            Expression::Type(ty) => ty.resolve(self.aliases),
            Expression::GenericValue(name, args) if self.aliases.data.contains(name) => TypeRef {
                concrete: None,
                at: base.at.clone(),
                name: Some(name.clone()),
                args: args.clone(),
            }
            .resolve(self.aliases),
            Expression::Name(name) if lookup_type(name, self.aliases).is_some() => {
                if self.names.contains_key(name) {
                    return Err(base
                        .at
                        .error(format!("binding `{name}` shadows a type qualifier")));
                }
                Ok(lookup_type(name, self.aliases).unwrap())
            }
            _ => Ok(None),
        }
    }
    pub(super) fn variant(
        &mut self,
        ty: Ty,
        name: &str,
        args: Option<&[Expr]>,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let Ty::Enum(mut t) = ty else {
            return Err(at.error("variant qualification requires an enum type"));
        };
        let tag = t
            .get()
            .variants
            .iter()
            .position(|v| v.name == name)
            .ok_or_else(|| at.error(format!("unknown variant `{}.{name}`", t.name)))?;
        let fields = t.get().variants[tag].fields.clone();
        if fields.is_empty() && args.is_some() {
            return Err(at.error("nullary variants do not take parentheses"));
        }
        if args.map_or(0, <[Expr]>::len) != fields.len() || (!fields.is_empty() && args.is_none()) {
            return Err(at.error(format!(
                "variant `{name}` requires {} payload arguments",
                fields.len()
            )));
        }
        for (arg, field) in args.unwrap_or(&[]).iter().zip(&fields) {
            let expected = if *field == Ty::Unit {
                None
            } else {
                Some(field.clone())
            };
            let actual = self.expr_expected(arg, expected.clone(), ops)?;
            self.same(actual.clone(), expected, &arg.at)?;
            if t.propagatable() && field.unresolved_generator() {
                let resolved = crate::generator::refine(field, &actual.unwrap()).unwrap();
                let updated = if t.is_option() {
                    crate::sum::option(resolved)
                } else if tag == 0 {
                    crate::sum::result(resolved, t.get().variants[1].fields[0].clone())
                } else {
                    crate::sum::result(t.get().variants[0].fields[0].clone(), resolved)
                };
                let Ty::Enum(updated) = updated else {
                    unreachable!()
                };
                t = updated;
            }
            if *field == Ty::Unit {
                ops.push(Op::PushUnit);
            }
        }
        let op = EnumOp::New(t, tag);
        let output = op.signature().unwrap().1;
        ops.push(Op::Enum(op));
        Ok(Some(output))
    }

    pub(super) fn match_cases(
        &mut self,
        value: &Expr,
        cases: &[Case],
        tail: bool,
        ops: &mut Vec<Op>,
    ) -> Result<BlockResult> {
        let temporary_start = self.expression_temps.len();
        let ty = match self.value(value, ops)? {
            // Matching a box matches its content: an owned box is consumed,
            // a borrowed one lends its content.
            Ty::Box(content) => {
                ops.push(Op::Box(crate::boxed::BoxOp::Take((*content).clone())));
                (*content).clone()
            }
            reference @ Ty::Ref(..) => self.unbox_reference(reference, ops),
            ty => ty,
        };
        let (t, borrowed) = match &ty {
            Ty::Enum(t) => (t.clone(), None),
            Ty::Ref(inner, mutable) => {
                let Ty::Enum(t) = &**inner else {
                    return Err(value.at.error("match requires an enum value or reference"));
                };
                (
                    t.clone(),
                    Some((self.reference_origin(value, ops)?, *mutable)),
                )
            }
            _ => return Err(value.at.error("match requires an enum value or reference")),
        };
        let source = self.slot(ty, &value.at)?;
        ops.push(Op::StoreLocal(source));
        self.finish_temporaries(temporary_start, ops);
        ops.push(Op::LoadLocal(source));
        if let Some((loan, mutable)) = borrowed {
            ops.push(Op::Access(self.loans[loan].root, false, Some(loan)));
            ops.push(Op::Enum(EnumOp::TagRef(t.clone(), mutable)));
            ops.push(Op::UseLoan(loan));
        } else {
            ops.push(Op::Enum(EnumOp::Tag(t.clone())));
        }
        let saved = self.names.clone();
        let mut covered = HashSet::new();
        let mut wildcard = false;
        let mut arms = Vec::new();
        let mut joined: Option<Type> = None;
        for case in cases {
            if wildcard || covered.len() == t.get().variants.len() {
                return Err(case.at.error("unreachable case"));
            }
            self.names = saved.clone();
            let case_start = self.locals.len();
            let mut body = Vec::new();
            let pattern = if let Some((owner, name, bindings)) = &case.pattern {
                if let Some(owner) = owner {
                    self.same(
                        owner.resolve(self.aliases)?,
                        Some(Ty::Enum(t.clone())),
                        &owner.at,
                    )?;
                } else if !prelude_owner(name, &Ty::Enum(t.clone())) {
                    return Err(case.at.error(format!(
                        "`{name}` matches only {} values; qualify user-defined variants",
                        prelude_family(name)
                    )));
                }
                let tag = t
                    .get()
                    .variants
                    .iter()
                    .position(|v| &v.name == name)
                    .ok_or_else(|| case.at.error(format!("unknown variant `{name}`")))?;
                if !covered.insert(tag) {
                    return Err(case.at.error("duplicate variant case"));
                }
                let fields = &t.get().variants[tag].fields;
                if bindings.as_ref().map_or(0, Vec::len) != fields.len()
                    || (fields.is_empty() && bindings.is_some())
                {
                    return Err(case.at.error(format!("case `{name}` requires {} payload bindings (no parentheses for nullary variants)", fields.len())));
                }
                let mut bound = HashSet::new();
                // A consuming match moves every field of a multi-field variant
                // out at once; unbound positions are dropped immediately.
                let split = borrowed.is_none() && fields.len() > 1;
                let mut split_slots = vec![None; fields.len()];
                for (field, (name, ty)) in bindings
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .zip(fields)
                    .enumerate()
                {
                    if name == "_" {
                        continue;
                    }
                    if !bound.insert(name) {
                        return Err(case.at.error("duplicate payload binding"));
                    }
                    let binding_ty = if let Some((_, mutable)) = borrowed {
                        Ty::Ref(Rc::new(ty.clone()), mutable)
                    } else {
                        ty.clone()
                    };
                    let slot = self.slot(binding_ty.clone(), &case.at)?;
                    self.names.insert(
                        name.clone(),
                        Local {
                            slot,
                            ty: binding_ty,
                            mutable: false,
                        },
                    );
                    if let Some((parent, mutable)) = borrowed {
                        let loan = self.new_loan(
                            self.loans[parent].root,
                            mutable,
                            Some(parent),
                            &mut body,
                        );
                        // A payload loan protects its enclosing variant from
                        // replacement, while sibling fields remain disjoint.
                        if self.loans[loan].precise {
                            self.loans[loan].fields.push(field);
                            if let Some(Op::Loan(fact)) = body.last_mut() {
                                *fact = self.loans[loan].clone();
                            }
                        }
                        self.reference_locals.insert(slot, loan);
                        body.extend([
                            Op::LoadLocal(source),
                            Op::Enum(EnumOp::FieldRef(t.clone(), tag, field, mutable)),
                            Op::StoreLocal(slot),
                        ]);
                    } else if split {
                        split_slots[field] = Some(slot);
                    } else {
                        body.extend([
                            Op::MoveLocal(
                                source,
                                format!("{}:{}: matched payload", case.at.line, case.at.column),
                            ),
                            Op::Enum(if ty.affine() {
                                EnumOp::Take(t.clone(), tag, field)
                            } else {
                                EnumOp::Field(t.clone(), tag, field)
                            }),
                            Op::StoreLocal(slot),
                        ]);
                    }
                }
                if split {
                    body.extend([
                        Op::MoveLocal(
                            source,
                            format!("{}:{}: matched payload", case.at.line, case.at.column),
                        ),
                        Op::Split(t.clone(), tag),
                    ]);
                    body.extend(
                        split_slots
                            .iter()
                            .rev()
                            .map(|slot| slot.map_or(Op::Drop, Op::StoreLocal)),
                    );
                }
                Pattern::Int {
                    value: Value::I64(tag as i64),
                    explicit_ty: true,
                }
            } else {
                wildcard = true;
                Pattern::Wildcard
            };
            if let BlockResult::Continues(ty) = self.block(&case.body, &mut body, tail)? {
                if !tail {
                    self.cleanup(case_start, &mut body);
                }
                if let Some(expected) = &joined {
                    self.same(ty.clone(), expected.clone(), &case.at)?;
                } else {
                    joined = Some(ty);
                }
            }
            arms.push(MatchArm {
                pattern,
                body: body.into(),
            });
        }
        self.names = saved;
        if !wildcard && covered.len() != t.get().variants.len() {
            let missing = t
                .get()
                .variants
                .iter()
                .enumerate()
                .filter(|(i, _)| !covered.contains(i))
                .map(|(_, v)| format!("{}.{}", t.name, v.name))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(value
                .at
                .error(format!("non-exhaustive match; missing {missing}")));
        }
        if !wildcard {
            arms.push(MatchArm {
                pattern: Pattern::Wildcard,
                body: vec![Op::Unreachable].into(),
            });
        }
        ops.push(Op::Match(arms.into()));
        if !tail && joined.is_some() {
            ops.push(Op::DropLocal(source));
        }
        Ok(joined.map_or(BlockResult::Exits, BlockResult::Continues))
    }
}

pub(super) fn prelude_variant(name: &str) -> bool {
    matches!(name, "Ok" | "Err" | "Some" | "Nothing")
}
fn prelude_family(name: &str) -> &'static str {
    if matches!(name, "Some" | "Nothing") {
        "Option"
    } else {
        "Result"
    }
}
fn prelude_owner(name: &str, ty: &Ty) -> bool {
    matches!(ty, Ty::Enum(t) if t.name.starts_with(&format!("{}[", prelude_family(name))))
}
