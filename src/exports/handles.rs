//! Opaque native owners and their fallible, automatically dropped source wrappers.
use super::*;
use crate::record::ClassType;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct Handle {
    pub class: Rc<ClassType>,
    pub name: String,
    pub pointer: String,
    pub c_name: String,
    pub destroy: String,
}

pub(super) fn short_name(class: &ClassType) -> &str {
    class.name.rsplit('.').next().unwrap()
}

pub(super) fn collect(
    library: &str,
    exports: &[Export],
) -> Result<Vec<Handle>, Box<dyn std::error::Error>> {
    let mut classes = BTreeMap::new();
    for export in exports {
        let output = export
            .signature
            .outputs
            .first()
            .and_then(result_payloads)
            .map(|(ok, _)| ok);
        for ty in export
            .signature
            .inputs
            .iter()
            .map(|(_, ty)| ty)
            .chain(output)
        {
            let ty = if let Ty::Ref(inner, _) = ty {
                inner.as_ref()
            } else {
                ty
            };
            let Ty::Class(class) = ty else { continue };
            let name = short_name(class);
            super::check_library_name(name)?;
            if classes
                .insert(name, class)
                .is_some_and(|previous| previous != class)
            {
                return Err(
                    format!("exported classes have conflicting interface name `{name}`").into(),
                );
            }
        }
    }
    let mut handles = Vec::new();
    let mut c_names = BTreeSet::new();
    for (name, class) in classes {
        if name.starts_with("plenty_") {
            return Err(format!(
                "exported class `{name}` conflicts with the reserved C metadata namespace"
            )
            .into());
        }
        let c_name = format!("{library}_{name}");
        let destroy = format!("{c_name}_destroy");
        let pointer = format!("_plenty_handle_{name}");
        let drop = format!("_plenty_drop_{name}");
        for identifier in [&c_name, &destroy] {
            if !c_names.insert(identifier.clone()) {
                return Err(format!(
                    "generated C identifier collision `{identifier}` between exported classes"
                )
                .into());
            }
        }
        if exports.iter().any(|e| {
            [name, &pointer, &drop].contains(&e.name.as_str())
                || [c_name.as_str(), destroy.as_str()].contains(&e.symbol.as_str())
        }) {
            return Err(format!("export conflicts with generated handle `{name}`").into());
        }
        handles.push(Handle {
            class: class.clone(),
            name: name.into(),
            pointer,
            c_name,
            destroy,
        });
    }
    Ok(handles)
}

pub(super) fn source_type(ty: &Ty) -> String {
    match ty {
        Ty::Class(class) => short_name(class).into(),
        Ty::Ref(inner, mutable) => format!(
            "&{}{}",
            if *mutable { "mut " } else { "" },
            source_type(inner)
        ),
        _ => ty.to_string(),
    }
}

pub(super) fn arguments(export: &Export) -> (Vec<String>, Vec<String>, String) {
    let mut transfers = String::new();
    let (parameters, arguments) = export
        .signature
        .inputs
        .iter()
        .enumerate()
        .map(|(i, (_, ty))| {
            if let Ty::Class(class) = ty {
                let pointer = format!("_plenty_handle_{}", short_name(class));
                transfers.push_str(&format!("    mut consumed{i} = p{i}\n    raw{i} = consumed{i}._handle\n    consumed{i}._handle = {pointer}.null()\n"));
                return (format!("p{i}: {pointer}"), format!("raw{i}"));
            }
            if let Ty::Ref(inner, _) = ty {
                if let Ty::Class(class) = inner.as_ref() {
                    let pointer = format!("_plenty_handle_{}", short_name(class));
                    return (format!("p{i}: {pointer}"), format!("p{i}._handle"));
                }
            }
            (format!("p{i}: {ty}"), format!("p{i}"))
        })
        .unzip();
    (parameters, arguments, transfers)
}

impl Handle {
    pub(super) fn declarations(&self, source: &mut String, header: &mut String) {
        let Self {
            name,
            pointer,
            c_name,
            destroy,
            ..
        } = self;
        source.push_str(&format!("opaque {pointer}\nextern def _plenty_drop_{name}(value: {pointer}) -> () = \"{destroy}\"\npub class {name}:\n    _handle: {pointer}\n    def __del__(self: &mut {name}) -> ():\n        _plenty_drop_{name}(self._handle)\n\n"));
        header.push_str(&format!("\ntypedef struct {c_name} {c_name};\n/* Requires: null or one live owned handle from this library instance.\n * Use its creating thread, with no outstanding borrows or concurrent/reentrant access.\n * Keep the originating library loaded through this call.\n * Guarantees: null is a no-op; otherwise consumes and releases the owner,\n * running its destructor and field cleanup. All aliases become invalid.\n * Call exactly once per owned handle; never use free() or another library's destroy.\n */\nvoid {destroy}({c_name} *value);\n"));
    }
}
