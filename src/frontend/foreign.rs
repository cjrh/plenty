//! Trusted declarations live only in explicit .plentyi or .pi interface modules.
use super::*;

pub(super) fn loader_symbol(name: &str) -> bool {
    matches!(
        name,
        "plenty_library_open_v1"
            | "plenty_library_symbol_v1"
            | "plenty_library_close_v1"
            | "plenty_library_contract_v1"
            | "plenty_library_require_origin_v1"
    )
}

fn check_loader_signature(f: &Function, inputs: &[(String, Ty)], output: &Type) -> Result<()> {
    use crate::foreign::Argument;
    let declaration = f.foreign.as_ref().unwrap();
    let Some(symbol) = declaration.symbol().filter(|name| loader_symbol(name)) else {
        return Ok(());
    };
    let shape: Vec<_> = inputs
        .iter()
        .zip(&declaration.arguments)
        .map(|((_, ty), adapter)| match (ty, adapter) {
            (Ty::ForeignPtr(..), Argument::Direct) => "handle",
            (Ty::Ref(inner, true), Argument::Direct)
                if matches!(inner.as_ref(), Ty::ForeignPtr(..)) =>
            {
                "output"
            }
            (Ty::Ref(inner, false), Argument::Utf8) if **inner == Ty::Str => "text",
            _ => "invalid",
        })
        .collect();
    let valid = match symbol {
        "plenty_library_open_v1" => shape == ["text", "output"] && *output == Some(Ty::U32),
        "plenty_library_symbol_v1" => {
            shape == ["handle", "text", "output"] && *output == Some(Ty::U32)
        }
        "plenty_library_close_v1" => shape == ["handle"] && output.is_none(),
        "plenty_library_require_origin_v1" => shape == ["handle", "handle"] && output.is_none(),
        "plenty_library_contract_v1" => {
            shape == ["handle", "text", "text"] && *output == Some(Ty::U32)
        }
        _ => unreachable!(),
    };
    if !valid {
        return Err(f.at.error(format!(
            "invalid fixed runtime loader signature for `{symbol}`"
        )));
    }
    Ok(())
}

pub(super) fn check_symbol(name: &str, at: &Token) -> Result<()> {
    let mut chars = name.chars();
    if !chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        || name == "main"
        || name.starts_with("plenty_")
        || name.starts_with("__plenty_")
    {
        return Err(
            at.error("C symbol must be an identifier outside the reserved main/plenty_ namespaces")
        );
    }
    Ok(())
}

pub(super) fn check_signature(f: &Function, inputs: &[(String, Ty)], output: &Type) -> Result<()> {
    if let crate::foreign::Target::Intrinsic(intrinsic) = &f.foreign.as_ref().unwrap().target {
        use crate::foreign::Intrinsic;
        let pointer = |ty: &Ty| matches!(ty, Ty::ForeignPtr(..));
        let scalar = |ty: &Ty| ty.is_numeric() || *ty == Ty::Bool || pointer(ty);
        let valid = match intrinsic.as_ref() {
            Intrinsic::Load => {
                inputs.len() == 2
                    && pointer(&inputs[0].1)
                    && inputs[1].1 == Ty::U64
                    && output.as_ref().is_some_and(scalar)
            }
            Intrinsic::Store => {
                inputs.len() == 3
                    && pointer(&inputs[0].1)
                    && inputs[1].1 == Ty::U64
                    && scalar(&inputs[2].1)
                    && output.is_none()
            }
            Intrinsic::Offset => {
                inputs.len() == 2
                    && pointer(&inputs[0].1)
                    && inputs[1].1 == Ty::U64
                    && output.as_ref() == Some(&inputs[0].1)
            }
            Intrinsic::Address(_) | Intrinsic::Static { .. } | Intrinsic::Object(_) => {
                inputs.is_empty() && output.as_ref().is_some_and(pointer)
            }
        };
        if !valid
            || f.foreign
                .as_ref()
                .unwrap()
                .arguments
                .iter()
                .any(|a| *a != crate::foreign::Argument::Direct)
        {
            return Err(f.at.error("invalid trusted C intrinsic signature: load(pointer, u64) -> scalar/pointer; store(pointer, u64, scalar/pointer) -> (); offset(pointer, u64) -> same pointer; address/static/storage () -> pointer"));
        }
        return Ok(());
    }
    check_loader_signature(f, inputs, output)?;
    use crate::foreign::Argument;
    let declaration = f.foreign.as_ref().unwrap();
    if let crate::foreign::Target::Parameter(index) = declaration.target
        && (!matches!(inputs[index].1, Ty::ForeignPtr(..))
            || declaration.arguments[index] != Argument::Direct)
    {
        return Err(f
            .at
            .error("C function-address parameter must have an opaque pointer type"));
    }
    if declaration
        .arguments
        .iter()
        .filter(|mode| **mode == Argument::Errno)
        .count()
        > 1
    {
        return Err(f
            .at
            .error("a C declaration permits only one errno output adapter"));
    }
    let scalar = |ty: &Ty| ty.is_numeric() || *ty == Ty::Bool || matches!(ty, Ty::ForeignPtr(..));
    for ((_, ty), mode) in inputs.iter().zip(&declaration.arguments) {
        let valid = match mode {
            Argument::Direct => scalar(ty) || matches!(ty, Ty::Ref(t, _) if scalar(t)),
            Argument::Utf8 | Argument::CString => matches!(ty, Ty::Ref(t, false) if **t == Ty::Str),
            Argument::Errno => matches!(ty, Ty::Ref(t, true) if **t == Ty::I32),
            Argument::CStrings => {
                matches!(ty, Ty::Ref(t, false) if matches!(t.as_ref(), Ty::List(element) if **element == Ty::Str))
            }
            Argument::Strided => {
                matches!(ty, Ty::Ref(t, _) if matches!(t.as_ref(), Ty::List(element) if element.is_numeric()))
                    || matches!(ty, Ty::View(element, false) if **element == Ty::U8)
            }
        };
        if !valid {
            let requirement = match mode {
                Argument::Errno => "errno requires &mut i32",
                Argument::CStrings => "c_strings requires &list[str]",
                _ => "text adapters require &str, raw pointers require scalars or opaque types",
            };
            return Err(f.at.error(format!(
                "`{ty}` has no supported C ABI for {mode:?}; {requirement}"
            )));
        }
    }
    let output = if declaration.fallible() {
        let Some(Ty::Enum(t)) = output else {
            return Err(f.at.error(
                "c_string/c_strings adapters require Result[T, CStrError] as the declared return type",
            ));
        };
        if !t.propagatable()
            || t.is_option()
            || t.get().variants[1].fields != [crate::sum::c_str_error()]
        {
            return Err(f.at.error(
                "c_string/c_strings adapters require Result[T, CStrError] as the declared return type",
            ));
        }
        Some(&t.get().variants[0].fields[0])
    } else {
        output.as_ref()
    };
    // Worker code may hold a marked value and so may call anything that
    // takes it, including the release function its owner's `__del__` calls.
    // A function-address call has no symbol to certify; `threading::inspect`
    // rejects it in worker code instead.
    let marked = |ty: &Ty| {
        let ty = match ty {
            Ty::Ref(inner, _) => inner.as_ref(),
            ty => ty,
        };
        matches!(ty, Ty::ForeignPtr(_, threads) if *threads != crate::foreign::Threads::None)
    };
    if !declaration.thread_safe
        && declaration.symbol().is_some()
        && let Some(ty) = inputs
            .iter()
            .map(|(_, ty)| ty)
            .chain(output)
            .find(|ty| marked(ty))
    {
        return Err(f.at.error(format!(
            "C function taking or returning `{ty}` must be `@thread_safe`: its opaque type has a thread contract, so worker code may call it"
        )));
    }
    if let Some(ty) = output
        && *ty != Ty::Unit
        && !scalar(ty)
    {
        return Err(f.at.error(format!(
            "`{ty}` has no supported C ABI return representation"
        )));
    }
    Ok(())
}

pub(super) fn check_symbols<'a>(
    functions: impl Iterator<Item = &'a Function>,
    sigs: &super::functions::Functions,
) -> Result<()> {
    let mut symbols = HashMap::default();
    fn abi(ty: &Ty) -> String {
        if matches!(ty, Ty::ForeignPtr(..) | Ty::Ref(..)) {
            "pointer".into()
        } else {
            ty.to_string()
        }
    }
    for f in functions {
        if let Some(declaration) = &f.foreign {
            let Some(symbol) = declaration.symbol() else {
                continue;
            };
            let sig = &sigs[&f.name];
            let mut inputs = Vec::new();
            for ((_, ty), mode) in sig.inputs.iter().zip(&declaration.arguments) {
                match mode {
                    crate::foreign::Argument::Direct => inputs.push(abi(ty)),
                    crate::foreign::Argument::Utf8 => {
                        inputs.extend(["pointer".into(), "u64".into()])
                    }
                    crate::foreign::Argument::CString | crate::foreign::Argument::CStrings => {
                        inputs.push("pointer".into())
                    }
                    crate::foreign::Argument::Errno => {}
                    crate::foreign::Argument::Strided => {
                        inputs.extend(["pointer".into(), "u64".into(), "u64".into()])
                    }
                }
            }
            let signature = (inputs, declaration.output(sig).map(abi));
            if let Some((previous, thread_safe)) =
                symbols.insert(symbol, (signature.clone(), declaration.thread_safe))
            {
                if previous != signature {
                    return Err(f
                        .at
                        .error(format!("conflicting C signatures for symbol `{symbol}`")));
                }
                // One C function has one thread-safety; declarations that
                // disagree leave a reader unable to tell which is right.
                if thread_safe != declaration.thread_safe {
                    return Err(f.at.error(format!(
                        "conflicting `@thread_safe` declarations for symbol `{symbol}`; annotate every declaration of it or none"
                    )));
                }
            }
        }
    }
    Ok(())
}
