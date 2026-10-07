//! Generate a typed, explicit loader for a known C export contract.
use super::{Export, Interface};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn literal(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}

pub(super) fn generate(interface: &Interface, exports: &[Export]) -> Result<String> {
    if exports.len() > 128 {
        return Err("runtime interfaces support at most 128 exported functions".into());
    }
    for export in exports {
        if export.signature.inputs.len() > 254 {
            return Err("runtime interface methods support at most 254 parameters".into());
        }
        if export
            .signature
            .inputs
            .iter()
            .any(|(_, ty)| !ty.is_numeric())
            || export.signature.outputs.iter().any(|ty| !ty.is_numeric())
        {
            return Err(
                "runtime interface generation currently supports numeric scalars and unit".into(),
            );
        }
    }
    let mut source =
        String::from("# Generated typed runtime interface. Importing does not load native code.\n");
    source.push_str(include_str!("dynamic_helpers.plentyi"));
    for (i, export) in exports.iter().enumerate() {
        let params: Vec<_> = export
            .signature
            .inputs
            .iter()
            .enumerate()
            .map(|(n, (_, ty))| format!("p{n}: {ty}"))
            .collect();
        let output = export
            .signature
            .outputs
            .first()
            .map(ToString::to_string)
            .unwrap_or("()".into());
        let tail = if params.is_empty() {
            String::new()
        } else {
            format!(", {}", params.join(", "))
        };
        source.push_str(&format!(
            "extern def _call{i}(address: _FunctionAddress{tail}) -> {output} = address\n"
        ));
    }
    source.push_str("\npub class Library:\n");
    for i in 0..exports.len() {
        source.push_str(&format!("    _code{i}: _FunctionAddress\n"));
    }
    for (i, export) in exports.iter().enumerate() {
        let params: Vec<_> = export
            .signature
            .inputs
            .iter()
            .enumerate()
            .map(|(n, (_, ty))| format!("p{n}: {ty}"))
            .collect();
        let args: Vec<_> = (0..params.len()).map(|n| format!("p{n}")).collect();
        let output = export
            .signature
            .outputs
            .first()
            .map(ToString::to_string)
            .unwrap_or("()".into());
        let params = if params.is_empty() {
            String::new()
        } else {
            format!(", {}", params.join(", "))
        };
        let args = if args.is_empty() {
            String::new()
        } else {
            format!(", {}", args.join(", "))
        };
        source.push_str(&format!(
            "    pub def {}(self{params}) -> {output}:\n        _call{i}(self._code{i}{args})\n",
            export.name
        ));
    }
    source.push_str("\npub def load(path: &str) -> Result[Library, LoadError]:\n    mut lease = _lease()?\n    mut handle = _LibraryLease.null()\n    status = _open(path, &mut handle)\n    if status != 0:\n        return Err(_error(status))\n    lease._handle = handle\n");
    source.push_str(&format!("    discovery = {}\n    expected = {}\n    checked = _check(handle, &discovery, &expected)\n    if checked != 0:\n        return Err(_error(checked))\n    _lookup(handle, {})?\n", literal(&interface.discovery), literal(&interface.source), literal(&interface.contract_guard)));
    for (i, export) in exports.iter().enumerate() {
        source.push_str(&format!(
            "    code{i} = _lookup(handle, {})?\n",
            literal(&export.symbol)
        ));
    }
    let args: Vec<_> = (0..exports.len()).map(|i| format!("code{i}")).collect();
    source.push_str(&format!("    match Library({}):\n        case Ok(value):\n            Ok(value)\n        case Err(error):\n            Err(_allocation(error))\n", args.join(", ")));
    Ok(source)
}
