//! Concrete nominal sum types and their checked operations.
use crate::op::Ty;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct EnumType {
    pub facts: std::cell::OnceCell<crate::type_facts::Facts>,
    pub restricted_storage: bool,
    // Names are unique within a compilation; builtin names include concrete args.
    pub name: String,
    pub variants: Vec<Variant>,
    /// False only when no payload can need cleanup; see `Ty::managed`.
    pub managed: bool,
    /// Whether some payload actually needs cleanup; excluded from equality.
    pub managed_fields: std::cell::OnceCell<bool>,
    pub inline_range: bool,
    /// Layout cache, excluded from nominal type equality and hashing.
    pub payload_bytes: std::cell::OnceCell<usize>,
    /// Whether any variant keeps owner-local storage; excluded from equality.
    pub storage: std::cell::OnceCell<bool>,
}
impl PartialEq for EnumType {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl Eq for EnumType {}
impl Hash for EnumType {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}
impl crate::nominal::Nominal<EnumType> {
    pub fn tag_bits(&self) -> u32 {
        self.get().variants.len().next_power_of_two().ilog2().max(1)
    }
    pub fn tuple(&self) -> bool {
        self.name.starts_with("tuple[")
    }
    pub fn propagatable(&self) -> bool {
        self.is_option() || self.is_result()
    }
    pub fn is_option(&self) -> bool {
        self.name.starts_with("Option[")
    }
    pub fn is_result(&self) -> bool {
        self.name.starts_with("Result[")
    }
    pub fn discards_error(&self) -> bool {
        self.is_result()
            && matches!(self.local().variants[1].fields.as_slice(), [Ty::Enum(t)] if t.name == "Failure")
    }
    /// Whether this Result can receive the source's error through `?`.
    pub fn accepts_result_error(&self, source: &Self) -> bool {
        self.is_result()
            && source.is_result()
            && (self.discards_error()
                || self.get().variants[1].fields == source.get().variants[1].fields)
    }
}

/// A payload-free marker for callers that deliberately discard error details.
pub fn failure() -> Ty {
    Ty::Enum(crate::nominal::Nominal::new(EnumType {
        restricted_storage: false,
        name: "Failure".into(),
        variants: vec![Variant {
            name: "Unspecified".into(),
            fields: vec![],
        }],
        facts: std::cell::OnceCell::new(),
        managed: false,
        inline_range: false,
        payload_bytes: std::cell::OnceCell::new(),
        storage: std::cell::OnceCell::new(),
        managed_fields: std::cell::OnceCell::new(),
    }))
}

/// An allocation error must itself be constructible without allocating.
pub fn alloc_error() -> Ty {
    Ty::Enum(crate::nominal::Nominal::new(EnumType {
        restricted_storage: false,
        name: "AllocError".into(),
        variants: ["OutOfMemory", "CapacityOverflow"]
            .into_iter()
            .map(|name| Variant {
                name: name.into(),
                fields: vec![],
            })
            .collect(),
        facts: std::cell::OnceCell::new(),
        managed: false,
        inline_range: false,
        payload_bytes: std::cell::OnceCell::new(),
        storage: std::cell::OnceCell::new(),
        managed_fields: std::cell::OnceCell::new(),
    }))
}

pub fn allocation_result() -> Ty {
    result(Ty::Unit, alloc_error())
}

/// Native creation errors are represented inline, even under memory pressure.
pub fn thread_error() -> Ty {
    let Ty::Enum(template) = alloc_error() else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.facts = std::cell::OnceCell::new();
    ty.name = "ThreadError".into();
    ty.variants = vec![Variant {
        name: "System".into(),
        fields: vec![Ty::I32],
    }];
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

/// A failed consuming start returns the complete job, without allocating an
/// error object. The native default-attributes path distinguishes resource
/// failure from permission denial; borrowed starts retain their raw OS code.
pub fn spawn_error(job: Ty) -> Ty {
    let Ty::Enum(template) = option(job.clone()) else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.name = format!("SpawnError[{job}]");
    ty.variants = ["Unavailable", "PermissionDenied"]
        .into_iter()
        .map(|name| Variant {
            name: name.into(),
            fields: vec![job.clone()],
        })
        .collect();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

pub fn channel_error() -> Ty {
    let Ty::Enum(template) = result(Ty::Unit, alloc_error()) else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.name = "ChannelError".into();
    ty.variants[0] = Variant {
        name: "InvalidCapacity".into(),
        fields: vec![],
    };
    ty.variants[1].name = "Allocation".into();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

pub fn send_error(value: Ty) -> Ty {
    let Ty::Enum(template) = spawn_error(value.clone()) else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.name = format!("SendError[{value}]");
    ty.variants[0].name = "Full".into();
    ty.variants[1].name = "Disconnected".into();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

pub fn recv_error() -> Ty {
    let Ty::Enum(template) = alloc_error() else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.name = "RecvError".into();
    ty.variants[0].name = "Empty".into();
    ty.variants[1].name = "Disconnected".into();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

pub fn send_timeout_error(value: Ty) -> Ty {
    let Ty::Enum(template) = send_error(value.clone()) else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.name = format!("SendTimeoutError[{value}]");
    ty.variants[0].name = "Disconnected".into();
    ty.variants[1].name = "TimedOut".into();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

pub fn recv_timeout_error() -> Ty {
    let Ty::Enum(template) = recv_error() else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.name = "RecvTimeoutError".into();
    ty.variants[0].name = "Disconnected".into();
    ty.variants[1].name = "TimedOut".into();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

/// Both alternatives carry one owned value in caller-provided inline storage.
pub fn selected(first: Ty, second: Ty) -> Ty {
    let Ty::Enum(template) = result(first.clone(), second.clone()) else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.name = format!("Selected[{first}, {second}]");
    ty.variants[0].name = "First".into();
    ty.variants[1].name = "Second".into();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

pub fn select_error() -> Ty {
    let Ty::Enum(template) = recv_error() else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.name = "SelectError".into();
    ty.variants.push(Variant {
        name: "TimedOut".into(),
        fields: vec![],
    });
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

pub fn pool_error() -> Ty {
    let Ty::Enum(template) = result(Ty::Unit, alloc_error()) else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.name = "PoolError".into();
    ty.variants = vec![
        Variant {
            name: "InvalidSize".into(),
            fields: vec![],
        },
        Variant {
            name: "Allocation".into(),
            fields: vec![alloc_error()],
        },
        Variant {
            name: "Thread".into(),
            fields: vec![thread_error()],
        },
    ];
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

pub fn submit_error(job: Ty) -> Ty {
    let Ty::Enum(template) = spawn_error(job.clone()) else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.name = format!("SubmitError[{job}]");
    ty.variants = ["Full", "Shutdown", "OutOfMemory", "CapacityOverflow"]
        .into_iter()
        .map(|name| Variant {
            name: name.into(),
            fields: vec![job.clone()],
        })
        .collect();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

pub fn future_error() -> Ty {
    let Ty::Enum(template) = failure() else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.name = "FutureError".into();
    ty.variants[0].name = "Cancelled".into();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

pub fn pool_map_error() -> Ty {
    let Ty::Enum(template) = result(alloc_error(), Ty::Unit) else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.name = "PoolMapError".into();
    ty.variants[0].name = "Allocation".into();
    ty.variants[1] = Variant {
        name: "Shutdown".into(),
        fields: vec![],
    };
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

/// Preserve a worker's concrete error without allocating an error wrapper.
pub fn parallel_error(error: Ty) -> Ty {
    let Ty::Enum(template) = result(error.clone(), alloc_error()) else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.name = format!("ParallelError[{error}]");
    ty.variants = vec![
        Variant {
            name: "Allocation".into(),
            fields: vec![alloc_error()],
        },
        Variant {
            name: "Shutdown".into(),
            fields: vec![],
        },
        Variant {
            name: "Worker".into(),
            fields: vec![error],
        },
    ];
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

/// Loader failures are fieldless markers with a three-bit inline discriminant.
pub fn load_error() -> Ty {
    let Ty::Enum(template) = alloc_error() else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.facts = std::cell::OnceCell::new();
    ty.name = "LoadError".into();
    ty.variants = [
        "OutOfMemory",
        "CapacityOverflow",
        "InvalidPath",
        "OpenFailed",
        "InvalidSymbol",
        "MissingSymbol",
        "IncompatibleContract",
    ]
    .into_iter()
    .map(|name| Variant {
        name: name.into(),
        fields: vec![],
    })
    .collect();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

pub fn data_error() -> Ty {
    let Ty::Enum(template) = result(Ty::Unit, alloc_error()) else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.facts = std::cell::OnceCell::new();
    ty.name = "DataError".into();
    ty.variants[0] = Variant {
        name: "InvalidUtf8".into(),
        fields: vec![],
    };
    ty.variants[1].name = "Allocation".into();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

/// Conversion failures before a native C-string call. These errors allocate nothing.
pub fn c_str_error() -> Ty {
    let Ty::Enum(template) = data_error() else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.facts = std::cell::OnceCell::new();
    ty.name = "CStrError".into();
    ty.variants[0].name = "EmbeddedNul".into();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

/// `System` carries the operating system's code. The payload-free variants
/// are failures the runtime detects itself, for which no code exists. The
/// runtime builds these values by tag: keep the order in step with its
/// `text_io::Kind` and the count with `io::IO_ERROR_TAG_BITS`.
pub fn io_error() -> Ty {
    let Ty::Enum(template) = result(Ty::I32, data_error()) else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.facts = std::cell::OnceCell::new();
    ty.name = "IoError".into();
    ty.variants[0].name = "System".into();
    ty.variants[1].name = "Data".into();
    ty.variants.extend(
        [
            "InvalidMode",
            "Closed",
            "NotReadable",
            "NotWritable",
            "InvalidInput",
            "Unsupported",
            "Other",
        ]
        .map(|name| Variant {
            name: name.into(),
            fields: vec![],
        }),
    );
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

pub fn parse_error() -> Ty {
    let Ty::Enum(template) = alloc_error() else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.facts = std::cell::OnceCell::new();
    ty.name = "ParseError".into();
    ty.variants[0].name = "Invalid".into();
    ty.variants[1].name = "OutOfRange".into();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}
#[derive(Clone, Debug)]
pub struct Variant {
    pub name: String,
    pub fields: Vec<Ty>,
}

/// Structural products are single-variant inline enums.
pub fn tuple(fields: Vec<Ty>) -> Ty {
    Ty::Enum(crate::nominal::Nominal::new(EnumType {
        name: format!(
            "tuple[{}]",
            fields
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        facts: std::cell::OnceCell::new(),
        restricted_storage: fields.iter().any(|t| !t.heap_storable()),
        managed: true,
        inline_range: false,
        payload_bytes: std::cell::OnceCell::new(),
        storage: std::cell::OnceCell::new(),
        managed_fields: std::cell::OnceCell::new(),
        variants: vec![Variant {
            name: String::new(),
            fields,
        }],
    }))
}

pub fn option(element: Ty) -> Ty {
    Ty::Enum(crate::nominal::Nominal::new(EnumType {
        restricted_storage: element.restricted_storage(),
        name: format!("Option[{element}]"),
        facts: std::cell::OnceCell::new(),
        managed: element.managed(),
        inline_range: element.has_inline_range(),
        payload_bytes: std::cell::OnceCell::new(),
        storage: std::cell::OnceCell::new(),
        managed_fields: std::cell::OnceCell::new(),
        variants: vec![
            Variant {
                name: "Nothing".into(),
                fields: vec![],
            },
            Variant {
                name: "Some".into(),
                fields: vec![element],
            },
        ],
    }))
}
pub fn result(ok: Ty, error: Ty) -> Ty {
    Ty::Enum(crate::nominal::Nominal::new(EnumType {
        restricted_storage: ok.restricted_storage() || error.restricted_storage(),
        name: format!("Result[{ok}, {error}]"),
        facts: std::cell::OnceCell::new(),
        managed: ok.managed() || error.managed(),
        inline_range: ok.has_inline_range() || error.has_inline_range(),
        payload_bytes: std::cell::OnceCell::new(),
        storage: std::cell::OnceCell::new(),
        managed_fields: std::cell::OnceCell::new(),
        variants: vec![
            Variant {
                name: "Ok".into(),
                fields: vec![ok],
            },
            Variant {
                name: "Err".into(),
                fields: vec![error],
            },
        ],
    }))
}

#[derive(Clone, Debug, PartialEq)]
pub enum EnumOp {
    New(crate::nominal::Nominal<EnumType>, usize),
    Unwrap(crate::nominal::Nominal<EnumType>),
    Tag(crate::nominal::Nominal<EnumType>),
    TagRef(crate::nominal::Nominal<EnumType>, bool),
    FieldRef(crate::nominal::Nominal<EnumType>, usize, usize, bool),
    Field(crate::nominal::Nominal<EnumType>, usize, usize),
    Take(crate::nominal::Nominal<EnumType>, usize, usize),
}
impl EnumOp {
    pub fn signature(&self) -> Option<(Vec<Ty>, Ty)> {
        Some(match self {
            Self::Unwrap(t) if t.propagatable() => (
                vec![Ty::Enum(t.clone())],
                t.get().variants[usize::from(t.is_option())].fields[0].clone(),
            ),
            Self::Unwrap(_) => return None,
            Self::New(t, tag) => (
                t.get().variants.get(*tag)?.fields.clone(),
                Ty::Enum(t.clone()),
            ),
            Self::Tag(t) => (vec![Ty::Enum(t.clone())], Ty::I64),
            Self::TagRef(t, mutable) => (
                vec![Ty::Ref(Rc::new(Ty::Enum(t.clone())), *mutable)],
                Ty::I64,
            ),
            Self::FieldRef(t, tag, field, mutable) => (
                vec![Ty::Ref(Rc::new(Ty::Enum(t.clone())), *mutable)],
                Ty::Ref(
                    Rc::new(t.get().variants.get(*tag)?.fields.get(*field)?.clone()),
                    *mutable,
                ),
            ),
            Self::Field(t, tag, field) | Self::Take(t, tag, field) => (
                vec![Ty::Enum(t.clone())],
                t.get().variants.get(*tag)?.fields.get(*field)?.clone(),
            ),
        })
    }
}
