//! C output parameters become ordinary typed method results.
use crate::exports::{error_variants, handles, result_payloads, Export, Interface};
use crate::op::Ty;

pub(super) struct Method {
    pub declaration: String,
    pub definition: String,
}

fn tail(items: &[String]) -> String {
    if items.is_empty() {
        String::new()
    } else {
        format!(", {}", items.join(", "))
    }
}

pub(super) fn generate(export: &Export, index: usize, interface: &Interface) -> Method {
    let parameters: Vec<_> = export
        .signature
        .inputs
        .iter()
        .enumerate()
        .map(|(n, (_, ty))| format!("p{n}: {}", handles::source_type(ty)))
        .collect();
    let (mut raw_parameters, mut arguments, transfers) = handles::arguments(export);
    let transfers: String = transfers
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect();
    let mut output = export
        .signature
        .outputs
        .first()
        .map(ToString::to_string)
        .unwrap_or("()".into());
    let mut body = String::new();
    for (i, (_, ty)) in export.signature.inputs.iter().enumerate() {
        let ty = if let Ty::Ref(inner, _) = ty {
            inner.as_ref()
        } else {
            ty
        };
        if matches!(ty, Ty::Class(_)) {
            body.push_str(&format!(
                "        _require_origin(self._origin, p{i}._origin)\n"
            ));
        }
    }
    let mut raw_output = output.clone();
    if let Some((ok, error)) = export.signature.outputs.first().and_then(result_payloads) {
        raw_output = "u32".into();
        let owner = if let Ty::Class(class) = ok {
            interface
                .handles
                .iter()
                .find(|h| h.class.name == class.name)
        } else {
            None
        };
        if *ok != Ty::Unit {
            let storage = owner
                .map(|h| h.pointer.clone())
                .unwrap_or_else(|| ok.to_string());
            raw_parameters.push(format!("out_ok: &mut {storage}"));
            arguments.push("&mut out_ok".into());
            if let Some(handle) = owner {
                let name = &handle.name;
                output = format!("Result[{name}, AllocError]");
                body.push_str(&format!("        mut owner = {name}(self._origin, self._drop_{name}, {storage}.null())?\n        mut out_ok = {storage}.null()\n"));
            } else {
                body.push_str(&format!("        mut out_ok: {ok} = 0\n"));
            }
        }
        let error_storage = if error_variants(error).is_some() {
            "u32".into()
        } else {
            error.to_string()
        };
        raw_parameters.push(format!("out_error: &mut {error_storage}"));
        arguments.push("&mut out_error".into());
        body.push_str(&transfers);
        body.push_str(&format!("        mut out_error: {error_storage} = 0\n        status = _call{index}(self._code{index}{})\n        if status == 0:\n", tail(&arguments)));
        if owner.is_some() {
            body.push_str("            owner._handle = out_ok\n            Ok(owner)\n");
        } else {
            body.push_str(&format!(
                "            Ok({})\n",
                if *ok == Ty::Unit { "()" } else { "out_ok" }
            ));
        }
        body.push_str("        else:\n");
        if let Some(variants) = error_variants(error) {
            for (code, variant) in variants.iter().enumerate().take(variants.len() - 1) {
                body.push_str(&format!(
                    "            if out_error == {code}:\n                return Err({error}.{})\n",
                    variant.name
                ));
            }
            body.push_str(&format!(
                "            Err({error}.{})\n",
                variants.last().unwrap().name
            ));
        } else {
            body.push_str("            Err(out_error)\n");
        }
    } else {
        body.push_str(&transfers);
        body.push_str(&format!(
            "        _call{index}(self._code{index}{})\n",
            tail(&arguments)
        ));
    }
    Method {
        declaration: format!(
            "extern def _call{index}(address: _FunctionAddress{}) -> {raw_output} = address\n",
            tail(&raw_parameters)
        ),
        definition: format!(
            "    pub def {}(self{}) -> {output}:\n{body}",
            export.name,
            tail(&parameters)
        ),
    }
}
