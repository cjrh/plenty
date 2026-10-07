//! Explicit, cached monomorphization before typed body lowering.
use super::*;

pub(super) fn expand(
    functions: Vec<Function>,
    aliases: &TypeAliases,
    protocols: &[protocols::Protocol],
    access: &modules::AccessMap,
) -> Result<Vec<Function>> {
    let mut declarations = HashMap::new();
    for protocol in protocols {
        if aliases.contains_key(&protocol.name)
            || functions.iter().any(|f| f.name == protocol.name)
            || declarations
                .insert(protocol.name.clone(), protocol)
                .is_some()
        {
            return Err(protocol.at.error(format!(
                "protocol `{}` is already defined or conflicts with another declaration",
                protocol.name
            )));
        }
        // Validate requirements even if no function uses this protocol. The
        // receiver placeholder is replaced by the implementing class at use.
        let mut validation = aliases.clone();
        validation.insert(protocol.name.clone(), Some(Ty::I64));
        for method in &protocol.methods {
            protocols::signature(method, &validation)?;
        }
    }
    let mut engine = Engine {
        protocols: declarations,
        methods: functions
            .iter()
            .filter(|f| f.type_params.is_empty())
            .map(|f| Ok((f.name.clone(), protocols::signature(f, aliases)?)))
            .collect::<Result<_>>()?,
        access,
        templates: HashMap::new(),
        cache: HashMap::new(),
        pending: Vec::new(),
        aliases,
    };
    let mut names = HashSet::new();
    for f in functions {
        if !names.insert(f.name.clone()) || aliases.contains_key(&f.name) {
            return Err(f.at.error(format!(
                "function `{}` is already defined or conflicts with a type alias",
                f.name
            )));
        }
        if f.type_params.is_empty() {
            engine.pending.push(f);
        } else {
            if f.name == "main" {
                return Err(f.at.error("main cannot have type parameters"));
            }
            for (_, bound) in &f.type_params {
                if let Some(bound) = bound {
                    if (bound.name.as_deref() != Some("IntType")
                        && !bound
                            .name
                            .as_ref()
                            .is_some_and(|n| engine.protocols.contains_key(n)))
                        || !bound.args.is_empty()
                    {
                        return Err(bound
                            .at
                            .error("generic constraints require IntType or a declared protocol"));
                    }
                }
            }
            engine.templates.insert(f.name.clone(), f);
        }
    }
    let mut result = Vec::new();
    let mut index = 0;
    // Instantiations are queued, rather than recursively lowering their bodies.
    while index < engine.pending.len() {
        let mut f = engine.pending[index].clone();
        engine.block(&mut f.body, &HashMap::new())?;
        result.push(f);
        index += 1;
    }
    Ok(result)
}

struct Engine<'a> {
    protocols: HashMap<String, &'a protocols::Protocol>,
    methods: HashMap<String, (Vec<Ty>, Type)>,
    access: &'a modules::AccessMap,
    templates: HashMap<String, Function>,
    cache: HashMap<(String, Vec<Ty>), String>,
    pending: Vec<Function>,
    aliases: &'a TypeAliases,
}

type Substitution = HashMap<String, TypeRef>;

fn substitute(t: &mut TypeRef, replacements: &Substitution) -> Result<()> {
    if let Some(replacement) = t.name.as_ref().and_then(|n| replacements.get(n)) {
        if !t.args.is_empty() {
            return Err(t.at.error("a type parameter cannot take type arguments"));
        }
        *t = replacement.clone();
    } else {
        for arg in &mut t.args {
            substitute(arg, replacements)?;
        }
    }
    Ok(())
}

impl Engine<'_> {
    fn instantiate(&mut self, name: &str, types: &[TypeRef], at: &Token) -> Result<String> {
        let template = self
            .templates
            .get(name)
            .ok_or_else(|| at.error(format!("`{name}` is not a generic function")))?;
        if types.len() != template.type_params.len() {
            return Err(at.error(format!(
                "generic function `{name}` requires {} type arguments",
                template.type_params.len()
            )));
        }
        let actual = types
            .iter()
            .map(|t| {
                t.resolve(self.aliases)?
                    .ok_or_else(|| t.at.error("unit type arguments are not supported yet"))
            })
            .collect::<Result<Vec<_>>>()?;
        let key = (name.to_owned(), actual.clone());
        if let Some(symbol) = self.cache.get(&key) {
            return Ok(symbol.clone());
        }
        for ((_, bound), ty) in template.type_params.iter().zip(&actual) {
            if ty.contains_reference() {
                return Err(at.error(
                    "reference type arguments are not supported; borrow T in the signature",
                ));
            }
            if let Some(bound) = bound {
                if bound.name.as_deref() == Some("IntType") {
                    if !ty.is_int() {
                        return Err(at.error(format!("{ty} does not satisfy IntType")));
                    }
                } else {
                    protocols::check(
                        self.protocols[bound.name.as_ref().unwrap()],
                        ty,
                        &self.methods,
                        self.aliases,
                        self.access,
                        &bound.at,
                        at,
                    )?;
                }
            }
        }
        if self.cache.len() >= 256 {
            return Err(at.error(
                "generic specialization limit of 256 exceeded; check for expanding recursion",
            ));
        }
        let symbol = format!("__plenty_generic_{}", self.cache.len());
        let mut f = template.clone();
        let replacements = f
            .type_params
            .iter()
            .map(|(n, _)| n.clone())
            .zip(types.iter().cloned())
            .collect();
        f.name = symbol.clone();
        f.type_params.clear();
        for (_, ty) in &mut f.inputs {
            substitute(ty, &replacements)?;
        }
        substitute(&mut f.output, &replacements)?;
        // Cache before rewriting recursive calls, so identical instances are shared.
        self.cache.insert(key, symbol.clone());
        self.substitute_block(&mut f.body, &replacements)?;
        self.pending.push(f);
        Ok(symbol)
    }

    fn substitute_block(&mut self, body: &mut [Stmt], substitutions: &Substitution) -> Result<()> {
        // Rewriting explicit calls happens when this queued body is processed.
        self.walk_block(body, substitutions, false)
    }
    fn block(&mut self, body: &mut [Stmt], substitutions: &Substitution) -> Result<()> {
        self.walk_block(body, substitutions, true)
    }
    fn walk_block(
        &mut self,
        body: &mut [Stmt],
        substitutions: &Substitution,
        specialize: bool,
    ) -> Result<()> {
        for stmt in body {
            match &mut stmt.kind {
                Statement::Assign {
                    annotation, value, ..
                } => {
                    if let Some(t) = annotation {
                        substitute(t, substitutions)?;
                    }
                    self.expr(value, substitutions, specialize)?;
                }
                Statement::Expr(e) | Statement::Yield(e) | Statement::Unpack { value: e, .. } => {
                    self.expr(e, substitutions, specialize)?
                }
                Statement::Return(e) => {
                    if let Some(e) = e {
                        self.expr(e, substitutions, specialize)?;
                    }
                }
                Statement::SetIndex { target, value } => {
                    self.expr(value, substitutions, specialize)?;
                    self.expr(target, substitutions, specialize)?;
                }
                Statement::If { condition, yes, no } => {
                    self.expr(condition, substitutions, specialize)?;
                    self.walk_block(yes, substitutions, specialize)?;
                    self.walk_block(no, substitutions, specialize)?;
                }
                Statement::While { condition, body } => {
                    self.expr(condition, substitutions, specialize)?;
                    self.walk_block(body, substitutions, specialize)?;
                }
                Statement::For { iterable, body, .. } => {
                    self.expr(iterable, substitutions, specialize)?;
                    self.walk_block(body, substitutions, specialize)?;
                }
                Statement::With { manager, body, .. } => {
                    self.expr(manager, substitutions, specialize)?;
                    self.walk_block(body, substitutions, specialize)?;
                }
                Statement::Match { value, cases } => {
                    self.expr(value, substitutions, specialize)?;
                    for case in cases {
                        if let Some((Some(t), _, _)) = &mut case.pattern {
                            substitute(t, substitutions)?;
                        }
                        self.walk_block(&mut case.body, substitutions, specialize)?;
                    }
                }
                Statement::Pass | Statement::Break | Statement::Continue => {}
            }
        }
        Ok(())
    }
    fn expr(&mut self, e: &mut Expr, substitutions: &Substitution, specialize: bool) -> Result<()> {
        match &mut e.kind {
            Expression::GenericCall(name, types, args) => {
                for t in types.iter_mut() {
                    substitute(t, substitutions)?;
                }
                for arg in args.iter_mut() {
                    self.expr(arg, substitutions, specialize)?;
                }
                if specialize {
                    let symbol = self.instantiate(name, types, &e.at)?;
                    e.kind = Expression::Call(symbol, std::mem::take(args));
                }
            }
            Expression::Call(name, args) => {
                for arg in args.iter_mut() {
                    self.expr(arg, substitutions, specialize)?;
                }
                if let Some(t) = substitutions.get(name) {
                    if t.args.is_empty() {
                        *name = t.name.clone().unwrap();
                    } else {
                        e.kind = Expression::Constructor(t.clone(), std::mem::take(args));
                    }
                } else if specialize && self.templates.contains_key(name) {
                    return Err(e.at.error(format!(
                        "generic function `{name}` requires explicit type arguments"
                    )));
                }
            }
            Expression::Type(t) => substitute(t, substitutions)?,
            Expression::Constructor(t, args) => {
                substitute(t, substitutions)?;
                for arg in args {
                    self.expr(arg, substitutions, specialize)?;
                }
            }
            Expression::Tuple(args, _) => {
                for arg in args {
                    self.expr(arg, substitutions, specialize)?;
                }
            }
            Expression::Method(base, _, args) => {
                self.expr(base, substitutions, specialize)?;
                for arg in args {
                    self.expr(arg, substitutions, specialize)?;
                }
            }
            Expression::Member(base, _)
            | Expression::Unary(_, base)
            | Expression::Group(base)
            | Expression::Try(base)
            | Expression::FallibleCollection(base)
            | Expression::ClassReady(_, base) => self.expr(base, substitutions, specialize)?,
            Expression::Index(a, b) | Expression::Binary(_, a, b) => {
                self.expr(a, substitutions, specialize)?;
                self.expr(b, substitutions, specialize)?;
            }
            Expression::Conditional { condition, yes, no } => {
                self.expr(condition, substitutions, specialize)?;
                self.expr(yes, substitutions, specialize)?;
                self.expr(no, substitutions, specialize)?;
            }
            Expression::Collection {
                entries, clauses, ..
            } => {
                for clause in clauses {
                    match clause {
                        Clause::For(_, e) | Clause::If(e) => {
                            self.expr(e, substitutions, specialize)?
                        }
                    }
                }
                for (k, v) in entries {
                    self.expr(k, substitutions, specialize)?;
                    if let Some(v) = v {
                        self.expr(v, substitutions, specialize)?;
                    }
                }
            }
            Expression::Name(n) => {
                if let Some(t) = substitutions.get(n) {
                    e.kind = Expression::Type(t.clone());
                }
            }
            Expression::Number(_)
            | Expression::Text(_)
            | Expression::Bool(_)
            | Expression::Unit
            | Expression::ClassNew(..) => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_and_recursive_calls_share_one_specialization() {
        let mut source = String::from("def down[T: IntType](x: T) -> T:\n    if x == 0:\n        return x\n    down[T](x - 1)\ndef main() -> ():\n");
        for _ in 0..100 {
            source.push_str("    print(down[i64](3))\n");
        }
        let parsed = modules::single(&source).unwrap();
        let start = std::time::Instant::now();
        let functions = expand(
            parsed.functions,
            &TypeAliases::new(),
            &[],
            &modules::AccessMap::default(),
        )
        .unwrap();
        assert_eq!(functions.len(), 2);
        eprintln!(
            "100 calls and recursive body: one specialization in {:?}",
            start.elapsed()
        );
    }
}
