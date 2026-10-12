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
    pub(super) at: Token,
    pub(super) pattern: patterns::Pat,
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
        let mut declared = HashSet::new();
        while !matches!(self.peek().kind, Kind::Dedent | Kind::Eof) {
            let variant = self.name()?;
            if !declared.insert(variant.clone()) {
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
            let pattern = self.pattern(0)?;
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
            .tag(name)
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
        let (ty, borrowed) = match ty {
            Ty::Ref(inner, mutable) if matches!(*inner, Ty::Enum(_)) => {
                let borrowed = Some((self.reference_origin(value, ops)?, mutable));
                (Ty::Ref(inner, mutable), borrowed)
            }
            // No pattern binds part of an integer, string, or boolean, so a
            // reference to one is read once and the copy is matched.
            Ty::Ref(inner, _) if literal_matchable(&inner) => {
                let loan = self.reference_origin(value, ops)?;
                ops.push(Op::Access(self.loans[loan].root, false, Some(loan)));
                ops.push(Op::ReadRef((*inner).clone()));
                ops.push(Op::UseLoan(loan));
                ((*inner).clone(), None)
            }
            ty if matches!(ty, Ty::Enum(_)) || literal_matchable(&ty) => (ty, None),
            _ => return Err(value.at.error(NOT_MATCHABLE)),
        };
        let matched = match &ty {
            Ty::Ref(inner, _) => (**inner).clone(),
            ty => ty.clone(),
        };
        let source = self.slot(ty, &value.at)?;
        ops.push(Op::StoreLocal(source));
        self.finish_temporaries(temporary_start, ops);
        let subject = patterns::Subject { source, borrowed };
        self.match_patterns(value, cases, &matched, subject, tail, ops)
    }
}

const NOT_MATCHABLE: &str =
    "match requires an enum, integer, `str`, or `bool` value, or a reference to one; compare other values with an `if`/`elif` chain";

/// The types a literal pattern can match.
fn literal_matchable(ty: &Ty) -> bool {
    ty.is_int() || matches!(ty, Ty::Str | Ty::Bool)
}

pub(super) fn prelude_variant(name: &str) -> bool {
    matches!(name, "Ok" | "Err" | "Some" | "Nothing")
}
pub(super) fn prelude_family(name: &str) -> &'static str {
    if matches!(name, "Some" | "Nothing") {
        "Option"
    } else {
        "Result"
    }
}
pub(super) fn prelude_owner(name: &str, ty: &Ty) -> bool {
    matches!(ty, Ty::Enum(t) if t.name.starts_with(&format!("{}[", prelude_family(name))))
}
