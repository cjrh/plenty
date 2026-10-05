//! Statically typed collection operations shared by checking and native lowering.
use crate::op::Ty;

#[derive(Clone, Debug, PartialEq)]
pub enum CollectionOp {
    Copy(Ty),
    Next(Ty),
    New(Ty),
    Insert(Ty), // private builder: collection, element (or key, value) -> collection
    Append(Ty), // exclusive in-place update of a list or set
    Put(Ty),    // exclusive in-place indexed update
    Get(Ty),
    Len(Ty),
    IterGet(Ty),
    IterTake(Ty),
    Contains(Ty),
    Range,
    Values(Ty),
    TextByteLen,
    TextAtByte,
}

impl Ty {
    pub fn uses_value_runtime(&self) -> bool {
        self.is_collection() || matches!(self, Self::Enum(_) | Self::Class(_) | Self::Generator(_))
    }
    pub fn is_collection(&self) -> bool {
        matches!(
            self,
            Self::List(_) | Self::Set(_) | Self::Dict(_, _) | Self::Range
        )
    }
    pub fn element(&self) -> Option<Ty> {
        match self {
            Self::List(t) | Self::Set(t) | Self::Dict(t, _) => Some((**t).clone()),
            Self::Range => Some(Self::I64),
            Self::Str => Some(Self::Str),
            Self::Generator(t) => Some((**t).clone()),
            _ => None,
        }
    }
    pub fn hashable(&self) -> bool {
        self.is_int() || matches!(self, Self::Bool | Self::Str)
    }
    /// Prefix encoding consumed by the native runtime, never a source-level type.
    pub fn descriptor(&self) -> String {
        fn write(ty: &Ty, out: &mut String, enums: &mut std::collections::HashMap<String, usize>) {
            match ty {
                Ty::Class(t) => {
                    if let Some(id) = enums.get(&t.name) {
                        out.push_str(&format!("@{id}:"));
                        return;
                    }
                    enums.insert(t.name.clone(), enums.len());
                    out.push_str(&format!("C{}:{}{}:", t.name.len(), t.name, t.fields.len()));
                    for (name, field) in &t.fields {
                        out.push_str(&format!("{}:{}", name.len(), name));
                        write(field, out, enums);
                    }
                }
                Ty::Enum(t) => {
                    if let Some(id) = enums.get(&t.name) {
                        out.push_str(&format!("@{id}:"));
                        return;
                    }
                    enums.insert(t.name.clone(), enums.len());
                    out.push_str(&format!(
                        "E{}:{}{}:",
                        t.name.len(),
                        t.name,
                        t.variants.len()
                    ));
                    for variant in &t.variants {
                        out.push_str(&format!(
                            "{}:{}{}:",
                            variant.name.len(),
                            variant.name,
                            variant.fields.len()
                        ));
                        for field in &variant.fields {
                            write(field, out, enums);
                        }
                    }
                }
                Ty::List(t) | Ty::Set(t) => {
                    out.push(if matches!(ty, Ty::List(_)) { 'L' } else { 'S' });
                    write(t, out, enums);
                }
                Ty::Dict(k, v) => {
                    out.push('D');
                    write(k, out, enums);
                    write(v, out, enums);
                }
                Ty::Generator(_) => {
                    unreachable!("generator descriptors are never stored in values")
                }
                ty => out.push(match ty {
                    Ty::I8 => '1',
                    Ty::I16 => '2',
                    Ty::I32 => '3',
                    Ty::I64 => '4',
                    Ty::U8 => '5',
                    Ty::U16 => '6',
                    Ty::U32 => '7',
                    Ty::U64 => '8',
                    Ty::Bool => 'b',
                    Ty::Str => 's',
                    Ty::Range => 'R',
                    _ => unreachable!(),
                }),
            }
        }
        let mut out = String::new();
        write(self, &mut out, &mut std::collections::HashMap::new());
        out
    }
}

impl CollectionOp {
    pub fn signature(&self) -> (Vec<Ty>, Ty) {
        use CollectionOp::*;
        match self {
            Copy(t) => (vec![t.clone()], t.clone()),
            Next(t) => (
                vec![t.clone()],
                crate::sum::option(t.element().expect("generator element")),
            ),
            TextByteLen => (vec![Ty::Str], Ty::I64),
            TextAtByte => (vec![Ty::Str, Ty::I64], Ty::Str),
            New(t) => (vec![], t.clone()),
            Insert(t) | Append(t) => {
                let mut args = vec![t.clone(), t.element().expect("collection element")];
                if let Ty::Dict(_, v) = t {
                    args.push((**v).clone());
                }
                (args, t.clone())
            }
            Put(t) => match t {
                Ty::List(v) => (vec![t.clone(), Ty::I64, (**v).clone()], t.clone()),
                Ty::Dict(k, v) => (vec![t.clone(), (**k).clone(), (**v).clone()], t.clone()),
                _ => unreachable!(),
            },
            Get(t) => match t {
                Ty::Dict(k, v) => (vec![t.clone(), (**k).clone()], (**v).clone()),
                _ => (vec![t.clone(), Ty::I64], t.element().unwrap()),
            },
            Len(t) => (vec![t.clone()], Ty::I64),
            IterGet(t) | IterTake(t) => (vec![t.clone(), Ty::I64], t.element().unwrap()),
            Contains(t) => (vec![t.element().unwrap(), t.clone()], Ty::Bool),
            Range => (vec![Ty::I64, Ty::I64, Ty::I64], Ty::Range),
            Values(t) => {
                let Ty::Dict(_, v) = t else { unreachable!() };
                (vec![t.clone()], Ty::List(v.clone()))
            }
        }
    }
    pub fn opcode(&self) -> i64 {
        match self {
            Self::Copy(_) => 14,
            Self::Next(_) => 24,
            Self::New(_) => 0,
            Self::Insert(_) => 1,
            Self::Append(_) => 2,
            Self::Put(_) => 3,
            Self::Get(_) => 4,
            Self::Len(_) => 5,
            Self::IterGet(_) => 6,
            Self::IterTake(_) => 15,
            Self::Contains(_) => 7,
            Self::Range => 10,
            Self::Values(_) => 11,
            Self::TextByteLen => 12,
            Self::TextAtByte => 13,
        }
    }
}
