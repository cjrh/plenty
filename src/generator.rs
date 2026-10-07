//! Concrete suspended-frame identities and finite inline layouts.
use crate::op::Ty;
use std::cell::OnceCell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

#[derive(Debug)]
pub struct GeneratorType {
    pub element: Ty,
    /// None is a source annotation awaiting a concrete producer.
    pub name: Option<String>,
    pub slots: OnceCell<Vec<Ty>>,
    pub bytes: OnceCell<usize>,
}
impl PartialEq for GeneratorType {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.element == other.element
    }
}
impl Eq for GeneratorType {}
impl Hash for GeneratorType {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.element.hash(state);
    }
}
impl GeneratorType {
    /// Reject cycles before installing strong slot-type links, so invalid
    /// recursive source cannot leave a reference-counted metadata cycle behind.
    pub fn set_slots(&self, slots: Vec<Ty>) -> Result<(), String> {
        let mut work = slots.iter().collect::<Vec<_>>();
        let mut seen = std::collections::HashSet::new();
        while let Some(ty) = work.pop() {
            match ty {
                Ty::Generator(t) => {
                    if t.name == self.name {
                        return Err(format!("recursive inline generator layout involving `{}`; nested frames must have a finite size", self.name.as_deref().unwrap()));
                    }
                    if seen.insert(t.name.as_deref()) {
                        if let Some(children) = t.slots.get() {
                            work.extend(children);
                        }
                    }
                }
                Ty::Enum(t) if t.inline() && t.restricted_storage => {
                    if seen.insert(Some(t.name.as_str())) {
                        work.extend(t.variants.iter().flat_map(|v| &v.fields));
                    }
                }
                Ty::Ref(t, _) => work.push(t),
                _ => {}
            }
        }
        self.slots
            .set(slots)
            .map_err(|_| "generator body lowered twice".into())
    }
}
pub fn ty(element: Ty, name: Option<String>) -> Ty {
    Ty::Generator(Rc::new(GeneratorType {
        element,
        name,
        slots: OnceCell::new(),
        bytes: OnceCell::new(),
    }))
}

/// Source annotations constrain the yielded type while allowing the producer
/// identity to be inferred. Concrete identities must agree at joins and moves.
pub fn refine(pattern: &Ty, actual: &Ty) -> Option<Ty> {
    if pattern == actual {
        return Some(actual.clone());
    }
    match (pattern, actual) {
        (Ty::Generator(a), Ty::Generator(b)) if a.element == b.element => {
            if a.name.is_none() {
                Some(actual.clone())
            } else if b.name.is_none() {
                Some(pattern.clone())
            } else {
                None
            }
        }
        (Ty::Ref(a, am), Ty::Ref(b, bm)) if am == bm => Some(Ty::Ref(Rc::new(refine(a, b)?), *am)),
        (Ty::Enum(a), Ty::Enum(b)) if a.is_option() && b.is_option() => Some(crate::sum::option(
            refine(&a.variants[1].fields[0], &b.variants[1].fields[0])?,
        )),
        (Ty::Enum(a), Ty::Enum(b))
            if a.propagatable() && b.propagatable() && !a.is_option() && !b.is_option() =>
        {
            Some(crate::sum::result(
                refine(&a.variants[0].fields[0], &b.variants[0].fields[0])?,
                refine(&a.variants[1].fields[0], &b.variants[1].fields[0])?,
            ))
        }
        _ => None,
    }
}

impl Ty {
    pub fn unresolved_generator(&self) -> bool {
        match self {
            Self::Generator(t) => t.name.is_none(),
            Self::Ref(t, _) => t.unresolved_generator(),
            Self::Enum(t) if t.inline() && t.restricted_storage => t
                .variants
                .iter()
                .flat_map(|v| &v.fields)
                .any(Ty::unresolved_generator),
            _ => false,
        }
    }
    pub fn has_inline_storage(&self) -> bool {
        match self {
            Self::Range(_) | Self::Generator(_) => true,
            Self::Enum(t) if t.inline() => t.inline_range || t.restricted_storage,
            _ => false,
        }
    }
    pub fn inline_bytes(&self) -> usize {
        match self {
            Self::Range(_) => 32,
            Self::Generator(t) => *t.bytes.get().expect("resolved generator layout"),
            Self::Enum(t) if t.inline() => *t.payload_bytes.get_or_init(|| {
                if !self.has_inline_storage() {
                    return 0;
                }
                t.variants
                    .iter()
                    .flat_map(|v| &v.fields)
                    .map(Ty::inline_bytes)
                    .max()
                    .unwrap_or(0)
            }),
            _ => 0,
        }
    }
}

pub fn layout(ty: &Ty, active: &mut Vec<String>) -> Result<usize, String> {
    match ty {
        Ty::Generator(t) => {
            if let Some(bytes) = t.bytes.get() {
                return Ok(*bytes);
            }
            let name = t
                .name
                .as_ref()
                .ok_or("cannot infer a concrete generator type")?;
            if active.contains(name) {
                return Err(format!("recursive inline generator layout involving `{name}`; nested frames must have a finite size"));
            }
            if active.len() >= 64 {
                return Err(
                    "inline generator nesting exceeds the implementation limit of 64".into(),
                );
            }
            active.push(name.clone());
            let mut bytes = 64usize;
            for slot in t.slots.get().ok_or("missing generator frame body")? {
                bytes = bytes
                    .checked_add(16 + layout(slot, active)?)
                    .filter(|n| *n <= i32::MAX as usize)
                    .ok_or("generator frame exceeds the native stack-layout limit")?;
            }
            active.pop();
            t.bytes.set(bytes).expect("layout computed once");
            Ok(bytes)
        }
        Ty::Enum(t) if t.inline() => {
            if let Some(bytes) = t.payload_bytes.get() {
                return Ok(*bytes);
            }
            let bytes = if ty.has_inline_storage() {
                t.variants
                    .iter()
                    .flat_map(|v| &v.fields)
                    .try_fold(0, |n, t| Ok::<_, String>(n.max(layout(t, active)?)))?
            } else {
                0
            };
            t.payload_bytes
                .set(bytes)
                .expect("sum layout computed once");
            Ok(bytes)
        }
        Ty::Range(_) => Ok(32),
        Ty::Ref(t, _) => {
            layout(t, active)?;
            Ok(0)
        }
        _ => Ok(0),
    }
}

/// Include types that occur only in temporary operands (such as an empty
/// Option[Generator[T]]) so unresolved annotations cannot reach native layout.
pub fn validate_ops(ops: &[crate::op::Op]) -> Result<(), String> {
    use crate::op::Op;
    for op in ops {
        let types = match op {
            Op::Collection(op) => {
                let (mut inputs, output) = op.signature();
                inputs.push(output);
                inputs
            }
            Op::Enum(op) => {
                let (mut inputs, output) = op.signature().ok_or("invalid enum operation")?;
                inputs.push(output);
                inputs
            }
            Op::Try {
                source,
                target,
                cleanup,
            } => {
                validate_ops(cleanup)?;
                vec![Ty::Enum(source.clone()), Ty::Enum(target.clone())]
            }
            Op::ReadRef(t) | Op::Reborrow(t) | Op::WriteRef(t) | Op::Yield(t) => vec![t.clone()],
            Op::Match(arms) => {
                for arm in arms.iter() {
                    validate_ops(&arm.body)?;
                }
                Vec::new()
            }
            Op::Loop { condition, body } => {
                validate_ops(condition)?;
                validate_ops(body)?;
                Vec::new()
            }
            _ => Vec::new(),
        };
        for ty in types {
            layout(&ty, &mut Vec::new())?;
        }
    }
    Ok(())
}
