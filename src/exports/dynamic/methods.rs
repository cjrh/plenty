//! C output parameters become ordinary typed method results.
use crate::exports::{error_variants, result_payloads, Export};
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

pub(super) fn generate(export: &Export, index: usize) -> Method {
    let parameters: Vec<_> = export
        .signature
        .inputs
        .iter()
        .enumerate()
        .map(|(n, (_, ty))| format!("p{n}: {ty}"))
        .collect();
    let mut raw_parameters = parameters.clone();
    let mut arguments: Vec<_> = (0..parameters.len()).map(|n| format!("p{n}")).collect();
    let output = export
        .signature
        .outputs
        .first()
        .map(ToString::to_string)
        .unwrap_or("()".into());
    let mut body = String::new();
    let mut raw_output = output.clone();
    if let Some((ok, error)) = export.signature.outputs.first().and_then(result_payloads) {
        raw_output = "u32".into();
        if *ok != Ty::Unit {
            raw_parameters.push(format!("out_ok: &mut {ok}"));
            arguments.push("&mut out_ok".into());
            body.push_str(&format!("        mut out_ok: {ok} = 0\n"));
        }
        let error_storage = if error_variants(error).is_some() {
            "u32".into()
        } else {
            error.to_string()
        };
        raw_parameters.push(format!("out_error: &mut {error_storage}"));
        arguments.push("&mut out_error".into());
        body.push_str(&format!("        mut out_error: {error_storage} = 0\n        status = _call{index}(self._code{index}{})\n        if status == 0:\n            Ok({})\n        else:\n", tail(&arguments), if *ok == Ty::Unit { "()" } else { "out_ok" }));
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
