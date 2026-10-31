//! Typed operations and independent checking before native code generation.
//! Also contains the historical stack-syntax lowering used by backend tests.

use crate::hash::HashMap;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

use crate::lexer::Tok;
use crate::value::{Heap, StrId, Value};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// A Plenty type, as it appears in a function's type header (§11.2).
///
/// Sized integers (§11.2): the user picks an exact bit width, signed or
/// unsigned, so the program's memory footprint and overflow semantics are
/// declared on the surface rather than hidden behind a polymorphic "Int".
/// Collections, enums, and generators carry resolved concrete type metadata;
/// generic functions are specialized before native lowering.
#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Ty {
    /// Affine token for a frontend-local fused plan; its state lives in local slots.
    Iter(Arc<Ty>, usize),
    /// Lexical join token: only direct `.join()` consumes a source task.
    Task(Arc<crate::threading::Task>),
    /// Shared channel core, with affine sender (true) or receiver (false) handles.
    Channel(Arc<Ty>, bool),
    CancellationToken,
    /// `std.atomic.Atomic[T]`: an inline integer or bool slot accessed only
    /// through atomic instructions.
    Atomic(Arc<Ty>),
    Executor,
    Future(Arc<Ty>),
    Closure(Arc<crate::closure::ClosureType>),
    Callable(Arc<CallableSig>),
    /// Explicitly allocated, owned shared-call environment with structural identity.
    BoxedCallable(Arc<CallableSig>),
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    /// Stored enum payload marker; source-level unit expressions have no operand.
    Unit,
    Str,
    /// `bytes`: an immutable byte literal in read-only data. It is copied as
    /// a pointer and never released, so it needs no ownership.
    ByteStr,
    File,
    /// Private native owners used by the embedded filesystem library.
    FsReadDir,
    FsDirEntry,
    FsMetadata,
    /// Private contiguous byte owner used by the embedded `std.bytes`.
    ByteStorage,
    /// Nominal opaque C pointer. No dereference or integer conversions.
    ForeignPtr(Arc<str>),
    Bool,
    List(Arc<Ty>),
    /// Dense native-width columns for a primitive-field class.
    Clist(Arc<Ty>),
    Set(Arc<Ty>),
    /// Local-only shared dictionary-key projection. The owner carries its
    /// complete row descriptor; the view never owns or retains that owner.
    KeyView(Arc<Ty>),
    /// `&[T]` (false) or `&mut [T]` (true): a borrowed run of contiguous
    /// list or byte rows, made by `&xs[a:b]`. Its value is the first row's
    /// address and the row count; the frontend keeps the owner's loan live
    /// for as long as the view is used. Local-only until stored references
    /// (backlog B11); see book/src/design/language/04-expressions/slices.md.
    View(Arc<Ty>, bool),
    Dict(Arc<Ty>, Arc<Ty>),
    /// One owned heap value; recursive types reach themselves through boxes.
    Box(Arc<Ty>),
    /// `Shared[T]`: one atomically counted heap value with several affine
    /// owners. It reuses the box layout and gives only shared access; see
    /// book/src/design/facilities/01-builtins/shared.md.
    Shared(Arc<Ty>),
    Range(Arc<Ty>),
    Enum(crate::nominal::Nominal<crate::sum::EnumType>),
    Class(crate::nominal::Nominal<crate::record::ClassType>),
    Generator(Arc<crate::generator::GeneratorType>),
    Ref(Arc<Ty>, bool),
}

/// Why a type cannot enter heap storage; see `Ty::storage_barrier`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageBarrier {
    Iter,
    KeyView,
    View,
    Reference,
    Generator,
    ScopedTask,
    /// A `Closure`/`OnceClosure` annotation that no producer has resolved.
    UnresolvedClosure,
    BorrowedClosure,
    GeneratorClosure,
    /// An `Option`/`Result` whose success payload is a reference.
    CarriedReference,
}

impl StorageBarrier {
    /// The diagnostic for storing the rejected type in `place`, e.g. "collections".
    pub fn message(self, place: &str) -> String {
        let subject = match self {
            Self::Iter => "iterator pipelines",
            Self::KeyView => "dictionary key views",
            Self::View => {
                return format!(
                    "slice views cannot be stored in {place} yet (backlog B11); store the bounds, or an owned copy `xs[a:b]?`"
                );
            }
            Self::Reference => "references",
            Self::Generator => "generators",
            Self::ScopedTask => "scoped tasks",
            Self::BorrowedClosure => "closures with borrowed captures",
            Self::GeneratorClosure => "closures that capture generators",
            Self::CarriedReference => {
                return format!(
                    "an Option or Result carrying a reference cannot be stored in {place}; match it and store an owned copy or an index"
                );
            }
            Self::UnresolvedClosure => {
                return format!(
                    "a closure signature does not name a concrete environment, so it cannot be stored in {place}; let the type be inferred from the stored closure or use a generic parameter bounded by Callable"
                );
            }
        };
        format!("{subject} cannot be stored in {place}")
    }
}

impl Ty {
    /// Range payloads also occur inside standard sums. Tags remain in the
    /// ordinary 128-bit representation; slot_bytes includes owner-local payloads.
    pub fn has_inline_range(&self) -> bool {
        matches!(self, Self::Range(_))
            || matches!(self, Self::Enum(t) if t.try_get().is_some_and(|t| t.inline_range))
    }
    pub fn slot_bytes(&self) -> usize {
        16 + self.inline_bytes()
    }
    /// Whether a field of this type, in a class record or a multi-field
    /// variant or tuple, is stored flat: its record's bytes sit at the field's
    /// offset in the owner, with no value word of their own. A class with a record (after removing newtype layers) qualifies;
    /// its value is then the field's address. See
    /// book/src/design/toolchain/02-architecture/values.md.
    pub fn flat_field(&self) -> bool {
        matches!(&*self.newtype_base(), Self::Class(_)) && self.has_inline_storage()
    }
    /// The bytes a field of this type takes in its owner's record.
    pub fn field_bytes(&self) -> usize {
        if self.flat_field() {
            self.inline_bytes()
        } else {
            self.slot_bytes()
        }
    }
    /// Values carried as 128 bits: a payload word plus binary tags. A class's
    /// own tag records whether its initializer completed.
    pub fn wide(&self) -> bool {
        matches!(self, Self::Enum(_) | Self::Class(_))
    }
    /// The field a one-field class is represented as
    /// (`ClassType::newtype_field`), if `self` is such a class.
    pub fn newtype_field(&self) -> Option<Ty> {
        match self {
            Self::Class(t) => t.get().newtype_field().cloned(),
            _ => None,
        }
    }
    /// `T` when `self` is the builtin `NonZero[T]`. Its name is reserved, so
    /// the identity name identifies it.
    pub fn nonzero_element(&self) -> Option<Ty> {
        match self {
            Self::Class(t) if t.name.starts_with("NonZero[") => Some(t.get().fields[0].1.clone()),
            _ => None,
        }
    }
    /// `self` with every newtype class layer removed: the type whose layout
    /// the value actually has.
    pub fn newtype_base(&self) -> std::borrow::Cow<'_, Ty> {
        let Some(mut base) = self.newtype_field() else {
            return std::borrow::Cow::Borrowed(self);
        };
        while let Some(inner) = base.newtype_field() {
            base = inner;
        }
        std::borrow::Cow::Owned(base)
    }
    pub fn is_float(&self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }
    pub fn is_numeric(&self) -> bool {
        self.is_int() || self.is_float()
    }
    /// Types covered by the runtime's total sorting order. Tuple ordering
    /// recursively uses that same order, including for floating-point fields.
    pub fn is_orderable(&self) -> bool {
        match self {
            Self::Str => true,
            Self::Enum(t) if t.tuple() => t.get().variants[0].fields.iter().all(Self::is_orderable),
            _ => self.is_numeric(),
        }
    }
    pub fn layout_depth(&self) -> usize {
        match self {
            Self::Closure(t) => t.depth,
            Ty::Callable(sig) | Ty::BoxedCallable(sig) => {
                1 + sig
                    .inputs
                    .iter()
                    .chain(sig.output.iter())
                    .map(Ty::layout_depth)
                    .max()
                    .unwrap_or(0)
            }
            Self::List(t)
            | Self::Clist(t)
            | Self::Set(t)
            | Self::Channel(t, _)
            | Self::Future(t)
            | Self::Box(t)
            | Self::Shared(t) => 1 + t.layout_depth(),
            Self::Generator(t) => 1 + t.element.layout_depth(),
            Self::Dict(k, v) => 1 + k.layout_depth().max(v.layout_depth()),
            Self::Enum(_) | Self::Class(_) => self.facts().depth,
            _ => 0,
        }
    }
    pub fn affine(&self) -> bool {
        matches!(
            self,
            Self::Iter(..)
                | Self::List(_)
                | Self::Clist(_)
                | Self::Task(_)
                | Self::Channel(..)
                | Self::CancellationToken
                | Self::Atomic(_)
                | Self::Executor
                | Self::Future(_)
                | Self::Closure(_)
                | Self::BoxedCallable(_)
                | Self::Set(_)
                | Self::Dict(_, _)
                | Self::Generator(_)
                | Self::Box(_)
                | Self::Shared(_)
                | Self::File
                | Self::FsReadDir
                | Self::FsDirEntry
                | Self::FsMetadata
                | Self::ByteStorage
        ) || matches!(self, Self::Enum(_) if self.facts().affine)
            // An undefined forward reference is conservatively affine.
            || matches!(self, Self::Class(t) if t.try_get().is_none_or(|t| !t.copy))
    }
    pub fn restricted_storage(&self) -> bool {
        match self {
            Self::Iter(..)
            | Self::Task(_)
            | Self::Generator(_)
            | Self::Closure(_)
            | Self::Ref(..)
            | Self::KeyView(_)
            | Self::View(..) => true,
            Self::Enum(t) => t.try_get().is_some_and(|t| t.restricted_storage),
            _ => false,
        }
    }
    /// Heap owners can embed a resolved, wholly owned environment.
    /// Keep this separate from inline-layout facts used by Option and Result.
    pub fn heap_storable(&self) -> bool {
        self.storage_barrier().is_none()
    }
    /// Whether the type becomes heap-storable once its signature-only closures
    /// take concrete producers, as a declared return type's do (issue #336).
    pub fn heap_storable_once_resolved(&self) -> bool {
        match self {
            Self::Closure(t) if t.name.is_empty() => !t.borrowed,
            Self::Enum(t) if t.try_get().is_some_and(|t| t.restricted_storage) => t
                .get()
                .variants
                .iter()
                .flat_map(|v| &v.fields)
                .all(Self::heap_storable_once_resolved),
            _ => self.heap_storable(),
        }
    }
    /// An anonymous sum with a signature-only closure member. Only a declared
    /// return type can resolve one, from what the body returns or propagates.
    pub fn unresolved_sum_member(&self) -> bool {
        match self {
            Self::Ref(t, _) => t.unresolved_sum_member(),
            Self::Enum(t) if t.try_get().is_some_and(|t| t.restricted_storage) => {
                let anonymous = t.anonymous();
                t.get().variants.iter().flat_map(|v| &v.fields).any(|f| {
                    (anonymous && f.unresolved_producer().is_some()) || f.unresolved_sum_member()
                })
            }
            _ => false,
        }
    }
    /// The first reason a heap owner cannot embed this type.
    pub fn storage_barrier(&self) -> Option<StorageBarrier> {
        match self {
            Self::Iter(..) => Some(StorageBarrier::Iter),
            Self::KeyView(_) => Some(StorageBarrier::KeyView),
            Self::View(..) => Some(StorageBarrier::View),
            Self::Ref(..) => Some(StorageBarrier::Reference),
            Self::Generator(_) => Some(StorageBarrier::Generator),
            Self::Task(_) => Some(StorageBarrier::ScopedTask),
            Self::Closure(t) if t.name.is_empty() => Some(StorageBarrier::UnresolvedClosure),
            Self::Closure(t) if t.borrowed => Some(StorageBarrier::BorrowedClosure),
            Self::Closure(_) if self.contains_generator_frame() => {
                Some(StorageBarrier::GeneratorClosure)
            }
            _ if self.carried_reference().is_some() => Some(StorageBarrier::CarriedReference),
            Self::Enum(t) if t.try_get().is_some_and(|t| t.restricted_storage) => t
                .get()
                .variants
                .iter()
                .flat_map(|v| &v.fields)
                .find_map(Self::storage_barrier),
            _ => None,
        }
    }
    /// The first type a `std.actor` node cannot hold, and the class or enum
    /// that holds it. The spawn hook reads the parent's node through a shared
    /// borrow while the child's heap is current, so the node may hold only
    /// values that such a borrow cannot alias into, or change from, another
    /// heap. A `str` copy shares its storage; shared owners, atomics (and so
    /// `Mutex`), channels, tokens, closures and native handles share or
    /// change state through `&`. `std.bytes.Bytes` has no such method, and
    /// `std.tls` configurations are immutable and allocated outside every
    /// heap. A `std.tls.Session` changes only through `&mut` and only reads
    /// its own state through `&`. See book/src/design/stdlib/actor.md.
    pub fn actor_barrier(&self) -> Option<(Ty, Option<Ty>)> {
        let mut work: Vec<(Ty, Option<Ty>)> = vec![(self.clone(), None)];
        let mut seen = crate::hash::HashSet::default();
        while let Some((ty, holder)) = work.pop() {
            match &ty {
                _ if ty.is_numeric() => {}
                Self::Bool | Self::Unit | Self::ByteStr | Self::File | Self::Callable(_) => {}
                Self::Range(t) | Self::List(t) | Self::Clist(t) | Self::Set(t) | Self::Box(t) => {
                    work.push(((**t).clone(), holder));
                }
                Self::Dict(k, v) => {
                    work.push(((**k).clone(), holder.clone()));
                    work.push(((**v).clone(), holder));
                }
                Self::Class(t)
                    if matches!(
                        &*t.name,
                        "std.bytes.Bytes"
                            | "std.tls.ClientConfig"
                            | "std.tls.ServerConfig"
                            | "std.tls.Session"
                    ) => {}
                Self::Class(t) => {
                    if seen.insert(t.name.clone()) {
                        let fields = t
                            .get()
                            .fields
                            .iter()
                            .map(|(_, f)| f.clone())
                            .collect::<Vec<_>>();
                        work.extend(fields.into_iter().map(|f| (f, Some(ty.clone()))));
                    }
                }
                Self::Enum(t) => {
                    if seen.insert(t.name.clone()) {
                        let fields = t
                            .get()
                            .variants
                            .iter()
                            .flat_map(|v| v.fields.iter().cloned())
                            .collect::<Vec<_>>();
                        work.extend(fields.into_iter().map(|f| (f, Some(ty.clone()))));
                    }
                }
                _ => return Some((ty.clone(), holder)),
            }
        }
        None
    }
    pub fn contains_generator_frame(&self) -> bool {
        self.find_generator_frame(true)
    }
    /// Formatting renders a closure as an opaque marker, never its captures.
    pub fn prints_generator_frame(&self) -> bool {
        self.find_generator_frame(false)
    }
    fn find_generator_frame(&self, inspect_captures: bool) -> bool {
        let mut work = vec![self];
        let mut seen = crate::hash::HashSet::default();
        while let Some(ty) = work.pop() {
            match ty {
                Self::Generator(_) => return true,
                Self::Enum(t) if t.get().restricted_storage && seen.insert((0, &*t.name)) => {
                    work.extend(t.local().variants.iter().flat_map(|v| &v.fields));
                }
                Self::Closure(t) if inspect_captures && seen.insert((1, t.name.as_str())) => {
                    work.extend(t.captures.iter().map(|(_, t)| t));
                }
                _ => {}
            }
        }
        false
    }
    /// The first value kind that keeps `str.repr` from formatting this type,
    /// looking through enums, tuples, options and results.
    pub fn format_blocker(&self) -> Option<FormatBlocker> {
        let mut work = vec![self];
        let mut seen = crate::hash::HashSet::default();
        while let Some(ty) = work.pop() {
            match ty {
                Self::Generator(_) => return Some(FormatBlocker::Generator),
                Self::Closure(_) => return Some(FormatBlocker::Closure),
                Self::Task(_) => return Some(FormatBlocker::Task),
                // Only a reference-carrying Option or Result reaches a
                // reference here; formatting reads through it to a value
                // held wholly in its slot.
                Self::Ref(inner, _) if inner.inline_bytes() == 0 && !inner.has_inline_range() => {
                    work.push(inner)
                }
                Self::Ref(..) => return Some(FormatBlocker::Reference),
                Self::Enum(t) if t.get().restricted_storage && seen.insert(&*t.name) => {
                    work.extend(
                        t.local()
                            .variants
                            .iter()
                            .rev()
                            .flat_map(|v| v.fields.iter().rev()),
                    );
                }
                _ => {}
            }
        }
        None
    }
    pub fn contains_reference(&self) -> bool {
        // Other composite references are rejected at their construction boundary.
        matches!(self, Self::Ref(..) | Self::KeyView(_) | Self::View(..))
            || matches!(self, Self::Closure(t) if t.borrowed)
            || self.carried_reference().is_some()
    }
    /// The `&T` or `&mut T` that a reference-carrying `Option` or `Result`
    /// holds in its success payload, directly or through further
    /// `Option`/`Result` success payloads. Error payloads never carry one.
    /// See book/src/design/language/05-ownership/05-optional-references.md.
    pub fn carried_reference(&self) -> Option<Ty> {
        let Self::Enum(t) = self else {
            return None;
        };
        if !t.propagatable() || !t.try_get().is_some_and(|t| t.restricted_storage) {
            return None;
        }
        match &t.get().variants[usize::from(t.is_option())].fields[0] {
            reference @ Self::Ref(..) => Some(reference.clone()),
            success => success.carried_reference(),
        }
    }
    /// Whether a value of this type holds a loan: a reference, or an
    /// `Option`/`Result` carrying one.
    pub fn carries_loan(&self) -> bool {
        matches!(self, Self::Ref(..)) || self.carried_reference().is_some()
    }
    /// Whether the direct or carried reference is exclusive.
    pub fn carries_mutable_loan(&self) -> bool {
        match self {
            Self::Ref(_, mutable) => *mutable,
            ty => matches!(ty.carried_reference(), Some(Self::Ref(_, true))),
        }
    }
    pub fn key_view(&self) -> bool {
        match self {
            Self::KeyView(_) => true,
            Self::Ref(t, _) => t.key_view(),
            _ => false,
        }
    }
    pub fn can_copy(&self) -> bool {
        self.facts().copyable
    }
    pub fn has_destructor(&self) -> bool {
        self.facts().destructor
    }
    /// Values requiring cleanup have one owner per operand/local. This includes
    /// inline generators as well as heap-backed values; scalars copy as bits.
    pub fn managed(&self) -> bool {
        match self {
            // Inline values need cleanup only when a payload or field does.
            Self::Enum(t) => {
                return t.try_get().is_none_or(|definition| {
                    definition.managed
                        && *definition.managed_fields.get_or_init(|| {
                            definition
                                .variants
                                .iter()
                                .flat_map(|v| &v.fields)
                                .any(Ty::managed)
                        })
                });
            }
            // The runtime's environment retain and release only visit the
            // captures, so an environment whose captures need no cleanup needs
            // none itself (issue #384). A closure without a concrete producer
            // stays conservatively managed.
            Self::Closure(t) if !t.name.is_empty() => {
                return t.captures.iter().any(|(_, t)| t.managed());
            }
            // An undefined forward reference is conservatively managed.
            Self::Class(t) => {
                return t.try_get().is_none_or(|definition| {
                    *definition.managed.get_or_init(|| {
                        definition.destructor.is_some()
                            || definition.fields.iter().any(|(_, t)| t.managed())
                    })
                });
            }
            _ => {}
        }
        matches!(
            self,
            Self::Str
                | Self::Closure(_)
                | Self::BoxedCallable(_)
                | Self::File
                | Self::FsReadDir
                | Self::FsDirEntry
                | Self::FsMetadata
                | Self::ByteStorage
                | Self::Channel(..)
                | Self::Clist(_)
                | Self::CancellationToken
                | Self::Executor
                | Self::Future(_)
                | Self::List(_)
                | Self::Set(_)
                | Self::Dict(_, _)
                | Self::Box(_)
                | Self::Shared(_)
                | Self::Class(_)
                | Self::Generator(_)
        )
    }
    /// True for the eight explicit-width integer types.
    pub fn is_int(&self) -> bool {
        matches!(
            self,
            Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64
        )
    }

    /// The half-open range `[min, max+1)` of `i128` values that fit in
    /// this integer type, or `None` for non-integer types. Used to check
    /// that pattern literals (parsed as `i64`) fit the scrutinee's type
    /// at compile time, before the runtime narrowing of `pattern_matches`.
    pub fn int_range(&self) -> Option<(i128, i128)> {
        let r = match self {
            Ty::I8 => (i8::MIN as i128, i8::MAX as i128 + 1),
            Ty::I16 => (i16::MIN as i128, i16::MAX as i128 + 1),
            Ty::I32 => (i32::MIN as i128, i32::MAX as i128 + 1),
            Ty::I64 => (i64::MIN as i128, i64::MAX as i128 + 1),
            Ty::U8 => (0, u8::MAX as i128 + 1),
            Ty::U16 => (0, u16::MAX as i128 + 1),
            Ty::U32 => (0, u32::MAX as i128 + 1),
            Ty::U64 => (0, u64::MAX as i128 + 1),
            _ => return None,
        };
        Some(r)
    }
}

/// Byte budget for the identity names that make up one concrete type name.
pub const MAX_IDENTITY_NAME_LEN: usize = 16_384;

impl Ty {
    /// Whether the members' identity names together exceed
    /// [`MAX_IDENTITY_NAME_LEN`]. Callers choose the diagnostic, since some
    /// pair this with a layout-depth check under a nesting message.
    pub fn identity_names_exceed_limit(members: &[Ty]) -> bool {
        members
            .iter()
            .map(|t| t.identity_name().len())
            .sum::<usize>()
            > MAX_IDENTITY_NAME_LEN
    }

    /// Stable compiler spelling for names derived from concrete types. Display
    /// may describe origins shared by distinct specialized environments.
    pub fn identity_name(&self) -> String {
        match self {
            Ty::Closure(t) => {
                let signature = t.signature.identity_name().replacen(
                    "Callable",
                    if t.once { "OnceClosure" } else { "Closure" },
                    1,
                );
                if t.name.is_empty() {
                    signature
                } else {
                    format!("{signature} from `{}`", t.name)
                }
            }
            Ty::Callable(t) => t.identity_name(),
            Ty::BoxedCallable(t) => t.identity_name().replacen("Callable", "BoxedCallable", 1),
            Ty::Enum(t) => t.name.to_string(),
            Ty::Class(t) => t.name.to_string(),
            Ty::List(t) => format!("list[{}]", t.identity_name()),
            Ty::Clist(t) => format!("clist[{}]", t.identity_name()),
            Ty::Set(t) => format!("set[{}]", t.identity_name()),
            Ty::Dict(k, v) => format!("dict[{}, {}]", k.identity_name(), v.identity_name()),
            Ty::Box(t) => format!("Box[{}]", t.identity_name()),
            Ty::Shared(t) => format!("Shared[{}]", t.identity_name()),
            Ty::Future(t) => format!("Future[{}]", t.identity_name()),
            Ty::Atomic(t) => format!("Atomic[{}]", t.identity_name()),
            Ty::Channel(t, sender) => format!(
                "{}[{}]",
                if *sender { "Sender" } else { "Receiver" },
                t.identity_name()
            ),
            Ty::Ref(t, mutable) => format!(
                "&{}{}",
                if *mutable { "mut " } else { "" },
                t.identity_name()
            ),
            Ty::Range(t) if **t != Ty::I64 => format!("range[{}]", t.identity_name()),
            Ty::Generator(t) => {
                let mut name = format!("Generator[{}]", t.element.identity_name());
                if let Some(producer) = &t.name {
                    name.push_str(&format!(" from `{producer}`"));
                }
                name
            }
            _ => self.to_string(),
        }
    }
}

/// A value kind that cannot be formatted, for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatBlocker {
    Generator,
    Closure,
    Task,
    Reference,
}

impl FormatBlocker {
    pub fn plural(self) -> &'static str {
        match self {
            Self::Generator => "generators",
            Self::Closure => "closures",
            Self::Task => "scoped tasks",
            Self::Reference => "references",
        }
    }
    pub fn singular(self) -> &'static str {
        match self {
            Self::Generator => "a generator",
            Self::Closure => "a closure",
            Self::Task => "a scoped task",
            Self::Reference => "a reference",
        }
    }
    /// The diagnostic for formatting a value of type `ty` whose first
    /// unformattable part is this kind.
    pub fn message(self, ty: &Ty) -> String {
        let direct = matches!(
            (self, ty),
            (Self::Generator, Ty::Generator(_))
                | (Self::Closure, Ty::Closure(_))
                | (Self::Task, Ty::Task(_))
                | (Self::Reference, Ty::Ref(..))
        );
        if direct {
            let advice = match self {
                Self::Closure => "; call the closure and format its result",
                _ => "",
            };
            format!("{} cannot be formatted{advice}", self.plural())
        } else {
            format!(
                "`{ty}` holds {}, and {} cannot be formatted; match the value and format its parts instead",
                self.singular(),
                self.plural()
            )
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Ty::Task(_) => "scoped task",
            Ty::Executor => "ThreadPoolExecutor",
            Ty::CancellationToken => "CancellationToken",
            Ty::Atomic(t) => return write!(f, "Atomic[{t}]"),
            Ty::Future(t) => return write!(f, "Future[{t}]"),
            Ty::Channel(t, sender) => {
                return write!(f, "{}[{t}]", if *sender { "Sender" } else { "Receiver" });
            }
            Ty::Closure(t) => {
                let signature = t.signature.to_string().replacen(
                    "Callable",
                    if t.once { "OnceClosure" } else { "Closure" },
                    1,
                );
                return if let Some(location) = &t.location {
                    write!(f, "{signature} defined at {location}")
                } else {
                    f.write_str(&signature)
                };
            }
            Ty::Callable(sig) => return write!(f, "{sig}"),
            Ty::BoxedCallable(sig) => {
                return f.write_str(&sig.to_string().replacen("Callable", "BoxedCallable", 1));
            }
            Ty::I8 => "i8",
            Ty::I16 => "i16",
            Ty::I32 => "i32",
            Ty::I64 => "i64",
            Ty::U8 => "u8",
            Ty::U16 => "u16",
            Ty::U32 => "u32",
            Ty::U64 => "u64",
            Ty::F32 => "f32",
            Ty::F64 => "f64",
            Ty::Unit => "()",
            Ty::Str => "str",
            Ty::ByteStr => "bytes",
            Ty::File => "File",
            Ty::FsReadDir => "FsReadDir",
            Ty::FsDirEntry => "FsDirEntry",
            Ty::FsMetadata => "FsMetadata",
            Ty::ByteStorage => "ByteStorage",
            Ty::ForeignPtr(name) => return f.write_str(name),
            Ty::Bool => "bool",
            Ty::Iter(t, _) => return write!(f, "Iter[{t}]"),
            Ty::List(t) => return write!(f, "list[{t}]"),
            Ty::Set(t) => return write!(f, "set[{t}]"),
            Ty::KeyView(t) => return write!(f, "KeyView[{t}]"),
            Ty::View(t, false) => return write!(f, "&[{t}]"),
            Ty::View(t, true) => return write!(f, "&mut [{t}]"),
            Ty::Clist(t) => return write!(f, "clist[{t}]"),
            Ty::Dict(k, v) => return write!(f, "dict[{k}, {v}]"),
            Ty::Box(t) => return write!(f, "Box[{t}]"),
            Ty::Shared(t) => return write!(f, "Shared[{t}]"),
            Ty::Range(t) => {
                return if **t == Ty::I64 {
                    f.write_str("range")
                } else {
                    write!(f, "range[{t}]")
                };
            }
            Ty::Enum(t) => return f.write_str(t.display_name()),
            Ty::Class(t) => return f.write_str(t.display_name()),
            Ty::Generator(t) => {
                write!(f, "Generator[{}]", t.element)?;
                if let Some(name) = &t.name {
                    write!(f, " from `{name}`")?;
                }
                return Ok(());
            }
            Ty::Ref(t, mutable) => return write!(f, "&{}{t}", if *mutable { "mut " } else { "" }),
        })
    }
}

/// The concrete type of a sized integer literal.
impl From<Value> for Ty {
    fn from(v: Value) -> Ty {
        match v {
            Value::I8(_) => Ty::I8,
            Value::I16(_) => Ty::I16,
            Value::I32(_) => Ty::I32,
            Value::I64(_) => Ty::I64,
            Value::U8(_) => Ty::U8,
            Value::U16(_) => Ty::U16,
            Value::U32(_) => Ty::U32,
            Value::U64(_) => Ty::U64,
        }
    }
}

/// A function's stack-effect signature: what it consumes and what it leaves.
///
/// Inputs are `(name, type)` pairs because the names matter — the body refers
/// to them as locals (§11.5). Outputs are bare types because there is nothing
/// for an output name to bind to; users may *write* output names for
/// documentation (the parser accepts them) but they are discarded here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FnSig {
    pub inputs: Vec<(String, Ty)>,
    pub outputs: Vec<Ty>,
}

/// Structural signature of a capture-free Plenty function value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CallableSig {
    pub inputs: Vec<Ty>,
    pub output: Option<Ty>,
}
impl CallableSig {
    fn identity_name(&self) -> String {
        format!(
            "Callable[[{}], {}]",
            self.inputs
                .iter()
                .map(Ty::identity_name)
                .collect::<Vec<_>>()
                .join(", "),
            self.output
                .as_ref()
                .map(Ty::identity_name)
                .unwrap_or_else(|| "()".into())
        )
    }
    pub fn from_function(sig: &FnSig) -> Self {
        Self {
            inputs: sig.inputs.iter().map(|(_, ty)| ty.clone()).collect(),
            output: sig.outputs.first().cloned(),
        }
    }
    pub fn function(&self) -> FnSig {
        FnSig {
            inputs: self
                .inputs
                .iter()
                .enumerate()
                .map(|(i, ty)| (format!("arg{i}"), ty.clone()))
                .collect(),
            outputs: self.output.iter().cloned().collect(),
        }
    }
}
impl fmt::Display for CallableSig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Callable[[{}], {}]",
            self.inputs
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            self.output
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "()".into())
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegerOp {
    And,
    Or,
    Xor,
    Not,
    ShiftLeft,
    ShiftRight,
    WrappingAdd,
    WrappingSub,
    WrappingMul,
    RotateLeft,
    RotateRight,
}

impl IntegerOp {
    pub(crate) fn binary(operator: &str) -> Option<Self> {
        Some(match operator {
            "&" => Self::And,
            "|" => Self::Or,
            "^" => Self::Xor,
            "<<" => Self::ShiftLeft,
            ">>" => Self::ShiftRight,
            _ => return None,
        })
    }
}

/// Endpoint-holding slots that a statement's expression moves into a value
/// it is still building. Waits inside the expression, such as a nested
/// `result()`, and the pool shutdowns and joins a `match` case's cleanup runs
/// before its value is stored, happen while that value holds them.
#[derive(Clone, Debug, PartialEq)]
pub enum Building {
    /// The start of the expression, its position, and the slots it moves.
    Open(crate::ownership::Site, Arc<[u8]>),
    /// The slots that the final expression of a `match` case used as a
    /// value moves; that value stays in flight until the statement's `Close`.
    Hold(crate::ownership::Site, Arc<[u8]>),
    /// The end of the statement, after the value is stored or dropped.
    Close,
}

/// Cancellation-token facts for the skipped-cancel check (issue #355).
#[derive(Clone, Debug, PartialEq)]
pub enum TokenMark {
    /// Token slots, or references to them, that the arguments of the
    /// `spawn` or `submit` that follows share, borrow, or move.
    Holds(Arc<[u8]>),
    /// The slot just stored holds the share of a `cancel_on_exit()` guard,
    /// whose `with` exit cancels it.
    Guard(u8),
}

/// The two ends of the arguments of a call whose result can hold a future.
/// Both carry an identity that is unique within the function.
#[derive(Clone, Debug, PartialEq)]
pub enum Forward {
    /// Before the first argument. Present only when an argument can hold a
    /// future.
    Arguments(u32),
    /// After the last argument, before the call. `results` are the result
    /// types `T` of the `Future[T]` values the call's result can hold.
    Call(u32, Arc<[Ty]>),
}

/// A typed operation lowered into native code.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Thread(crate::threading::ThreadOp),
    Channel(crate::channel::ChannelOp),
    Control(crate::control::ControlOp),
    Atomic(crate::atomic::AtomicOp),
    Mutex(crate::mutex::MutexOp),
    Executor(crate::executor::ExecutorOp),
    ClosureNew(Arc<crate::closure::ClosureType>),
    ClosureCall(Arc<crate::closure::ClosureType>),
    BoxedCallableNew(Box<Ty>, String, Arc<CallableSig>),
    BoxedCallableCall(Arc<CallableSig>),
    FunctionAddress(String, Arc<CallableSig>),
    CallIndirect(Arc<CallableSig>),
    TailCallIndirect(Arc<CallableSig>),
    /// Unwrap a standard sum or return its residual, releasing pending operands.
    Try {
        source: Box<crate::nominal::Nominal<crate::sum::EnumType>>,
        target: Box<crate::nominal::Nominal<crate::sum::EnumType>>,
        cleanup: Arc<[Op]>,
    },
    Class(crate::record::ClassOp),
    BorrowLocal(u8, bool),
    ReadRef(Ty),
    Reborrow(Ty),
    WriteRef(Ty),
    /// `WriteRef` into storage known to be empty, such as a field `__init__`
    /// assigns for the first time: there is no old owner to release.
    FillRef(Ty),
    /// Exchange initialized storage with a new owner, returning its old owner.
    ReplaceRef(Ty),
    Loan(crate::ownership::Loan),
    UseLoan(usize),
    /// A `mut` reference binding is retargeted here: the loan it holds ends,
    /// and later uses of the binding hold it again for the new target.
    EndLoan(usize),
    Access(u8, bool, Option<usize>),
    /// Source position of the operations that follow; diagnostics only.
    Site(crate::ownership::Site),
    /// Call expression position after evaluating arguments; runtime caller
    /// attribution only, without changing ownership diagnostic positions.
    CallerSite(crate::ownership::Site),
    /// Endpoint-holding slots that the value of the following `return`, or
    /// of the function's final expression, moves, and that statement's
    /// position. That value leaves the frame only after the exit's joins and
    /// pool shutdowns; ownership checks only.
    Returning(crate::ownership::Site, Arc<[u8]>),
    /// The operations from here to the next `Return`, `Break` or `Continue`
    /// of this sequence are the scope cleanup of that exit, with an empty
    /// operand stack at each `DropLocal`, then a `Continue`'s loop step. Code
    /// generation may share them between exits (issues #417 and #419); every
    /// other pass reads them in place.
    Leaving,
    /// A statement-level value under construction; ownership checks only.
    Building(Building),
    /// Future and pool slots that the arguments of the call that follows
    /// move or borrow. The callee may wait for such a future's result or shut
    /// such a pool down (issue #349); ownership checks only.
    Passes(Arc<[u8]>),
    /// Brackets the arguments of a call whose result can hold a future, so
    /// that future keeps the jobs of the futures it can be handed (issue
    /// #352); ownership checks only.
    Forwards(Forward),
    /// Cancellation-token provenance; ownership checks only.
    Tokens(TokenMark),
    Yield(Ty),
    Next(u8),
    MoveLocal(u8),
    DropLocal(u8),
    /// Write the empty value, which owns nothing, to a local. The ownership
    /// check treats the slot as initialized; the frontend emits this only
    /// for plan state that is read under a flag saying it holds a value,
    /// such as a `flat_map` inner source between outer items.
    EmptyLocal(u8),
    /// A field of an owned local moves out or is assigned; ownership checks
    /// only. The move itself is a `ReplaceRef` that leaves the field empty.
    FieldMove(crate::ownership::FieldMove),
    Enum(crate::sum::EnumOp),
    Box(crate::boxed::BoxOp),
    /// Move every field out of a multi-field variant or tuple, in field order.
    Split(crate::nominal::Nominal<crate::sum::EnumType>, usize),
    /// Move every field out of a class without `__del__` and at least two
    /// fields, in field order; a class pattern in a consuming `match`.
    SplitClass(crate::nominal::Nominal<crate::record::ClassType>),
    /// Defensive trap after an exhaustive finite-domain match.
    Unreachable,
    Collection(crate::collection::CollectionOp),
    Numeric(crate::numeric::NumericOp),
    /// A `checked:` block. The body leaves its value `T` on top of the stack,
    /// which becomes `Ok(T)` of `result`, a `Result[T, E]`. A `CheckedArith`
    /// in the body that fails exits here instead, with `Err`.
    CheckedBlock {
        body: Arc<[Op]>,
        result: Ty,
    },
    /// Pop `T T`; push the result of `operation`. On overflow or a zero
    /// divisor, release the operands pending above the innermost
    /// `CheckedBlock`, run `cleanup` (the block's locals) and exit that block
    /// with `ZeroDivisionError` or `OverflowError[T]`, whose `wrapped` field
    /// is the result modulo 2**N. Unary minus is `0 - x`.
    CheckedArith {
        operation: crate::numeric::Checked,
        ty: Ty,
        cleanup: Arc<[Op]>,
    },
    /// The condition leaves bool; a continuing body preserves the operand stack.
    Loop {
        condition: Arc<[Op]>,
        body: Arc<[Op]>,
    },
    /// An `if` statement. Pops the bool left by the first condition; a later
    /// condition runs only when every earlier one was false. The `elif` arms
    /// are a list rather than nested `otherwise` bodies, so that no pass
    /// recurses once per arm (issue #70).
    If {
        then: Arc<[Op]>,
        elifs: Arc<[ElifArm]>,
        otherwise: Arc<[Op]>,
    },
    /// Push an integer literal onto the stack. The payload is always an
    /// integer `Value`; retaining its width makes suffixed literals direct.
    PushInt(Value),
    PushFloat {
        bits: u64,
        ty: Ty,
    },
    PushUnit,
    PipelineToken(Arc<Ty>, usize),
    FloatNeg,
    /// Push a string literal — already stored in the heap — onto the stack.
    PushStr(StrId),
    /// Push a byte literal, stored in the heap's literal pool, as `bytes`.
    PushBytes(StrId),
    /// Push a `Bool` literal onto the stack (`true` / `false`).
    PushBool(bool),
    /// Pop two values; push their sum (integers) or concatenation (text).
    Add,
    /// Pop two same-typed numbers `a b`; push `a - b`.
    Sub,
    /// Pop two same-typed numbers `a b`; push `a * b`.
    Mul,
    /// Divide same-typed numbers (legacy integer division truncates).
    Div,
    /// Integer division rounded toward negative infinity (modern `//`).
    FloorDiv,
    Modulo,
    /// Integer operations that never use implicit overflow checking.
    Integer(IntegerOp),
    /// Pop two values; push `true` if they are equal, `false` otherwise.
    /// Polymorphic over Int/Str/Bool (§11.8); mixed-type pairs are rejected
    /// by the type checker, never reached at runtime by a compiled source.
    Eq,
    /// Pop two same-typed numbers `a b`; push `a < b`.
    Lt,
    /// Pop two same-typed numbers `a b`; push `a > b`.
    Gt,
    /// Pop a `Bool`; push its negation.
    Not,
    /// Pop two same-typed values; push whether they differ.
    Ne,
    /// Pop two same-typed numbers; push whether the first is at most the second.
    Le,
    /// Pop two same-typed numbers; push whether the first is at least the second.
    Ge,
    /// Pop two `Bool`s; push their strict conjunction.
    And,
    /// Pop two `Bool`s; push their strict disjunction.
    Or,
    /// Remove the top value, whatever its type.
    Drop,
    /// Copy the top value, whatever its type.
    Dup,
    /// Exchange the top two values, whatever their types.
    Swap,
    /// Print the whole stack — the `.` word.
    Display,
    /// Discard every value on the stack.
    Clear,
    /// Define a function: bind `name` to an already-compiled body and docstring.
    ///
    /// This declaration emits a native function without affecting operand values.
    DefineFn(String, Box<CompiledFn>),
    /// Invoke a user-defined function by name. Non-tail position.
    Call(String),
    ForeignCall {
        declaration: crate::foreign::Declaration,
        sig: Arc<FnSig>,
    },
    ForeignNull(Ty),
    /// Invoke a user-defined function by name from tail position (§11.8).
    /// Native lowering reuses the caller's frame. Emitted by tail-call marking.
    TailCall(String),
    /// Lifetime fact about the call op that immediately follows: it passes
    /// references, and each one refers to storage borrowed through a reference
    /// parameter of the enclosing function, so none points into this frame.
    /// A tail call may release the frame before transferring only with this
    /// fact; any other call passing a reference keeps the frame until it
    /// returns. The frontend states it per call from loan origins; it is never
    /// derived from types.
    ForwardsReferences,
    /// Leave the current function with its declared results on the stack.
    /// Unlike the end of a match arm, this exits the entire call frame.
    Return,
    /// Transfer control to the innermost loop's exit or condition.
    Break,
    Continue,
    /// Push the value of the `i`-th input local of the enclosing call's frame
    /// Only emitted inside function bodies.
    LoadLocal(u8),
    /// Pop a value into an already allocated, statically typed local slot.
    StoreLocal(u8),
    /// Pop the top of the stack and dispatch on it (§11.8). The first arm
    /// whose pattern matches runs; the value itself is *consumed* by the
    /// match. Exhaustiveness has been checked at compile time, so on a
    /// well-formed source the search always finds a match.
    Match(Arc<[MatchArm]>),
    /// Pop an integer of any width; push its representation at the target
    /// integer width. Surface syntax is `:as-i8` ... `:as-u64`. Conversion
    /// follows Rust's `as` semantics: widening sign-extends signed sources
    /// and zero-extends unsigned ones; narrowing truncates; equal-width
    /// signedness change reinterprets the bit pattern. Casts that would
    /// silently change a value's mathematical meaning are still allowed —
    /// that is the whole point of an explicit cast word. `T.wrapping_from`
    /// lowers to this op; the bare cast `T(x)` lowers to `CheckedCast`.
    Cast(Ty),
    /// Pop a number; push it converted to the target numeric type. A value
    /// the target integer type cannot represent traps: an out-of-range
    /// integer, or a float that is NaN or out of range once truncated toward
    /// zero. Conversions to a float type, and integer conversions that
    /// preserve every source value, emit no check.
    CheckedCast(Ty),
    /// Pop an integer of any width; push `Result[target, RangeError]`: `Ok`
    /// with the value at the target integer width when it is in range, and
    /// `Err(RangeError.OutOfRange)` otherwise. Unlike `Cast`, it never
    /// changes the value's mathematical meaning.
    TryCast(Ty),
    /// Pop `start stop step` (two integers of the given type and the nonzero
    /// `i64` step); push the `i64` element count of `range(start, stop, step)`.
    /// A count above `i64::MAX` fails with the same message as constructing
    /// the range value. Counted `for` loops over a `range`
    /// call use this instead of a range value (issue #121).
    RangeLength(Ty),
    /// Pop a `range[T]` value; push its first element (`T`), its `i64` step,
    /// and its `i64` element count. The range was validated when it was
    /// built, so this never fails. `for` loops over a stored range use this
    /// to run the same counted loop as a direct `range` call (issue #371).
    RangeParts(Ty),
    /// Read one newline-terminated line from stdin into the heap and push
    /// (line, got-line?). On EOF, line is the empty string and the Bool
    /// is `false`; the user is expected to discriminate via `match` on
    /// the Bool. The trailing `\n` (and `\r\n`) is stripped. Pending sum
    /// types (§12.14), this two-output shape is the cleanest way to
    /// report success-or-EOF on a stack-based surface.
    ReadLine,
    /// Pop two strings `haystack needle`; push `true` if `needle` is a
    /// substring of `haystack`, `false` otherwise. Byte-level match
    /// using `strstr` semantics in the native runtime.
    Contains,
    /// Pop one string; write its bytes to stdout followed by a `\n`.
    /// This is the bare-text output primitive; `.` remains the stack
    /// introspection word.
    PrintLn,
    /// Pop one value of any type and render it without a newline. The
    /// rendering matches one entry in the `.` stack display.
    Print,
}

/// One `elif` of an [`Op::If`]. `condition` adds a bool to the stack it found.
#[derive(Clone, Debug, PartialEq)]
pub struct ElifArm {
    pub condition: Arc<[Op]>,
    pub body: Arc<[Op]>,
}

/// One arm of a [`Op::Match`]. The pattern is matched against the popped
/// value; if the match succeeds, `body` is executed against the current
/// data stack and the enclosing call's locals frame.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Arc<[Op]>,
}

/// What a match-arm pattern can be. Today: typed literals plus the wildcard.
/// Sum-type patterns with payload binders are designed (§11.8) but deferred
/// until sum types themselves land (§12.14).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pattern {
    /// An integer pattern. Unsuffixed literals retain the historic `i64`
    /// spelling and may match a narrower scrutinee when they fit; a suffixed
    /// literal must have exactly the scrutinee's type.
    Int {
        value: Value,
        explicit_ty: bool,
    },
    Str(StrId),
    Bool(bool),
    Wildcard,
}

/// A compiled function: the signature (§11.2), the docstring (§11.7), and
/// the body.
///
/// Shared fields avoid copying bodies and signatures during compiler passes.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledFn {
    /// Source location for diagnostics from the independent IR/ownership checker.
    pub location: Option<Arc<str>>,
    /// Source file and binding names for ownership diagnostics.
    pub origins: Arc<crate::ownership::Origins>,
    pub generator: Option<Ty>,
    pub sig: Arc<FnSig>,
    pub doc: Arc<str>,
    pub body: Arc<[Op]>,
    /// Types of local slots after the parameters. Allocated once per call.
    pub locals: Arc<[Ty]>,
}

/// Compile lexed words into ops, interning string literals into `heap`.
///
/// This is the only path from `Tok` to `Op`. It is used both for top-level
/// source and, recursively, for function bodies, so it depends on nothing but
/// the `Heap`.
pub fn compile(toks: &[Tok], heap: &mut Heap) -> Result<Vec<Op>> {
    Compiler {
        toks,
        pos: 0,
        heap,
        local_scopes: Vec::new(),
    }
    .compile_seq(Stop::EndOfInput)
}

/// What ends the run of tokens a [`Compiler::compile_seq`] call is reading.
#[derive(Clone, Copy, PartialEq)]
enum Stop {
    /// The top level: stop at end of input; a `;`, `]`, or `end` here is an error.
    EndOfInput,
    /// A function body: stop at — and consume — the matching `;`.
    Semicolon,
    /// A match-arm body: stop at — and consume — the matching `]`.
    CloseBracket,
}

/// A cursor over a token slice that compiles it to ops.
///
/// Bundled into a struct because the four things — the tokens, the position
/// within them, the heap that literals are interned into, and the stack of
/// enclosing functions' input-name lists — all travel together through the
/// recursion that handles nested `: ... ;` definitions and `match ... end`
/// dispatches.
///
/// `local_scopes` is a stack only so that nested definitions can push and pop
/// cleanly; per §11.5, **only the innermost (topmost) scope is visible** at
/// any point. Outer scopes are inaccessible by design: nested functions do
/// not see their enclosing function's locals. Match-arm bodies do *not* push
/// a new scope — they share their enclosing function's locals (§11.8).
struct Compiler<'t, 'src> {
    toks: &'t [Tok<'src>],
    pos: usize,
    heap: &'t mut Heap,
    local_scopes: Vec<Vec<String>>,
}

impl Compiler<'_, '_> {
    /// Compile tokens from the current position until `stop` is reached,
    /// consuming the terminating delimiter where there is one.
    fn compile_seq(&mut self, stop: Stop) -> Result<Vec<Op>> {
        let mut ops = Vec::new();
        while let Some(tok) = self.toks.get(self.pos).cloned() {
            self.pos += 1;
            match tok {
                Tok::Word(";") if stop == Stop::Semicolon => return Ok(ops),
                Tok::Word(";") => return Err("';' has no matching ':'".into()),
                Tok::Word("]") if stop == Stop::CloseBracket => return Ok(ops),
                Tok::Word("]") => return Err("']' has no matching '['".into()),
                Tok::Word("[") => return Err("'[' is only valid inside a `match` arm".into()),
                Tok::Word("end") => return Err("`end` has no matching `match`".into()),
                Tok::Word("match") => ops.push(self.compile_match()?),
                Tok::Word(":") => ops.push(self.compile_definition()?),
                Tok::Word(w) => match self.lookup_local(w) {
                    Some(ix) => ops.push(Op::LoadLocal(ix)),
                    None => {
                        let op = compile_word(w, self.heap)?;
                        if !self.local_scopes.is_empty() && matches!(op, Op::PushStr(_)) {
                            return Err(format!(
                                "unknown word `{w}` in a function body; quote text as \"{w}\""
                            )
                            .into());
                        }
                        ops.push(op);
                    }
                },
                Tok::Text(s) => ops.push(Op::PushStr(self.heap.add_str(unescape(s)?))),
            }
        }
        match stop {
            Stop::Semicolon => Err("':' has no matching ';'".into()),
            Stop::CloseBracket => Err("'[' has no matching ']'".into()),
            Stop::EndOfInput => Ok(ops),
        }
    }

    /// If `name` is one of the enclosing function's input names, return its
    /// index. Only the innermost (topmost) scope is consulted — nested
    /// definitions deliberately do not inherit outer locals (§11.5).
    fn lookup_local(&self, name: &str) -> Option<u8> {
        let scope = self.local_scopes.last()?;
        scope.iter().position(|n| n == name).map(|i| i as u8)
    }

    /// Compile a `: name { sig } "doc" body... ;` definition. The opening `:`
    /// has already been consumed; the cursor sits on the name. A nested `:`
    /// inside the body is handled by the recursive `compile_seq` call, so
    /// definitions nest.
    fn compile_definition(&mut self) -> Result<Op> {
        let name = match self.toks.get(self.pos).cloned() {
            Some(Tok::Word(w)) if w != ":" && w != ";" => w.to_string(),
            Some(Tok::Word(_)) | None => {
                return Err("':' must be followed by a function name".into());
            }
            Some(Tok::Text(_)) => {
                return Err("a function name must be a plain word, not a text literal".into());
            }
        };
        self.pos += 1;
        if is_reserved_function_name(&name) {
            return Err(format!("function name `{name}` is reserved for a builtin word").into());
        }
        let sig: Arc<FnSig> = self.compile_sig(&name)?.into();
        if sig.inputs.len() > u8::MAX as usize {
            return Err(format!(
                "function `{name}` has too many inputs \
                 (max {}, got {})",
                u8::MAX,
                sig.inputs.len()
            )
            .into());
        }
        // A docstring is optional. When present, it must immediately follow
        // the header, so tools can still identify it without parsing a body.
        let doc: Arc<str> = match self.toks.get(self.pos).cloned() {
            Some(Tok::Text(s)) => {
                self.pos += 1;
                unescape(s)?.into()
            }
            Some(_) | None => "".into(),
        };
        // The input names are in scope for the duration of the body. Pushing
        // a fresh scope per definition is what gives nested definitions their
        // own (non-inheriting) frame; pop on every exit, success or error, so
        // the scope stack tracks the lexical structure faithfully.
        let locals: Vec<String> = sig.inputs.iter().map(|(n, _)| n.clone()).collect();
        self.local_scopes.push(locals);
        let body_result = self.compile_seq(Stop::Semicolon);
        self.local_scopes.pop();
        let mut body = body_result?;
        // Tail-call rewrite — §11.8. Done after the body is fully compiled so
        // we can identify "last op in body / last op in last match arm" purely
        // structurally.
        mark_tail_calls(&mut body);
        Ok(Op::DefineFn(
            name,
            Box::new(CompiledFn {
                location: None,
                origins: Arc::default(),
                sig,
                doc,
                generator: None,
                body: body.into(),
                locals: Arc::from([]),
            }),
        ))
    }

    /// Compile a `match PATTERN [ BODY ] PATTERN [ BODY ] ... end` dispatch.
    /// The opening `match` has already been consumed; the cursor sits on the
    /// first pattern (or on `end` for an empty match, which is rejected).
    fn compile_match(&mut self) -> Result<Op> {
        let mut arms: Vec<MatchArm> = Vec::new();
        loop {
            // Pattern or end-of-match.
            let pattern = match self.toks.get(self.pos).cloned() {
                Some(Tok::Word("end")) => {
                    self.pos += 1;
                    break;
                }
                Some(Tok::Word("[")) => {
                    return Err("match arm is missing a pattern before `[`".into());
                }
                Some(Tok::Word(";")) | Some(Tok::Word("]")) | None => {
                    return Err("`match` has no matching `end`".into());
                }
                Some(Tok::Word(w)) => {
                    self.pos += 1;
                    parse_pattern_word(w)?
                }
                Some(Tok::Text(s)) => {
                    self.pos += 1;
                    Pattern::Str(self.heap.add_str(unescape(s)?))
                }
            };
            // Opening bracket — patterns are followed *only* by `[`.
            match self.toks.get(self.pos).cloned() {
                Some(Tok::Word("[")) => self.pos += 1,
                _ => {
                    return Err(
                        "match arm pattern must be followed by `[` to open the arm body".into(),
                    );
                }
            }
            // Body, up to the matching `]`. `compile_seq` consumes the `]`.
            let body = self.compile_seq(Stop::CloseBracket)?;
            arms.push(MatchArm {
                pattern,
                body: body.into(),
            });
        }
        if arms.is_empty() {
            return Err("`match` requires at least one arm".into());
        }
        Ok(Op::Match(arms.into()))
    }

    /// Compile a `{ name Type ... -> Type ... }` header (§11.2).
    ///
    /// Inputs are `name Type` pairs; outputs are either bare `Type`s or
    /// `name Type` pairs (the name is documentation-only and discarded).
    /// The `->` is mandatory; both sides may be empty. `fn_name` is used for
    /// error messages only.
    fn compile_sig(&mut self, fn_name: &str) -> Result<FnSig> {
        match self.toks.get(self.pos).cloned() {
            Some(Tok::Word("{")) => self.pos += 1,
            _ => {
                return Err(format!(
                    "function `{fn_name}` is missing a type header \
                     (expected `{{ ... -> ... }}` after the name)"
                )
                .into());
            }
        }

        let mut inputs = Vec::new();
        loop {
            match self.toks.get(self.pos).cloned() {
                Some(Tok::Word("->")) => {
                    self.pos += 1;
                    break;
                }
                Some(Tok::Word("}")) => {
                    return Err(format!(
                        "function `{fn_name}` type header is missing `->` \
                         (write `{{ -> ... }}` for a function with no inputs)"
                    )
                    .into());
                }
                Some(Tok::Word(w)) if parse_type(w).is_some() => {
                    return Err(format!(
                        "function `{fn_name}` type header: input requires a name \
                         before the type `{w}` (write `{{ x {w} -> ... }}`)"
                    )
                    .into());
                }
                Some(Tok::Word(w)) if !w.is_empty() => {
                    if !is_valid_input_name(w) {
                        return Err(format!(
                            "function `{fn_name}` type header: `{w}` is not a valid input name"
                        )
                        .into());
                    }
                    self.pos += 1;
                    let ty = self.consume_type(fn_name)?;
                    inputs.push((w.to_string(), ty));
                }
                Some(_) | None => {
                    return Err(format!(
                        "function `{fn_name}` type header: unexpected token \
                         while reading inputs"
                    )
                    .into());
                }
            }
        }

        let mut outputs = Vec::new();
        loop {
            match self.toks.get(self.pos).cloned() {
                Some(Tok::Word("}")) => {
                    self.pos += 1;
                    break;
                }
                Some(Tok::Word(w)) if parse_type(w).is_some() => {
                    self.pos += 1;
                    outputs.push(parse_type(w).expect("just checked"));
                }
                Some(Tok::Word(_)) => {
                    // Named output: name, then type. The name is discarded.
                    self.pos += 1;
                    let ty = self.consume_type(fn_name)?;
                    outputs.push(ty);
                }
                Some(_) | None => {
                    return Err(format!(
                        "function `{fn_name}` type header: unexpected token \
                         while reading outputs (or missing `}}`)"
                    )
                    .into());
                }
            }
        }

        Ok(FnSig { inputs, outputs })
    }

    /// Consume one token and require it to name a Plenty type.
    fn consume_type(&mut self, fn_name: &str) -> Result<Ty> {
        match self.toks.get(self.pos).cloned() {
            Some(Tok::Word(w)) => match parse_type(w) {
                Some(ty) => {
                    self.pos += 1;
                    Ok(ty)
                }
                None => Err(format!(
                    "function `{fn_name}` type header: `{w}` is not a known type \
                     (expected one of `i8`..`i64`, `u8`..`u64`, `Str`, `Bool`)"
                )
                .into()),
            },
            _ => Err(format!(
                "function `{fn_name}` type header: expected a type, found end of header"
            )
            .into()),
        }
    }
}

/// Parse a single word as a Plenty type name. Returns `None` for words that
/// are not type names; that lets callers reject them with a context-specific
/// message rather than a generic "not a type" error.
fn parse_type(w: &str) -> Option<Ty> {
    match w {
        "i8" => Some(Ty::I8),
        "i16" => Some(Ty::I16),
        "i32" => Some(Ty::I32),
        "i64" => Some(Ty::I64),
        "u8" => Some(Ty::U8),
        "u16" => Some(Ty::U16),
        "u32" => Some(Ty::U32),
        "u64" => Some(Ty::U64),
        "Str" => Some(Ty::Str),
        "Bool" => Some(Ty::Bool),
        _ => None,
    }
}

/// A parsed integer literal. `explicit_ty` distinguishes `1` (which may
/// match any integer type when in range) from `1i64` (which matches `i64`
/// only). The distinction matters only in match patterns.
#[derive(Clone, Copy)]
struct IntLiteral {
    value: Value,
    explicit_ty: bool,
}

/// Parse an integer literal. Unsuffixed literals are `i64`; suffixed literals
/// use their declared type and must fit it. This makes `255u8` direct while
/// preserving `:as-u8` for explicit narrowing and reinterpretation.
fn parse_integer_literal(word: &str) -> Result<Option<IntLiteral>> {
    if let Ok(n) = word.parse::<i64>() {
        return Ok(Some(IntLiteral {
            value: Value::I64(n),
            explicit_ty: false,
        }));
    }
    // A signed digit string that `i64` rejects is an out-of-range number, not
    // a word; falling through would compile it as text (issue #168).
    let unsigned = word.strip_prefix(['-', '+']).unwrap_or(word);
    if !unsigned.is_empty() && unsigned.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("integer literal `{word}` does not fit i64").into());
    }

    for (suffix, ty) in [
        ("i8", Ty::I8),
        ("i16", Ty::I16),
        ("i32", Ty::I32),
        ("i64", Ty::I64),
        ("u8", Ty::U8),
        ("u16", Ty::U16),
        ("u32", Ty::U32),
        ("u64", Ty::U64),
    ] {
        let Some(digits) = word.strip_suffix(suffix) else {
            continue;
        };
        let starts_numeric =
            digits.starts_with('-') || digits.as_bytes().first().is_some_and(u8::is_ascii_digit);
        if !starts_numeric {
            continue;
        }
        let value = match ty {
            Ty::I8 => digits
                .parse::<i8>()
                .map(Value::I8)
                .map_err(|_| format!("integer literal `{word}` does not fit {ty}"))?,
            Ty::I16 => digits
                .parse::<i16>()
                .map(Value::I16)
                .map_err(|_| format!("integer literal `{word}` does not fit {ty}"))?,
            Ty::I32 => digits
                .parse::<i32>()
                .map(Value::I32)
                .map_err(|_| format!("integer literal `{word}` does not fit {ty}"))?,
            Ty::I64 => digits
                .parse::<i64>()
                .map(Value::I64)
                .map_err(|_| format!("integer literal `{word}` does not fit {ty}"))?,
            Ty::U8 => digits
                .parse::<u8>()
                .map(Value::U8)
                .map_err(|_| format!("integer literal `{word}` does not fit {ty}"))?,
            Ty::U16 => digits
                .parse::<u16>()
                .map(Value::U16)
                .map_err(|_| format!("integer literal `{word}` does not fit {ty}"))?,
            Ty::U32 => digits
                .parse::<u32>()
                .map(Value::U32)
                .map_err(|_| format!("integer literal `{word}` does not fit {ty}"))?,
            Ty::U64 => digits
                .parse::<u64>()
                .map(Value::U64)
                .map_err(|_| format!("integer literal `{word}` does not fit {ty}"))?,
            _ => unreachable!("only integer suffixes are listed"),
        };
        return Ok(Some(IntLiteral {
            value,
            explicit_ty: true,
        }));
    }
    Ok(None)
}

/// Parse a match-arm pattern from a bare word. Numbers parse as `Pattern::Int`,
/// `true`/`false` as `Pattern::Bool`, `_` as `Pattern::Wildcard`. A pattern
/// must be a literal or a wildcard — never an arbitrary word.
fn parse_pattern_word(w: &str) -> Result<Pattern> {
    if w == "_" {
        return Ok(Pattern::Wildcard);
    }
    if w == "true" {
        return Ok(Pattern::Bool(true));
    }
    if w == "false" {
        return Ok(Pattern::Bool(false));
    }
    if let Some(lit) = parse_integer_literal(w)? {
        return Ok(Pattern::Int {
            value: lit.value,
            explicit_ty: lit.explicit_ty,
        });
    }
    Err(format!(
        "match-arm pattern `{w}` is not a recognised literal \
         (use a number, `true`, `false`, a `\"...\"` string, or `_`)"
    )
    .into())
}

/// Decode the `\"` and `\\` escapes inside a raw string-literal slice. Any
/// other `\X` is an error. The lexer guarantees that every `\` is followed by
/// some character, so trailing-backslash is unreachable from real input — the
/// defensive check is cheap and keeps the function honest in isolation.
fn unescape(raw: &str) -> Result<String> {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => return Err(format!("invalid escape: \\{other}").into()),
                None => return Err("invalid escape: trailing backslash".into()),
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

/// Resolve a single ordinary word — never `:` or `;`, which the caller handles
/// — into a number, a builtin, a function call (`:name`), or top-level text.
fn compile_word(word: &str, heap: &mut Heap) -> Result<Op> {
    if let Some(lit) = parse_integer_literal(word)? {
        return Ok(Op::PushInt(lit.value));
    }
    Ok(match word {
        "true" => Op::PushBool(true),
        "false" => Op::PushBool(false),
        "+" => Op::Add,
        "-" => Op::Sub,
        "*" => Op::Mul,
        "/" => Op::Div,
        "=" => Op::Eq,
        "<" => Op::Lt,
        ">" => Op::Gt,
        "!=" => Op::Ne,
        "<=" => Op::Le,
        ">=" => Op::Ge,
        "not" => Op::Not,
        "and" => Op::And,
        "or" => Op::Or,
        "drop" => Op::Drop,
        "dup" => Op::Dup,
        "swap" => Op::Swap,
        "." => Op::Display,
        ":clear" => Op::Clear,
        ":as-i8" => Op::Cast(Ty::I8),
        ":as-i16" => Op::Cast(Ty::I16),
        ":as-i32" => Op::Cast(Ty::I32),
        ":as-i64" => Op::Cast(Ty::I64),
        ":as-u8" => Op::Cast(Ty::U8),
        ":as-u16" => Op::Cast(Ty::U16),
        ":as-u32" => Op::Cast(Ty::U32),
        ":as-u64" => Op::Cast(Ty::U64),
        ":readline" => Op::ReadLine,
        ":contains" => Op::Contains,
        ":println" => Op::PrintLn,
        ":print" => Op::Print,
        _ => match word.strip_prefix(':') {
            Some(name) if !name.is_empty() => Op::Call(name.to_string()),
            _ => Op::PushStr(heap.add_str(word.to_string())),
        },
    })
}

/// Names whose `:name` call spelling is already owned by a builtin. Rejecting
/// matching definitions prevents a function that can never be called.
fn is_reserved_function_name(name: &str) -> bool {
    matches!(
        name,
        "clear"
            | "as-i8"
            | "as-i16"
            | "as-i32"
            | "as-i64"
            | "as-u8"
            | "as-u16"
            | "as-u32"
            | "as-u64"
            | "readline"
            | "contains"
            | "println"
            | "print"
    )
}

/// Input names are ordinary identifiers, not literals, operators, or call
/// spellings. This prevents `{ 2 i64 -> ... }` from turning `2` in a body
/// into a local load instead of an integer literal.
fn is_valid_input_name(name: &str) -> bool {
    !matches!(parse_integer_literal(name), Ok(Some(_)) | Err(_))
        && !matches!(
            name,
            "true"
                | "false"
                | "match"
                | "end"
                | "not"
                | "and"
                | "or"
                | "drop"
                | "dup"
                | "swap"
                | "+"
                | "-"
                | "*"
                | "/"
                | "="
                | "!="
                | "<"
                | "<="
                | ">"
                | ">="
                | "."
        )
        && !name.starts_with(':')
}

// --- tail-call detection (§11.8) -------------------------------------------

/// Rewrite the last `Call` in `body` to `TailCall`, recursing through the
/// last arm-bodies of trailing `Match` ops. A function body's last op is in
/// tail position; the last op of a match arm is in tail position iff the
/// match itself is in tail position — that recursion is what this function
/// implements.
///
/// The rewrite is structural: we walk only the *tail* of the body, so
/// non-tail calls anywhere else stay `Call`. Match arms are stored as
/// `Arc<[Op]>`, so mutating an arm body means rebuilding it; we only do that
/// for arms that actually contain a tail call.
pub(crate) fn mark_tail_calls(body: &mut [Op]) {
    let Some(last) = body.last_mut() else {
        return;
    };
    match last {
        Op::Call(name) => {
            *last = Op::TailCall(std::mem::take(name));
        }
        Op::CallIndirect(signature) => {
            *last = Op::TailCallIndirect(signature.clone());
        }
        Op::Match(arms) => {
            // Rebuild arms with each arm's tail rewritten.
            let new_arms: Vec<MatchArm> = arms
                .iter()
                .map(|arm| {
                    let mut new_body: Vec<Op> = arm.body.iter().cloned().collect();
                    mark_tail_calls(&mut new_body);
                    MatchArm {
                        pattern: arm.pattern,
                        body: new_body.into(),
                    }
                })
                .collect();
            *arms = new_arms.into();
        }
        Op::If {
            then,
            elifs,
            otherwise,
        } => {
            let mark = |body: &mut Arc<[Op]>| {
                let mut ops = body.to_vec();
                mark_tail_calls(&mut ops);
                *body = ops.into();
            };
            mark(then);
            for arm in Arc::make_mut(elifs) {
                mark(&mut arm.body);
            }
            mark(otherwise);
        }
        _ => {}
    }
}

// --- type checking (§11.6) -------------------------------------------------

/// Independently check a complete module's operations and function signatures.
/// Abstract operand stacks track concrete types without executing the program.
#[cfg_attr(feature = "profile", hotpath::measure)]
pub fn check(ops: &[Op]) -> Result<()> {
    check_with(ops, crate::parallel::workers())
}

/// Functions below which another checking thread costs more to start than it
/// saves: a typical body takes microseconds to check.
const FUNCTIONS_PER_WORKER: usize = 16;

/// Function bodies are checked on `workers` threads: a body reads only the
/// signature table. The error reported is the first in program order.
fn check_with(ops: &[Op], workers: usize) -> Result<()> {
    // Nearly every top-level operation defines a function.
    let mut sigs = HashMap::with_capacity_and_hasher(ops.len(), Default::default());
    collect_sigs(ops, &mut sigs);
    let functions: Vec<_> = ops
        .iter()
        .filter_map(|op| match op {
            Op::DefineFn(name, f) => Some((name, f)),
            _ => None,
        })
        .collect();
    let workers = workers.min(functions.len() / FUNCTIONS_PER_WORKER + 1);
    let program = crate::ownership::Program::new(ops);
    let mut checked = crate::parallel::map(functions.len(), workers, |i| {
        let (name, f) = functions[i];
        check_function(name, f, &sigs, &program).map_err(|e| e.to_string())
    })
    .into_iter();
    // Top-level: locals are empty (the compiler will never have emitted a
    // `LoadLocal` here either), and there is no end-of-stream invariant.
    let mut stack = Vec::new();
    let mut flow = Flow::Continues;
    for op in ops {
        if flow == Flow::Exits {
            return Err("unreachable operation after a control-flow exit".into());
        }
        flow = match op {
            Op::DefineFn(..) => {
                checked.next().expect("one result per function")?;
                Flow::Continues
            }
            _ => step(op, &mut stack, &[], &sigs, None, None, None)?,
        };
    }
    crate::threading::check(ops)?;
    Ok(())
}

/// Every function's signature by name. Keys and signatures borrow from the
/// program: a second owned table costs a name copy and a reference-count
/// write per function, each a cache miss in a large program (issue #57).
type Sigs<'a> = HashMap<&'a str, &'a FnSig>;

/// Only paths that continue participate in a branch's stack-shape join.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
    Continues,
    Exits,
}

fn check_sequence(
    ops: &[Op],
    stack: &mut Vec<Ty>,
    locals: &[Ty],
    sigs: &Sigs,
    returns: Option<&[Ty]>,
    loop_stack: Option<&[Ty]>,
    yield_ty: Option<&Ty>,
) -> Result<Flow> {
    let mut flow = Flow::Continues;
    for op in ops {
        if flow == Flow::Exits {
            return Err("unreachable operation after a control-flow exit".into());
        }
        flow = step(op, stack, locals, sigs, returns, loop_stack, yield_ty)?;
    }
    Ok(flow)
}

fn check_return(stack: &[Ty], returns: Option<&[Ty]>) -> Result<Flow> {
    let expected = returns.ok_or("return outside a function")?;
    if stack != expected {
        return Err(format!(
            "function exit leaves [{}], but signature declares outputs [{}]",
            fmt_types(stack),
            fmt_types(expected),
        )
        .into());
    }
    Ok(Flow::Exits)
}

/// Add the sig of every `DefineFn` reachable from `ops` — top-level and
/// nested — to `out`. Walking recursively makes the resulting table a
/// safe over-approximation of "what's callable somewhere in this source":
/// it allows forward references at the cost of accepting calls to a
/// nested function before its enclosing definition has run. The latter is
/// caught at runtime as an "undefined function" error, which is fine —
/// the checker's job is to catch *type* mismatches, not to police call
/// ordering.
fn collect_sigs<'a>(ops: &'a [Op], out: &mut Sigs<'a>) {
    for op in ops {
        match op {
            Op::DefineFn(name, f) => {
                out.insert(name, &f.sig);
                collect_sigs(&f.body, out);
            }
            Op::Loop { condition, body } => {
                collect_sigs(condition, out);
                collect_sigs(body, out);
            }
            Op::CheckedBlock { body, .. } => collect_sigs(body, out),
            Op::Match(arms) => {
                for arm in arms.iter() {
                    collect_sigs(&arm.body, out);
                }
            }
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                collect_sigs(then, out);
                for arm in elifs.iter() {
                    collect_sigs(&arm.condition, out);
                    collect_sigs(&arm.body, out);
                }
                collect_sigs(otherwise, out);
            }
            _ => {}
        }
    }
}

/// Apply one op to the abstract stack.
///
/// `locals` types the active function's input names by index — empty at
/// the top level, non-empty inside a body. `sigs` is the resolved table
/// of every function callable in this source.
fn step(
    op: &Op,
    stack: &mut Vec<Ty>,
    locals: &[Ty],
    sigs: &Sigs,
    returns: Option<&[Ty]>,
    loop_stack: Option<&[Ty]>,
    yield_ty: Option<&Ty>,
) -> Result<Flow> {
    match op {
        Op::Control(operation) => {
            let (inputs, output) = operation.signature();
            for ty in inputs.iter().rev() {
                if stack.pop().as_ref() != Some(ty) {
                    return Err("invalid cancellation token argument".into());
                }
            }
            stack.push(output);
        }
        Op::Atomic(operation) => {
            let (inputs, output) = operation.signature();
            for ty in inputs.iter().rev() {
                if stack.pop().as_ref() != Some(ty) {
                    return Err("invalid atomic argument".into());
                }
            }
            stack.push(output);
        }
        Op::Mutex(operation) => {
            let (inputs, output) = operation.signature();
            for ty in inputs.iter().rev() {
                if stack.pop().as_ref() != Some(ty) {
                    return Err("invalid mutex argument".into());
                }
            }
            stack.push(output);
        }
        Op::Executor(operation) => {
            let (inputs, output) = operation.signature();
            for ty in inputs.iter().rev() {
                if stack.pop().as_ref() != Some(ty) {
                    return Err("invalid executor argument".into());
                }
            }
            stack.push(output);
        }
        Op::Channel(operation) => {
            let (inputs, output) = operation.signature();
            for ty in inputs.iter().rev() {
                if stack.pop().as_ref() != Some(ty) {
                    return Err("invalid channel argument".into());
                }
            }
            stack.push(output);
        }
        Op::Thread(operation) => match operation {
            crate::threading::ThreadOp::Start(task, slot) => {
                if locals.get(*slot as usize) != Some(&Ty::Task(task.clone())) {
                    return Err("invalid scoped thread slot".into());
                }
                for ty in task.inputs.iter().rev() {
                    if stack.pop().as_ref() != Some(ty) {
                        return Err("invalid scoped thread argument".into());
                    }
                }
                stack.push(task.start_result());
            }
            crate::threading::ThreadOp::Join(task) => {
                if stack.pop() != Some(Ty::Task(task.clone())) {
                    return Err("invalid scoped join token".into());
                }
                if task.output != Ty::Unit {
                    stack.push(task.output.clone());
                }
            }
            crate::threading::ThreadOp::Finish(task, slot) => {
                if locals.get(*slot as usize) != Some(&Ty::Task(task.clone())) {
                    return Err("invalid scoped thread cleanup".into());
                }
            }
            crate::threading::ThreadOp::Run(task) => {
                for ty in task.inputs.iter().rev() {
                    if stack.pop().as_ref() != Some(ty) {
                        return Err("invalid __actor_run argument".into());
                    }
                }
                if stack.pop() != Some(Ty::I64) {
                    return Err("invalid __actor_run heap".into());
                }
                stack.push(task.run_result());
            }
            crate::threading::ThreadOp::Forget(ty) => {
                if stack.pop().as_ref() != Some(ty) {
                    return Err("invalid forgotten value".into());
                }
            }
        },
        Op::Try {
            source,
            target,
            cleanup,
        } => {
            let mut empty = Vec::new();
            check_sequence(
                cleanup, &mut empty, locals, sigs, returns, loop_stack, yield_ty,
            )?;
            if !empty.is_empty() {
                return Err("propagation cleanup must return unit".into());
            }
            if !source.propagatable()
                || !target.propagatable()
                || source.is_option() != target.is_option()
                || (!source.is_option() && !target.accepts_result_error(source))
                || yield_ty.is_some()
                || returns != Some(&[Ty::Enum((**target).clone())][..])
                || stack.pop() != Some(Ty::Enum((**source).clone()))
            {
                return Err("invalid Result/Option propagation".into());
            }
            stack.push(source.get().variants[usize::from(source.is_option())].fields[0].clone());
        }
        Op::Loan(_)
        | Op::UseLoan(_)
        | Op::EndLoan(_)
        | Op::Access(..)
        | Op::Site(_)
        | Op::CallerSite(_)
        | Op::Returning(..)
        | Op::Leaving
        | Op::Building(_)
        | Op::Passes(_)
        | Op::Forwards(_)
        | Op::Tokens(_)
        | Op::FieldMove(_)
        | Op::ForwardsReferences => {}
        Op::BorrowLocal(i, mutable) => {
            let ty = locals.get(*i as usize).ok_or("invalid borrowed place")?;
            stack.push(Ty::Ref(Arc::new(ty.clone()), *mutable));
        }
        Op::ReadRef(ty) => {
            if !matches!(stack.pop(), Some(Ty::Ref(t, _)) if t.as_ref() == ty) {
                return Err("invalid reference read".into());
            }
            stack.push(ty.clone());
        }
        Op::Reborrow(target) => {
            let ((Some(Ty::Ref(source, writable)), Ty::Ref(inner, mutable))
            | (Some(Ty::View(source, writable)), Ty::View(inner, mutable))) = (stack.pop(), target)
            else {
                return Err("invalid reborrow".into());
            };
            if source != *inner || (*mutable && !writable) {
                return Err("invalid reborrow permissions".into());
            }
            stack.push(target.clone());
        }
        Op::WriteRef(ty) | Op::FillRef(ty) | Op::ReplaceRef(ty) => {
            if stack.pop() != Some(Ty::Ref(Arc::new(ty.clone()), true))
                || stack.pop().as_ref() != Some(ty)
            {
                return Err("invalid reference write".into());
            }
            if matches!(op, Op::ReplaceRef(_)) {
                stack.push(ty.clone());
            }
        }
        Op::Collection(operation) => {
            match operation {
                crate::collection::CollectionOp::ListLenRef(ty)
                | crate::collection::CollectionOp::ListRowsRef(ty)
                | crate::collection::CollectionOp::ListRowBytesRef(ty)
                | crate::collection::CollectionOp::ListRowRef(ty, _)
                | crate::collection::CollectionOp::ListRowTake(ty)
                    if !matches!(ty, Ty::List(_)) =>
                {
                    return Err("list length and rows require a list reference".into());
                }
                crate::collection::CollectionOp::ViewNew(
                    Ty::List(_) | Ty::ByteStorage | Ty::ByteStr | Ty::View(..),
                    _,
                ) => {}
                crate::collection::CollectionOp::ViewNew(..) => {
                    return Err("a slice view requires a list, byte, or view owner".into());
                }
                crate::collection::CollectionOp::ViewLen(ty)
                | crate::collection::CollectionOp::ViewRow(ty, _)
                | crate::collection::CollectionOp::ViewItem(ty, _)
                | crate::collection::CollectionOp::ViewEqual(ty)
                    if !matches!(ty, Ty::View(..)) =>
                {
                    return Err("view operations require a slice view".into());
                }
                crate::collection::CollectionOp::ViewRow(Ty::View(_, false), true)
                | crate::collection::CollectionOp::ViewItem(Ty::View(_, false), true) => {
                    return Err("a shared slice view cannot lend a mutable row".into());
                }
                crate::collection::CollectionOp::ListScalarGetRef(ty)
                | crate::collection::CollectionOp::ListScalarGetView(ty)
                | crate::collection::CollectionOp::ListScalarSetRef(ty)
                    if !matches!(ty, Ty::List(element) if element.is_numeric() || **element == Ty::Bool) =>
                {
                    return Err("scalar list access requires a scalar list reference".into());
                }
                crate::collection::CollectionOp::ListSortBy(sort) => {
                    let Ty::List(element) = &sort.list else {
                        return Err("key sorting requires a list".into());
                    };
                    let item = Ty::Ref(element.clone(), false);
                    let valid = sigs.get(sort.key_of.as_str()).is_some_and(|key_of| {
                        key_of
                            .inputs
                            .iter()
                            .map(|(_, t)| t)
                            .eq([&sort.callback, &item])
                            && key_of.outputs == [sort.key.clone()]
                    });
                    if !valid || !sort.key.is_orderable() {
                        return Err("key sort helper signature mismatch".into());
                    }
                }
                _ => {}
            }
            let (inputs, output) = operation.signature();
            if stack.len() < inputs.len() || stack[stack.len() - inputs.len()..] != inputs {
                return Err("collection operation type mismatch".into());
            }
            stack.truncate(stack.len() - inputs.len());
            stack.push(output);
        }
        Op::Class(operation) => {
            let (inputs, output) = operation.signature().ok_or("invalid class operation")?;
            if stack.len() < inputs.len() || stack[stack.len() - inputs.len()..] != inputs {
                return Err("class operation type mismatch".into());
            }
            stack.truncate(stack.len() - inputs.len());
            stack.push(output);
        }
        Op::Enum(operation) => {
            let (inputs, output) = operation.signature().ok_or("invalid enum operation")?;
            if stack.len() < inputs.len() || stack[stack.len() - inputs.len()..] != inputs {
                return Err("enum operation type mismatch".into());
            }
            stack.truncate(stack.len() - inputs.len());
            stack.push(output);
        }
        Op::Box(operation) => {
            let (inputs, output) = operation.signature();
            if stack.len() < inputs.len() || stack[stack.len() - inputs.len()..] != inputs {
                return Err("box operation type mismatch".into());
            }
            stack.truncate(stack.len() - inputs.len());
            stack.push(output);
        }
        Op::Split(t, tag) => {
            let definition = t.get();
            let fields = &definition
                .variants
                .get(*tag)
                .ok_or("invalid split variant")?
                .fields;
            if fields.len() < 2 || stack.pop() != Some(Ty::Enum(t.clone())) {
                return Err("invalid split".into());
            }
            stack.extend(fields.iter().cloned());
        }
        Op::SplitClass(t) => {
            let definition = t.get();
            if definition.fields.len() < 2
                || definition.destructor.is_some()
                || stack.pop() != Some(Ty::Class(t.clone()))
            {
                return Err("invalid class split".into());
            }
            stack.extend(definition.fields.iter().map(|(_, ty)| ty.clone()));
        }
        Op::Unreachable => return Ok(Flow::Exits),
        Op::Yield(ty) => {
            if yield_ty != Some(ty) || stack.pop().as_ref() != Some(ty) || !stack.is_empty() {
                return Err(
                    "yield requires its declared element type and an empty residual operand stack"
                        .into(),
                );
            }
        }
        Op::Next(slot) => {
            let Some(Ty::Generator(element)) = locals.get(*slot as usize) else {
                return Err("next requires a generator local".into());
            };
            stack.push(crate::sum::option(element.element.clone()));
        }
        Op::DropLocal(i) => {
            locals.get(*i as usize).ok_or("invalid drop local")?;
        }
        Op::EmptyLocal(i) => {
            locals.get(*i as usize).ok_or("invalid empty local")?;
        }
        Op::Loop { condition, body } => {
            let initial = stack.clone();
            let mut cond = initial.clone();
            if check_sequence(condition, &mut cond, locals, sigs, returns, None, yield_ty)?
                != Flow::Continues
                || cond.pop() != Some(Ty::Bool)
                || cond != initial
            {
                return Err("loop condition must produce bool".into());
            }
            let mut iter = initial.clone();
            if check_sequence(
                body,
                &mut iter,
                locals,
                sigs,
                returns,
                Some(&initial),
                yield_ty,
            )? == Flow::Continues
                && iter != initial
            {
                return Err("loop body must preserve operand types".into());
            }
        }
        // Unsuffixed integer literals are `i64`; a suffix records its chosen
        // width directly in the `Value` carried by the operation.
        Op::PushInt(value) => stack.push(Ty::from(*value)),
        Op::PushFloat { ty, .. } => {
            if !ty.is_float() {
                return Err("float literal requires a float type".into());
            }
            stack.push(ty.clone());
        }
        Op::PushUnit => stack.push(Ty::Unit),
        Op::PipelineToken(item, id) => stack.push(Ty::Iter(item.clone(), *id)),
        Op::FloatNeg => {
            let ty = stack.last().ok_or("stack underflow on float negation")?;
            if !ty.is_float() {
                return Err("float negation requires a float".into());
            }
        }
        Op::PushStr(_) => stack.push(Ty::Str),
        Op::PushBytes(_) => stack.push(Ty::ByteStr),
        Op::PushBool(_) => stack.push(Ty::Bool),
        Op::ForeignNull(ty) => {
            if !matches!(ty, Ty::ForeignPtr(_)) {
                return Err("foreign null requires an opaque pointer".into());
            }
            stack.push(ty.clone());
        }
        Op::ForeignCall { sig, .. } => {
            for (_, expected) in sig.inputs.iter().rev() {
                if stack.pop().as_ref() != Some(expected) {
                    return Err("foreign call argument type mismatch".into());
                }
            }
            stack.extend(sig.outputs.iter().cloned());
        }
        Op::Add => {
            let (a, b) = pop2(stack, "+")?;
            let out = match (a.clone(), b.clone()) {
                (Ty::Str, Ty::Str) => Ty::Str,
                (a, b) if a == b && a.is_numeric() => a,
                _ => {
                    return Err(format!(
                        "`+` requires same-typed numbers or (Str Str), got ({a} {b})"
                    )
                    .into());
                }
            };
            stack.push(out);
        }
        Op::Sub => arith(stack, "-")?,
        Op::Mul => arith(stack, "*")?,
        Op::Div => arith(stack, "/")?,
        Op::FloorDiv | Op::Modulo => {
            if !stack.last().is_some_and(Ty::is_int) {
                return Err("floor division and modulo require integers".into());
            }
            arith(stack, "// or %")?;
        }
        Op::Integer(kind) => {
            let ty = stack.pop().ok_or("integer operation stack underflow")?;
            if !ty.is_int() {
                return Err("bitwise and wrapping operations require integers".into());
            }
            if *kind != IntegerOp::Not && stack.pop().as_ref() != Some(&ty) {
                return Err("integer operations require same-typed operands".into());
            }
            stack.push(ty);
        }
        Op::Numeric(operation) => operation.check(stack)?,
        Op::CheckedArith {
            operation,
            ty,
            cleanup,
        } => {
            let mut empty = Vec::new();
            check_sequence(
                cleanup, &mut empty, locals, sigs, returns, loop_stack, yield_ty,
            )?;
            if !empty.is_empty() {
                return Err("checked arithmetic cleanup must leave no value".into());
            }
            operation.check(ty, stack)?;
        }
        Op::CheckedBlock { body, result } => {
            let Ty::Enum(sum) = result else {
                return Err("a checked block must produce a Result".into());
            };
            if !sum.is_result() {
                return Err("a checked block must produce a Result".into());
            }
            if check_sequence(body, stack, locals, sigs, returns, loop_stack, yield_ty)?
                != Flow::Continues
                || stack.pop().as_ref() != Some(&sum.get().variants[0].fields[0])
            {
                return Err("a checked block must leave its success value".into());
            }
            stack.push(result.clone());
        }
        Op::Eq => {
            let (a, b) = pop2(stack, "=")?;
            if a != b {
                return Err(
                    format!("`=` requires both operands of the same type, got ({a} {b})").into(),
                );
            }
            stack.push(Ty::Bool);
        }
        Op::Lt => cmp_int(stack, "<")?,
        Op::Gt => cmp_int(stack, ">")?,
        Op::Not => {
            let top = stack.pop().ok_or("stack underflow on `not`")?;
            if top != Ty::Bool {
                return Err(format!("`not` requires Bool, got {top}").into());
            }
            stack.push(Ty::Bool);
        }
        Op::Ne => {
            let (a, b) = pop2(stack, "!=")?;
            if a != b {
                return Err(
                    format!("`!=` requires both operands of the same type, got ({a} {b})").into(),
                );
            }
            stack.push(Ty::Bool);
        }
        Op::Le => cmp_int(stack, "<=")?,
        Op::Ge => cmp_int(stack, ">=")?,
        Op::And | Op::Or => {
            let label = if matches!(op, Op::And) { "and" } else { "or" };
            let (a, b) = pop2(stack, label)?;
            if a != Ty::Bool || b != Ty::Bool {
                return Err(format!("`{label}` requires (Bool Bool), got ({a} {b})").into());
            }
            stack.push(Ty::Bool);
        }
        Op::Drop => {
            stack.pop().ok_or("stack underflow on `drop`")?;
        }
        Op::Dup => {
            let top = stack.last().ok_or("stack underflow on `dup`")?.clone();
            stack.push(top);
        }
        Op::Swap => {
            if stack.len() < 2 {
                return Err(format!(
                    "stack underflow on `swap` (need 2 values, have {})",
                    stack.len()
                )
                .into());
            }
            let len = stack.len();
            stack.swap(len - 1, len - 2);
        }
        Op::Display => {}
        Op::Clear => stack.clear(),
        Op::LoadLocal(i) | Op::MoveLocal(i) => {
            let ty = locals.get(*i as usize).cloned().ok_or_else(|| {
                format!("LoadLocal({i}) has no matching input in the enclosing function")
            })?;
            stack.push(ty);
        }
        Op::StoreLocal(i) => {
            let expected = locals.get(*i as usize).ok_or("invalid local slot")?;
            if stack.pop().as_ref() != Some(expected) {
                return Err("local assignment type mismatch".into());
            }
        }
        // Without the enclosing program, every call this body makes is unknown.
        Op::DefineFn(name, f) => {
            check_function(name, f, sigs, &crate::ownership::Program::new(&[]))?
        }
        Op::Call(name) => check_call(name, stack, sigs)?,
        Op::ClosureNew(t) => {
            for (_, ty) in t.captures.iter().rev() {
                if stack.pop().as_ref() != Some(ty) {
                    return Err("closure capture type mismatch".into());
                }
            }
            let expected = FnSig {
                inputs: t
                    .captures
                    .iter()
                    .enumerate()
                    .map(|(i, (n, _))| (n.clone(), t.parameter(i)))
                    .chain(t.signature.function().inputs)
                    .collect(),
                outputs: t.signature.output.iter().cloned().collect(),
            };
            let actual = sigs.get(t.name.as_str()).ok_or("undefined closure body")?;
            if actual
                .inputs
                .iter()
                .map(|(_, t)| t)
                .ne(expected.inputs.iter().map(|(_, t)| t))
                || actual.outputs != expected.outputs
            {
                return Err("closure body signature mismatch".into());
            }
            stack.push(Ty::Closure(t.clone()));
        }
        Op::ClosureCall(t) => {
            for ty in t.signature.inputs.iter().rev() {
                if stack.pop().as_ref() != Some(ty) {
                    return Err("closure argument type mismatch".into());
                }
            }
            let expected = if t.once {
                Ty::Closure(t.clone())
            } else {
                Ty::Ref(Arc::new(Ty::Closure(t.clone())), t.mutable)
            };
            if stack.pop() != Some(expected) {
                return Err("closure call environment ownership mismatch".into());
            }
            stack.extend(t.signature.output.iter().cloned());
        }
        Op::BoxedCallableNew(source, adapter, signature) => {
            if signature.inputs.len() >= 256 || matches!(signature.output, Some(Ty::Ref(..))) {
                return Err("invalid boxed callable signature".into());
            }
            let source: &Ty = source;
            let expected = match source {
                Ty::Callable(sig) => sig.as_ref(),
                Ty::Closure(t) if !t.once && !t.mutable && source.storage_barrier().is_none() => {
                    &t.signature
                }
                _ => return Err("invalid boxed callable environment".into()),
            };
            if expected != signature.as_ref() || stack.pop().as_ref() != Some(source) {
                return Err("boxed callable construction signature mismatch".into());
            }
            let mut adapted = signature.function();
            adapted.inputs.insert(
                0,
                (
                    "environment".into(),
                    Ty::Ref(Arc::new(source.clone()), false),
                ),
            );
            if sigs.get(adapter.as_str()).copied() != Some(&adapted) {
                return Err("boxed callable adapter signature mismatch".into());
            }
            stack.push(crate::sum::result(
                Ty::BoxedCallable(signature.clone()),
                crate::sum::alloc_error(),
            ));
        }
        Op::BoxedCallableCall(signature) => {
            if matches!(signature.output, Some(Ty::Ref(..))) {
                return Err("boxed callable cannot return references".into());
            }
            for expected in signature.inputs.iter().rev() {
                if stack.pop().as_ref() != Some(expected) {
                    return Err("boxed callable argument mismatch".into());
                }
            }
            if stack.pop()
                != Some(Ty::Ref(
                    Arc::new(Ty::BoxedCallable(signature.clone())),
                    false,
                ))
            {
                return Err("boxed callable requires a shared owner borrow".into());
            }
            stack.extend(signature.output.iter().cloned());
        }
        Op::FunctionAddress(name, signature) => {
            let actual = sigs.get(name.as_str()).ok_or("undefined function value")?;
            if actual.outputs.len() > 1 || CallableSig::from_function(actual) != **signature {
                return Err("function value signature mismatch".into());
            }
            stack.push(Ty::Callable(signature.clone()));
        }
        Op::CallIndirect(signature) | Op::TailCallIndirect(signature) => {
            for expected in signature.inputs.iter().rev() {
                if stack.pop().as_ref() != Some(expected) {
                    return Err("indirect call argument mismatch".into());
                }
            }
            if stack.pop() != Some(Ty::Callable(signature.clone())) {
                return Err("indirect call requires a matching callable".into());
            }
            stack.extend(signature.output.iter().cloned());
            if matches!(op, Op::TailCallIndirect(_)) {
                return check_return(stack, returns);
            }
        }
        Op::TailCall(name) => {
            check_call(name, stack, sigs)?;
            return check_return(stack, returns);
        }
        Op::Return => return check_return(stack, returns),
        Op::Break | Op::Continue => {
            let expected = loop_stack.ok_or("loop control outside a loop")?;
            if stack != expected {
                return Err("loop control must preserve operand types".into());
            }
            return Ok(Flow::Exits);
        }
        Op::Match(arms) => {
            return check_match(arms, stack, locals, sigs, returns, loop_stack, yield_ty);
        }
        Op::If {
            then,
            elifs,
            otherwise,
        } => {
            if stack.pop() != Some(Ty::Bool) {
                return Err("`if` condition must produce bool".into());
            }
            let snapshot = stack.clone();
            for arm in elifs.iter() {
                let mut cond = snapshot.clone();
                if check_sequence(
                    &arm.condition,
                    &mut cond,
                    locals,
                    sigs,
                    returns,
                    loop_stack,
                    yield_ty,
                )? != Flow::Continues
                    || cond.pop() != Some(Ty::Bool)
                    || cond != snapshot
                {
                    return Err("`elif` condition must produce bool".into());
                }
            }
            // Bodies join as match arms do: every continuing one leaves the
            // same stack.
            let bodies = std::iter::once(then)
                .chain(elifs.iter().map(|arm| &arm.body))
                .chain(std::iter::once(otherwise));
            let mut joined: Option<Vec<Ty>> = None;
            for (i, body) in bodies.enumerate() {
                let mut branch = snapshot.clone();
                if check_sequence(
                    body,
                    &mut branch,
                    locals,
                    sigs,
                    returns,
                    loop_stack,
                    yield_ty,
                )? == Flow::Exits
                {
                    continue;
                }
                match &joined {
                    None => joined = Some(branch),
                    Some(expected) if &branch != expected => {
                        return Err(format!(
                            "`if` branch {i} leaves [{}], but the first continuing branch \
                             leaves [{}] (every continuing branch must produce the same \
                             stack effect)",
                            fmt_types(&branch),
                            fmt_types(expected),
                        )
                        .into());
                    }
                    Some(_) => {}
                }
            }
            let Some(joined) = joined else {
                return Ok(Flow::Exits);
            };
            *stack = joined;
        }
        Op::Cast(target) => {
            let top = stack.pop().ok_or("stack underflow on cast")?;
            if !top.is_numeric() || !target.is_numeric() {
                return Err(format!(
                    "cast `:as-{target}` requires numeric source and target types, got {top}"
                )
                .into());
            }
            stack.push(target.clone());
        }
        Op::RangeLength(element) => {
            let step = stack.pop();
            let stop = stack.pop();
            let start = stack.pop();
            if !element.is_int()
                || step != Some(Ty::I64)
                || stop.as_ref() != Some(element)
                || start.as_ref() != Some(element)
            {
                return Err(format!("range length requires ({element} {element} i64)").into());
            }
            stack.push(Ty::I64);
        }
        Op::RangeParts(element) => {
            let range = stack.pop();
            if !element.is_int() || range != Some(Ty::Range(Arc::new(element.clone()))) {
                return Err(format!("range parts requires range[{element}]").into());
            }
            stack.extend([element.clone(), Ty::I64, Ty::I64]);
        }
        Op::CheckedCast(target) => {
            let top = stack.pop().ok_or("stack underflow on cast")?;
            if !top.is_numeric() || !target.is_numeric() {
                return Err(format!(
                    "cast to {target} requires numeric source and target types, got {top}"
                )
                .into());
            }
            stack.push(target.clone());
        }
        Op::TryCast(target) => {
            let top = stack.pop().ok_or("stack underflow on checked cast")?;
            if !top.is_int() || !target.is_int() {
                return Err(format!(
                    "checked cast to {target} requires integer source and target types, got {top}"
                )
                .into());
            }
            stack.push(crate::sum::result(
                target.clone(),
                crate::sum::range_error(),
            ));
        }
        Op::ReadLine => {
            stack.push(Ty::Str);
            stack.push(Ty::Bool);
        }
        Op::Contains => {
            let (a, b) = pop2(stack, ":contains")?;
            if a != Ty::Str || b != Ty::Str {
                return Err(format!("`:contains` requires (Str Str), got ({a} {b})").into());
            }
            stack.push(Ty::Bool);
        }
        Op::PrintLn => {
            let top = stack.pop().ok_or("stack underflow on `:println`")?;
            if top != Ty::Str {
                return Err(format!("`:println` requires Str, got {top}").into());
            }
        }
        Op::Print => {
            stack.pop().ok_or("stack underflow on `:print`")?;
        }
    }
    Ok(Flow::Continues)
}

/// Pop two values off the abstract stack; produce a uniform underflow
/// error message that names the operator.
fn pop2(stack: &mut Vec<Ty>, op_label: &str) -> Result<(Ty, Ty)> {
    if stack.len() < 2 {
        return Err(format!(
            "stack underflow on `{op_label}` (need 2 values, have {})",
            stack.len()
        )
        .into());
    }
    let b = stack.pop().expect("length checked");
    let a = stack.pop().expect("length checked");
    Ok((a, b))
}

/// Stack effect for `-`, `*`, `/`: same-typed numbers in, same type out.
/// No implicit widening — the operands' types must match exactly, which is
/// the hard rule §11.2 commits to over the convenience of mixed-width
/// arithmetic.
fn arith(stack: &mut Vec<Ty>, op_label: &str) -> Result<()> {
    let (a, b) = pop2(stack, op_label)?;
    if !a.is_numeric() || a != b {
        return Err(format!("`{op_label}` requires same-typed numbers, got ({a} {b})").into());
    }
    stack.push(a);
    Ok(())
}

/// Stack effect for ordering: matching orderable operands in, Bool out.
fn cmp_int(stack: &mut Vec<Ty>, op_label: &str) -> Result<()> {
    let (a, b) = pop2(stack, op_label)?;
    if !a.is_orderable() || a != b {
        return Err(format!(
            "`{op_label}` requires same-typed numbers, strings, or orderable tuples, got ({a} {b})"
        )
        .into());
    }
    stack.push(Ty::Bool);
    Ok(())
}

/// Stack effect for a `Call(name)`: verify the top of the stack matches
/// the function's declared inputs in declaration order, then replace them
/// with the declared outputs.
fn check_call(name: &str, stack: &mut Vec<Ty>, sigs: &Sigs) -> Result<()> {
    let sig = sigs
        .get(name)
        .ok_or_else(|| format!("call to undefined function `{name}`"))?;
    let n = sig.inputs.len();
    if stack.len() < n {
        return Err(format!(
            "calling `{name}`: needs {n} value(s) on the stack, have {}",
            stack.len()
        )
        .into());
    }
    // Inputs appear in declaration order, deepest operand first.
    let split = stack.len() - n;
    for (i, (param, expected)) in sig.inputs.iter().enumerate() {
        let actual = stack[split + i].clone();
        // A pipeline's identity names the plan its holder lowers; the
        // borrower of a lent pipeline knows it by another.
        let lent = matches!((&actual, expected), (Ty::Ref(a, true), Ty::Ref(b, true))
            if matches!((&**a, &**b), (Ty::Iter(a, _), Ty::Iter(b, _)) if a == b));
        if actual != *expected && !lent {
            return Err(format!(
                "calling `{name}`: argument `{param}` (position {i}) \
                 expects {expected}, got {actual}"
            )
            .into());
        }
    }
    stack.truncate(split);
    for out in &sig.outputs {
        stack.push(out.clone());
    }
    Ok(())
}

/// Stack effect for `match`: pop the matched value's type, type-check
/// every arm body against a copy of the abstract stack, require continuing
/// arms to agree pointwise, and require exhaustiveness (§11.8). Exiting arms
/// are checked against the enclosing function's return signature instead.
///
/// The agreed-on shape becomes the post-match stack.
fn check_match(
    arms: &[MatchArm],
    stack: &mut Vec<Ty>,
    locals: &[Ty],
    sigs: &Sigs,
    returns: Option<&[Ty]>,
    loop_stack: Option<&[Ty]>,
    yield_ty: Option<&Ty>,
) -> Result<Flow> {
    let matched_ty = stack
        .pop()
        .ok_or("stack underflow on `match` (no value to match against)")?;
    if arms.is_empty() {
        return Err("`match` requires at least one arm".into());
    }

    // Pattern compatibility — each pattern must be reachable on the
    // matched type. Wildcards are always reachable; integer patterns are
    // legal against any integer width but their value must fit (otherwise
    // the arm could never fire after the runtime narrowing in
    // `pattern_matches`).
    for arm in arms {
        let compatible = match (matched_ty.clone(), arm.pattern) {
            (_, Pattern::Wildcard) => true,
            (Ty::Str, Pattern::Str(_)) => true,
            (Ty::Bool, Pattern::Bool(_)) => true,
            (t, Pattern::Int { value, explicit_ty }) if t.is_int() => {
                let pattern_ty = Ty::from(value);
                if explicit_ty {
                    if pattern_ty != t {
                        return Err(format!(
                            "pattern literal has type {pattern_ty}, but the matched type is {matched_ty}"
                        )
                        .into());
                    }
                } else {
                    let Value::I64(n) = value else {
                        unreachable!("unsuffixed integer patterns are i64")
                    };
                    let (lo, hi) = t.int_range().expect("integer types have a range");
                    let n = n as i128;
                    if n < lo || n >= hi {
                        return Err(format!(
                            "pattern literal {n} is out of range for {matched_ty}"
                        )
                        .into());
                    }
                }
                true
            }
            _ => false,
        };
        if !compatible {
            return Err(format!(
                "match-arm pattern is incompatible with the matched type {matched_ty}"
            )
            .into());
        }
    }

    // Exhaustiveness — Bool requires both literals (or a wildcard);
    // every other type (integers and Str) is treated as unbounded and
    // requires a wildcard arm. We deliberately do not special-case `u8`
    // (256 values, technically exhaustible by listing); that would be a
    // soft rule and §11.2 chose the hard one.
    let has_wildcard = arms.iter().any(|a| matches!(a.pattern, Pattern::Wildcard));
    let exhaustive = match matched_ty {
        Ty::Bool => {
            has_wildcard
                || (arms
                    .iter()
                    .any(|a| matches!(a.pattern, Pattern::Bool(true)))
                    && arms
                        .iter()
                        .any(|a| matches!(a.pattern, Pattern::Bool(false))))
        }
        _ => has_wildcard,
    };
    if !exhaustive {
        return Err(
            format!("non-exhaustive `match` on {matched_ty} (add the missing arm or `_`)").into(),
        );
    }

    // Check every arm body against a fresh copy of the abstract stack;
    // require all continuing arms to leave the stack in the same shape.
    let snapshot = stack.clone();
    let mut joined: Option<Vec<Ty>> = None;
    for (i, arm) in arms.iter().enumerate() {
        let mut arm_stack = snapshot.clone();
        if check_sequence(
            &arm.body,
            &mut arm_stack,
            locals,
            sigs,
            returns,
            loop_stack,
            yield_ty,
        )? == Flow::Exits
        {
            continue;
        }
        match &joined {
            None => joined = Some(arm_stack),
            Some(expected) => {
                if &arm_stack != expected {
                    return Err(format!(
                        "match arm {i} leaves [{}], but the first continuing arm leaves [{}] \
                         (every continuing arm must produce the same stack effect)",
                        fmt_types(&arm_stack),
                        fmt_types(expected),
                    )
                    .into());
                }
            }
        }
    }
    match joined {
        Some(joined) => {
            *stack = joined;
            Ok(Flow::Continues)
        }
        None => Ok(Flow::Exits),
    }
}

/// Check one function definition, naming its file in the error.
fn check_function(
    name: &str,
    f: &CompiledFn,
    sigs: &Sigs,
    program: &crate::ownership::Program,
) -> Result<()> {
    check_body(name, f, sigs, program).map_err(|e| -> Box<dyn std::error::Error> {
        // An ownership error that knows its own position already names
        // the file; the header is only a fallback. One in a generic
        // instance is positioned at the use that required the instance.
        let located = e
            .downcast_ref::<crate::ownership::Error>()
            .is_some_and(|e| e.primary.is_some() || e.required.is_some());
        match &f.location {
            Some(at) if !located => format!("{at}: {e}").into(),
            _ => e,
        }
    })
}

/// Check one function body against its declared sig.
///
/// The body's abstract data stack starts **empty** — inputs are drained
/// into the locals frame by `Op::Call`, not left on the stack — and the
/// inputs become the body's `locals` for `LoadLocal` to resolve against.
/// At end of body the abstract stack must equal the declared outputs
/// exactly; anything else is a type error.
fn check_body(
    fn_name: &str,
    f: &CompiledFn,
    sigs: &Sigs,
    program: &crate::ownership::Program,
) -> Result<()> {
    let (sig, body, yield_ty) = (&*f.sig, &*f.body, f.generator.as_ref());
    let locals: Vec<Ty> = sig
        .inputs
        .iter()
        .map(|(_, t)| t.clone())
        .chain(f.locals.iter().cloned())
        .collect();
    let mut stack: Vec<Ty> = Vec::new();
    let returns = if yield_ty.is_some() {
        &[][..]
    } else {
        &sig.outputs
    };
    crate::ownership::check(body, &locals, sig.inputs.len(), &f.origins, program)
        .map_err(|e| -> Box<dyn std::error::Error> { e })?;
    let flow = check_sequence(
        body,
        &mut stack,
        &locals,
        sigs,
        Some(returns),
        None,
        yield_ty,
    )
    .map_err(|e| -> Box<dyn Error> { format!("in `{fn_name}`: {e}").into() })?;
    if flow == Flow::Continues && stack != returns {
        return Err(format!(
            "function `{fn_name}` body leaves [{}], but signature declares outputs [{}]",
            fmt_types(&stack),
            fmt_types(&sig.outputs),
        )
        .into());
    }
    Ok(())
}

/// Render a sequence of types for a human, space-separated, the same
/// orientation as the runtime `stack_repr` (deepest on the left).
fn fmt_types(tys: &[Ty]) -> String {
    tys.iter()
        .map(|t| t.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Later passes read the lifetime fact from the op stream, so its position
    /// is part of the contract: directly before the call it describes.
    #[test]
    fn lifetime_fact_directly_precedes_only_forwarding_calls() {
        let source = r#"
def read(value: &i64, n: i64) -> i64:
    *value + n
def view(value: &i64) -> &i64:
    value
def forwards(value: &i64) -> i64:
    read(value, 1)
def forwards_result(value: &i64) -> &i64:
    return view(value)
def forwards_indirectly(value: &i64) -> i64:
    operation = read
    operation(value, 1)
def lends_local(value: &i64) -> i64:
    local = 2
    read(&local, 1)
def lends_owned(value: i64) -> i64:
    read(&value, 1)
def lends_one(value: &i64, other: i64) -> i64:
    read(value, read(&other, 1))
def main() -> ():
    pass
"#;
        let mut heap = crate::value::Heap::default();
        let program = crate::frontend::compile(source, &mut heap).unwrap();
        let body = |name: &str| {
            program
                .ops
                .iter()
                .find_map(|op| match op {
                    Op::DefineFn(n, f) if n == name => Some(f.body.clone()),
                    _ => None,
                })
                .unwrap()
        };
        for name in ["forwards", "forwards_result"] {
            assert!(
                matches!(
                    &body(name)[..],
                    [.., Op::ForwardsReferences, Op::TailCall(_)]
                ),
                "{name}"
            );
        }
        assert!(matches!(
            &body("forwards_indirectly")[..],
            [.., Op::ForwardsReferences, Op::TailCallIndirect(_)]
        ));
        for name in ["lends_local", "lends_owned"] {
            let body = body(name);
            assert!(
                !body.iter().any(|op| matches!(
                    op,
                    Op::ForwardsReferences | Op::TailCall(_) | Op::TailCallIndirect(_)
                )),
                "{name}"
            );
        }
        // The fact belongs to one call: the inner call lends a parameter slot.
        let body = body("lends_one");
        let facts = body
            .iter()
            .filter(|op| matches!(op, Op::ForwardsReferences))
            .count();
        assert_eq!(facts, 1);
        assert!(matches!(
            &body[..],
            [.., Op::ForwardsReferences, Op::TailCall(_)]
        ));
    }

    /// Bodies are checked out of order, so the error must be chosen by position.
    #[test]
    fn first_error_in_program_order_for_every_worker_count() {
        let source: String = (0..64)
            .map(|i| format!("def unit{i}() -> i64:\n    {i}\n"))
            .chain(["def main() -> ():\n    pass\n".to_string()])
            .collect();
        let mut heap = crate::value::Heap::default();
        let mut program = crate::frontend::compile(&source, &mut heap).unwrap();
        fn corrupt(ops: &mut [Op], name: &str, body: Vec<Op>) {
            for op in ops {
                match op {
                    Op::DefineFn(n, f) if n == name => f.body = body.clone().into(),
                    _ => {}
                }
            }
        }
        corrupt(&mut program.ops, "unit40", Vec::new());
        let later = check_with(&program.ops, 1).unwrap_err().to_string();
        corrupt(&mut program.ops, "unit9", vec![Op::PushBool(true)]);
        let first = check_with(&program.ops, 1).unwrap_err().to_string();
        assert_ne!(first, later);
        for workers in [2, 8] {
            let error = check_with(&program.ops, workers).unwrap_err().to_string();
            assert_eq!(error, first);
        }
    }

    #[test]
    fn legacy_unsuffixed_literal_must_fit_i64() {
        for source in [
            "18446744073709551615 .",
            "-9223372036854775809 .",
            ": f { -> i64 } 9223372036854775808 ;",
        ] {
            let toks = crate::lexer::lex(source).unwrap();
            let mut heap = crate::value::Heap::default();
            let error = compile(&toks, &mut heap).unwrap_err().to_string();
            assert!(error.contains("does not fit i64"), "{source}: {error}");
        }
    }

    /// Every lowered operation stays live until its function is emitted, so
    /// the largest variant sets the size of all of them (issue #54). Box a
    /// payload that would raise this bound.
    #[test]
    fn operations_stay_small() {
        assert!(std::mem::size_of::<Op>() <= 96);
    }
}
