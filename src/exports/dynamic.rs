//! Generate a typed, explicit loader for a known C export contract.
use super::{Export, Interface};
mod methods;

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
        if export.signature.inputs.len() > 240 {
            return Err("runtime interface methods support at most 240 parameters".into());
        }
        if export.name.starts_with('_') {
            return Err("runtime interface method names beginning with `_` are reserved".into());
        }
    }
    if !interface.handles.is_empty() {
        return Err("runtime interface generation does not yet support owned class handles".into());
    }
    let methods: Vec<_> = exports
        .iter()
        .enumerate()
        .map(|(i, e)| methods::generate(e, i))
        .collect();
    let mut source =
        String::from("# Generated typed runtime interface. Importing does not load native code.\n");
    source.push_str(include_str!("dynamic_helpers.plentyi"));
    for method in &methods {
        source.push_str(&method.declaration);
    }
    source.push_str("\npub class Library:\n");
    for i in 0..exports.len() {
        source.push_str(&format!("    _code{i}: _FunctionAddress\n"));
    }
    for method in &methods {
        source.push_str(&method.definition);
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
