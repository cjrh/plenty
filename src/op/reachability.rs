//! Definition reachability shared by emission and checks for erased provider effects.
use super::{Op, Ty};
use crate::hash::{HashMap, HashSet};

/// With `executable` false, all user bodies and generators are roots. Unused
/// standard methods are still lowered for checking, but cannot create owners.
pub(crate) fn dead_definitions(
    ops: &[Op],
    exports: &[crate::exports::Export],
    executable: bool,
) -> HashSet<String> {
    let candidates: HashMap<&str, &crate::op::CompiledFn> = ops
        .iter()
        .filter_map(|op| match op {
            Op::DefineFn(name, f)
                if executable || f.origins.standard_library && f.generator.is_none() =>
            {
                Some((name.as_str(), f.as_ref()))
            }
            _ => None,
        })
        .collect();
    if candidates.is_empty() {
        return HashSet::default();
    }
    let mut pending = Vec::new();
    let mut live = HashSet::default();
    let mut require = |name: &str| {
        if let Some((&name, _)) = candidates.get_key_value(name)
            && live.insert(name)
        {
            pending.push(name);
        }
    };
    for export in exports {
        require(&export.function);
    }
    for op in ops.iter() {
        if matches!(op, Op::DefineFn(name, _) if candidates.contains_key(name.as_str())) {
            continue;
        }
        references(std::slice::from_ref(op), &mut require);
    }
    while let Some(name) = pending.pop() {
        let function = candidates[name];
        let mut require = |name: &str| {
            if let Some((&name, _)) = candidates.get_key_value(name)
                && live.insert(name)
            {
                pending.push(name);
            }
        };
        references(&function.body, &mut require);
        if executable {
            generator_references(function, &mut require);
        }
    }
    candidates
        .keys()
        .filter(|name| !live.contains(**name))
        .map(|name| (*name).to_owned())
        .collect()
}

/// Concrete frames can be passed or dropped without an explicit resume call.
/// Their constructor owns the resume callback and frame descriptors. Preserve
/// those identities through nested stored types of every live function.
#[expect(
    clippy::mutable_key_type,
    reason = "nominal identities exclude definition cells"
)]
fn generator_references(function: &crate::op::CompiledFn, require: &mut impl FnMut(&str)) {
    let mut pending: Vec<_> = function
        .sig
        .inputs
        .iter()
        .map(|(_, ty)| ty.clone())
        .chain(function.sig.outputs.iter().cloned())
        .chain(function.locals.iter().cloned())
        .collect();
    let mut seen = HashSet::default();
    while let Some(ty) = pending.pop() {
        if !seen.insert(ty.clone()) {
            continue;
        }
        if let Ty::Generator(generator) = &ty
            && generator.members.is_empty()
            && let Some(name) = &generator.name
        {
            require(name);
        }
        crate::threading::contained(&ty, &mut pending);
    }
}

fn references(ops: &[Op], require: &mut impl FnMut(&str)) {
    // Executable emission follows concrete generator identities separately;
    // inspectable artifacts retain all generator definitions as roots.
    crate::threading::walk(ops, &mut |op| {
        crate::threading::function_references(op, require);
    });
}
