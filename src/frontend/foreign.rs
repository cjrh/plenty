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
    for ty in inputs.iter().map(|(_, t)| t).chain(output.iter()) {
        if !(ty.is_numeric() || matches!(ty, Ty::ForeignPtr(_))) {
            return Err(f.at.error(format!("C imports require fixed-width integers, floats, opaque pointers, or a unit return; `{ty}` has no supported C ABI")));
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
        if matches!(ty, Ty::ForeignPtr(_)) {
            "pointer".into()
        } else {
            ty.to_string()
        }
    }
    for f in functions {
        if let Some(symbol) = &f.foreign {
            let sig = &sigs[&f.name];
            let signature = (
                sig.inputs.iter().map(|(_, t)| abi(t)).collect::<Vec<_>>(),
                sig.outputs.iter().map(abi).collect::<Vec<_>>(),
            );
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
