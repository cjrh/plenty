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
                Ty::Enum(t) if t.try_get().is_some_and(|t| t.restricted_storage) => {
                    if seen.insert(Some(t.name.as_str())) {
                        work.extend(t.local().variants.iter().flat_map(|v| &v.fields));
                    }
                }
                Ty::Ref(t, _) => work.push(t),
                Ty::Closure(t) if seen.insert(Some(t.name.as_str())) => {
                    work.extend(t.captures.iter().map(|(_, ty)| ty));
                }
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
        (Ty::Closure(a), Ty::Closure(b)) if a.signature == b.signature && a.once == b.once => {
            if a.name.is_empty() {
                Some(actual.clone())
            } else if b.name.is_empty() {
                Some(pattern.clone())
            } else {
                None
            }
        }
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
        (Ty::Enum(a), Ty::Enum(b)) if a.is_option() && b.is_option() => {
            Some(crate::sum::option(refine(
                &a.get().variants[1].fields[0],
                &b.get().variants[1].fields[0],
            )?))
        }
        (Ty::Enum(a), Ty::Enum(b))
            if a.name.starts_with("SpawnError[") && b.name.starts_with("SpawnError[") =>
        {
            Some(crate::sum::spawn_error(refine(
                &a.get().variants[0].fields[0],
                &b.get().variants[0].fields[0],
            )?))
        }
        (Ty::Enum(a), Ty::Enum(b))
            if a.name.starts_with("SendError[") && b.name.starts_with("SendError[") =>
        {
            Some(crate::sum::send_error(refine(
                &a.get().variants[0].fields[0],
                &b.get().variants[0].fields[0],
            )?))
        }
        (Ty::Enum(a), Ty::Enum(b))
            if a.name.starts_with("SubmitError[") && b.name.starts_with("SubmitError[") =>
        {
            Some(crate::sum::submit_error(refine(
                &a.get().variants[0].fields[0],
                &b.get().variants[0].fields[0],
            )?))
        }
        (Ty::Enum(a), Ty::Enum(b))
            if a.name.starts_with("SendTimeoutError[")
                && b.name.starts_with("SendTimeoutError[") =>
        {
            Some(crate::sum::send_timeout_error(refine(
                &a.get().variants[0].fields[0],
                &b.get().variants[0].fields[0],
            )?))
        }
        (Ty::Enum(a), Ty::Enum(b))
            if a.name.starts_with("Selected[") && b.name.starts_with("Selected[") =>
        {
            Some(crate::sum::selected(
                refine(
                    &a.get().variants[0].fields[0],
                    &b.get().variants[0].fields[0],
                )?,
                refine(
                    &a.get().variants[1].fields[0],
                    &b.get().variants[1].fields[0],
                )?,
            ))
        }
        (Ty::Enum(a), Ty::Enum(b))
            if a.name.starts_with("ParallelError[") && b.name.starts_with("ParallelError[") =>
        {
            Some(crate::sum::parallel_error(refine(
                &a.get().variants[2].fields[0],
                &b.get().variants[2].fields[0],
            )?))
        }
        (Ty::Enum(a), Ty::Enum(b))
            if a.propagatable() && b.propagatable() && !a.is_option() && !b.is_option() =>
        {
            Some(crate::sum::result(
                refine(
                    &a.get().variants[0].fields[0],
                    &b.get().variants[0].fields[0],
                )?,
                refine(
                    &a.get().variants[1].fields[0],
                    &b.get().variants[1].fields[0],
                )?,
            ))
        }
        _ => None,
    }
}

impl Ty {
    pub fn unresolved_generator(&self) -> bool {
        match self {
            Self::Closure(t) => t.name.is_empty(),
            Self::Generator(t) => t.name.is_none(),
            Self::Ref(t, _) => t.unresolved_generator(),
            Self::Enum(t) if t.try_get().is_some_and(|t| t.restricted_storage) => t
                .get()
                .variants
                .iter()
                .flat_map(|v| &v.fields)
                .any(Ty::unresolved_generator),
            _ => false,
        }
    }
    /// Whether a value keeps owner-local bytes after its 16-byte slot word.
    pub fn has_inline_storage(&self) -> bool {
        match self {
            Self::Range(_) | Self::Generator(_) | Self::Closure(_) => true,
            Self::Class(t) => t.get().state_bytes() != 0 || !t.get().fields.is_empty(),
            Self::Enum(t) => *t.get().storage.get_or_init(|| {
                t.get().variants.iter().any(|v| match v.fields.as_slice() {
                    [] => false,
                    [field] => field.has_inline_storage(),
                    _ => true,
                })
            }),
            _ => false,
        }
    }
    /// A class, or an enum with one multi-field variant, whose fields keep no
    /// inline data: copying its storage needs no relocation.
    pub fn flat_record(&self) -> bool {
        match self {
            Self::Class(t) => t.get().fields.iter().all(|(_, t)| !t.has_inline_storage()),
            Self::Enum(t) => match t.get().variants.as_slice() {
                [variant] if variant.fields.len() > 1 => {
                    variant.fields.iter().all(|t| !t.has_inline_storage())
                }
                _ => false,
            },
            _ => false,
        }
    }
    pub fn inline_bytes(&self) -> usize {
        match self {
            Self::Closure(t) => t.bytes(),
            Self::Range(_) => 32,
            Self::Generator(t) => *t.bytes.get().expect("resolved generator layout"),
            Self::Enum(t) => *t.get().payload_bytes.get_or_init(|| {
                t.get()
                    .variants
                    .iter()
                    .map(|v| variant_bytes(&v.fields, |t| Ok::<_, ()>(t.inline_bytes())).unwrap())
                    .max()
                    .unwrap_or(0)
            }),
            Self::Class(t) => *t.get().bytes.get_or_init(|| {
                t.get().state_bytes()
                    + t.get()
                        .fields
                        .iter()
                        .map(|(_, t)| t.slot_bytes())
                        .sum::<usize>()
            }),
            _ => 0,
        }
    }
}

/// A single field is the variant's payload; several fields form a record of
/// typed slots in the owner's storage.
fn variant_bytes<E>(
    fields: &[Ty],
    mut inline: impl FnMut(&Ty) -> Result<usize, E>,
) -> Result<usize, E> {
    match fields {
        [] => Ok(0),
        [field] => inline(field),
        _ => fields.iter().try_fold(0, |n, t| Ok(n + 16 + inline(t)?)),
    }
}

/// Inline values are copied on every move; keep them small enough for the
/// native stack. Larger data belongs in a `Box` or a collection.
pub const INLINE_LIMIT: usize = 64 * 1024;

pub fn layout(ty: &Ty, active: &mut Vec<String>) -> Result<usize, String> {
    match ty {
        Ty::Closure(t) => {
            if t.name.is_empty() {
                return Err("cannot infer a concrete closure environment".into());
            }
            // Environments without captured frames already have cached sizes.
            // Resolve frame-dependent environments once, after body lowering.
            t.layout(active)
        }
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
        Ty::Enum(t) => {
            if let Some(bytes) = t.get().payload_bytes.get() {
                return Ok(*bytes);
            }
            nested(&t.name, active, |active| {
                t.get().variants.iter().try_fold(0, |n, v| {
                    Ok(n.max(variant_bytes(&v.fields, |t| layout(t, active))?))
                })
            })
            .map(|bytes| *t.get().payload_bytes.get_or_init(|| bytes))
        }
        Ty::Class(t) => {
            if let Some(bytes) = t.get().bytes.get() {
                return Ok(*bytes);
            }
            nested(&t.name, active, |active| {
                t.get()
                    .fields
                    .iter()
                    .try_fold(t.get().state_bytes(), |n, (_, t)| {
                        Ok(n + 16 + layout(t, active)?)
                    })
            })
            .map(|bytes| *t.get().bytes.get_or_init(|| bytes))
        }
        // A box's content is laid out where the content type itself occurs,
        // so a recursive type does not measure itself through its boxes.
        Ty::Box(_) => Ok(0),
        Ty::Range(_) => Ok(32),
        Ty::Ref(t, _) => {
            layout(t, active)?;
            Ok(0)
        }
        _ => Ok(0),
    }
}

/// Lay out a nominal type's storage, rejecting a value that contains itself.
fn nested(
    name: &str,
    active: &mut Vec<String>,
    measure: impl FnOnce(&mut Vec<String>) -> Result<usize, String>,
) -> Result<usize, String> {
    if active.iter().any(|n| n == name) {
        return Err(format!(
            "`{name}` contains itself and would have infinite size; store the recursive field in a `Box`, list, set, or dict"
        ));
    }
    active.push(name.into());
    let bytes = measure(active);
    active.pop();
    let bytes = bytes?;
    if bytes > INLINE_LIMIT {
        return Err(format!(
            "`{name}` needs {bytes} bytes of inline storage, more than the limit of {INLINE_LIMIT}; store large fields in a `Box`"
        ));
    }
    Ok(bytes)
}

/// Include types that occur only in temporary operands (such as an empty
/// Option[Generator[T]]) so unresolved annotations cannot reach native layout.
pub fn validate_ops(ops: &[crate::op::Op]) -> Result<(), String> {
    use crate::op::Op;
    for op in ops {
        let types = match op {
            Op::ClosureNew(t) | Op::ClosureCall(t) => vec![Ty::Closure(t.clone())],
            Op::Collection(op) => {
                let (mut inputs, output) = op.signature();
                inputs.push(output);
                inputs
            }
            Op::Channel(op) => {
                let (mut inputs, output) = op.signature();
                inputs.push(output);
                inputs
            }
            Op::Executor(op) => {
                let (mut inputs, output) = op.signature();
                inputs.push(output);
                inputs
            }
            Op::Control(op) => {
                let (mut inputs, output) = op.signature();
                inputs.push(output);
                inputs
            }
            Op::Enum(op) => {
                let (mut inputs, output) = op.signature().ok_or("invalid enum operation")?;
                inputs.push(output);
                inputs
            }
            Op::Box(op) => {
                let (mut inputs, output) = op.signature();
                inputs.push(output);
                inputs
            }
            Op::Split(t, tag) => {
                let mut types = vec![Ty::Enum(t.clone())];
                types.extend(t.get().variants[*tag].fields.iter().cloned());
                types
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
