//! Typed operations: the IR the frontend lowers to and every backend reads.
//! [`check`] verifies it independently; [`legacy`] compiles the historical
//! stack syntax that backend tests are written in.

use crate::hash::HashMap;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

use crate::value::{Heap, StrId, Value};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// A Plenty type, as it appears in a function's type header (§11.2).
///
/// Builtin integer domains, represented by a private single-field newtype.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegerDomain {
    NonZero,
    Positive,
    NonNegative,
}

impl IntegerDomain {
    pub fn name(self) -> &'static str {
        match self {
            Self::NonZero => "NonZero",
            Self::Positive => "Positive",
            Self::NonNegative => "NonNegative",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        match name {
            "NonZero" => Some(Self::NonZero),
            "Positive" => Some(Self::Positive),
            "NonNegative" => Some(Self::NonNegative),
            _ => None,
        }
    }
}

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
    /// Shared local dictionary projection; true selects values, false selects pairs.
    DictView(Arc<Ty>, bool),
    /// `&[T]` (false) or `&mut [T]` (true): a borrowed run of contiguous
    /// list or byte rows, made by `&xs[a:b]`. Its value is the first row's
    /// address and the row count; the frontend keeps the owner's loan live
    /// for as long as the view is used. Local-only until stored references
    /// (backlog B11); see book/src/design/language/04-expressions/slices.md.
    View(Arc<Ty>, bool),
    /// The element marker of `&substr`, `View(Text, false)`: a borrowed run
    /// of UTF-8 bytes inside a `str` (issue #595). It appears only inside a
    /// view; its value is the first byte's address and the byte count.
    Text,
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
    DictView,
    View,
    Reference,
    Generator,
    ScopedTask,
    /// A `Closure`/`OnceClosure` annotation that no producer has resolved.
    UnresolvedClosure,
    BorrowedClosure,
    CheckedClosure,
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
            Self::DictView => "dictionary views",
            Self::View => {
                return format!(
                    "slice views cannot be stored in {place} yet (backlog B11); store the bounds, or an owned copy `xs[a:b]?`"
                );
            }
            Self::Reference => "references",
            Self::Generator => "generators",
            Self::ScopedTask => "scoped tasks",
            Self::CheckedClosure => "closures defined in `checked:` blocks",
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
    /// own tag records whether its initializer completed. Dictionary projections
    /// carry a complete shared reference (slot address and tag offset).
    pub fn wide(&self) -> bool {
        matches!(self, Self::Enum(_) | Self::Class(_) | Self::DictView(..))
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
    /// The domain and underlying integer of a builtin integer domain type.
    pub fn integer_domain(&self) -> Option<(IntegerDomain, Ty)> {
        let Self::Class(t) = self else { return None };
        let (name, _) = t.name.split_once('[')?;
        Some((IntegerDomain::named(name)?, t.get().fields[0].1.clone()))
    }
    pub fn domain_element(&self) -> Option<Ty> {
        self.integer_domain().map(|(_, element)| element)
    }
    pub fn positive_element(&self) -> Option<Ty> {
        self.integer_domain()
            .and_then(|(domain, element)| (domain == IntegerDomain::Positive).then_some(element))
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
            // An undefined forward reference is conservatively affine. An
            // instance of a generic `@copy` class moves when a field does,
            // and the facts walk is cycle-safe where a field recursion is not.
            || matches!(self, Self::Class(t) if t.try_get().is_none_or(|t| !t.copy))
            || matches!(self, Self::Class(_) if self.facts().affine)
    }
    pub fn restricted_storage(&self) -> bool {
        match self {
            Self::Iter(..)
            | Self::Task(_)
            | Self::Generator(_)
            | Self::Closure(_)
            | Self::Ref(..)
            | Self::KeyView(_)
            | Self::DictView(..)
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
    pub fn checked_error(&self) -> Option<&Ty> {
        match self {
            Self::Ref(inner, _) => inner.checked_error(),
            Self::Closure(closure) => closure.checked_error.as_ref(),
            _ => None,
        }
    }

    pub fn checked_closure(&self) -> bool {
        match self {
            Self::Closure(closure) => {
                closure.checked || closure.captures.iter().any(|(_, ty)| ty.checked_closure())
            }
            Self::Ref(inner, _) => inner.checked_closure(),
            Self::Enum(t) if t.try_get().is_some_and(|t| t.restricted_storage) => t
                .get()
                .variants
                .iter()
                .flat_map(|v| &v.fields)
                .any(Self::checked_closure),
            _ => false,
        }
    }

    /// The first reason a heap owner cannot embed this type.
    pub fn storage_barrier(&self) -> Option<StorageBarrier> {
        match self {
            Self::Iter(..) => Some(StorageBarrier::Iter),
            Self::KeyView(_) => Some(StorageBarrier::KeyView),
            Self::DictView(..) => Some(StorageBarrier::DictView),
            Self::View(..) => Some(StorageBarrier::View),
            Self::Ref(..) => Some(StorageBarrier::Reference),
            Self::Generator(_) => Some(StorageBarrier::Generator),
            Self::Task(_) => Some(StorageBarrier::ScopedTask),
            Self::Closure(t) if t.name.is_empty() => Some(StorageBarrier::UnresolvedClosure),
            Self::Closure(_) if self.checked_closure() => Some(StorageBarrier::CheckedClosure),
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
    /// `std.tls` configurations are immutable; sessions opened from another
    /// heap reconstruct their configuration without retaining foreign owners.
    /// A `std.tls.Session` changes only through `&mut` and only reads
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
        matches!(
            self,
            Self::Ref(..) | Self::KeyView(_) | Self::DictView(..) | Self::View(..)
        ) || matches!(self, Self::Closure(t) if t.borrowed)
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
    pub fn dictionary_view(&self) -> bool {
        match self {
            Self::KeyView(_) | Self::DictView(..) => true,
            Self::Ref(t, _) => t.dictionary_view(),
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
            Ty::Text => "text",
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
            Ty::DictView(t, values) => {
                return write!(f, "{}View[{t}]", if *values { "Value" } else { "Item" });
            }
            Ty::KeyView(t) => return write!(f, "KeyView[{t}]"),
            Ty::View(t, false) if **t == Ty::Text => "&substr",
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
    /// Internal Result ABI for a specialization using a lexically checked
    /// callback. Source returns still return the declared (logical) type.
    CheckedFunction {
        body: Arc<[Op]>,
        result: Ty,
    },
    /// Unwrap a call's private failure channel into the innermost block.
    CheckedUnwrap {
        result: Ty,
        cleanup: Arc<[Op]>,
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
    FloorDiv(crate::numeric::Divisor),
    Modulo(crate::numeric::Divisor),
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
    /// Source position for diagnostics from the independent IR/ownership checker.
    pub location: Option<crate::diagnostic::Position>,
    /// Source file and binding names for ownership diagnostics.
    pub origins: Arc<crate::ownership::Origins>,
    pub generator: Option<Ty>,
    pub sig: Arc<FnSig>,
    pub doc: Arc<str>,
    pub body: Arc<[Op]>,
    /// Types of local slots after the parameters. Allocated once per call.
    pub locals: Arc<[Ty]>,
}

mod check;
pub(crate) mod legacy;

pub use check::check;

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
    /// Every lowered operation stays live until its function is emitted, so
    /// the largest variant sets the size of all of them (issue #54). Box a
    /// payload that would raise this bound.
    #[test]
    fn operations_stay_small() {
        assert!(std::mem::size_of::<Op>() <= 96);
    }
}
