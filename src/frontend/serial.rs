//! Codec entry points expand to ordinary checked Plenty bodies. Formats stay
//! outside the compiler, and handwritten hooks use the same call boundary.
use super::*;

pub(super) fn decode_hook(f: &Function) -> bool {
    f.name == "__decode__"
}

pub(super) fn validate_hook(f: &Function, owner: &str, aliases: &TypeAliases) -> Result<()> {
    if !matches!(f.name.as_str(), "__encode__" | "__decode__") {
        return Ok(());
    }
    let decoding = decode_hook(f);
    if f.type_params.len() != 2
        || f.inputs.len() != if decoding { 1 } else { 2 }
        || f.defaults.iter().any(Option::is_some)
        || generators::yields(&f.body)
    {
        return Err(f.at.error("serialization hooks require two method type parameters and the fixed codec signature; __decode__ has no self receiver"));
    }
    let error = &f.type_params[0].0;
    let adapter = &f.type_params[1].0;
    let expected = if decoding {
        "std.serial.Decoder"
    } else {
        "std.serial.Encoder"
    };
    let bound = f.type_params[1].1.as_ref();
    let valid_bound = f.type_params[0].1.is_none()
        && bound.is_some_and(|b| {
            b.name.as_deref() == Some(expected)
                && b.args.len() == 1
                && b.args[0].name.as_ref() == Some(error)
        });
    let parameter = &f.inputs[usize::from(!decoding)].1;
    let valid_parameter = parameter.name.as_deref() == Some("&mut")
        && parameter.args.len() == 1
        && parameter.args[0].name.as_ref() == Some(adapter);
    let receiver_valid = if decoding {
        f.inputs[0].0 != "self"
    } else {
        let receiver = &f.inputs[0];
        receiver.0 == "self"
            && receiver.1.name.as_deref() == Some("&")
            && receiver
                .1
                .args
                .first()
                .and_then(|t| t.resolve(aliases).ok().flatten())
                .as_ref()
                .and_then(classes::method_owner)
                == Some(owner)
    };
    let output = &f.output;
    let valid_output = output.name.as_deref() == Some("Result")
        && output.args.len() == 2
        && (if decoding {
            output.args[0]
                .resolve(aliases)?
                .as_ref()
                .and_then(classes::method_owner)
                == Some(owner)
        } else {
            output.args[0].name.is_none()
        })
        && output.args[1].name.as_deref() == Some("std.serial.SerialError")
        && output.args[1].args.len() == 1
        && output.args[1].args[0].name.as_ref() == Some(error);
    if !valid_bound || !valid_parameter || !valid_output || !receiver_valid {
        return Err(f.at.error("serialization hook must use [E, S: std.serial.Encoder[E]] with (self, out: &mut S) -> Result[(), std.serial.SerialError[E]], or [E, D: std.serial.Decoder[E]] with (source: &mut D) -> Result[Owner, std.serial.SerialError[E]]"));
    }
    Ok(())
}

/// Parse generated operations using the normal parser, binding only compiler
/// chosen identifiers. User source and format algorithms never enter this path.
fn body(source: &str, at: &Token, names: &[(&str, String)]) -> Result<Vec<Stmt>> {
    let text = format!("def generated() -> ():\n{source}");
    let mut lexer = Lexer::new(&text);
    let mut parser = Parser::new(at.file);
    while !lexer.done {
        parser.fill(&mut lexer, usize::MAX)?;
    }
    for token in &mut parser.tokens {
        if let Kind::Word(word) = &mut token.kind
            && let Some((_, replacement)) = names.iter().find(|(name, _)| *name == word.as_ref())
        {
            *word = replacement.as_str().into();
        }
        token.line = at.line;
        token.column = at.column;
    }
    Ok(parser.function_in(None)?.body)
}

pub(super) fn expand(
    name: &str,
    actual: &[Argument],
    template: &Function,
    replacements: &mut generics::Substitution,
    aliases: &TypeAliases,
    at: &Token,
) -> Result<Option<Arc<Function>>> {
    if !matches!(
        name,
        "std.serial.encode"
            | "std.serial.decode"
            | "std.serial.absent"
            | "std.serial.constructed_in"
    ) {
        return Ok(None);
    }
    let ty = actual[0].as_type().expect("codec value is a type");
    let decoding = name.ends_with("decode");
    let mut names = vec![];
    let mut hook = None;
    for identifier in [
        "encode",
        "decode",
        "wrap",
        "allocated",
        "constructed",
        "constructed_or_allocated",
        "error",
        "at",
        "Fault",
        "Step",
        "SerialError",
        "encode_bytes",
        "decode_bytes",
    ] {
        names.push((identifier, format!("std.serial.{identifier}")));
    }
    if name == "std.serial.absent" {
        let mut generated = template.clone();
        generated.body = body(
            if matches!(ty, Ty::Enum(option) if option.is_option()) {
                "    Ok(Nothing)\n"
            } else {
                "    Err(error(source.reject(Fault.MissingField(*name))))\n"
            },
            at,
            &names,
        )?;
        return Ok(Some(Arc::new(generated)));
    }
    if name == "std.serial.constructed_in" {
        // A generic constructor's domain is known only here; derived decoding
        // has already checked it with `serial_derive::constructor_matches`.
        let bridge = match serial_derive::Domain::of_type(ty, aliases, at)? {
            Some(domain) => domain.bridge(),
            None => {
                return Err(at.error(format!(
                    "constructed_in maps only std.serial.Fault, AllocError or AllocError | std.serial.Fault, not {ty}"
                )));
            }
        };
        let mut generated = template.clone();
        generated.body = body(
            &format!("    {bridge}T, E, D](value, source)\n"),
            at,
            &names,
        )?;
        return Ok(Some(Arc::new(generated)));
    }
    let mut child = |label: &str, ty: &Ty| {
        replacements.insert(label.into(), generics::type_ref(ty, at));
    };
    let operation = match ty {
        Ty::Bool => {
            if decoding {
                "    wrap(source.boolean())\n".into()
            } else {
                "    wrap(out.boolean(*value))\n".into()
            }
        }
        Ty::Str => {
            if decoding {
                "    wrap(source.text())\n".into()
            } else {
                "    wrap(out.text(value))\n".into()
            }
        }
        Ty::ByteStr => {
            if decoding {
                return Err(at.error("static `bytes` literals cannot be decoded at runtime; decode into `std.bytes.Bytes`"));
            }
            "    encode_bytes[E, S](value, out)\n".into()
        }
        Ty::Class(class) if class.get().name == "std.bytes.Bytes" => {
            if decoding {
                "    decode_bytes[E, D](source)\n".into()
            } else {
                "    encode_bytes[E, S](&value[:], out)\n".into()
            }
        }
        t if t.is_int() => {
            let unsigned = matches!(t, Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64);
            let method = if unsigned { "unsigned" } else { "integer" };
            let wide = if unsigned { "u64" } else { "i64" };
            if decoding {
                let check = if matches!(t, Ty::I64 | Ty::U64) {
                    String::new()
                } else {
                    let (min, max) = match t {
                        Ty::I8 => ("-128", "127"),
                        Ty::I16 => ("-32768", "32767"),
                        Ty::I32 => ("-2147483648", "2147483647"),
                        Ty::U8 => ("0u64", "255u64"),
                        Ty::U16 => ("0u64", "65535u64"),
                        Ty::U32 => ("0u64", "4294967295u64"),
                        _ => unreachable!(),
                    };
                    format!(
                        "    if number < {min} or number > {max}:\n        return Err(error(source.reject(Fault.OutOfRange)))\n"
                    )
                };
                format!("    number = wrap(source.{method}())?\n{check}    Ok(T(number))\n")
            } else {
                format!("    wrap(out.{method}({wide}(*value)))\n")
            }
        }
        Ty::F32 | Ty::F64 => {
            if decoding {
                if matches!(ty, Ty::F32) {
                    "    number = wrap(source.decimal())?\n    if number != f64.INF and number != -f64.INF and (number > 3.4028234663852886e38 or number < -3.4028234663852886e38):\n        return Err(error(source.reject(Fault.OutOfRange)))\n    Ok(T(number))\n".into()
                } else {
                    "    wrap(source.decimal())\n".into()
                }
            } else {
                "    wrap(out.decimal(f64(*value)))\n".into()
            }
        }
        Ty::List(item) | Ty::Set(item) => {
            child("Item", item);
            if decoding {
                let create = if matches!(ty, Ty::List(_)) {
                    "list"
                } else {
                    "set"
                };
                let append = if matches!(ty, Ty::List(_)) {
                    "append"
                } else {
                    "add"
                };
                let duplicate = if create == "set" {
                    "        if item in result:\n            return at[T, E](Err(error(source.reject(Fault.DuplicateElement))), Step.Index(index))\n"
                } else {
                    ""
                };
                format!(
                    "    wrap(source.start_array())?\n    mut result = allocated[{create}[Item], E, D]({create}[Item](), source)?\n    mut index = 0\n    while at(wrap(source.next_element()), Step.Index(index))?:\n        item = at(decode[Item, E, D](source), Step.Index(index))?\n{duplicate}        at(allocated[(), E, D](result.{append}(item), source), Step.Index(index))?\n        unchecked:\n            index += 1\n    Ok(result)\n"
                )
            } else {
                "    wrap(out.start_array())?\n    mut index = 0\n    for item in value:\n        at(encode[Item, E, S](item, out), Step.Index(index))?\n        unchecked:\n            index += 1\n    wrap(out.end_array())\n".into()
            }
        }
        Ty::Dict(key, item) if **key == Ty::Str => {
            child("Item", item);
            if decoding {
                "    wrap(source.start_object())?\n    mut result = allocated[dict[str, Item], E, D](dict[str, Item](), source)?\n    while True:\n        match wrap(source.next_key())?:\n            case Some(key):\n                if key in result:\n                    return at[T, E](Err(error(source.reject(Fault.DuplicateField(key)))), Step.Key(key))\n                item = at(decode[Item, E, D](source), Step.Key(key))?\n                at(allocated[(), E, D](result.insert(key, item), source), Step.Key(key))?\n            case Nothing:\n                break\n    Ok(result)\n".into()
            } else {
                "    wrap(out.start_object())?\n    for key, item in value.items():\n        at(wrap(out.key(key)), Step.Key(*key))?\n        at(encode[Item, E, S](item, out), Step.Key(*key))?\n    wrap(out.end_object())\n".into()
            }
        }
        Ty::Enum(option) if option.is_option() => {
            let item = &option.get().variants[1].fields[0];
            let mut represented = item;
            while let Ty::Box(inner) = represented {
                represented = inner;
            }
            if matches!(represented, Ty::Enum(t) if t.is_option()) {
                return Err(at.error(
                    "nested Option values cannot be serialized: both Nothing levels encode as null",
                ));
            }
            child("Item", item);
            if decoding {
                "    if wrap(source.null())?:\n        return Ok(Nothing)\n    Ok(Some(decode[Item, E, D](source)?))\n".into()
            } else {
                "    match value:\n        case Some(item):\n            encode[Item, E, S](item, out)\n        case Nothing:\n            wrap(out.null())\n".into()
            }
        }
        Ty::Enum(tuple) if tuple.tuple() => {
            let fields = &tuple.get().variants[0].fields;
            let mut operation = format!(
                "    wrap({}.start_array())?\n",
                if decoding { "source" } else { "out" }
            );
            for (i, t) in fields.iter().enumerate() {
                let label = format!("Part{i}");
                child(&label, t);
                if decoding {
                    operation.push_str(&format!("    if not at(wrap(source.next_element()), Step.Index({i}))?:\n        return Err(error(source.reject(Fault.WrongLength({}, {i}))))\n    part{i} = at(decode[{label}, E, D](source), Step.Index({i}))?\n",fields.len()));
                } else {
                    operation.push_str(&format!(
                        "    at(encode[{label}, E, S](&(value)[{i}], out), Step.Index({i}))?\n"
                    ));
                }
            }
            if decoding {
                operation.push_str(&format!("    if at(wrap(source.next_element()), Step.Index({}))?:\n        return Err(error(source.reject(Fault.WrongLength({}, {}))))\n    Ok(({}))\n",fields.len(),fields.len(),fields.len()+1,(0..fields.len()).map(|i|format!("part{i},")).collect::<Vec<_>>().join(" ")));
            } else {
                operation.push_str("    wrap(out.end_array())\n");
            }
            operation
        }
        Ty::Box(item) => {
            child("Item", item);
            if decoding {
                "    item = decode[Item, E, D](source)?\n    allocated[T, E, D](Box(item), source)\n"
                    .into()
            } else {
                "    encode[Item, E, S](&*value, out)\n".into()
            }
        }
        Ty::Class(_) | Ty::Enum(_) if !matches!(ty, Ty::Enum(t) if t.is_result() || t.anonymous()) =>
        {
            let owner = classes::method_owner(ty).unwrap();
            hook = Some(crate::record::method(
                owner,
                if decoding { "__decode__" } else { "__encode__" },
            ));
            if decoding {
                "    hook[E, D](source)\n".into()
            } else {
                "    hook[E, S](value, out)\n".into()
            }
        }
        _ => {
            return Err(at.error(format!(
                "{ty} cannot be {}: unsupported serialization type",
                if decoding { "decoded" } else { "encoded" }
            )));
        }
    };
    let mut generated = template.clone();
    generated.body = body(&operation, at, &names)?;
    if let Some(symbol) = hook {
        let Statement::Expr(Expr {
            kind: Expression::GenericCall(name, ..),
            ..
        }) = &mut generated.body[0].kind
        else {
            unreachable!("generated hook is a generic call")
        };
        *name = symbol;
    }
    Ok(Some(Arc::new(generated)))
}
