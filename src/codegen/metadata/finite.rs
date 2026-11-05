//! Canonical values of small finite sums, emitted once as immutable metadata.
//! Ordinals follow variant order, then a left-to-right mixed-radix product of
//! payload domains. This is also the runtime's membership-bit numbering.
use super::*;

const LIMIT: usize = 256;

struct Domain {
    ty: Ty,
    cardinality: usize,
    variants: Vec<Vec<Domain>>,
    offsets: Vec<u8>,
}

impl Domain {
    fn of(ty: &Ty) -> Option<Self> {
        let mut result = Self {
            ty: ty.clone(),
            cardinality: 0,
            variants: Vec::new(),
            offsets: Vec::new(),
        };
        result.cardinality = match ty {
            Ty::Bool => 2,
            Ty::Enum(t) => {
                let facts = ty.facts();
                if t.anonymous() || !facts.complete || facts.recursive || facts.affine {
                    return None;
                }
                let mut total = 0usize;
                for variant in &t.get().variants {
                    result.offsets.push(u8::try_from(total).ok()?);
                    let mut count = 1usize;
                    let mut fields = Vec::new();
                    for field in &variant.fields {
                        let domain = Self::of(field)?;
                        count = count
                            .checked_mul(domain.cardinality)
                            .filter(|n| *n <= LIMIT)?;
                        fields.push(domain);
                    }
                    total = total.checked_add(count).filter(|n| *n <= LIMIT)?;
                    result.variants.push(fields);
                }
                if total == 0 {
                    return None;
                }
                total
            }
            _ => return None,
        };
        Some(result)
    }

    /// Encode a value whose owner-local inline bytes begin at `base`. Pointer
    /// words are self-relocations into `storage`, never compiler-process pointers.
    fn encode(&self, ordinal: usize, storage: &mut Storage, base: usize) -> Packed {
        match &self.ty {
            Ty::Bool => Packed::scalar(ordinal as u64),
            Ty::Enum(t) => {
                let tag = self
                    .offsets
                    .partition_point(|offset| usize::from(*offset) <= ordinal)
                    - 1;
                let fields = &self.variants[tag];
                let mut ordinal = ordinal - usize::from(self.offsets[tag]);
                let payload = match fields.as_slice() {
                    [] => Packed::scalar(0),
                    [field] => field.encode(ordinal, storage, base),
                    _ => {
                        let mut radix: usize =
                            fields.iter().map(|field| field.cardinality).product();
                        let mut at = base;
                        for field in fields {
                            radix /= field.cardinality;
                            let digit = ordinal / radix;
                            ordinal %= radix;
                            let value = field.encode(digit, storage, at + 16);
                            storage.write(at, value);
                            at += field.ty.slot_bytes();
                        }
                        Packed {
                            low: Low::Address(base),
                            high: 0,
                        }
                    }
                };
                Packed {
                    low: payload.low,
                    high: (payload.high << t.tag_bits()) | tag as u64,
                }
            }
            _ => unreachable!("finite payload domain"),
        }
    }
}

#[derive(Clone, Copy)]
enum Low {
    Scalar(u64),
    Address(usize),
}
#[derive(Clone, Copy)]
struct Packed {
    low: Low,
    high: u64,
}
impl Packed {
    fn scalar(low: u64) -> Self {
        Self {
            low: Low::Scalar(low),
            high: 0,
        }
    }
}

struct Storage {
    bytes: Vec<u8>,
    pointers: Vec<(usize, usize)>,
}
impl Storage {
    fn write(&mut self, at: usize, value: Packed) {
        match value.low {
            Low::Scalar(low) => self.bytes[at..at + 8].copy_from_slice(&low.to_ne_bytes()),
            Low::Address(offset) => self.pointers.push((at, offset)),
        }
        self.bytes[at + 8..at + 16].copy_from_slice(&value.high.to_ne_bytes());
    }

    fn emit(self, module: &mut ObjectModule) -> Result<DataId> {
        let id = module.declare_anonymous_data(false, false)?;
        let mut data = DataDescription::new();
        data.set_align(16);
        data.define(self.bytes.into_boxed_slice());
        if !self.pointers.is_empty() {
            let reference = module.declare_data_in_data(id, &mut data);
            for (at, offset) in self.pointers {
                data.write_data_addr(at as u32, reference, offset as i64);
            }
        }
        module.define_data(id, &data)?;
        Ok(id)
    }
}

/// Return the optional `Finite` descriptor. Payload-free enums already use the
/// runtime's shared canonical tag table, so they need no per-type value table.
pub(super) fn emit(module: &mut ObjectModule, ty: &Ty) -> Result<Option<DataId>> {
    let Ty::Enum(t) = ty else { return Ok(None) };
    if t.get()
        .variants
        .iter()
        .all(|variant| variant.fields.is_empty())
    {
        return Ok(None);
    }
    let Some(domain) = Domain::of(ty) else {
        return Ok(None);
    };
    let mut values = vec![0; domain.cardinality * 16];
    let mut links = Vec::new();
    for ordinal in 0..domain.cardinality {
        let mut storage = Storage {
            bytes: vec![0; ty.inline_bytes()],
            pointers: Vec::new(),
        };
        let value = domain.encode(ordinal, &mut storage, 0);
        let at = ordinal * 16;
        match value.low {
            Low::Scalar(low) => values[at..at + 8].copy_from_slice(&low.to_ne_bytes()),
            Low::Address(offset) => links.push((at, storage.emit(module)?, offset)),
        }
        values[at + 8..at + 16].copy_from_slice(&value.high.to_ne_bytes());
    }
    let values_id = module.declare_anonymous_data(false, false)?;
    let mut data = DataDescription::new();
    data.set_align(16);
    data.define(values.into_boxed_slice());
    for (at, target, offset) in links {
        let reference = module.declare_data_in_data(target, &mut data);
        data.write_data_addr(at as u32, reference, offset as i64);
    }
    module.define_data(values_id, &data)?;
    let mut bytes = vec![0; 32];
    word(&mut bytes, 8, domain.cardinality);
    word(&mut bytes, 24, domain.offsets.len());
    let offsets = blob(module, domain.offsets, &[])?;
    Ok(Some(blob(module, bytes, &[(0, values_id), (16, offsets)])?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sum::{option, tuple};

    fn encoded(ty: &Ty, ordinal: usize) -> (Packed, Storage) {
        let domain = Domain::of(ty).expect("finite domain");
        let mut storage = Storage {
            bytes: vec![0; ty.inline_bytes()],
            pointers: vec![],
        };
        (domain.encode(ordinal, &mut storage, 0), storage)
    }

    fn low_word(storage: &Storage, at: usize) -> u64 {
        u64::from_ne_bytes(storage.bytes[at..at + 8].try_into().unwrap())
    }

    #[test]
    fn cardinality_stops_at_256_before_products_overflow() {
        for bits in 6..=8 {
            let domain = tuple(vec![Ty::Bool; bits]);
            assert_eq!(Domain::of(&domain).unwrap().cardinality, 1 << bits);
            assert_eq!(Domain::of(&option(domain)).is_some(), bits < 8);
        }
        assert!(Domain::of(&tuple(vec![Ty::Bool; 9])).is_none());
        assert!(Domain::of(&tuple(vec![Ty::Bool; usize::BITS as usize + 1])).is_none());
    }

    #[test]
    fn nested_sum_tags_and_repeated_domains_follow_mixed_radix_order() {
        let nested = option(option(Ty::Bool));
        let domain = Domain::of(&nested).unwrap();
        assert_eq!(domain.cardinality, 4);
        assert_eq!(domain.offsets, [0, 1]);
        for (ordinal, (low, high)) in [(0, 0), (0, 1), (0, 3), (1, 3)].into_iter().enumerate() {
            let (value, storage) = encoded(&nested, ordinal);
            assert!(matches!(value.low, Low::Scalar(n) if n == low));
            assert_eq!(value.high, high);
            assert!(storage.bytes.is_empty());
        }
        let repeated = tuple(vec![option(Ty::Bool), option(Ty::Bool)]);
        assert_eq!(Domain::of(&repeated).unwrap().cardinality, 9);
        // Ordinal five is (Some(False), Some(True)): 1 * 3 + 2.
        let (_, storage) = encoded(&repeated, 5);
        assert_eq!(low_word(&storage, 0), 0);
        assert_eq!(low_word(&storage, 8), 1);
        assert_eq!(low_word(&storage, 16), 1);
        assert_eq!(low_word(&storage, 24), 1);
    }

    #[test]
    fn payload_free_and_nested_records_keep_owner_local_storage_offsets() {
        let ty = tuple(vec![
            crate::sum::failure(),
            option(tuple(vec![Ty::Bool; 2])),
            Ty::Bool,
        ]);
        assert_eq!(Domain::of(&ty).unwrap().cardinality, 10);
        // The final ordinal is (Failure.Unspecified, Some((True, True)), True).
        let (value, storage) = encoded(&ty, 9);
        assert!(matches!(value.low, Low::Address(0)));
        assert_eq!(value.high, 0);
        assert_eq!(storage.bytes.len(), 80);
        assert_eq!(storage.pointers, [(16, 32)]);
        assert_eq!(&storage.bytes[..16], &[0; 16]);
        assert_eq!(low_word(&storage, 24), 1);
        for at in [32, 48, 64] {
            assert_eq!(low_word(&storage, at), 1);
            assert_eq!(low_word(&storage, at + 8), 0);
        }
    }

    #[test]
    fn nonfinite_anonymous_incomplete_and_recursive_domains_are_excluded() {
        for ty in [
            Ty::Unit,
            Ty::I64,
            Ty::F64,
            Ty::Box(Arc::new(Ty::Bool)),
            Ty::List(Arc::new(Ty::Bool)),
            crate::sum::anonymous(vec![Ty::Bool, crate::sum::failure()]),
        ] {
            assert!(Domain::of(&ty).is_none());
        }
        let types = Arc::new(crate::nominal::Types::default());
        let node = types.enumeration("Node");
        let ty = Ty::Enum(node.clone());
        assert!(Domain::of(&ty).is_none());
        let Ty::Enum(template) = crate::sum::failure() else {
            unreachable!()
        };
        let mut definition = (*template.get()).clone();
        definition.name = "Node".into();
        definition.variants[0].fields = vec![Ty::Box(Arc::new(ty.clone()))];
        node.define(definition);
        assert!(Domain::of(&ty).is_none());
        let Ty::Enum(template) = crate::sum::failure() else {
            unreachable!()
        };
        let mut empty = (*template.get()).clone();
        empty.name = "Empty".into();
        empty.variants.clear();
        assert!(Domain::of(&Ty::Enum(crate::nominal::Nominal::new(empty))).is_none());
        assert!(Domain::of(&Ty::Class(types.class("NotDefined"))).is_none());
    }
}
