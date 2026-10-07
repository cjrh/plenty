//! Immutable native type metadata. No runtime parsing or metadata allocation.
use super::*;

fn blob(module: &mut ObjectModule, bytes: Vec<u8>, links: &[(usize, DataId)]) -> Result<DataId> {
    let id = module.declare_anonymous_data(false, false)?;
    define(module, id, bytes, links)?;
    Ok(id)
}
fn define(
    module: &mut ObjectModule,
    id: DataId,
    bytes: Vec<u8>,
    links: &[(usize, DataId)],
) -> Result<()> {
    let mut data = DataDescription::new();
    data.set_align(8);
    data.define(bytes.into_boxed_slice());
    for &(offset, target) in links {
        let reference = module.declare_data_in_data(target, &mut data);
        data.write_data_addr(offset as u32, reference, 0);
    }
    module.define_data(id, &data)?;
    Ok(())
}
fn word(bytes: &mut [u8], offset: usize, value: usize) {
    bytes[offset..offset + 8].copy_from_slice(&(value as u64).to_ne_bytes());
}
#[expect(
    clippy::mutable_key_type,
    reason = "generator layout caches never participate in type equality or hashing"
)]
fn reflexive(ty: &Ty, seen: &mut std::collections::HashSet<Ty>) -> bool {
    if !seen.insert(ty.clone()) {
        return true;
    }
    match ty {
        Ty::F32 | Ty::F64 => false,
        Ty::Enum(t) => t
            .variants
            .iter()
            .flat_map(|v| &v.fields)
            .all(|t| reflexive(t, seen)),
        Ty::Class(t) => t.fields.iter().all(|(_, t)| reflexive(t, seen)),
        Ty::List(t) | Ty::Set(t) => reflexive(t, seen),
        Ty::Dict(k, v) => reflexive(k, seen) && reflexive(v, seen),
        _ => true,
    }
}

pub(super) fn declare(module: &mut ObjectModule, runtime: &Runtime, ty: &Ty) -> Result<DataId> {
    if let Some(id) = runtime.type_data.borrow().get(ty) {
        return Ok(*id);
    }
    let id = module.declare_anonymous_data(false, false)?;
    runtime.type_data.borrow_mut().insert(ty.clone(), id);
    // Mirrors plenty_runtime::aggregates::{Type, Variant} on the 64-bit host.
    let mut bytes = vec![0; 56];
    bytes[0] = match ty {
        Ty::I8 => b'1',
        Ty::I16 => b'2',
        Ty::I32 => b'3',
        Ty::I64 => b'4',
        Ty::U8 => b'5',
        Ty::U16 => b'6',
        Ty::U32 => b'7',
        Ty::U64 => b'8',
        Ty::F32 => b'f',
        Ty::F64 => b'd',
        Ty::Unit => b'v',
        Ty::Bool => b'b',
        Ty::Str => b's',
        Ty::File => b'F',
        Ty::List(_) => b'L',
        Ty::Set(_) => b'S',
        Ty::Dict(..) => b'D',
        Ty::Range(_) => b'R',
        Ty::Class(_) => b'C',
        Ty::Generator(_) => b'G',
        Ty::Enum(t) => {
            if t.inline() {
                b'B'
            } else {
                b'E'
            }
        }
        Ty::Ref(..) => b'v',
    };
    bytes[1] = u8::from(ty.affine());
    bytes[2] = u8::from(reflexive(ty, &mut Default::default()));
    bytes[3] = u8::from(ty.has_inline_range());
    bytes[4..8].copy_from_slice(&(ty.inline_bytes() as u32).to_ne_bytes());
    let mut links = Vec::new();
    match ty {
        Ty::List(t) | Ty::Set(t) | Ty::Range(t) => links.push((8, declare(module, runtime, t)?)),
        Ty::Dict(k, v) => {
            links.push((8, declare(module, runtime, k)?));
            links.push((16, declare(module, runtime, v)?));
        }
        _ => {}
    }
    let (name, variants): (&str, Vec<(&str, Vec<&Ty>)>) = match ty {
        Ty::Enum(t) => (
            &t.name,
            t.variants
                .iter()
                .map(|v| (v.name.as_str(), v.fields.iter().collect()))
                .collect(),
        ),
        Ty::Class(t) => (
            &t.name,
            t.fields
                .iter()
                .map(|(n, t)| (n.as_str(), vec![t]))
                .collect(),
        ),
        _ => ("", Vec::new()),
    };
    links.push((24, blob(module, name.as_bytes().to_vec(), &[])?));
    word(&mut bytes, 32, name.len());
    let mut entries = vec![0; variants.len() * 32];
    let mut variant_links = Vec::new();
    for (i, (name, fields)) in variants.iter().enumerate() {
        variant_links.push((i * 32, blob(module, name.as_bytes().to_vec(), &[])?));
        word(&mut entries, i * 32 + 8, name.len());
        let mut field_links = Vec::new();
        for (j, ty) in fields.iter().enumerate() {
            field_links.push((j * 8, declare(module, runtime, ty)?));
        }
        variant_links.push((
            i * 32 + 16,
            blob(module, vec![0; fields.len() * 8], &field_links)?,
        ));
        word(&mut entries, i * 32 + 24, fields.len());
    }
    links.push((40, blob(module, entries, &variant_links)?));
    word(&mut bytes, 48, variants.len());
    define(module, id, bytes, &links)?;
    Ok(id)
}
