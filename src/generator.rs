//! Concrete suspended-frame identities and finite inline layouts.
use crate::op::Ty;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::OnceLock;

#[derive(Debug)]
pub struct GeneratorType {
    pub element: Ty,
    /// None is a source annotation awaiting a concrete producer.
    pub name: Option<String>,
    pub slots: OnceLock<Vec<Ty>>,
    pub layout: OnceLock<Measured>,
}

/// Frames and environments nested more deeply than this are rejected.
pub const NESTING_LIMIT: usize = 64;

/// A finished inline layout measurement. `reach` counts the nominal layouts
/// from this one, inclusive, down to the deepest frame or environment the
/// nesting limit applies to, or is zero when the limit applies to none.
///
/// Frames are validated on several threads that share these caches, so a
/// measurement may meet a type another thread already measured. A cached
/// layout is used only where measuring it again could not reach the limit;
/// otherwise the type is measured again and reports the same error a serial,
/// uncached measurement would (issue #126).
#[derive(Clone, Copy, Debug)]
pub struct Measured {
    pub bytes: usize,
    pub reach: usize,
}

impl Measured {
    /// This cached layout, if it does not hide a nesting error at `depth`.
    pub fn at_depth(&self, depth: usize) -> Option<Self> {
        (depth + self.reach <= NESTING_LIMIT).then_some(*self)
    }
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
        let mut seen = crate::hash::HashSet::default();
        while let Some(ty) = work.pop() {
            match ty {
                Ty::Generator(t) => {
                    if t.name == self.name {
                        return Err(format!(
                            "recursive inline generator layout involving `{}`; nested frames must have a finite size",
                            self.name.as_deref().unwrap()
                        ));
                    }
                    if seen.insert(t.name.as_deref())
                        && let Some(children) = t.slots.get()
                    {
                        work.extend(children);
                    }
                }
                Ty::Enum(t) if t.try_get().is_some_and(|t| t.restricted_storage) => {
                    if seen.insert(Some(&t.name)) {
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
    Ty::Generator(Arc::new(GeneratorType {
        element,
        name,
        slots: OnceLock::new(),
        layout: OnceLock::new(),
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
        // A `&mut Iter[T]` parameter takes the lent plan's identity.
        (Ty::Iter(a, usize::MAX), Ty::Iter(b, _)) if a == b => Some(actual.clone()),
        (Ty::Ref(a, am), Ty::Ref(b, bm)) if am == bm => Some(Ty::Ref(Arc::new(refine(a, b)?), *am)),
        // Member order follows identity names, which change as producers
        // resolve; pair members by refinement instead of position.
        (Ty::Enum(a), Ty::Enum(b)) if a.anonymous() && b.anonymous() => {
            let mut left = crate::sum::members(pattern);
            let right = crate::sum::members(actual);
            if left.len() != right.len() {
                return None;
            }
            let mut refined = Vec::with_capacity(right.len());
            for member in &right {
                let (slot, ty) = left
                    .iter()
                    .enumerate()
                    .find_map(|(slot, t)| refine(t, member).map(|ty| (slot, ty)))?;
                left.swap_remove(slot);
                refined.push(ty);
            }
            Some(crate::sum::anonymous(refined))
        }
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
        self.unresolved_producer().is_some()
    }
    /// The first closure or generator whose concrete producer is still
    /// unknown, so diagnostics can name the kind of value that lacks identity.
    pub fn unresolved_producer(&self) -> Option<Ty> {
        match self {
            Self::Closure(t) if t.name.is_empty() => Some(self.clone()),
            Self::Generator(t) if t.name.is_none() => Some(self.clone()),
            Self::Iter(_, usize::MAX) => Some(self.clone()),
            Self::Ref(t, _) => t.unresolved_producer(),
            Self::Enum(t) if t.try_get().is_some_and(|t| t.restricted_storage) => t
                .get()
                .variants
                .iter()
                .flat_map(|v| &v.fields)
                .find_map(Ty::unresolved_producer),
            _ => None,
        }
    }
    /// Whether a value keeps owner-local bytes after its 16-byte slot word.
    pub fn has_inline_storage(&self) -> bool {
        match self {
            Self::Range(_) | Self::Generator(_) | Self::Closure(_) => true,
            Self::Class(t) => match t.get().newtype_field() {
                Some(field) => field.has_inline_storage(),
                None => t.get().state_bytes() != 0 || !t.get().fields.is_empty(),
            },
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
            Self::Class(t) => match t.get().newtype_field() {
                Some(field) => field.flat_record(),
                // A flat class field adds no address of its own.
                None => t
                    .get()
                    .fields
                    .iter()
                    .all(|(_, t)| !t.has_inline_storage() || t.flat_field() && t.flat_record()),
            },
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
            Self::Generator(t) => t.layout.get().expect("resolved generator layout").bytes,
            Self::Enum(t) => *t.get().payload_bytes.get_or_init(|| {
                t.get()
                    .variants
                    .iter()
                    .map(|v| variant_bytes(&v.fields, |t| Ok::<_, ()>(t.inline_bytes())).unwrap())
                    .max()
                    .unwrap_or(0)
            }),
            Self::Class(t) => *t.get().bytes.get_or_init(|| match t.get().newtype_field() {
                Some(field) => field.inline_bytes(),
                None => {
                    t.get().state_bytes()
                        + t.get()
                            .fields
                            .iter()
                            .map(|(_, t)| t.field_bytes())
                            .sum::<usize>()
                }
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
    measure(ty, active).map(|m| m.bytes)
}

/// The layout of `ty` nested inside the nominal types in `active`.
pub fn measure(ty: &Ty, active: &mut Vec<String>) -> Result<Measured, String> {
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
            if let Some(cached) = t.layout.get().and_then(|m| m.at_depth(active.len())) {
                return Ok(cached);
            }
            let name = t
                .name
                .as_ref()
                .ok_or("cannot infer a concrete generator type")?;
            if active.contains(name) {
                return Err(format!(
                    "recursive inline generator layout involving `{name}`; nested frames must have a finite size"
                ));
            }
            if active.len() >= NESTING_LIMIT {
                return Err(
                    "inline generator nesting exceeds the implementation limit of 64".into(),
                );
            }
            active.push(name.clone());
            let mut bytes = 64usize;
            let mut reach = 0;
            for slot in t.slots.get().ok_or("missing generator frame body")? {
                let slot = measure(slot, active)?;
                reach = reach.max(slot.reach);
                bytes = bytes
                    .checked_add(16 + slot.bytes)
                    .filter(|n| *n <= i32::MAX as usize)
                    .ok_or("generator frame exceeds the native stack-layout limit")?;
            }
            active.pop();
            // Threads measuring the same generator at once agree.
            Ok(*t.layout.get_or_init(|| Measured {
                bytes,
                reach: reach + 1,
            }))
        }
        // Only an enum with restricted storage can hold a frame or an
        // environment, so only its cached size could hide a nesting error.
        // It is measured again; its contents' layouts are cached.
        Ty::Enum(t) => {
            if !t.get().restricted_storage
                && let Some(bytes) = t.get().payload_bytes.get()
            {
                return Ok(Measured {
                    bytes: *bytes,
                    reach: 0,
                });
            }
            nested(&t.name, active, |active| {
                t.get()
                    .variants
                    .iter()
                    .try_fold(Measured { bytes: 0, reach: 0 }, |n, v| {
                        let mut reach = n.reach;
                        let bytes = variant_bytes(&v.fields, |t| {
                            let field = measure(t, active)?;
                            reach = reach.max(field.reach);
                            Ok::<_, String>(field.bytes)
                        })?;
                        Ok(Measured {
                            bytes: n.bytes.max(bytes),
                            reach,
                        })
                    })
            })
            .map(|m| Measured {
                bytes: *t.get().payload_bytes.get_or_init(|| m.bytes),
                reach: m.reach,
            })
        }
        // Class fields cannot hold a frame or a frame-dependent environment.
        Ty::Class(t) => {
            if let Some(bytes) = t.get().bytes.get() {
                return Ok(Measured {
                    bytes: *bytes,
                    reach: 0,
                });
            }
            nested(&t.name, active, |active| {
                if let Some(field) = t.get().newtype_field() {
                    return measure(field, active);
                }
                t.get().fields.iter().try_fold(
                    Measured {
                        bytes: t.get().state_bytes(),
                        reach: 0,
                    },
                    |n, (_, t)| {
                        let field = measure(t, active)?;
                        let word = if t.flat_field() { 0 } else { 16 };
                        Ok(Measured {
                            bytes: n.bytes + word + field.bytes,
                            reach: n.reach.max(field.reach),
                        })
                    },
                )
            })
            .map(|m| Measured {
                bytes: *t.get().bytes.get_or_init(|| m.bytes),
                reach: m.reach,
            })
        }
        // A box's content is laid out where the content type itself occurs,
        // so a recursive type does not measure itself through its boxes.
        Ty::Box(_) | Ty::Shared(_) => Ok(Measured { bytes: 0, reach: 0 }),
        Ty::Range(_) => Ok(Measured {
            bytes: 32,
            reach: 0,
        }),
        Ty::Ref(t, _) => Ok(Measured {
            bytes: 0,
            reach: measure(t, active)?.reach,
        }),
        _ => Ok(Measured { bytes: 0, reach: 0 }),
    }
}

/// Lay out a nominal type's storage, rejecting a value that contains itself.
/// The type itself is one level of nesting above its contents.
fn nested(
    name: &str,
    active: &mut Vec<String>,
    measure: impl FnOnce(&mut Vec<String>) -> Result<Measured, String>,
) -> Result<Measured, String> {
    if active.iter().any(|n| n == name) {
        return Err(format!(
            "`{name}` contains itself and would have infinite size; store the recursive field in a `Box`, list, set, or dict"
        ));
    }
    active.push(name.into());
    let measured = measure(active);
    active.pop();
    let Measured { bytes, reach } = measured?;
    if bytes > INLINE_LIMIT {
        return Err(format!(
            "`{name}` needs {bytes} bytes of inline storage, more than the limit of {INLINE_LIMIT}; store large fields in a `Box`"
        ));
    }
    Ok(Measured {
        bytes,
        reach: if reach == 0 { 0 } else { reach + 1 },
    })
}

/// Include types that occur only in temporary operands (such as an empty
/// Option[Generator[T]]) so unresolved annotations cannot reach native layout.
pub fn validate_ops(ops: &[crate::op::Op]) -> Result<(), String> {
    use crate::op::Op;
    let flatten = |(mut inputs, output): (Vec<Ty>, Ty)| {
        inputs.push(output);
        inputs
    };
    for op in ops {
        let types = match op {
            Op::ClosureNew(t) | Op::ClosureCall(t) => vec![Ty::Closure(t.clone())],
            Op::Collection(op) => flatten(op.signature()),
            Op::Channel(op) => flatten(op.signature()),
            Op::Executor(op) => flatten(op.signature()),
            Op::Control(op) => flatten(op.signature()),
            Op::Atomic(op) => flatten(op.signature()),
            Op::Mutex(op) => flatten(op.signature()),
            Op::Enum(op) => flatten(op.signature().ok_or("invalid enum operation")?),
            Op::Box(op) => flatten(op.signature()),
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
                vec![Ty::Enum((**source).clone()), Ty::Enum((**target).clone())]
            }
            Op::ReadRef(t)
            | Op::Reborrow(t)
            | Op::WriteRef(t)
            | Op::FillRef(t)
            | Op::ReplaceRef(t)
            | Op::Yield(t) => vec![t.clone()],
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
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                validate_ops(then)?;
                for arm in elifs.iter() {
                    validate_ops(&arm.condition)?;
                    validate_ops(&arm.body)?;
                }
                validate_ops(otherwise)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Generators `g0` to `g70`, each holding two of the next, so a frame
    /// doubles in size at each level.
    fn chain() -> Vec<Ty> {
        let mut frames = vec![ty(Ty::I64, Some("g70".into()))];
        let Ty::Generator(last) = &frames[0] else {
            unreachable!()
        };
        last.set_slots(Vec::new()).unwrap();
        for i in (0..70).rev() {
            let next = frames.last().unwrap().clone();
            let frame = ty(Ty::I64, Some(format!("g{i}")));
            let Ty::Generator(t) = &frame else {
                unreachable!()
            };
            t.set_slots(vec![next.clone(), next]).unwrap();
            frames.push(frame);
        }
        frames.reverse();
        frames
    }

    #[test]
    fn cached_layouts_do_not_change_the_error() {
        let fresh = layout(&chain()[0], &mut Vec::new()).unwrap_err();
        assert!(fresh.contains("nesting exceeds"), "{fresh}");
        // Another thread may already have measured the deeper frames, which
        // fit by themselves.
        let frames = chain();
        let measured = frames
            .iter()
            .rev()
            .take_while(|frame| layout(frame, &mut Vec::new()).is_ok())
            .count();
        assert!((10..60).contains(&measured), "{measured}");
        assert_eq!(layout(&frames[0], &mut Vec::new()).unwrap_err(), fresh);
    }
}
