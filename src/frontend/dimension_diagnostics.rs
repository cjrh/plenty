//! Source-scoped dimension aliases explain canonical types in diagnostics.
//! Names never enter type identity, specialization keys, or runtime formatting.
use super::*;
use std::fmt::Write;

pub(super) type Bindings = Arc<Vec<(String, String)>>;

pub(super) type Shadows = HashMap<(Option<FileId>, u32), HashSet<String>>;

#[derive(Default)]
pub(super) struct Names {
    /// Written spelling and resolved symbol, in declaration/import order.
    pub(super) bindings: HashMap<Option<FileId>, Bindings>,
    /// Generic parameters shadow module names at these source lines. Recorded
    /// during name resolution, before specialization substitutes those names.
    pub(super) shadows: Shadows,
}

impl TypeAliases {
    /// Keep the canonical diagnostic and explain each distinct dimension with
    /// the first matching alias actually visible at its source position. Walk
    /// the types themselves: arbitrary error text is never searched or rewritten.
    pub(super) fn dimension_error<'t>(
        &self,
        at: &Token,
        message: impl std::fmt::Display,
        types: impl IntoIterator<Item = &'t Ty>,
    ) -> Box<dyn Error> {
        let mut message = message.to_string();
        let Some(bindings) = self.dimension_names.bindings.get(&at.file) else {
            return at.error(message);
        };
        let mut pending: Vec<_> = types.into_iter().cloned().collect();
        pending.reverse();
        // Diagnostics have few distinct types. Do not hash Ty: some variants
        // carry compiler state with interior mutability.
        let mut seen = Vec::new();
        while let Some(ty) = pending.pop() {
            if seen.contains(&ty) {
                continue;
            }
            seen.push(ty.clone());
            if let Ty::Dimension(dimension) = &ty {
                let hidden = self.dimension_names.shadows.get(&(at.file, at.line));
                if let Some((name, _)) = bindings.iter().find(|(name, symbol)| {
                    !hidden.is_some_and(|hidden| hidden.contains(name))
                        && self.get(symbol).and_then(Option::as_ref) == Some(&ty)
                }) {
                    write!(
                        message,
                        "\n  note: dimension alias `{name}` = `{dimension}`"
                    )
                    .unwrap();
                }
                continue;
            }
            if let Some((_, arguments)) = self.data.arguments(&ty) {
                pending.extend(arguments.into_iter().rev());
                continue;
            }
            match &ty {
                Ty::Iter(t, _)
                | Ty::Channel(t, _)
                | Ty::Atomic(t)
                | Ty::Future(t)
                | Ty::List(t)
                | Ty::Clist(t)
                | Ty::Set(t)
                | Ty::KeyView(t)
                | Ty::DictView(t, _)
                | Ty::View(t, _)
                | Ty::Box(t)
                | Ty::Shared(t)
                | Ty::Range(t)
                | Ty::Ref(t, _) => pending.push((**t).clone()),
                Ty::Dict(k, v) => pending.extend([(**v).clone(), (**k).clone()]),
                Ty::Callable(sig) | Ty::BoxedCallable(sig) => {
                    pending.extend(sig.output.iter().cloned());
                    pending.extend(sig.inputs.iter().rev().cloned());
                }
                Ty::BoxedProtocol(p) => pending.extend(p.types().cloned()),
                Ty::Closure(c) => {
                    pending.extend(c.signature.output.iter().cloned());
                    pending.extend(c.signature.inputs.iter().rev().cloned());
                }
                Ty::Generator(g) => pending.push(g.element.clone()),
                Ty::Enum(e) if e.tuple() || e.anonymous() || e.is_option() || e.is_result() => {
                    pending.extend(
                        e.get()
                            .variants
                            .iter()
                            .rev()
                            .flat_map(|v| v.fields.iter().rev().cloned()),
                    );
                }
                _ => {}
            }
        }
        at.error(message)
    }
}
