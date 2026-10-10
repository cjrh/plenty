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
    if interface.source.len() > 4 * 1024 * 1024 {
        return Err("runtime interface contract exceeds the 4 MiB discovery limit".into());
    }
    if exports
        .iter()
        .map(|e| e.symbol.as_str())
        .chain(interface.handles.iter().map(|h| h.destroy.as_str()))
        .chain([
            interface.discovery.as_str(),
            interface.contract_guard.as_str(),
        ])
        .any(|symbol| symbol.len() >= 512)
    {
        return Err("runtime interface symbol names must be shorter than 512 bytes".into());
    }
    if exports.len() + interface.handles.len() > 128 {
        return Err(
            "runtime interfaces support at most 128 functions including handle destructors".into(),
        );
    }
    for export in exports {
        if export.signature.inputs.len() > 240 {
            return Err("runtime interface methods support at most 240 parameters".into());
        }
        let consumed = export
            .signature
            .inputs
            .iter()
            .filter(|(_, ty)| matches!(ty, crate::op::Ty::Class(_)))
            .count();
        if export.signature.inputs.len() + consumed * 2 > 240 {
            return Err(
                "runtime interface method exceeds parameter/ownership temporary slot limits".into(),
            );
        }
        if export.name.starts_with('_') || matches!(export.name.as_str(), "new" | "self") {
            return Err(
                "runtime interface method names beginning with `_`, `new`, and `self` are reserved"
                    .into(),
            );
        }
    }
    if interface
        .handles
        .iter()
        .any(|h| matches!(h.name.as_str(), "Library" | "load"))
    {
        return Err(
            "exported class `Library` or `load` conflicts with the runtime interface API".into(),
        );
    }
    let methods: Vec<_> = exports
        .iter()
        .enumerate()
        .map(|(i, e)| methods::generate(e, i, interface))
        .collect();
    let mut source =
        String::from("# Generated typed runtime interface. Importing does not load native code.\n");
    source.push_str(include_str!("dynamic_helpers.plentyi"));
    for handle in &interface.handles {
        let name = &handle.name;
        let pointer = &handle.pointer;
        source.push_str(&format!("\nopaque {pointer}\nextern def _destroy_{name}(address: _FunctionAddress, value: {pointer}) -> () = address\npub class {name}:\n    _origin: _FunctionAddress\n    _destroy: _FunctionAddress\n    _handle: {pointer}\n    def __del__(self: &mut {name}) -> ():\n        _destroy_{name}(self._destroy, self._handle)\n\n"));
    }
    for method in &methods {
        source.push_str(&method.declaration);
    }
    source.push_str("\npub class Library:\n    _origin: _FunctionAddress\n");
    for i in 0..exports.len() {
        source.push_str(&format!("    _code{i}: _FunctionAddress\n"));
    }
    for handle in &interface.handles {
        source.push_str(&format!("    _drop_{}: _FunctionAddress\n", handle.name));
    }
    for method in &methods {
        source.push_str(&method.definition);
    }
    source.push_str("\npub def load(path: &str) -> Result[Library, LoadError]:\n    mut lease = _lease()?\n    mut handle = _LibraryLease.null()\n    status = _open(path, &mut handle)\n    if status != 0:\n        return Err(_error(status))\n    lease._handle = handle\n");
    source.push_str(&format!("    discovery = {}\n    expected = {}\n    checked = _check(handle, &discovery, &expected)\n    if checked != 0:\n        return Err(_error(checked))\n    origin = _lookup(handle, {})?\n", literal(&interface.discovery), literal(&interface.source), literal(&interface.contract_guard)));
    for (i, export) in exports.iter().enumerate() {
        source.push_str(&format!(
            "    code{i} = _lookup(handle, {})?\n",
            literal(&export.symbol)
        ));
    }
    let mut args = vec!["origin".to_string()];
    args.extend((0..exports.len()).map(|i| format!("code{i}")));
    for (i, handle) in interface.handles.iter().enumerate() {
        source.push_str(&format!(
            "    destructor{i} = _lookup(handle, {})?\n",
            literal(&handle.destroy)
        ));
        args.push(format!("destructor{i}"));
    }
    source.push_str(&format!("    Ok(Library({}))\n", args.join(", ")));
    Ok(source)
}
