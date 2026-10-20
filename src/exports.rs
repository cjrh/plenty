//! A checked C export drives native entry adapters and both interface views.
use crate::op::{FnSig, Ty};
use std::sync::Arc;
mod dynamic;
mod handles;
pub(crate) use handles::Handle;

pub(crate) struct Export {
    pub function: String,
    pub name: String,
    pub symbol: String,
    pub signature: Arc<FnSig>,
    pub documentation: String,
}

pub(crate) fn check_signature(inputs: &[(String, Ty)], output: &Option<Ty>) -> Result<(), String> {
    fn generic_class(ty: &Ty) -> bool {
        match ty {
            Ty::Class(class) => class.name.contains('['),
            Ty::Ref(inner, _) => generic_class(inner),
            _ => false,
        }
    }
    if inputs.iter().any(|(_, ty)| generic_class(ty))
        || output
            .as_ref()
            .and_then(result_payloads)
            .is_some_and(|(ok, _)| generic_class(ok))
    {
        return Err(
            "generic class instances cannot be C export handles yet; use a nongeneric facade class"
                .into(),
        );
    }
    if let Some((ok, Ty::Enum(error))) = output.as_ref().and_then(result_payloads) {
        if *ok == Ty::Unit || ok.is_numeric() {
            unsupported_error(error)?;
        }
    }
    if inputs
        .iter()
        .any(|(_, ty)| !ty.is_numeric() && !matches!(ty, Ty::Class(_)) && !matches!(ty, Ty::Ref(inner, _) if inner.is_numeric() || matches!(inner.as_ref(), Ty::Class(_))))
        || output.as_ref().is_some_and(|ty| {
            !ty.is_numeric()
                && !result_payloads(ty).is_some_and(|(ok, error)| {
                    ((*ok == Ty::Unit || ok.is_numeric())
                        && (error.is_numeric() || error_variants(error).is_some()))
                        || (matches!(ok, Ty::Class(_)) && *error == crate::sum::alloc_error())
                })
        })
    {
        return Err("C exports support numeric scalar or owned class parameters and their borrows; returns must be numeric scalar, (), Result[scalar or (), E], or Result[Class, AllocError], where E is numeric, AllocError, ParseError, RangeError, Failure, or a payload-free enum".into());
    }
    Ok(())
}

const BUILTIN_ERRORS: [&str; 4] = ["AllocError", "ParseError", "RangeError", "Failure"];

/// Explains why an enum error cannot cross C as a `uint32_t` code.
fn unsupported_error(error: &crate::nominal::Nominal<crate::sum::EnumType>) -> Result<(), String> {
    if BUILTIN_ERRORS.contains(&&*error.name) || error.propagatable() || error.tuple() {
        return Ok(());
    }
    if error.anonymous() {
        return Err(format!("C exports cannot return the error sum `{}` yet (backlog B27); return one payload-free error enum", Ty::Enum(error.clone())));
    }
    if error.name.contains('[') {
        return Err(format!(
            "C export error enum `{}` is generic; use a nongeneric payload-free enum",
            Ty::Enum(error.clone())
        ));
    }
    let name = short_name(error);
    // An unfinalized definition is not coded; the general diagnostic covers it.
    let Some(definition) = error.try_get() else {
        return Ok(());
    };
    let variants = &definition.variants;
    if let Some(variant) = variants.iter().find(|v| !v.fields.is_empty()) {
        return Err(format!("C export error enum `{name}` has payload variant `{name}.{}`; only payload-free enums cross C, as uint32_t variant codes", variant.name));
    }
    if crate::frontend::builtin(name) {
        return Err(format!(
            "builtin error `{name}` has no C error code; use AllocError, ParseError, RangeError, Failure, or a payload-free enum"
        ));
    }
    if name.starts_with("plenty_") {
        return Err(format!(
            "C export error enum `{name}` conflicts with the reserved C metadata namespace"
        ));
    }
    Ok(())
}

fn short_name(error: &crate::nominal::Nominal<crate::sum::EnumType>) -> &str {
    error.name.rsplit('.').next().unwrap()
}

/// A payload-free enum crosses C as a `uint32_t` code: its variant's
/// declaration index. Returns the variant names in code order.
pub(crate) fn error_variants(ty: &Ty) -> Option<Vec<String>> {
    match ty {
        Ty::Enum(t) if BUILTIN_ERRORS.contains(&&*t.name) || declared_error(ty).is_some() => {
            Some(t.get().variants.iter().map(|v| v.name.clone()).collect())
        }
        _ => None,
    }
}

/// A coded user enum, which generated interfaces re-declare so they stay
/// self-contained; returns its interface name.
pub(crate) fn declared_error(ty: &Ty) -> Option<&str> {
    let Ty::Enum(t) = ty else { return None };
    (!BUILTIN_ERRORS.contains(&&*t.name)
        && !t.propagatable()
        && !t.tuple()
        && !t.anonymous()
        && unsupported_error(t).is_ok()
        && t.try_get().is_some_and(|t| !t.variants.is_empty()))
    .then(|| short_name(t))
}

/// The error's spelling in generated interface source.
pub(crate) fn error_source(ty: &Ty) -> String {
    declared_error(ty).map_or_else(|| ty.to_string(), Into::into)
}

pub(crate) fn result_payloads(ty: &Ty) -> Option<(&Ty, &Ty)> {
    match ty {
        Ty::Enum(t) if t.propagatable() && !t.is_option() => Some((
            &t.local().variants[0].fields[0],
            &t.local().variants[1].fields[0],
        )),
        _ => None,
    }
}

fn c_type(ty: &Ty, library: &str) -> String {
    if let Ty::Ref(inner, mutable) = ty {
        if matches!(inner.as_ref(), Ty::Class(_)) {
            return format!(
                "{}{}",
                if *mutable { "" } else { "const " },
                c_type(inner, library)
            );
        }
        return format!(
            "{}{} *",
            if *mutable { "" } else { "const " },
            c_type(inner, library)
        );
    }
    match ty {
        Ty::Class(t) => return format!("{library}_{} *", handles::short_name(t)),
        Ty::I8 => "int8_t",
        Ty::I16 => "int16_t",
        Ty::I32 => "int32_t",
        Ty::I64 => "int64_t",
        Ty::U8 => "uint8_t",
        Ty::U16 => "uint16_t",
        Ty::U32 => "uint32_t",
        Ty::U64 => "uint64_t",
        Ty::F32 => "float",
        Ty::F64 => "double",
        Ty::Enum(_) if error_variants(ty).is_some() => "uint32_t",
        _ => unreachable!("checked C export type"),
    }
    .into()
}

/// The namespace is also an importable Plenty module name and a C identifier.
pub(crate) fn check_library_name(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    if name.is_empty()
        || !name.as_bytes()[0].is_ascii_alphabetic()
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        || crate::frontend::builtin(name)
        || crate::frontend::reserved(name)
    {
        return Err(
            "library name must be a non-builtin ASCII identifier beginning with a letter".into(),
        );
    }
    Ok(())
}

/// Declares each coded error's C constants once, and returns the source
/// declarations of the user enums among them.
fn error_codes(
    library: &str,
    exports: &[Export],
    handles: &[Handle],
    discovery: &str,
    header: &mut String,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut seen: Vec<&Ty> = Vec::new();
    let mut constants = crate::hash::HashSet::default();
    let mut source = String::new();
    for export in exports {
        let Some((_, error)) = export.signature.outputs.first().and_then(result_payloads) else {
            continue;
        };
        let Some(variants) = error_variants(error) else {
            continue;
        };
        let interface_name = error_source(error);
        if let Some(previous) = seen
            .iter()
            .find(|previous| error_source(previous) == interface_name)
        {
            if *previous != error {
                return Err(format!(
                    "exported error enums have conflicting interface name `{interface_name}`"
                )
                .into());
            }
            continue;
        }
        if handles.iter().any(|h| h.name == interface_name)
            || exports.iter().any(|e| e.name == interface_name)
        {
            return Err(format!(
                "exported error enum `{interface_name}` conflicts with a generated interface name"
            )
            .into());
        }
        seen.push(error);
        header.push_str(&format!(
            "\n/* {interface_name} codes, written to *out_error when a call returns 1. */\n"
        ));
        for (code, variant) in variants.iter().enumerate() {
            let constant = format!("{library}_{interface_name}_{variant}");
            if !constants.insert(constant.clone())
                || exports.iter().any(|e| e.symbol == constant)
                || handles
                    .iter()
                    .any(|h| h.c_name == constant || h.destroy == constant)
                || constant == discovery
                || constant.starts_with(&format!("{library}_plenty_"))
            {
                return Err(format!(
                    "generated C error constant `{constant}` collides with another C identifier"
                )
                .into());
            }
            header.push_str(&format!("#define {constant} UINT32_C({code})\n"));
        }
        if declared_error(error).is_some() {
            source.push_str(&format!("pub enum {interface_name}:\n"));
            for variant in &variants {
                source.push_str(&format!("    {variant}\n"));
            }
            source.push('\n');
        }
    }
    Ok(source)
}

/// Rebuilds an `Err` from the C error output, inside a returning branch.
pub(crate) fn rebuild_error(error: &Ty, indent: &str) -> String {
    let Some(variants) = error_variants(error) else {
        return format!("{indent}Err(out_error)\n");
    };
    let name = error_source(error);
    let (last, rest) = variants.split_last().unwrap();
    let mut body = String::new();
    for (code, variant) in rest.iter().enumerate() {
        body.push_str(&format!(
            "{indent}if out_error == {code}:\n{indent}    return Err({name}.{variant})\n"
        ));
    }
    body.push_str(&format!("{indent}Err({name}.{last})\n"));
    body
}

pub(crate) struct Interface {
    pub name: String,
    pub discovery: String,
    pub source: String,
    pub header: String,
    pub handles: Vec<Handle>,
    pub contract_guard: String,
    /// Source declarations of the user error enums the exports return.
    pub errors: String,
}

impl Interface {
    pub fn runtime_source(&self, exports: &[Export]) -> Result<String, Box<dyn std::error::Error>> {
        dynamic::generate(self, exports)
    }
    pub fn validate_imports(
        &self,
        imports: &crate::hash::HashSet<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let metadata = format!("{}_plenty_contract", self.name);
        for symbol in [&self.discovery, &self.contract_guard, &metadata]
            .into_iter()
            .chain(self.handles.iter().map(|h| &h.destroy))
        {
            if imports.contains(symbol) {
                return Err(format!(
                    "imported C symbol `{symbol}` conflicts with a generated library symbol"
                )
                .into());
            }
        }
        Ok(())
    }

    pub fn new(name: &str, exports: &[Export]) -> Result<Self, Box<dyn std::error::Error>> {
        check_library_name(name)?;
        if exports.is_empty() {
            return Err("library requires at least one `export def` declaration".into());
        }
        let discovery = format!("{name}_plenty_interface_v1");
        let prefix = format!("{name}_");
        let handles = handles::collect(name, exports)?;
        for export in exports {
            if export.name.starts_with("_plenty_") {
                return Err(
                    "export names beginning `_plenty_` are reserved for generated adapters".into(),
                );
            }
            if !export.symbol.starts_with(&prefix)
                || export.symbol.starts_with(&format!("{name}_plenty_"))
            {
                return Err(format!("C export `{}` must start with `{prefix}` and cannot use reserved discovery symbol `{discovery}`", export.symbol).into());
            }
        }
        let mut source = format!("# plenty-interface-format: 1\n# library: {name}\n# target: {}\n# abi: C\n# This file is generated; its exact UTF-8 bytes are embedded in the library.\n# Linking remains explicit. Calls may terminate the process on runtime traps.\n\n", crate::native_target());
        // Preserve case: two case-distinct library namespaces need distinct guards.
        let guard = format!("PLENTY_HEADER_{name}_V1");
        let mut header = format!("/* Generated by Plenty. Target: {}. C ABI contract format 1. */\n#ifndef {guard}\n#define {guard}\n#include <stdint.h>\n#include <stddef.h>\n#ifdef __cplusplus\nextern \"C\" {{\n#endif\n\n/* All calls must be serialized on the same caller thread.\n * Foreign dependencies must honor their declared contracts.\n * Runtime traps and unchecked allocation failures terminate the process;\n * no exception or nonlocal jump may cross a Plenty frame.\n * Keep the originating shared library loaded while calling its functions.\n */\n", crate::native_target());
        for handle in &handles {
            handle.declarations(&mut source, &mut header);
        }
        let errors = error_codes(name, exports, &handles, &discovery, &mut header)?;
        source.push_str(&errors);
        for export in exports {
            let result = export.signature.outputs.first().and_then(result_payloads);
            header.push_str("\n/* Plenty declaration: ");
            header.push_str(&export.name);
            header.push_str(
                ".\n * Guarantees: scalar values are copied; borrowed storage is never retained.\n",
            );
            if let Some((Ty::Class(class), _)) = result {
                let handle = handles.iter().find(|h| h.class.name == class.name).unwrap();
                header.push_str(&format!(" * On Ok, out_ok receives one non-null owned handle from this library.\n * Release it exactly once with {}; never use free().\n * On Err no handle is transferred and out_ok is unchanged.\n", handle.destroy));
            } else if !export
                .signature
                .inputs
                .iter()
                .any(|(_, ty)| matches!(ty, Ty::Class(_)))
            {
                header.push_str(" * No ownership crosses this interface.\n");
            }
            for (i, (_, ty)) in export.signature.inputs.iter().enumerate() {
                if let Ty::Class(class) = ty {
                    let handle = handles.iter().find(|h| h.class.name == class.name).unwrap();
                    header.push_str(&format!(" * Requires p{i}: one live non-null owned {} handle from this library\n * instance, on its creating thread, with no outstanding borrows or aliases\n * accessing it. It must not alias any other handle argument.\n * Consumes p{i} on entry, on both Ok and Err. Never access or destroy the\n * original handle after calling; only a newly returned owner may be used.\n", handle.c_name));
                }
                if let Ty::Ref(inner, mutable) = ty {
                    if let Ty::Class(class) = inner.as_ref() {
                        let handle = handles.iter().find(|h| h.class.name == class.name).unwrap();
                        header.push_str(&format!(" * Requires p{i}: a live non-null {} handle from this library instance,\n * used on its creating thread, with the library kept loaded.\n", handle.c_name));
                        if *mutable {
                            header.push_str(" * Exclusive borrow: no other argument or alias may access this owner or\n * its fields during this call, including concurrent/reentrant access.\n * Ownership remains with the caller. The whole owner is not replaced or\n * destroyed, but fields may change even on Err; no rollback is promised.\n");
                        } else {
                            header.push_str(" * Shared borrow: no mutation or destruction through any alias during\n * the call; shared reads may alias. Ownership remains with the caller.\n");
                        }
                        continue;
                    }
                    header.push_str(&format!(" * Requires p{i}: non-null, aligned, initialized {} storage, valid\n * for this whole call (one element, sizeof({}) bytes).\n", c_type(inner, name), c_type(inner, name)));
                    if *mutable {
                        header.push_str(&format!(" * p{i} is borrowed exclusively: storage must be readable and writable,\n * must not overlap any other borrowed argument, and no other alias may\n * access it during the call (including concurrent or reentrant access).\n * On every normal return, the final value is written back to p{i}.\n"));
                    } else {
                        header.push_str(&format!(" * p{i} is borrowed read-only: storage must be readable and must not\n * be mutated through any alias during the call. Shared reads may alias.\n * This call leaves p{i}'s storage unchanged.\n"));
                    }
                }
            }
            if !export.documentation.is_empty() {
                header
                    .push_str(" * Author documentation (additional to the generated contract):\n");
                for line in export.documentation.lines() {
                    // Do not let author text close a comment or splice C source lines.
                    header.push_str(&format!(
                        " * {}\n",
                        line.replace("*/", "* /")
                            .replace('\\', "\\ ")
                            .replace('?', "? ")
                    ));
                }
            }
            header.push_str(" */\n");
            let output = export
                .signature
                .outputs
                .first()
                .filter(|_| result.is_none())
                .map(|ty| c_type(ty, name))
                .unwrap_or_else(|| "void".into());
            let mut parameters: Vec<_> = export
                .signature
                .inputs
                .iter()
                .enumerate()
                .map(|(i, (_, ty))| format!("{} p{i}", c_type(ty, name)))
                .collect();
            if let Some((ok, error)) = result {
                header.push_str("/* Returns 0 for Ok, 1 for Err; no other status is returned.\n * Output pointers must be non-null, aligned, writable, mutually disjoint,\n * and disjoint from all input borrows for the whole call.\n * Output storage need not be initialized. Only the selected output is written;\n * the other output is unchanged. No old output value is released.\n * Err does not promise to undo mutations to borrowed inputs.\n */\n");
                if *ok != Ty::Unit {
                    parameters.push(format!("{} *out_ok", c_type(ok, name)));
                }
                parameters.push(format!("{} *out_error", c_type(error, name)));
                if error_variants(error).is_some() {
                    header.push_str(&format!(
                        "/* On Err, *out_error is one of the {name}_{}_* codes. */\n",
                        error_source(error)
                    ));
                }
            }
            header.push_str(&format!(
                "{} {}({});\n",
                if result.is_some() {
                    "uint32_t"
                } else {
                    &output
                },
                export.symbol,
                if parameters.is_empty() {
                    "void".into()
                } else {
                    parameters.join(", ")
                }
            ));
            // Generated names avoid C keyword and Unicode parameter hazards.
            let parameters: Vec<_> = export
                .signature
                .inputs
                .iter()
                .enumerate()
                .map(|(i, (_, ty))| format!("p{i}: {}", handles::source_type(ty)))
                .collect();
            let mut output = export
                .signature
                .outputs
                .first()
                .map(ToString::to_string)
                .unwrap_or("()".into());
            if let Some((ok, error)) = result {
                let raw = format!("_plenty_c_{}", export.name);
                let (mut raw_parameters, mut args, transfers) = handles::arguments(export);
                let mut body = String::from("    _plenty_require_contract()\n");
                let owner = if let Ty::Class(class) = ok {
                    handles.iter().find(|h| h.class.name == class.name)
                } else {
                    None
                };
                if *ok != Ty::Unit {
                    let storage = owner
                        .map(|h| h.pointer.clone())
                        .unwrap_or_else(|| ok.to_string());
                    raw_parameters.push(format!("out_ok: &mut {storage}"));
                    args.push("&mut out_ok".into());
                    if let Some(handle) = owner {
                        output = format!("Result[{}, AllocError]", handle.name);
                        body.push_str(&format!("    mut owner = {}({storage}.null())\n    mut out_ok = {storage}.null()\n", handle.name));
                    } else {
                        body.push_str(&format!("    mut out_ok: {ok} = 0\n"));
                    }
                }
                if owner.is_none() {
                    output = format!("Result[{ok}, {}]", error_source(error));
                }
                let error_storage = if error_variants(error).is_some() {
                    "u32".into()
                } else {
                    error.to_string()
                };
                raw_parameters.push(format!("out_error: &mut {error_storage}"));
                args.push("&mut out_error".into());
                body.push_str(&transfers);
                body.push_str(&format!("    mut out_error: {error_storage} = 0\n    status = {raw}({})\n    if status == 0:\n", args.join(", ")));
                if owner.is_some() {
                    body.push_str("        owner._handle = out_ok\n        Ok(owner)\n");
                } else {
                    body.push_str(&format!(
                        "        Ok({})\n",
                        if *ok == Ty::Unit { "()" } else { "out_ok" }
                    ));
                }
                body.push_str("    else:\n");
                body.push_str(&rebuild_error(error, "        "));
                source.push_str(&format!(
                    "extern def {raw}({}) -> u32 = \"{}\"\npub def {}({}) -> {output}:\n{body}\n",
                    raw_parameters.join(", "),
                    export.symbol,
                    export.name,
                    parameters.join(", ")
                ));
            } else {
                let (raw_parameters, args, transfers) = handles::arguments(export);
                let raw = format!("_plenty_c_{}", export.name);
                source.push_str(&format!("extern def {raw}({}) -> {output} = \"{}\"\npub def {}({}) -> {output}:\n    _plenty_require_contract()\n{transfers}    {raw}({})\n\n", raw_parameters.join(", "), export.symbol, export.name, parameters.join(", "), args.join(", ")));
            }
        }
        let fingerprint = crate::library_metadata::fingerprint(source.as_bytes());
        let contract_guard = format!("{name}_plenty_contract_v1_{fingerprint}");
        source.push_str(&format!("# interface-sha256: {fingerprint}\nextern def _plenty_require_contract() -> () = \"{contract_guard}\"\n"));
        header.push_str(&format!("\n/* Interface SHA-256: {fingerprint}\n * Optional C compatibility guard: calling this no-op requires the exact\n * generated contract at link/symbol resolution. Does not allocate.\n * A fingerprint checks compatibility, not authenticity or implementation safety.\n */\nvoid {contract_guard}(void);\n"));
        header.push_str(&format!("\n/* Requires: length points to writable, aligned size_t storage (not null).\n * Writes the byte length; returns immutable UTF-8 metadata, without a NUL\n * terminator. Borrowed until library unload; never modify or free it.\n * The call does not allocate or retain length.\n */\nconst uint8_t *{discovery}(size_t *length);\n\n#ifdef __cplusplus\n}}\n#endif\n#endif /* {guard} */\n"));
        Ok(Self {
            name: name.into(),
            discovery,
            source,
            header,
            handles,
            contract_guard,
            errors,
        })
    }
}
