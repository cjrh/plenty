//! `@serial` emits ordinary private hooks, checked and specialized like user code.
//! No field metadata or reflective operation is retained at runtime.
use super::*;
use std::fmt::Write as _;

pub(super) const BINDING: &str = "$serial";

fn parsed(
    source: &str,
    owner: Option<&str>,
    at: &Token,
    names: &[(String, String)],
    types: &generics::Substitution,
) -> Result<Function> {
    let mut lexer = Lexer::new(source);
    let mut parser = Parser::new(at.file);
    while !lexer.done {
        parser.fill(&mut lexer, usize::MAX)?;
    }
    for token in &mut parser.tokens {
        if let Kind::Word(word) = &mut token.kind
            && let Some((_, replacement)) = names.iter().find(|(name, _)| name == word.as_ref())
        {
            *word = replacement.as_str().into();
        }
        token.line = at.line;
        token.column = at.column;
    }
    let mut function = parser.function_in(owner)?;
    for (_, input) in &mut function.inputs {
        generics::substitute(input, types)?;
    }
    generics::substitute(&mut function.output, types)?;
    generics::substitute_block(&mut function.body, types)?;
    Ok(function)
}

fn names() -> Vec<(String, String)> {
    [
        ("SER", BINDING),
        ("OwnerType", "$serial_owner_type"),
        ("ErrType", "$serial_error"),
        ("Adapter", "$serial_adapter"),
        ("out", "$serial_out"),
        ("source", "$serial_source"),
        ("slots", "$serial_slots"),
        ("name", "$serial_name"),
        ("value", "$serial_value"),
        ("payload", "$serial_payload"),
        ("tag", "$serial_tag"),
        ("item", "$serial_item"),
    ]
    .into_iter()
    .map(|(a, b)| (a.into(), b.into()))
    .collect()
}

fn type_ref(at: &Token, name: &str) -> TypeRef {
    TypeRef {
        at: at.clone(),
        name: Some(name.into()),
        args: vec![],
        concrete: None,
        metadata: None,
    }
}

fn owner_type(at: &Token, name: &str, parameters: &[(String, Option<TypeRef>)]) -> TypeRef {
    let mut owner = type_ref(at, name);
    owner.args = parameters
        .iter()
        .map(|(name, _)| type_ref(at, name))
        .collect();
    owner
}

fn add_types(fields: &[(String, TypeRef)], owner: TypeRef) -> generics::Substitution {
    let mut types = generics::Substitution::default();
    types.insert("$serial_owner_type".into(), owner);
    for (i, (_, ty)) in fields.iter().enumerate() {
        types.insert(format!("$serial_field_type{i}"), ty.clone());
    }
    types
}

fn hook(
    source: &str,
    name: &str,
    parameters: &[(String, Option<TypeRef>)],
    at: &Token,
    names: &[(String, String)],
    types: &generics::Substitution,
) -> Result<Function> {
    let mut f = parsed(source, Some(name), at, names, types)?;
    if f.name == "__encode__" {
        let owner = classes::receiver_owner_mut(&mut f.inputs[0].1);
        owner.args = parameters
            .iter()
            .map(|(name, _)| type_ref(at, name))
            .collect();
    }
    Ok(f)
}

/// The name each member is serialized under: its `@serial.name`, or its
/// source name. A wire name must differ from the source name and from every
/// other member's wire name, and needs a derived hook to apply to.
fn wire_names<'a>(
    kind: &str,
    owner: &str,
    members: impl Iterator<Item = &'a str>,
    renamed: &[(usize, annotations::WireName)],
    methods: &[Function],
) -> Result<Vec<String>> {
    let sources: Vec<&str> = members.collect();
    if let Some((_, first)) = renamed.first()
        && methods.iter().any(|f| f.name == "__encode__")
        && methods.iter().any(|f| f.name == "__decode__")
    {
        return Err(first.at.error(format!(
            "`@serial.name` has no effect: `{owner}` writes both `__encode__` and `__decode__`; remove it, or use the name in the hooks"
        )));
    }
    let mut wire: Vec<String> = sources.iter().map(|&s| s.to_owned()).collect();
    for (index, name) in renamed {
        if sources[*index] == name.name {
            return Err(name.at.error(format!(
                "`@serial.name({:?})` repeats the {kind}'s own name; remove it",
                name.name
            )));
        }
        wire[*index] = name.name.clone();
    }
    let annotation = |i: usize| {
        renamed
            .iter()
            .find(|(index, _)| *index == i)
            .map(|(_, w)| w)
    };
    let mut seen: HashMap<&str, usize> = HashMap::default();
    for (i, name) in wire.iter().enumerate() {
        if let Some(&first) = seen.get(name.as_str()) {
            // Source names are distinct, so at least one of the pair is renamed.
            let at = &annotation(i)
                .or_else(|| annotation(first))
                .expect("a clash involves a renamed member")
                .at;
            return Err(at.error(format!(
                "{kind}s `{}` and `{}` of `{owner}` both serialize as {name:?}; wire names must be distinct",
                sources[first], sources[i]
            )));
        }
        seen.insert(name, i);
    }
    Ok(wire)
}

/// Constructor error domains automatic decoding can map without a choice from
/// the author. Validation uses the adapter's `reject`; allocation failure keeps
/// its typed cause in the adapter's allocation domain. Any other spelling is
/// generated as `Fault` here; `constructor_matches` then requires the
/// specialized domain to be the one generated for. A type-parameter domain is
/// generated before specialization, so it too decodes only as `Fault` (#914).
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Domain {
    Unit,
    Fault,
    Parameter,
    Allocation,
    Both,
}

impl Domain {
    fn of(error: &TypeRef, parameters: &[(String, Option<TypeRef>)]) -> Self {
        let is = |ty: &TypeRef, name: &str| ty.args.is_empty() && ty.name.as_deref() == Some(name);
        if parameters.iter().any(|(name, _)| is(error, name)) {
            Domain::Parameter
        } else if is(error, "AllocError") {
            Domain::Allocation
        } else if error.name.as_deref() == Some("|")
            && error.args.len() == 2
            && error.args.iter().any(|t| is(t, "AllocError"))
            && error.args.iter().any(|t| is(t, "std.serial.Fault"))
        {
            Domain::Both
        } else {
            Domain::Fault
        }
    }
    fn spelling(self) -> &'static str {
        match self {
            Domain::Unit => unreachable!("an infallible initializer has no error domain"),
            Domain::Fault | Domain::Parameter => "SER.Fault",
            Domain::Allocation => "AllocError",
            Domain::Both => "AllocError | SER.Fault",
        }
    }
    fn bridge(self) -> &'static str {
        match self {
            Domain::Unit => unreachable!("an infallible initializer has no error domain"),
            Domain::Fault | Domain::Parameter => "constructed",
            Domain::Allocation => "allocated",
            Domain::Both => "constructed_or_allocated",
        }
    }
}

pub(super) fn class(
    c: &mut classes::ClassDecl,
    resolve: &dyn Fn(&TypeRef) -> Result<TypeRef>,
) -> Result<Vec<Function>> {
    let wire = wire_names(
        "field",
        &c.name,
        c.fields.iter().map(|(name, _)| name.as_str()),
        &c.wire_names,
        &c.methods,
    )?;
    let mut names = names();
    for (i, (name, _)) in c.fields.iter().enumerate() {
        names.push((format!("field{i}"), name.clone()));
        names.push((format!("FieldType{i}"), format!("$serial_field_type{i}")));
    }
    for (i, (name, _)) in c.type_params.iter().enumerate() {
        names.push((format!("OwnerParam{i}"), name.clone()));
    }
    let types = add_types(&c.fields, owner_type(&c.at, &c.name, &c.type_params));
    if !c.methods.iter().any(|f| f.name == "__encode__") {
        let mut source = "def __encode__[ErrType, Adapter: SER.Encoder[ErrType]](self, out: &mut Adapter) -> Result[(), SER.SerialError[ErrType]]:\n    SER.wrap(out.start_object())?\n".to_string();
        for (i, name) in wire.iter().enumerate() {
            writeln!(
                source,
                "    SER.at(SER.wrap(out.field({name:?})), SER.Step.Field({name:?}))?"
            )
            .unwrap();
            writeln!(source,"    SER.at(SER.encode[FieldType{i}, ErrType, Adapter](&self.field{i}, out), SER.Step.Field({name:?}))?").unwrap();
        }
        source.push_str("    SER.wrap(out.end_object())\n");
        let mut generated = hook(&source, &c.name, &c.type_params, &c.at, &names, &types)?;
        for (i, (_, ty)) in c.fields.iter().enumerate() {
            stamp_statement(&mut generated.body[1 + 2 * i], &ty.at);
            stamp_statement(&mut generated.body[2 + 2 * i], &ty.at);
        }
        c.derived_encode = true;
        c.methods.push(generated);
    }
    if c.methods.iter().any(|f| f.name == "__decode__") {
        return Ok(vec![]);
    }
    let init = c.methods.iter().find(|f| f.name == "__init__");
    let order: Vec<usize> = if let Some(init) = init {
        if init.inputs.len() != c.fields.len() + 1 {
            return Ok(vec![]);
        }
        let mut order = vec![];
        for (name, _) in init.inputs.iter().skip(1) {
            let Some(i) = c.fields.iter().position(|(field, _)| field == name) else {
                return Ok(vec![]);
            };
            order.push(i);
        }
        order
    } else {
        (0..c.fields.len()).collect()
    };
    let domain = if let Some(init) = init {
        let output = resolve(&init.output)?;
        if output.name.is_none() {
            None
        } else if output.name.as_deref() == Some("Result")
            && output.args.len() == 2
            && output.args[0].name.is_none()
        {
            Some(Domain::of(&output.args[1], &c.type_params))
        } else {
            return Ok(vec![]);
        }
    } else {
        None
    };
    let fallible = domain.is_some();
    let mut helpers = vec![];
    let mut default_calls = HashMap::default();
    if let Some(init) = init {
        for (param, &field) in order.iter().enumerate() {
            let Some(default) = init.defaults.get(param + 1).and_then(Option::as_ref) else {
                continue;
            };
            let helper = format!("$serial_default_{}_{}", c.name, field);
            let placeholder = format!("default{field}");
            names.push((placeholder.clone(), helper.clone()));
            let ret = if let Some(domain) = domain {
                format!("Result[FieldType{field}, {}]", domain.spelling())
            } else {
                format!("FieldType{field}")
            };
            let mut f = parsed(
                &format!(
                    "def helper[Dummy](item: &mut Option[FieldType{field}]) -> {ret}:\n    match replace(*item, Nothing):\n        case Some(value):\n            {}\n        case Nothing:\n            {}\n",
                    if fallible { "Ok(value)" } else { "value" },
                    if fallible { "Ok(False)" } else { "False" }
                ),
                None,
                &c.at,
                &names,
                &types,
            )?;
            f.name = helper;
            f.type_params = c.type_params.clone();
            f.type_params.push(("$serial_default_unused".into(), None));
            // Only the absent branch evaluates the original AST. It resolves in
            // the defining module, with no user-spellable generated locals.
            let Statement::Match { cases, .. } = &mut f.body[0].kind else {
                unreachable!()
            };
            let Statement::Expr(expr) = &mut cases[1].body[0].kind else {
                unreachable!()
            };
            if fallible {
                let Expression::Call(_, args) = &mut expr.kind else {
                    unreachable!()
                };
                args[0] = default.clone();
            } else {
                *expr = default.clone();
            }
            let mut arguments = c
                .type_params
                .iter()
                .enumerate()
                .map(|(i, _)| format!("OwnerParam{i}"))
                .collect::<Vec<_>>();
            arguments.push("bool".into());
            default_calls.insert(
                field,
                format!(
                    "{placeholder}[{}](&mut slots[{field}])",
                    arguments.join(",")
                ),
            );
            helpers.push(f);
        }
    }
    let mut source="def __decode__[ErrType, Adapter: SER.Decoder[ErrType]](source: &mut Adapter) -> Result[OwnerType, SER.SerialError[ErrType]]:\n    SER.wrap(source.start_object())?\n".to_string();
    if !c.fields.is_empty() {
        let tys = (0..c.fields.len())
            .map(|i| format!("Option[FieldType{i}]"))
            .collect::<Vec<_>>()
            .join(",");
        let vals = vec!["Nothing"; c.fields.len()].join(",");
        writeln!(source, "    mut slots: ({tys},) = ({vals},)").unwrap();
    }
    source.push_str("    while True:\n        match SER.wrap(source.next_key())?:\n            case Some(name):\n");
    if c.fields.is_empty() {
        source.push_str(
            "                SER.at(SER.wrap(source.unknown_key(&name)), SER.Step.Field(name))?\n",
        );
    } else {
        source.push_str("                match name:\n");
        for (i, name) in wire.iter().enumerate() {
            writeln!(source,"                    case {name:?}:\n                        SER.read_field[FieldType{i}, ErrType, Adapter](&mut slots[{i}], source, {name:?})?").unwrap();
        }
        source.push_str("                    case _:\n                        SER.at(SER.wrap(source.unknown_key(&name)), SER.Step.Field(name))?\n");
    }
    source.push_str("            case Nothing:\n                break\n");
    // Constructor temporaries are only needed after recursive decoding has
    // returned. Keep them in a separate ordinary checked function rather than
    // retaining their native stack frame at every recursive member.
    let mut construction = String::new();
    let mut args = vec![];
    for &i in &order {
        let name = &wire[i];
        let value = match default_calls.get(&i) {
            Some(call) if fallible => format!(
                "SER.at(SER.{}[FieldType{i}, ErrType, Adapter]({call}, source), SER.Step.Field({name:?}))?",
                domain.unwrap().bridge()
            ),
            Some(call) => call.clone(),
            None => format!(
                "SER.require_field[FieldType{i}, ErrType, Adapter](&mut slots[{i}], source, {name:?})?"
            ),
        };
        args.push(format!("field{i}={value}"));
    }
    let ctor = format!("OwnerType({})", args.join(", "));
    if let Some(domain) = domain {
        writeln!(
            construction,
            "    SER.{}[OwnerType, ErrType, Adapter]({ctor}, source)",
            domain.bridge()
        )
        .unwrap();
    } else {
        writeln!(construction, "    Ok({ctor})").unwrap();
    }
    if c.fields.is_empty() {
        source.push_str(&construction);
    } else {
        let helper = format!("$serial_construct_{}", c.name);
        names.push(("ConstructOwner".into(), helper));
        let tys = (0..c.fields.len())
            .map(|i| format!("Option[FieldType{i}]"))
            .collect::<Vec<_>>()
            .join(",");
        let mut function = parsed(
            &format!(
                "def ConstructOwner[ErrType, Adapter: SER.Decoder[ErrType]](slots: &mut ({tys},), source: &Adapter) -> Result[OwnerType, SER.SerialError[ErrType]]:\n{construction}"
            ),
            None,
            &c.at,
            &names,
            &types,
        )?;
        let mut parameters = c.type_params.clone();
        parameters.append(&mut function.type_params);
        function.type_params = parameters;
        helpers.push(function);
        let mut arguments = (0..c.type_params.len())
            .map(|i| format!("OwnerParam{i}"))
            .collect::<Vec<_>>();
        arguments.extend(["ErrType".into(), "Adapter".into()]);
        writeln!(
            source,
            "    ConstructOwner[{}](&mut slots, source)",
            arguments.join(",")
        )
        .unwrap();
    }
    c.derived_decode = Some(domain.unwrap_or(Domain::Unit));
    let mut generated = hook(&source, &c.name, &c.type_params, &c.at, &names, &types)?;
    if !c.fields.is_empty() {
        let Statement::While { body, .. } = &mut generated.body[2].kind else {
            unreachable!()
        };
        let Statement::Match { cases, .. } = &mut body[0].kind else {
            unreachable!()
        };
        let Statement::Match { cases, .. } = &mut cases[0].body[0].kind else {
            unreachable!()
        };
        for (case, (_, ty)) in cases.iter_mut().zip(&c.fields) {
            stamp_statement(&mut case.body[0], &ty.at);
        }
    }
    c.methods.push(generated);
    Ok(helpers)
}

pub(super) fn enumeration(e: &mut enums::EnumDecl) -> Result<()> {
    let wire = wire_names(
        "variant",
        &e.name,
        e.variants.iter().map(|(name, _)| name.as_str()),
        &e.wire_names,
        &e.methods,
    )?;
    let mut names = names();
    names.push(("EnumName".into(), e.name.clone()));
    let mut types = generics::Substitution::default();
    types.insert(
        "$serial_owner_type".into(),
        owner_type(&e.at, &e.name, &e.type_params),
    );
    let mut generic = vec![];
    for (i, (name, _)) in e.type_params.iter().enumerate() {
        let placeholder = format!("Param{i}");
        names.push((placeholder.clone(), name.clone()));
        generic.push(placeholder);
    }
    let owner = if generic.is_empty() {
        "EnumName".into()
    } else {
        format!("EnumName[{}]", generic.join(","))
    };
    for (v, (name, fields)) in e.variants.iter().enumerate() {
        names.push((format!("variant{v}"), name.clone()));
        for (i, ty) in fields.iter().enumerate() {
            names.push((
                format!("Payload{v}Type{i}"),
                format!("$serial_payload{v}_type{i}"),
            ));
            types.insert(format!("$serial_payload{v}_type{i}"), ty.clone());
        }
    }
    if !e.methods.iter().any(|f| f.name == "__encode__") {
        let mut source="def __encode__[ErrType, Adapter: SER.Encoder[ErrType]](self, out: &mut Adapter) -> Result[(), SER.SerialError[ErrType]]:\n    match self:\n".to_string();
        for (v, ((_, fields), name)) in e.variants.iter().zip(&wire).enumerate() {
            if fields.is_empty() {
                writeln!(source,"        case EnumName.variant{v}:\n            SER.at(SER.wrap(out.text({name:?})), SER.Step.Variant({name:?}))").unwrap();
                continue;
            }
            let bind = (0..fields.len())
                .map(|i| format!("bind{v}item{i}"))
                .collect::<Vec<_>>();
            for b in &bind {
                names.push((b.clone(), format!("${b}")));
            }
            writeln!(
                source,
                "        case EnumName.variant{v}({}):",
                bind.join(",")
            )
            .unwrap();
            for op in [
                "out.start_object()".to_string(),
                format!("out.key({name:?})"),
            ] {
                writeln!(
                    source,
                    "            SER.at(SER.wrap({op}), SER.Step.Variant({name:?}))?"
                )
                .unwrap();
            }
            if fields.len() > 1 {
                writeln!(
                    source,
                    "            SER.at(SER.wrap(out.start_array()), SER.Step.Variant({name:?}))?"
                )
                .unwrap();
            }
            for (i, b) in bind.iter().enumerate() {
                let call = format!("SER.encode[Payload{v}Type{i}, ErrType, Adapter]({b}, out)");
                let call = if fields.len() > 1 {
                    format!("SER.at({call}, SER.Step.Index({i}))")
                } else {
                    call
                };
                writeln!(
                    source,
                    "            SER.at({call}, SER.Step.Variant({name:?}))?"
                )
                .unwrap();
            }
            if fields.len() > 1 {
                writeln!(
                    source,
                    "            SER.at(SER.wrap(out.end_array()), SER.Step.Variant({name:?}))?"
                )
                .unwrap();
            }
            writeln!(
                source,
                "            SER.at(SER.wrap(out.end_object()), SER.Step.Variant({name:?}))"
            )
            .unwrap();
        }
        e.methods.push(hook(
            &source,
            &e.name,
            &e.type_params,
            &e.at,
            &names,
            &types,
        )?);
    }
    if e.methods.iter().any(|f| f.name == "__decode__") {
        return Ok(());
    }
    let mut source="def __decode__[ErrType, Adapter: SER.Decoder[ErrType]](source: &mut Adapter) -> Result[OwnerType, SER.SerialError[ErrType]]:\n    tag, payload = SER.wrap(source.variant())?\n    match tag:\n".to_string();
    for (v, ((_, fields), name)) in e.variants.iter().zip(&wire).enumerate() {
        writeln!(source, "        case {name:?}:").unwrap();
        writeln!(source,"            if {}payload:\n                return Err(SER.error(source.reject(SER.Fault.UnexpectedKind)).at(SER.Step.Variant({name:?})))",if fields.is_empty(){""}else{"not "}).unwrap();
        if fields.is_empty() {
            writeln!(source, "            Ok({owner}.variant{v})").unwrap();
            continue;
        }
        let ty = if fields.len() == 1 {
            format!("Payload{v}Type0")
        } else {
            format!(
                "({},)",
                (0..fields.len())
                    .map(|i| format!("Payload{v}Type{i}"))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        writeln!(source,"            value = SER.at(SER.decode[{ty}, ErrType, Adapter](source), SER.Step.Variant({name:?}))?").unwrap();
        writeln!(source,"            if SER.at(SER.wrap(source.next_key()), SER.Step.Variant({name:?}))?.is_some():\n                return Err(SER.error(source.reject(SER.Fault.WrongLength(1, 2))).at(SER.Step.Variant({name:?})))").unwrap();
        let args = if fields.len() == 1 {
            "value".into()
        } else {
            let locals = (0..fields.len())
                .map(|i| format!("decoded{v}item{i}"))
                .collect::<Vec<_>>();
            for local in &locals {
                names.push((local.clone(), format!("${local}")));
            }
            writeln!(source, "            {} = value", locals.join(",")).unwrap();
            locals.join(",")
        };
        writeln!(source, "            Ok({owner}.variant{v}({args}))").unwrap();
    }
    source.push_str("        case _:\n            Err(SER.error(source.reject(SER.Fault.UnknownVariant(tag))))\n");
    e.methods.push(hook(
        &source,
        &e.name,
        &e.type_params,
        &e.at,
        &names,
        &types,
    )?);
    Ok(())
}

/// Whether `ty`'s derived decoding was generated for a type-parameter error
/// domain, so a rejection can name the actual restriction.
pub(super) fn parameter_domain(ty: &Ty, aliases: &TypeAliases) -> bool {
    aliases
        .data
        .origin_class(ty)
        .is_some_and(|(declaration, _)| declaration.derived_decode == Some(Domain::Parameter))
}

/// Constructor eligibility is semantic, after constants and generic nominal
/// identities have been resolved. Declaration AST spelling is not type identity.
pub(super) fn constructor_matches(ty: &Ty, aliases: &TypeAliases) -> Result<bool> {
    let Ty::Class(class) = ty else {
        return Ok(true);
    };
    let Some((declaration, arguments)) = aliases.data.origin_class(ty) else {
        return Ok(true);
    };
    let Some(domain) = declaration.derived_decode else {
        return Ok(true);
    };
    let Some(init) = declaration.methods.iter().find(|f| f.name == "__init__") else {
        return Ok(true);
    };
    let substitutions = declaration
        .type_params
        .iter()
        .map(|(name, _)| name.clone())
        .zip(arguments.iter().map(|t| t.reference(&init.at)))
        .collect();
    let mut output = init.output.clone();
    generics::substitute(&mut output, &substitutions)?;
    if let Some(output) = output.resolve(aliases)? {
        let fault = type_ref(&init.at, "std.serial.Fault");
        let allocation = type_ref(&init.at, "AllocError");
        let expected = match domain {
            Domain::Unit => return Ok(false),
            Domain::Fault | Domain::Parameter => fault,
            Domain::Allocation => allocation,
            Domain::Both => {
                let mut both = type_ref(&init.at, "|");
                both.args = vec![allocation, fault];
                both
            }
        };
        let expected = expected.resolve(aliases)?;
        if !matches!(output, Ty::Enum(ref result) if result.is_result()
            && result.get().variants[0].fields == [Ty::Unit]
            && Some(&result.get().variants[1].fields[0]) == expected.as_ref())
        {
            return Ok(false);
        }
    }
    let definition = class.get();
    for (name, written) in init.inputs.iter().skip(1) {
        let Some((_, field)) = definition.fields.iter().find(|(n, _)| n == name) else {
            return Ok(false);
        };
        let mut parameter = written.clone();
        generics::substitute(&mut parameter, &substitutions)?;
        if parameter.resolve(aliases)?.as_ref() != Some(field) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn expand_aliases(
    mut ty: TypeRef,
    aliases: &HashMap<String, TypeRef>,
    depth: usize,
) -> Result<TypeRef> {
    if depth > 64 {
        return Err(ty
            .at
            .error("serialization constructor type aliases exceed nesting limit"));
    }
    if let Some(alias) = ty.name.as_ref().and_then(|n| aliases.get(n)) {
        return expand_aliases(alias.clone(), aliases, depth + 1);
    }
    for arg in &mut ty.args {
        *arg = expand_aliases(arg.clone(), aliases, depth + 1)?;
    }
    Ok(ty)
}

// Generated field operations retain their original declaration span. Besides
// useful source positions, generic diagnostics can name the offending field.
fn stamp_statement(statement: &mut Stmt, at: &Token) {
    statement.at = at.clone();
    if let Statement::Expr(expr) = &mut statement.kind {
        stamp_expr(expr, at);
    }
}
fn stamp_expr(expr: &mut Expr, at: &Token) {
    expr.at = at.clone();
    match &mut expr.kind {
        Expression::Call(_, args)
        | Expression::GenericCall(_, _, args)
        | Expression::Constructor(_, args) => {
            for arg in args {
                stamp_expr(arg, at);
            }
        }
        Expression::Method(base, _, args, _) => {
            stamp_expr(base, at);
            for arg in args {
                stamp_expr(arg, at);
            }
        }
        Expression::Try(base) | Expression::Unary(_, base) | Expression::Member(base, _, _) => {
            stamp_expr(base, at)
        }
        Expression::Index(base, index) => {
            stamp_expr(base, at);
            stamp_expr(index, at);
        }
        Expression::Ambiguous { generic, index } => {
            stamp_expr(generic, at);
            if let Ok(index) = index {
                stamp_expr(index, at);
            }
        }
        _ => (),
    }
}
