//! Immutable native type metadata. No runtime parsing or metadata allocation.
use super::*;

fn blob(module: &mut ObjectModule, bytes: Vec<u8>, links: &[(usize, DataId)]) -> Result<DataId> {
    let id = module.declare_anonymous_data(false, false)?;
    define(module, id, bytes, links, None)?;
    Ok(id)
}
/// `hook` is a class's `__del__` adapter, stored at the descriptor's byte 56.
fn define(
    module: &mut ObjectModule,
    id: DataId,
    bytes: Vec<u8>,
    links: &[(usize, DataId)],
    hook: Option<FuncId>,
) -> Result<()> {
    let mut data = DataDescription::new();
    data.set_align(8);
    data.define(bytes.into_boxed_slice());
    for &(offset, target) in links {
        let reference = module.declare_data_in_data(target, &mut data);
        data.write_data_addr(offset as u32, reference, 0);
    }
    if let Some(hook) = hook {
        let reference = module.declare_func_in_data(hook, &mut data);
        data.write_function_addr(56, reference);
    }
    module.define_data(id, &data)?;
    Ok(())
}
/// The class record facts at descriptor byte 72, mirroring
/// `plenty_runtime::aggregates::Plan`: the strings' word offsets, whether
/// they are all the record owns, and whether its bytes move without repair.
fn plan(module: &mut ObjectModule, ty: &Ty, class: &crate::record::ClassType) -> Result<DataId> {
    let mut strings = Vec::new();
    let only_strings = string_words(class, 0, &mut strings);
    let mut offsets: Vec<u8> = strings.iter().flat_map(|w| w.to_ne_bytes()).collect();
    let len = strings.len();
    // An empty slice still needs a non-null, aligned address.
    offsets.resize(offsets.len().max(4), 0);
    let mut bytes = vec![0; 24];
    word(&mut bytes, 8, len);
    bytes[16] = u8::from(only_strings);
    bytes[17] = u8::from(ty.flat_record());
    let offsets = blob(module, offsets, &[])?;
    blob(module, bytes, &[(0, offsets)])
}
/// Collect the word offsets of the strings in `class`'s record, starting at
/// word `base`, through flat class fields. Returns whether they are all it
/// owns: no `__del__` at any depth and no other field needing cleanup.
fn string_words(class: &crate::record::ClassType, base: usize, out: &mut Vec<u32>) -> bool {
    if class.destructor.is_some() {
        return false;
    }
    let mut only = true;
    for (index, (_, field)) in class.fields.iter().enumerate() {
        let at = base + class.field_offset(index) / 16;
        match &*field.newtype_base() {
            Ty::Str => out.push(at as u32),
            Ty::Class(nested) if field.flat_field() => {
                only &= string_words(&nested.get(), at, out);
            }
            _ => only &= !field.managed(),
        }
    }
    only
}
fn word(bytes: &mut [u8], offset: usize, value: usize) {
    bytes[offset..offset + 8].copy_from_slice(&(value as u64).to_ne_bytes());
}
pub(super) fn declare(module: &mut ObjectModule, runtime: &Runtime, ty: &Ty) -> Result<DataId> {
    let mut pending = Vec::new();
    let root = reserve(module, runtime, ty, &mut pending)?;
    while let Some(ty) = pending.pop() {
        define_type(module, runtime, &ty, &mut pending)?;
    }
    Ok(root)
}

fn reserve(
    module: &mut ObjectModule,
    runtime: &Runtime,
    ty: &Ty,
    pending: &mut Vec<Ty>,
) -> Result<DataId> {
    if let Some(id) = runtime.type_data.lock().unwrap().get(ty) {
        return Ok(*id);
    }
    let id = module.declare_anonymous_data(false, false)?;
    runtime.type_data.lock().unwrap().insert(ty.clone(), id);
    pending.push(ty.clone());
    Ok(id)
}

fn define_type(
    module: &mut ObjectModule,
    runtime: &Runtime,
    ty: &Ty,
    pending: &mut Vec<Ty>,
) -> Result<()> {
    let id = runtime.type_data.lock().unwrap()[ty];
    // Mirrors plenty_runtime::aggregates::{Type, Variant} on the 64-bit host.
    let mut bytes = vec![0; 80];
    // A newtype class has its field's values, so the runtime gets the
    // field's layout under the class's name; the wrapper link at byte 64
    // names the field for formatting and the columnar-list schema.
    // `NonZero[T]` is described, and so formatted, as plain `T`.
    let integer;
    let ty = match ty.nonzero_element() {
        Some(element) => {
            integer = element;
            &integer
        }
        None => ty,
    };
    let newtype = match ty {
        Ty::Class(t) => t.get().newtype_field().map(|field| (t, field.clone())),
        _ => None,
    };
    let base;
    let (display_name, ty) = match &newtype {
        Some((class, _)) => {
            base = ty.newtype_base().into_owned();
            (Some(class.display_name()), &base)
        }
        None => (None, ty),
    };
    // Atomics have their element's layout but stay affine, so the runtime
    // never copies one. The frontend keeps them out of every concurrent or
    // copying runtime operation; unborrowed atomics still move through pop,
    // append, and insert.
    let affine = ty.affine();
    let ty = match ty {
        Ty::Atomic(t) => &**t,
        _ => ty,
    };
    bytes[0] = match ty {
        Ty::I8 => b'1',
        Ty::Callable(_) | Ty::Task(_) => b'c',
        Ty::Channel(_, true) => b'X',
        Ty::Channel(_, false) => b'Y',
        Ty::Executor => b'P',
        Ty::CancellationToken => b'K',
        Ty::Future(_) => b'Z',
        Ty::Closure(_) => b'H',
        Ty::BoxedCallable(_) => b'J',
        Ty::I16 => b'2',
        Ty::I32 => b'3',
        Ty::I64 => b'4',
        Ty::U8 => b'5',
        Ty::U16 => b'6',
        Ty::U32 => b'7',
        Ty::U64 | Ty::ForeignPtr(_) => b'8',
        Ty::F32 => b'f',
        Ty::F64 => b'd',
        Ty::Unit | Ty::Iter(..) => b'v',
        Ty::Bool => b'b',
        Ty::Str => b's',
        Ty::ByteStr => b'y',
        Ty::File => b'F',
        Ty::FsReadDir => b'A',
        Ty::FsDirEntry => b'I',
        Ty::FsMetadata => b'M',
        Ty::ByteStorage => b'U',
        Ty::List(_) => b'L',
        Ty::Clist(_) => b'Q',
        Ty::Set(_) => b'S',
        Ty::Dict(..) => b'D',
        Ty::Range(_) => b'R',
        Ty::Class(_) => b'C',
        Ty::Box(_) => b'O',
        // A shared owner is a box with more than one counted owner. Its own
        // kind lets `copy()` retain it instead of copying the box.
        Ty::Shared(_) => b'W',
        Ty::Generator(_) => b'G',
        Ty::Enum(_) => b'B',
        Ty::Ref(..) => b'r',
        Ty::KeyView(_) => b'v',
        Ty::Atomic(_) => unreachable!("replaced by its element"),
    };
    bytes[1] = u8::from(affine);
    bytes[2] = u8::from(ty.facts().reflexive);
    bytes[3] = u8::from(ty.has_inline_range());
    bytes[4..8].copy_from_slice(&(ty.inline_bytes() as u32).to_ne_bytes());
    let mut links = Vec::new();
    match ty {
        Ty::List(t)
        | Ty::Clist(t)
        | Ty::Set(t)
        | Ty::Range(t)
        | Ty::Channel(t, _)
        | Ty::Future(t)
        | Ty::Box(t)
        | Ty::Shared(t)
        | Ty::Ref(t, _) => links.push((8, reserve(module, runtime, t, pending)?)),
        Ty::Dict(k, v) => {
            links.push((8, reserve(module, runtime, k, pending)?));
            links.push((16, reserve(module, runtime, v, pending)?));
        }
        _ => {}
    }
    let enum_definition = if let Ty::Enum(t) = ty {
        Some(t.get())
    } else {
        None
    };
    let class_definition = if let Ty::Class(t) = ty {
        Some(t.get())
    } else {
        None
    };
    let (name, variants): (&str, Vec<(&str, Vec<&Ty>)>) = match ty {
        Ty::Closure(t) => (
            &t.name,
            vec![("captures", t.captures.iter().map(|(_, t)| t).collect())],
        ),
        Ty::Enum(t) => (
            t.display_name(),
            enum_definition
                .as_ref()
                .unwrap()
                .variants
                .iter()
                // Anonymous alternatives have no source constructor: the runtime
                // renders their sole payload without an invented wrapper.
                .map(|v| {
                    (
                        if t.anonymous() { "" } else { v.name.as_str() },
                        v.fields.iter().collect(),
                    )
                })
                .collect(),
        ),
        Ty::Class(t) => (
            t.display_name(),
            class_definition
                .as_ref()
                .unwrap()
                .fields
                .iter()
                .map(|(n, t)| (n.as_str(), vec![t]))
                .collect(),
        ),
        _ => ("", Vec::new()),
    };
    let name = display_name.unwrap_or(name);
    if let Some((class, field)) = &newtype {
        let field_name = class.get().fields[0].0.clone();
        let mut entry = vec![0; 32];
        word(&mut entry, 8, field_name.len());
        word(&mut entry, 24, 1);
        let field_link = reserve(module, runtime, field, pending)?;
        let entry_links = [
            (0, blob(module, field_name.into_bytes(), &[])?),
            (16, blob(module, vec![0; 8], &[(0, field_link)])?),
        ];
        links.push((64, blob(module, entry, &entry_links)?));
    }
    links.push((24, blob(module, name.as_bytes().to_vec(), &[])?));
    word(&mut bytes, 32, name.len());
    let mut entries = vec![0; variants.len() * 32];
    let mut variant_links = Vec::new();
    for (i, (name, fields)) in variants.iter().enumerate() {
        variant_links.push((i * 32, blob(module, name.as_bytes().to_vec(), &[])?));
        word(&mut entries, i * 32 + 8, name.len());
        let mut field_links = Vec::new();
        for (j, ty) in fields.iter().enumerate() {
            field_links.push((j * 8, reserve(module, runtime, ty, pending)?));
        }
        variant_links.push((
            i * 32 + 16,
            blob(module, vec![0; fields.len() * 8], &field_links)?,
        ));
        word(&mut entries, i * 32 + 24, fields.len());
    }
    links.push((40, blob(module, entries, &variant_links)?));
    word(&mut bytes, 48, variants.len());
    if let Ty::Class(class) = ty
        && ty.has_inline_storage()
    {
        links.push((72, plan(module, ty, &class.get())?));
    }
    let hook = class_definition
        .as_ref()
        .and_then(|t| t.destructor.as_ref())
        // Absent only when no live body arms the class (`reachability`), so
        // no instance can reach the hook.
        .and_then(|name| runtime.drop_hooks.get(name).copied());
    define(module, id, bytes, &links, hook)
}
