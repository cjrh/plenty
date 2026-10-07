//! Trusted declarations live only in explicit .plentyi interface modules.
use super::*;

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
    use crate::foreign::Argument;
    let declaration = f.foreign.as_ref().unwrap();
    let scalar = |ty: &Ty| ty.is_numeric() || matches!(ty, Ty::ForeignPtr(_));
    for ((_, ty), mode) in inputs.iter().zip(&declaration.arguments) {
        let valid = match mode {
            Argument::Direct => scalar(ty) || matches!(ty, Ty::Ref(t, _) if scalar(t)),
            Argument::Utf8 | Argument::CString => matches!(ty, Ty::Ref(t, false) if **t == Ty::Str),
        };
        if !valid {
            return Err(f.at.error(format!("`{ty}` has no supported C ABI for {mode:?}; text adapters require &str, raw pointers require scalars or opaque types")));
        }
    }
    let output = if declaration.fallible() {
        let Some(Ty::Enum(t)) = output else {
            return Err(f.at.error(
                "c_string adapters require Result[T, CStrError] as the declared return type",
            ));
        };
        if !t.propagatable() || t.is_option() || t.variants[1].fields != [crate::sum::c_str_error()]
        {
            return Err(f.at.error(
                "c_string adapters require Result[T, CStrError] as the declared return type",
            ));
        }
        Some(&t.variants[0].fields[0])
    } else {
        output.as_ref()
    };
    if let Some(ty) = output {
        if *ty != Ty::Unit && !scalar(ty) {
            return Err(f.at.error(format!(
                "`{ty}` has no supported C ABI return representation"
            )));
        }
    }
    Ok(())
}

pub(super) fn check_symbols<'a>(
    functions: impl Iterator<Item = &'a Function>,
    sigs: &HashMap<String, Rc<FnSig>>,
) -> Result<()> {
    let mut symbols = HashMap::new();
    fn abi(ty: &Ty) -> String {
        if matches!(ty, Ty::ForeignPtr(_) | Ty::Ref(..)) {
            "pointer".into()
        } else {
            ty.to_string()
        }
    }
    for f in functions {
        if let Some(declaration) = &f.foreign {
            let symbol = &declaration.symbol;
            let sig = &sigs[&f.name];
            let mut inputs = Vec::new();
            for ((_, ty), mode) in sig.inputs.iter().zip(&declaration.arguments) {
                match mode {
                    crate::foreign::Argument::Direct => inputs.push(abi(ty)),
                    crate::foreign::Argument::Utf8 => {
                        inputs.extend(["pointer".into(), "u64".into()])
                    }
                    crate::foreign::Argument::CString => inputs.push("pointer".into()),
                }
            }
            let signature = (inputs, declaration.output(sig).map(abi));
            if let Some(previous) = symbols.insert(symbol, signature.clone()) {
                if previous != signature {
                    return Err(f
                        .at
                        .error(format!("conflicting C signatures for symbol `{symbol}`")));
                }
            }
        }
    }
    Ok(())
}
