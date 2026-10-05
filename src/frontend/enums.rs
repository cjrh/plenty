use super::*;
use crate::sum::{EnumOp, EnumType, Variant};

pub(super) struct EnumDecl {
    at: Token,
    name: String,
    variants: Vec<(String, Vec<TypeRef>)>,
}
pub(super) struct Case {
    at: Token,
    pattern: Option<(TypeRef, String, Option<Vec<String>>)>,
    pub(super) body: Vec<Stmt>,
}

impl Parser {
    pub(super) fn enum_decl(&mut self) -> Result<EnumDecl> {
        let at = self.take();
        let name = self.name()?;
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
        Ok(EnumDecl { at, name, variants })
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
                let ty = self.ty()?;
                self.expect(".")?;
                let variant = self.name()?;
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
                Some((ty, variant, bindings))
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

/// Resolve aliases and enum fields in one acyclic dependency graph. Each name
/// finishes once; an explicit work list also handles long alias chains.
pub(super) fn resolve_types(aliases: &[TypeAlias], enums: &[EnumDecl]) -> Result<TypeAliases> {
    enum Decl<'a> {
        Alias(&'a TypeAlias),
        Enum(&'a EnumDecl),
    }
    impl Decl<'_> {
        fn name(&self) -> &str {
            match self {
                Self::Alias(a) => &a.name,
                Self::Enum(e) => &e.name,
            }
        }
        fn at(&self) -> &Token {
            match self {
                Self::Alias(a) => &a.at,
                Self::Enum(e) => &e.at,
            }
        }
        fn refs(&self) -> Vec<&TypeRef> {
            match self {
                Self::Alias(a) => vec![&a.target],
                Self::Enum(e) => e.variants.iter().flat_map(|(_, f)| f.iter()).collect(),
            }
        }
    }
    let declarations: Vec<_> = aliases
        .iter()
        .map(Decl::Alias)
        .chain(enums.iter().map(Decl::Enum))
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
    let mut resolved = TypeAliases::new();
    let mut active = HashSet::new();
    for root in 0..declarations.len() {
        let mut work = vec![(root, false)];
        while let Some((i, finish)) = work.pop() {
            let declaration = &declarations[i];
            let name = declaration.name();
            if resolved.contains_key(name) {
                continue;
            }
            if finish {
                let ty = match declaration {
                    Decl::Alias(a) => a.target.resolve(&resolved)?,
                    Decl::Enum(e) => {
                        let variants = e
                            .variants
                            .iter()
                            .map(|(name, fields)| {
                                let fields = fields
                                    .iter()
                                    .map(|field| {
                                        field.resolve(&resolved)?.ok_or_else(|| {
                                            field.at.error("enum payloads cannot be unit")
                                        })
                                    })
                                    .collect::<Result<Vec<_>>>()?;
                                if fields.iter().any(Ty::affine) {
                                    return Err(e
                                        .at
                                        .error("generators cannot be stored in enum payloads"));
                                }
                                Ok(Variant {
                                    name: name.clone(),
                                    fields,
                                })
                            })
                            .collect::<Result<Vec<_>>>()?;
                        Some(Ty::Enum(Rc::new(EnumType {
                            name: e.name.clone(),
                            depth: 1 + variants
                                .iter()
                                .flat_map(|v: &Variant| &v.fields)
                                .map(Ty::layout_depth)
                                .max()
                                .unwrap_or(0),
                            variants,
                        })))
                    }
                };
                if ty.as_ref().is_some_and(|t| t.layout_depth() > 64) {
                    return Err(declaration
                        .at()
                        .error("type nesting exceeds the implementation limit of 64"));
                }
                resolved.insert(name.to_owned(), ty);
                active.remove(&i);
                continue;
            }
            if !active.insert(i) {
                return Err(declaration.at().error(format!(
                    "cyclic type alias or recursive enum involving `{name}`"
                )));
            }
            work.push((i, true));
            let mut refs = declaration.refs();
            while let Some(t) = refs.pop() {
                refs.extend(&t.args);
                if let Some(name) = &t.name {
                    if let Some(&dep) = names.get(name.as_str()) {
                        if !resolved.contains_key(name) {
                            work.push((dep, false));
                        }
                    }
                }
            }
        }
    }
    Ok(resolved)
}

impl Lower<'_> {
    pub(super) fn qualified_type(&self, base: &Expr) -> Result<Type> {
        match &base.kind {
            Expression::Type(ty) => ty.resolve(self.aliases),
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
        let Ty::Enum(t) = ty else {
            return Err(at.error("variant qualification requires an enum type"));
        };
        let tag = t
            .variants
            .iter()
            .position(|v| v.name == name)
            .ok_or_else(|| at.error(format!("unknown variant `{}.{name}`", t.name)))?;
        let fields = &t.variants[tag].fields;
        if fields.is_empty() && args.is_some() {
            return Err(at.error("nullary variants do not take parentheses"));
        }
        if args.map_or(0, <[Expr]>::len) != fields.len() || (!fields.is_empty() && args.is_none()) {
            return Err(at.error(format!(
                "variant `{name}` requires {} payload arguments",
                fields.len()
            )));
        }
        for (arg, field) in args.unwrap_or(&[]).iter().zip(fields) {
            let actual = self.expr_expected(arg, Some(field.clone()), ops)?;
            self.same(actual, Some(field.clone()), &arg.at)?;
        }
        ops.push(Op::Enum(EnumOp::New(t.clone(), tag)));
        Ok(Some(Ty::Enum(t)))
    }

    pub(super) fn match_cases(
        &mut self,
        value: &Expr,
        cases: &[Case],
        tail: bool,
        ops: &mut Vec<Op>,
    ) -> Result<BlockResult> {
        let ty = self.value(value, ops)?;
        let Ty::Enum(t) = ty else {
            return Err(value.at.error("match currently requires an enum value"));
        };
        let source = self.slot(Ty::Enum(t.clone()), &value.at)?;
        ops.push(Op::StoreLocal(source));
        ops.push(Op::LoadLocal(source));
        ops.push(Op::Enum(EnumOp::Tag(t.clone())));
        let saved = self.names.clone();
        let mut covered = HashSet::new();
        let mut wildcard = false;
        let mut arms = Vec::new();
        let mut joined: Option<Type> = None;
        for case in cases {
            if wildcard || covered.len() == t.variants.len() {
                return Err(case.at.error("unreachable case"));
            }
            self.names = saved.clone();
            let case_start = self.locals.len();
            let mut body = Vec::new();
            let pattern = if let Some((owner, name, bindings)) = &case.pattern {
                self.same(
                    owner.resolve(self.aliases)?,
                    Some(Ty::Enum(t.clone())),
                    &owner.at,
                )?;
                let tag = t
                    .variants
                    .iter()
                    .position(|v| &v.name == name)
                    .ok_or_else(|| case.at.error(format!("unknown variant `{name}`")))?;
                if !covered.insert(tag) {
                    return Err(case.at.error("duplicate variant case"));
                }
                let fields = &t.variants[tag].fields;
                if bindings.as_ref().map_or(0, Vec::len) != fields.len()
                    || (fields.is_empty() && bindings.is_some())
                {
                    return Err(case.at.error(format!("case `{name}` requires {} payload bindings (no parentheses for nullary variants)", fields.len())));
                }
                let mut bound = HashSet::new();
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
                    let slot = self.slot(ty.clone(), &case.at)?;
                    self.names.insert(
                        name.clone(),
                        Local {
                            slot,
                            ty: ty.clone(),
                            mutable: false,
                        },
                    );
                    body.extend([
                        Op::LoadLocal(source),
                        Op::Enum(EnumOp::Field(t.clone(), tag, field)),
                        Op::StoreLocal(slot),
                    ]);
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
        if !wildcard && covered.len() != t.variants.len() {
            let missing = t
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
