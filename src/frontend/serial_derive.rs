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

pub(super) fn class(
    c: &mut classes::ClassDecl,
    resolve: &dyn Fn(&TypeRef) -> Result<TypeRef>,
) -> Result<Vec<Function>> {
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
        for (i, (name, _)) in c.fields.iter().enumerate() {
            writeln!(
                source,
                "    SER.at(SER.wrap(out.field({name:?})), SER.Step.Field({name:?}))?"
            )
            .unwrap();
            writeln!(source,"    SER.at(SER.encode[FieldType{i}, ErrType, Adapter](&self.field{i}, out), SER.Step.Field({name:?}))?").unwrap();
        }
        source.push_str("    SER.wrap(out.end_object())\n");
        c.methods.push(hook(
            &source,
            &c.name,
            &c.type_params,
            &c.at,
            &names,
            &types,
        )?);
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
            if !same_type(
                &resolve(&c.fields[i].1)?,
                &resolve(&init.inputs[order.len() + 1].1)?,
            ) {
                return Ok(vec![]);
            }
            order.push(i);
        }
        order
    } else {
        (0..c.fields.len()).collect()
    };
    let fallible = if let Some(init) = init {
        let output = resolve(&init.output)?;
        if output.name.is_none() {
            false
        } else if output.name.as_deref() == Some("Result")
            && output.args.len() == 2
            && output.args[0].name.is_none()
            && output.args[1].name.as_deref() == Some("std.serial.Fault")
        {
            true
        } else {
            return Ok(vec![]);
        }
    } else {
        false
    };
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
            let ret = if fallible {
                format!("Result[FieldType{field}, SER.Fault]")
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
        for (i, (name, _)) in c.fields.iter().enumerate() {
            writeln!(source,"                    case {name:?}:\n                        SER.read_field[FieldType{i}, ErrType, Adapter](&mut slots[{i}], source, {name:?})?").unwrap();
        }
        source.push_str("                    case _:\n                        SER.at(SER.wrap(source.unknown_key(&name)), SER.Step.Field(name))?\n");
    }
    source.push_str("            case Nothing:\n                break\n");
    let mut args = vec![];
    for &i in &order {
        let name = &c.fields[i].0;
        let value = match default_calls.get(&i) {
            Some(call) if fallible => format!(
                "SER.at(SER.constructed[FieldType{i}, ErrType, Adapter]({call}, source), SER.Step.Field({name:?}))?"
            ),
            Some(call) => call.clone(),
            None => format!(
                "SER.require_field[FieldType{i}, ErrType, Adapter](&mut slots[{i}], source, {name:?})?"
            ),
        };
        args.push(format!("field{i}={value}"));
    }
    let ctor = format!("OwnerType({})", args.join(", "));
    if fallible {
        writeln!(
            source,
            "    SER.constructed[OwnerType, ErrType, Adapter]({ctor}, source)"
        )
        .unwrap();
    } else {
        writeln!(source, "    Ok({ctor})").unwrap();
    }
    c.methods.push(hook(
        &source,
        &c.name,
        &c.type_params,
        &c.at,
        &names,
        &types,
    )?);
    Ok(helpers)
}

pub(super) fn enumeration(e: &mut enums::EnumDecl) -> Result<()> {
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
        for (v, (name, fields)) in e.variants.iter().enumerate() {
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
    for (v, (name, fields)) in e.variants.iter().enumerate() {
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
            (0..fields.len())
                .map(|i| format!("value[{i}]"))
                .collect::<Vec<_>>()
                .join(",")
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

/// Compare expanded declaration types, ignoring source position and std-only
/// parameter ceilings, which do not change the stored type.
fn same_type(a: &TypeRef, b: &TypeRef) -> bool {
    if a.name != b.name || a.concrete != b.concrete || a.args.len() != b.args.len() {
        return false;
    }
    match (a.value_expression(), b.value_expression()) {
        (None, None) => (),
        (Some(a), Some(b)) => match (&a.kind, &b.kind) {
            (Expression::Number(a), Expression::Number(b))
            | (Expression::Text(a), Expression::Text(b))
            | (Expression::Name(a), Expression::Name(b))
                if a == b => {}
            (Expression::Bool(a), Expression::Bool(b)) if a == b => (),
            _ => return false,
        },
        _ => return false,
    }
    if a.name.as_deref() == Some("|") {
        let mut matched = vec![false; b.args.len()];
        for arg in &a.args {
            let Some(i) = b
                .args
                .iter()
                .enumerate()
                .position(|(i, other)| !matched[i] && same_type(arg, other))
            else {
                return false;
            };
            matched[i] = true;
        }
        true
    } else {
        a.args.iter().zip(&b.args).all(|(a, b)| same_type(a, b))
    }
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
