//! Modern Plenty frontend: indentation → syntax tree → typed operations.
//!
//! Signatures are collected before bodies. Inference is local, expressions
//! have zero (unit) or one result, and no backend participates in inference.
use crate::diagnostic::{Diagnostic, Position};
use crate::hash::{HashMap, HashSet};
use std::cell::Cell;
use std::collections::VecDeque;
use std::collections::hash_map::Entry;
use std::error::Error;
use std::sync::Arc;

use crate::collection::CollectionOp;
use crate::executor::ExecutorOp;
use crate::op::{
    Building, CompiledFn, ElifArm, FnSig, Forward, IntegerOp, MatchArm, Op, Pattern, Ty,
    mark_tail_calls,
};
use crate::threading::ThreadOp;
use crate::value::{Heap, Value};
mod annotations;
mod atomic;
mod byte_buffers;
mod callables;
mod calls;
mod casts;
mod channels;
mod checked;
mod classes;
mod clists;
mod closures;
mod collections;
mod consts;
mod contexts;
mod control;
mod data;
mod defaults;
mod divisors;
mod enums;
mod executors;
mod files;
mod foreign;
mod fstrings;
mod functions;
mod generators;
mod generics;
pub(crate) mod ide;
#[cfg(test)]
mod lexer_tests;
mod mapping;
mod modules;
mod mutex;
mod nonzero;
mod numeric;
mod operators;
mod patterns;
mod pipelines;
mod protocols;
mod provenance;
mod references;
mod retention;
mod shared;
mod slices;
mod sorting;
mod tail_calls;
mod target;
mod threads;
mod views;

mod lexer;
mod parser;
mod syntax;
use lexer::*;
use parser::*;
use syntax::*;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Type = Option<Ty>; // Unit expressions have no operand; enum slots use Ty::Unit.
pub(crate) use byte_buffers::BYTES_CLASS;
pub(crate) use data::TypeAliases;

/// A checked binary's source entrypoint determines the native process result.
pub(crate) struct Program {
    pub(crate) ops: Vec<Op>,
    pub(crate) returns_status: bool,
    pub(crate) exports: Vec<crate::exports::Export>,
    pub(crate) imported_symbols: HashSet<String>,
    pub(crate) source_paths: Vec<std::path::PathBuf>,
    pub(crate) inputs: Inputs,
}

pub(crate) use modules::inputs::{Inputs, standard_digest};

/// A diagnostic that can leave a worker thread. A compiler diagnostic keeps
/// its value, so callers that inspect it see the same value as after a serial
/// pass.
fn sendable(error: Box<dyn Error>) -> Box<dyn Error + Send + Sync> {
    Diagnostic::from_boxed(error)
}

impl TypeRef {
    /// The type as written, with type parameters by name, for diagnostics
    /// about a generic definition rather than one of its instances.
    fn spelling(&self) -> String {
        if let Some(ty) = &self.concrete {
            return ty.to_string();
        }
        let Some(name) = &self.name else {
            return "()".into();
        };
        let args: Vec<String> = self.args.iter().map(TypeRef::spelling).collect();
        match name.as_str() {
            "|" => args.join(" | "),
            "&" | "&mut" => format!("{name}{}", args.concat()),
            _ if args.is_empty() => name.clone(),
            _ => format!("{name}[{}]", args.join(", ")),
        }
    }
    fn resolve(&self, aliases: &TypeAliases) -> Result<Type> {
        let ty = self.resolve_inner(aliases)?;
        if ty == Some(Ty::Text) {
            return Err(self.at.error(
                "`substr` is only the borrowed form `&substr`, a view of part of a string such as `&s[a:b]`; an owned string is `str`",
            ));
        }
        if ty.as_ref().is_some_and(|t| t.layout_depth() > 64) {
            return Err(self
                .at
                .error("type nesting exceeds the implementation limit of 64"));
        }
        Ok(ty)
    }
    fn resolve_inner(&self, aliases: &TypeAliases) -> Result<Type> {
        if let Some(ty) = &self.concrete {
            return Ok(Some((**ty).clone()));
        }
        let Some(name) = &self.name else {
            return Ok(None);
        };
        if name == "|" {
            let members = self
                .args
                .iter()
                .map(|t| t.resolve(aliases).map(|t| t.unwrap_or(Ty::Unit)))
                .collect::<Result<Vec<_>>>()?;
            if Ty::identity_names_exceed_limit(&members) {
                return Err(self
                    .at
                    .error("concrete type name exceeds the implementation limit"));
            }
            let restricted = members.iter().any(|t| !t.heap_storable_once_resolved());
            let normalized = crate::sum::anonymous(members);
            if restricted && matches!(&normalized, Ty::Enum(t) if t.anonymous()) {
                return Err(self
                    .at
                    .error("anonymous sum members must be storable owned types"));
            }
            return Ok((normalized != Ty::Unit).then_some(normalized));
        }
        if name == "OnceCallable" {
            return Err(self.at.error("OnceCallable is a generic constraint; use Callable or OnceClosure for a value annotation"));
        }
        if aliases.data.contains(name) {
            return aliases.instantiate(self).map(Some);
        }
        if matches!(
            name.as_str(),
            "Callable" | "BoxedCallable" | "Closure" | "OnceClosure"
        ) {
            let (output, inputs) = self.args.split_last().ok_or_else(|| {
                self.at
                    .error("use Callable[[parameter types], return type]")
            })?;
            let signature = crate::op::CallableSig {
                inputs: inputs
                    .iter()
                    .map(|t| {
                        t.resolve(aliases)?
                            .ok_or_else(|| t.at.error("callable parameters cannot have unit type"))
                    })
                    .collect::<Result<_>>()?,
                output: output.resolve(aliases)?,
            };
            callables::validate(&signature, &self.at)?;
            if matches!(signature.output, Some(Ty::View(..))) {
                return Err(self.at.error(views::RETURN));
            }
            if name == "BoxedCallable" && matches!(signature.output, Some(Ty::Ref(..))) {
                return Err(self
                    .at
                    .error("BoxedCallable callbacks cannot return references yet"));
            }
            if matches!(name.as_str(), "Closure" | "OnceClosure") {
                return Ok(Some(Ty::Closure(Arc::new(
                    crate::closure::ClosureType::new(
                        String::new(),
                        None,
                        signature,
                        vec![],
                        vec![],
                        name == "OnceClosure",
                    )
                    .map_err(|message| self.at.error(message))?,
                ))));
            }
            return Ok(Some(if name == "BoxedCallable" {
                Ty::BoxedCallable(Arc::new(signature))
            } else {
                Ty::Callable(Arc::new(signature))
            }));
        }
        if name == "tuple" {
            if self.args.is_empty() {
                return Err(self.at.error("use () for the empty tuple"));
            }
            let fields = self
                .args
                .iter()
                .map(|t| Ok(t.resolve(aliases)?.unwrap_or(Ty::Unit)))
                .collect::<Result<Vec<_>>>()?;
            if let Some(barrier) = fields.iter().find_map(Ty::storage_barrier) {
                return Err(self.at.error(barrier.message("tuples")));
            }
            if Ty::identity_names_exceed_limit(&fields) {
                return Err(self
                    .at
                    .error("concrete type name exceeds the implementation limit"));
            }
            return Ok(Some(crate::sum::tuple(fields)));
        }
        if name == "range" && !self.args.is_empty() {
            if self.args.len() != 1 {
                return Err(self.at.error("range requires one integer type argument"));
            }
            let ty = self.args[0]
                .resolve(aliases)?
                .ok_or_else(|| self.at.error("range requires an integer type"))?;
            if !ty.is_int() {
                return Err(self.at.error("range requires an integer type"));
            }
            return Ok(Some(Ty::Range(Arc::new(ty))));
        }
        if matches!(name.as_str(), VIEW_TYPE | MUT_VIEW_TYPE) {
            let element = self.args[0].resolve(aliases)?.unwrap_or(Ty::Unit);
            return view_type(element, name == MUT_VIEW_TYPE, &self.at).map(Some);
        }
        if matches!(name.as_str(), "&" | "&mut") {
            if self.args[0].concrete.is_none()
                && self.args[0].name.as_deref() == Some(SUBSTR)
                && self.args[0].args.is_empty()
            {
                if name == "&mut" {
                    return Err(self.at.error(
                        "a `str` is immutable, so a substring view is shared only; write `&substr`",
                    ));
                }
                return Ok(Some(text_view()));
            }
            // `&()` is the type of a borrowed unit payload; spelling it lets
            // generic `&T` signatures instantiate with `T = ()`.
            let inner = self.args[0].resolve(aliases)?.unwrap_or(Ty::Unit);
            if matches!(inner, Ty::Ref(..)) {
                return Err(self.at.error("references to references are not supported"));
            }
            if matches!(inner, Ty::View(..)) {
                return Err(self.at.error(
                    "a slice view is already a borrow; write `&[T]` or `&mut [T]` without another `&`",
                ));
            }
            return Ok(Some(Ty::Ref(Arc::new(inner), name == "&mut")));
        }
        if name == crate::atomic::ATOMIC {
            let [element] = self.args.as_slice() else {
                return Err(self.at.error("Atomic requires one element type"));
            };
            let element = element.resolve(aliases)?.unwrap_or(Ty::Unit);
            if !crate::atomic::supported(&element) {
                return Err(atomic::element_error(&element, &self.at));
            }
            return Ok(Some(crate::atomic::ty(element)));
        }
        if name == "Iter" {
            let [element] = self.args.as_slice() else {
                return Err(self.at.error("Iter requires one item type"));
            };
            let element = element
                .resolve(aliases)?
                .ok_or_else(|| self.at.error("iterator items cannot be unit"))?;
            return Ok(Some(Ty::Iter(Arc::new(element), usize::MAX)));
        }
        if name == "Generator" {
            if self.args.len() != 1 {
                return Err(self.at.error("Generator requires 1 type argument"));
            }
            let element = self.args[0]
                .resolve(aliases)?
                .ok_or_else(|| self.at.error("generator elements cannot be unit"))?;
            if element.restricted_storage() {
                // Closures stay rejected even when a heap owner could store
                // them: generator expressions reject them too.
                let subject = match &element {
                    Ty::Generator(_) => "generators",
                    Ty::Closure(_) => "closures",
                    Ty::Ref(..) => "references",
                    Ty::Task(_) => "scoped tasks",
                    _ => "enums carrying references, generators, closures, or scoped tasks",
                };
                return Err(self.at.error(format!("generators cannot yield {subject}")));
            }
            return Ok(Some(crate::generator::ty(element, None)));
        }
        if name == "Box" {
            if self.args.len() != 1 {
                return Err(self.at.error("Box requires 1 type argument"));
            }
            let content = self.args[0]
                .resolve(aliases)?
                .ok_or_else(|| self.at.error("a box cannot hold unit"))?;
            if let Some(barrier) = content.storage_barrier() {
                return Err(self.at.error(barrier.message("a box")));
            }
            return Ok(Some(Ty::Box(Arc::new(content))));
        }
        if name == "Shared" {
            if self.args.len() != 1 {
                return Err(self.at.error("Shared requires 1 type argument"));
            }
            let content = self.args[0]
                .resolve(aliases)?
                .ok_or_else(|| self.at.error("a shared owner cannot hold unit"))?;
            if let Some(barrier) = content.storage_barrier() {
                return Err(self.at.error(barrier.message("a shared owner")));
            }
            return Ok(Some(Ty::Shared(Arc::new(content))));
        }
        if name == "Future" {
            if self.args.len() != 1 {
                return Err(self.at.error("Future requires one result type"));
            }
            let output = self.args[0].resolve(aliases)?.unwrap_or(Ty::Unit);
            if !output.heap_storable() {
                return Err(self
                    .at
                    .error("future results require a concrete wholly owned type"));
            }
            return Ok(Some(Ty::Future(Arc::new(output))));
        }
        if matches!(name.as_str(), "Sender" | "Receiver") {
            if self.args.len() != 1 {
                return Err(self.at.error(format!("{name} requires one message type")));
            }
            let message = self.args[0]
                .resolve(aliases)?
                .ok_or_else(|| self.at.error("channel messages cannot be unit"))?;
            if !message.heap_storable() {
                return Err(self.at.error("channel messages require a concrete wholly owned type without references or generator frames"));
            }
            return Ok(Some(Ty::Channel(Arc::new(message), name == "Sender")));
        }
        // The constructor's arity fixes how many type arguments the name takes.
        enum SumCtor {
            One(fn(Ty) -> Ty),
            Two(fn(Ty, Ty) -> Ty),
        }
        let sum_ctor = match name.as_str() {
            "Option" => Some(SumCtor::One(crate::sum::option)),
            "Result" => Some(SumCtor::Two(crate::sum::result)),
            "SpawnError" => Some(SumCtor::One(crate::sum::spawn_error)),
            "SendError" => Some(SumCtor::One(crate::sum::send_error)),
            "AppendError" => Some(SumCtor::One(crate::sum::append_error)),
            "SendTimeoutError" => Some(SumCtor::One(crate::sum::send_timeout_error)),
            "Selected" => Some(SumCtor::Two(crate::sum::selected)),
            "SubmitError" => Some(SumCtor::One(crate::sum::submit_error)),
            "ParallelError" => Some(SumCtor::One(crate::sum::parallel_error)),
            _ => None,
        };
        if let Some(ctor) = sum_ctor {
            let count = match ctor {
                SumCtor::One(_) => 1,
                SumCtor::Two(_) => 2,
            };
            if self.args.len() != count {
                return Err(self
                    .at
                    .error(format!("{name} requires {count} type arguments")));
            }
            let args = self
                .args
                .iter()
                .map(|t| Ok(t.resolve(aliases)?.unwrap_or(Ty::Unit)))
                .collect::<Result<Vec<_>>>()?;
            if Ty::identity_names_exceed_limit(&args) {
                return Err(self
                    .at
                    .error("concrete type name exceeds the implementation limit"));
            }
            // Only the success payload of an Option or Result may carry a
            // reference: book/src/design/language/05-ownership/05-optional-references.md.
            let success = match name.as_str() {
                "Option" | "Result" => Some(0),
                _ => None,
            };
            for (i, arg) in args.iter().enumerate() {
                if !arg.contains_reference() || Some(i) == success && references::carriable(arg) {
                    continue;
                }
                if arg.storage_barrier() == Some(crate::op::StorageBarrier::View) {
                    return Err(self
                        .at
                        .error(crate::op::StorageBarrier::View.message("enum payloads")));
                }
                return Err(self.at.error(if name == "Result" && i == 1 {
                    "Result errors cannot carry references; copy the part of the input the error reports"
                } else {
                    "references cannot be stored in enum payloads"
                }));
            }
            if args
                .iter()
                .any(|ty| ty.storage_barrier() == Some(crate::op::StorageBarrier::Iter))
            {
                return Err(self
                    .at
                    .error("iterator pipelines cannot be stored in enum payloads"));
            }
            let mut args = args.into_iter();
            let mut next = || args.next().expect("argument count checked above");
            return Ok(Some(match ctor {
                SumCtor::One(make) => make(next()),
                SumCtor::Two(make) => {
                    let first = next();
                    make(first, next())
                }
            }));
        }
        if name == "clist" {
            if self.args.len() != 1 {
                return Err(self
                    .at
                    .error("clist requires one primitive-field class type"));
            }
            let row = self.args[0]
                .resolve(aliases)?
                .ok_or_else(|| self.at.error("clist requires a class"))?;
            return aliases.clist_type(row, &self.at).map(Some);
        }
        if name == "KeyView" {
            if self.args.len() != 1 {
                return Err(self.at.error("KeyView requires one key type"));
            }
            let key = self.args[0]
                .resolve(aliases)?
                .ok_or_else(|| self.at.error("KeyView keys cannot be unit"))?;
            if !key.hashable() {
                return Err(self.at.error("KeyView requires a hashable key type"));
            }
            return Ok(Some(Ty::KeyView(Arc::new(key))));
        }
        if matches!(name.as_str(), "list" | "dict" | "set") {
            let count = if name == "dict" { 2 } else { 1 };
            if self.args.len() != count {
                return Err(self
                    .at
                    .error(format!("{name} requires {count} type arguments")));
            }
            let args: Vec<Ty> = self
                .args
                .iter()
                .map(|t| {
                    t.resolve(aliases)?
                        .ok_or_else(|| t.at.error("collection elements cannot be unit"))
                })
                .collect::<Result<_>>()?;
            if let Some(barrier) = args.iter().find_map(Ty::storage_barrier) {
                return Err(self.at.error(barrier.message("collections")));
            }
            return Ok(Some(match name.as_str() {
                "list" => Ty::List(Arc::new(args[0].clone())),
                "set" => {
                    if !args[0].hashable() {
                        return Err(self.at.error(
                            "set elements must be integers, bool, str, or tuples of these",
                        ));
                    }
                    Ty::Set(Arc::new(args[0].clone()))
                }
                _ => {
                    if !args[0].hashable() {
                        return Err(self.at.error(
                            "dictionary keys must be integers, bool, str, or tuples of these",
                        ));
                    }
                    Ty::Dict(Arc::new(args[0].clone()), Arc::new(args[1].clone()))
                }
            }));
        }
        if !self.args.is_empty() {
            return Err(self.at.error("this type does not take type arguments"));
        }
        lookup_type(name, aliases).ok_or_else(|| self.at.error(unknown_type(name)))
    }
}

fn named_type(name: &str) -> Type {
    Some(match name {
        "i8" => Ty::I8,
        "i16" => Ty::I16,
        "i32" => Ty::I32,
        "i64" => Ty::I64,
        "u8" => Ty::U8,
        "u16" => Ty::U16,
        "u32" => Ty::U32,
        "u64" => Ty::U64,
        "f32" => Ty::F32,
        "f64" => Ty::F64,
        "bool" => Ty::Bool,
        "str" => Ty::Str,
        SUBSTR => Ty::Text,
        "bytes" => Ty::ByteStr,
        "File" => Ty::File,
        "FsReadDir" => Ty::FsReadDir,
        "FsDirEntry" => Ty::FsDirEntry,
        "FsMetadata" => Ty::FsMetadata,
        "ByteStorage" => Ty::ByteStorage,
        "range" => Ty::Range(Arc::new(Ty::I64)),
        "AllocError" => crate::sum::alloc_error(),
        "ParseError" => crate::sum::parse_error(),
        "RangeError" => crate::sum::range_error(),
        "DataError" => crate::sum::data_error(),
        "IoError" => crate::sum::io_error(),
        "Failure" => crate::sum::failure(),
        "CStrError" => crate::sum::c_str_error(),
        "LoadError" => crate::sum::load_error(),
        "ThreadError" => crate::sum::thread_error(),
        "RecvError" => crate::sum::recv_error(),
        "RecvTimeoutError" => crate::sum::recv_timeout_error(),
        "SelectError" => crate::sum::select_error(),
        "CancellationToken" => Ty::CancellationToken,
        "PoolError" => crate::sum::pool_error(),
        "FutureError" => crate::sum::future_error(),
        "PoolMapError" => crate::sum::pool_map_error(),
        "ThreadPoolExecutor" => Ty::Executor,
        _ => return None,
    })
}
fn lookup_type(name: &str, aliases: &TypeAliases) -> Option<Type> {
    named_type(name)
        .map(Some)
        .or_else(|| aliases.get(name).cloned())
        .or_else(|| aliases.data.lookup(name).map(Some))
}

/// `__bytes_adopt(storage)`: the `std.bytes.Bytes` owning a native state
/// that another std module created, such as a hasher or codec.
const BYTES_ADOPT: &str = "__bytes_adopt";

/// `__trap("<operation>: <argument> ...")`: std's statement for an argument
/// outside a domain that depends on run-time state, such as a range beyond a
/// buffer's length. It reports the message at the call, with the user caller,
/// and exits like any run-time trap.
const TRAP: &str = "__trap";

/// Native intrinsics that only std modules call. Each family belongs to the
/// std module that validates its operands and maps its results.
fn std_only_intrinsic(name: &str) -> bool {
    name == BYTES_ADOPT
        || name == TRAP
        || crate::numeric::FloatBits::named(name).is_some()
        || name == "__io_entropy_read"
        || [
            "__crypto_",
            "__compress_",
            "__tls_",
            "__sqlite_",
            "__net_",
            "__ready_",
        ]
        .iter()
        .any(|family| name.starts_with(family))
}

pub(crate) fn builtin(name: &str) -> bool {
    named_type(name).is_some()
        || crate::numeric::FloatBits::named(name).is_some()
        || crate::filesystem::FsOp::named(name).is_some()
        || crate::bytes::BytesOp::named(name).is_some()
        || crate::stream::StreamOp::named(name).is_some()
        || crate::crypto::CryptoOp::named(name).is_some()
        || crate::compress::CompressOp::named(name).is_some()
        || crate::process::ProcessOp::named(name).is_some()
        || crate::time::TimeOp::named(name).is_some()
        || crate::unicode::UnicodeOp::named(name).is_some()
        || crate::regex::RegexOp::named(name).is_some()
        || crate::text::TextOp::named(name).is_some()
        || crate::sqlite::SqliteOp::named(name).is_some()
        || crate::tls::TlsOp::named(name).is_some()
        || crate::net::NetOp::named(name).is_some()
        || matches!(
            name,
            "print"
                | "ord"
                | "chr"
                | "spawn"
                | "Future"
                | "SubmitError"
                | "ParallelError"
                | "open"
                | "write_stdout"
                | "write_stderr"
                | "__caller_location"
                | BYTES_ADOPT
                | TRAP
                | "__min_replaces"
                | "__max_replaces"
                | "__test_fault"
                | "__actor_heap"
                | "__actor_run"
                | "__actor_forget"
                | "__report_write"
                | "__io_is_terminal"
                | "flush_stdout"
                | "flush_stderr"
                | "input"
                | "args"
                | "args_bytes"
                | "input_bytes"
                | "encode_utf8"
                | "decode_utf8"
                | "write_stdout_bytes"
                | "write_stderr_bytes"
                | "read_bytes"
                | "write_bytes"
                | "read_text"
                | "write_text"
                | "append_text"
                | "contains"
                | "list"
                | "dict"
                | "KeyView"
                | "clist"
                | "AppendError"
                | "OverflowError"
                | "ZeroDivisionError"
                | "NonZero"
                | "Positive"
                | "NonNegative"
                | "set"
                | "len"
                | "sorted"
                | "range"
                | "Ok"
                | "Err"
                | "Some"
                | "Nothing"
                | "Option"
                | "Result"
                | "SpawnError"
                | "channel"
                | "Sender"
                | "Receiver"
                | "SendError"
                | "SendTimeoutError"
                | "Selected"
                | "select_recv"
                | "select_recv_nowait"
                | "select_recv_timeout"
                | "select_recv_timeout_ms"
                | "Generator"
                | "Iter"
                | "Callable"
                | "BoxedCallable"
                | "Closure"
                | "OnceClosure"
                | "OnceCallable"
                | "tuple"
                | "IntType"
                | "FloatType"
                | "CopyType"
                | "next"
                | "copy"
                | "drop"
                | "Box"
                | "Shared"
        )
}
/// The message for a reserved word that no Plenty construct uses yet.
pub(crate) fn unused_keyword(word: &str) -> Option<&'static str> {
    Some(match word {
        "async" | "await" => {
            "Plenty has no `async`/`await`; write an ordinary `def` and run concurrent work explicitly, such as on a `std.concurrent.ThreadPoolExecutor`"
        }
        "struct" => "Plenty has no `struct`; declare a `class` with typed fields",
        "trait" => "Plenty has no `trait`; declare a `protocol`",
        _ => return None,
    })
}
/// Defines the reserved-word list and a `reserved` test compiled to a match.
/// The book's keyword list and the language server's completions are checked
/// against `KEYWORDS`.
macro_rules! keywords {
    ($($word:literal)|+) => {
        /// Every word the parser reserves; none can be used as a name.
        pub(crate) const KEYWORDS: &[&str] = &[$($word),+];
        pub(crate) fn reserved(name: &str) -> bool {
            matches!(name, $($word)|+)
        }
    };
}
keywords!(
    "def"
        | "extern"
        | "export"
        | "opaque"
        | "try"
        | "with"
        | "type"
        | "const"
        | "return"
        | "if"
        | "elif"
        | "else"
        | "mut"
        | "pass"
        | "and"
        | "or"
        | "not"
        | "True"
        | "False"
        | "None"
        | "struct"
        | "enum"
        | "match"
        | "case"
        | "while"
        | "for"
        | "in"
        | "yield"
        | "async"
        | "await"
        | "class"
        | "lambda"
        | "trait"
        | "protocol"
        | "break"
        | "continue"
        | "import"
        | "from"
        | "as"
        | "pub"
);
/// Reserved words with no meaning in Plenty. They stay reserved so a Python
/// or Rust habit is rejected (with a specific diagnostic for `None`, `try` and
/// `lambda`) instead of silently becoming a name.
pub(crate) const REJECTED_KEYWORDS: &[&str] =
    &["None", "async", "await", "lambda", "struct", "trait", "try"];
fn is_comparison(op: &str) -> bool {
    matches!(op, "==" | "!=" | "<" | ">" | "<=" | ">=" | "in" | "not in")
}
fn numeric_suffix(text: &str) -> bool {
    [
        "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64",
    ]
    .iter()
    .any(|suffix| text.ends_with(suffix))
}
/// Negating an unsigned value overflows for every operand except zero, so
/// the operator is rejected by type instead of trapping when it runs. A sign
/// written directly before a literal is subject to the same type rule.
fn reject_unsigned_negation(ty: &Ty, at: &Token) -> Result<()> {
    let example = match ty {
        Ty::U8 => "-i16(value)",
        Ty::U16 => "-i32(value)",
        Ty::U32 => "-i64(value)",
        // No signed type holds every u64, so the conversion must be checked.
        Ty::U64 => "-i64.try_from(value)?",
        _ => return Ok(()),
    };
    Err(at.error(format!(
        "unary minus is not defined for unsigned type {ty}; convert to a signed type first, for example {example}"
    )))
}
fn binary(t: &Token) -> Option<(String, u8)> {
    let s = t.text()?;
    let precedence = match s {
        "or" => 1,
        "and" => 2,
        "==" | "!=" | "<" | ">" | "<=" | ">=" | "in" | "not in" => 3,
        "|" => 4,
        "^" => 5,
        "&" => 6,
        "<<" | ">>" => 7,
        "+" | "-" => 8,
        "*" | "%" | "/" | "//" => 9,
        _ => return None,
    };
    Some((s.to_string(), precedence))
}

#[derive(Clone)]
struct Local {
    slot: u8,
    ty: Ty,
    mutable: bool,
}

/// Names the Plenty spelling of a Python builtin that Plenty deliberately
/// omits; numeric helpers live in `std.math` (issue #573).
fn unknown_function(name: &str) -> String {
    let hint = match name {
        "abs" => "import `std.math` and call `math.abs(x)`",
        "min" => {
            "import `std.math` and call `math.min(a, b)`, or `math.min_all(&items)` for a list"
        }
        "max" => {
            "import `std.math` and call `math.max(a, b)`, or `math.max_all(&items)` for a list"
        }
        "sum" => "import `std.math` and call `math.sum(&items, 0)`",
        "round" => {
            "import `std.math` and call `math.round(x)?`, or keep the float with `x.round()`"
        }
        "int" => "convert a number with `i64(value)`, or parse text with `i64.parse(text)?`",
        "float" => "convert a number with `f64(value)`, or parse text with `f64.parse(text)?`",
        _ => return format!("unknown function `{name}`"),
    };
    format!("unknown function `{name}`; {hint}")
}
/// Names a Plenty type for a common Python or `typing` spelling, which
/// programmers and coding agents write from habit (issue #572).
pub(crate) fn unknown_type(name: &str) -> String {
    let hint = match name {
        "int" => "use `i64`, or another width such as `i32` or `u8`",
        "float" => "use `f64` or `f32`",
        "List" => "write `list[T]`",
        "Dict" => "write `dict[K, V]`",
        "Set" => "write `set[T]`",
        "Tuple" => "write `tuple[A, B]`",
        "Optional" => "write `Option[T]`",
        "Union" => "write the members as `A | B`",
        _ => return format!("unknown type `{name}`"),
    };
    format!("unknown type `{name}`; {hint}")
}
const NONE_VALUE: &str =
    "Plenty has no `None`; an absent value is `Nothing`, the empty variant of `Option[T]`";
const NONE_TYPE: &str =
    "Plenty has no `None` type; write `()` for no result, or `Option[T]` for an optional value";
const IF_VALUE_POSITION: &str = "an `if` block is a value only as the whole right side of a binding, a single-target assignment, or `return`; bind its value to a name first";
const MATCH_VALUE_POSITION: &str = "a `match` block is a value only as the whole right side of a binding, a single-target assignment, or `return`; bind its value to a name first";

/// What becomes of the value of a suite's final expression.
#[derive(Clone)]
enum Tail {
    /// It is destroyed, and a `Result` cannot be discarded this way.
    Discard,
    /// It is the function's result, and the function's cleanup follows.
    Function,
    /// It is a block value, with the type the block's context
    /// expects. The suite's own locals are cleaned up beneath it.
    Value(Type),
    /// It is returned from the function by `return if` or `return match`.
    /// A suite that ends without a value returns unit.
    Return,
}

/// An exited branch has no value to unify with paths that continue.
enum BlockResult {
    Continues(Type),
    Exits,
}

impl Lower<'_> {
    /// `__trap("message")`. The message is a literal so that reporting it
    /// cannot itself fail to allocate.
    fn trap_statement(&mut self, args: &[Expr], at: &Token, ops: &mut Vec<Op>) -> Result<()> {
        let standard = at
            .source()
            .as_ref()
            .is_some_and(|source| self.generics.source_facts(source).1);
        if !standard {
            return Err(at.error(format!("`{TRAP}` is reserved for the standard library")));
        }
        let [
            Expr {
                kind: Expression::Text(message),
                ..
            },
        ] = args
        else {
            return Err(at.error("`__trap` takes one string literal"));
        };
        ops.push(Op::PushStr(self.heap.add_str(message.clone())));
        ops.push(Op::Collection(CollectionOp::Trap));
        ops.push(Op::Unreachable);
        Ok(())
    }
}

fn exited_block(body: &[Stmt], index: usize) -> Result<BlockResult> {
    if let Some(next) = body.get(index + 1) {
        return Err(next
            .at
            .error("unreachable statement after a control-flow exit"));
    }
    Ok(BlockResult::Exits)
}

struct Lower<'a> {
    pipelines: Vec<pipelines::Pipeline>,
    /// Flags that keep a stopped `take_while` or `zip` stopped. Only `next(p)`
    /// can leave one set before a consumer starts, so consumers of pipelines
    /// not in `resumed_ends` fold the flag checks away.
    pipeline_ends: std::collections::BTreeSet<u8>,
    resumed_ends: std::collections::BTreeSet<u8>,
    /// Set while a pipeline binding is being built: its plan may be resumed
    /// later, so its end checks must stay.
    binding_pipeline: bool,
    /// `&mut Iter[T]` parameters by slot, with their plans.
    lent_parameters: HashMap<u8, pipelines::LentParameter>,
    /// Where each closure value a pipeline holds as a callback came from, and
    /// the binding it was read from, for lending diagnostics.
    pipeline_callbacks: HashMap<u8, (Token, Option<String>)>,
    /// Loans of the reference items of lent pipelines this borrower received.
    lent_item_loans: HashSet<usize>,
    pipeline_generators: usize,
    function_name: String,
    /// `function_name` in the function table.
    symbol: functions::Symbol,
    bare_self_receiver: bool,
    /// In a class `__init__`, the fields assigned so far in source order. The
    /// synthesized constructor is its only caller and passes zeroed storage,
    /// so a field's first assignment outside a loop fills an empty slot.
    init_fields: Option<HashSet<String>>,
    source_tail: Vec<crate::ownership::Site>,
    tail_candidates: Vec<tail_calls::Candidate>,
    closure_loans: HashMap<String, Vec<usize>>,
    captures: HashSet<String>,
    heap: &'a mut Heap,
    sigs: &'a mut functions::Functions,
    generics: &'a mut generics::Engine,
    aliases: &'a TypeAliases,
    access: &'a modules::AccessMap,
    names: HashMap<String, Local>,
    function_places: HashMap<String, Local>,
    function_initializers: Vec<Op>,
    locals: Vec<Ty>,
    parameters: usize,
    /// None at module scope; Some(None) for a unit-returning function.
    return_type: Option<Type>,
    /// Code executed before continuing the innermost loop (for-loop increment).
    loop_steps: Vec<Vec<Op>>,
    loop_scopes: Vec<usize>,
    /// The enclosing `checked:` blocks, innermost last.
    checked: Vec<checked::Scope>,
    /// The innermost block producing a value is a `checked:` block rather
    /// than a `match`, which only changes how a rejected tail is described.
    checked_value_tail: bool,
    indexed_lists: Vec<collections::IndexedLists>,
    yield_type: Type,
    /// Set while a terminal consumes a pipeline; see `consume_in_place`.
    consuming_pipeline: bool,
    loans: Vec<crate::ownership::Loan>,
    reference_locals: HashMap<u8, usize>,
    expression_temps: Vec<u8>,
    contexts: Vec<contexts::Context>,
    return_origin: Option<u8>,
    /// Source name of each slot, kept after the binding leaves scope so an
    /// ownership diagnostic can still name it.
    slot_names: Vec<Option<Arc<str>>>,
    value_origins: HashMap<u8, provenance::Origin>,
    /// Position of the last source marker, restored after scope cleanup.
    site: std::cell::Cell<Option<crate::ownership::Site>>,
    /// The next identity for `Forward` markers.
    forwarded: std::cell::Cell<u32>,
}

fn site(at: &Token, scope_end: bool) -> crate::ownership::Site {
    crate::ownership::Site {
        line: at.line,
        column: at.column,
        scope_end,
    }
}

impl Lower<'_> {
    /// Record the field or enum variant that `e`, a member expression, names.
    fn ide_member(&self, e: &Expr) {
        let Expression::Member(base, name, pos) = &e.kind else {
            return;
        };
        if !ide::active() {
            return;
        }
        match self.place_type(base).map(classes::unbox) {
            Some(Ty::Class(class)) => {
                ide::refer_member(&e.at, *pos, name, ide::field_key(&class.name, name));
            }
            _ => {
                if let Ok(Some(Ty::Enum(t))) = self.qualified_type(base) {
                    ide::refer_member(&e.at, *pos, name, ide::variant_key(&t.name, name));
                }
            }
        }
    }
    fn bind(&mut self, name: String, local: Local) -> Option<Local> {
        self.name_slot(local.slot, &name);
        self.names.insert(name, local)
    }
    /// Give a slot the source name that ownership diagnostics report for it.
    fn name_slot(&mut self, slot: u8, name: &str) {
        let slot = slot as usize;
        if self.slot_names.len() <= slot {
            self.slot_names.resize(slot + 1, None);
        }
        self.slot_names[slot] = Some(name.into());
    }
    /// Attribute the operations that follow to `at` in ownership diagnostics.
    /// Markers go before an expression's operations, never after them: several
    /// passes inspect the last operation of a sequence.
    fn mark(&self, at: &Token, ops: &mut Vec<Op>) {
        let site = site(at, false);
        self.site.set(Some(site));
        match ops.last_mut() {
            Some(Op::Site(last)) => *last = site,
            _ => ops.push(Op::Site(site)),
        }
    }
    fn mark_call(&self, at: &Token, ops: &mut Vec<Op>) {
        ops.push(Op::CallerSite(site(at, false)));
    }
    /// `slot_names` keeps every binding this function has made, so a name
    /// found there but no longer in `names` left scope with its block.
    fn unknown_binding(&self, name: &str) -> String {
        let hint = if self.slot_names.iter().flatten().any(|n| &**n == name) {
            format!(
                "; a binding declared inside a block ends with that block; bind `mut {name}` to a value before the block and assign it inside"
            )
        } else {
            String::new()
        };
        format!("unknown binding `{name}`{hint}")
    }
    /// Attribute a runtime trap in the integer operation pushed next to `at`.
    fn mark_arithmetic(&self, op: &str, ty: &Ty, at: &Token, ops: &mut Vec<Op>) {
        if !ty.is_float() && matches!(op, "+" | "-" | "*" | "//" | "%" | "<<" | ">>") {
            self.mark(at, ops);
        }
    }
    fn numeric_hint(&self, e: &Expr) -> Type {
        match &ungroup(e).kind {
            Expression::Constructor(ty, _) => ty
                .resolve(self.aliases)
                .ok()
                .flatten()
                .filter(Ty::is_numeric),
            Expression::Name(n) if self.captures.contains(n) => {
                self.place_type(e).filter(Ty::is_numeric)
            }
            Expression::GenericCall(name, types, _) => self
                .generics
                .explicit_output(name, types, self.aliases, &e.at)
                .filter(Ty::is_numeric),
            Expression::Number(n) if numeric_suffix(n) => {
                self.number(n, false, &e.at, &mut Vec::new()).ok()
            }
            Expression::Name(n) => self
                .names
                .get(n)
                .map(|l| l.ty.clone())
                .filter(Ty::is_numeric),
            Expression::Member(..) | Expression::Index(..) => self
                .place_type(e)
                .filter(Ty::is_numeric)
                .or_else(|| self.numeric_builtin_hint(e)),
            Expression::Call(..)
            | Expression::Invoke(..)
            | Expression::Method(..)
            | Expression::GenericMethod(..) => self
                .numeric_builtin_hint(e)
                .or_else(|| self.expression_type_hint(e).filter(Ty::is_numeric)),
            Expression::Unary(op, inner) if op == "+" || op == "-" || op == "~" => {
                self.numeric_hint(inner)
            }
            Expression::Binary(op, a, b)
                if matches!(
                    op.as_str(),
                    "+" | "-" | "*" | "/" | "//" | "%" | "&" | "|" | "^" | "<<" | ">>"
                ) =>
            {
                if let Some(output) = self.operator_output_hint(op, a, b) {
                    return output.filter(Ty::is_numeric);
                }
                self.numeric_hint(a).or_else(|| self.numeric_hint(b))
            }
            _ => None,
        }
    }
    fn numeric_binary(
        &mut self,
        op: &str,
        left: &Expr,
        right: &Expr,
        ty: Ty,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        let (a, mut loans) = self.observe_expected(left, Some(ty.clone()), ops)?;
        if a.is_numeric() && a != ty && self.fixes_numeric_type(right, &ty) {
            return Err(mixed_numeric(left, &a, right, &ty));
        }
        if op == "*" && matches!(a, Ty::List(_)) {
            return Err(left.at.error(LIST_REPETITION));
        }
        if op == "*" && a == Ty::Str {
            return Err(left.at.error(STR_REPETITION));
        }
        self.same(Some(a), Some(ty.clone()), &left.at)?;
        let (b, other_loans) = self.observe_expected(right, Some(ty.clone()), ops)?;
        if b.is_numeric() && b != ty && self.fixes_numeric_type(left, &ty) {
            return Err(mixed_numeric(left, &ty, right, &b));
        }
        if op == "*" && matches!(b, Ty::List(_)) {
            return Err(right.at.error(LIST_REPETITION));
        }
        if op == "*" && b == Ty::Str {
            return Err(right.at.error(STR_REPETITION));
        }
        self.same(Some(b), Some(ty.clone()), &right.at)?;
        loans.extend(other_loans);
        self.mark_arithmetic(op, &ty, at, ops);
        let output = self.numeric_operation(op, ty, at, ops)?;
        Self::end_reads(loans, ops);
        Ok(output)
    }
    /// Whether `e` has the numeric type `ty` by itself. When `ty` instead came
    /// from an annotation, a mismatching operand disagrees with that context
    /// and not with the other operand.
    fn fixes_numeric_type(&self, e: &Expr, ty: &Ty) -> bool {
        self.numeric_hint(e).as_ref() == Some(ty)
            || (ty.is_float()
                && matches!(&ungroup(e).kind, Expression::Number(n) if n.contains(['.', 'e', 'E'])))
    }
    fn numeric_operation(&mut self, op: &str, ty: Ty, at: &Token, ops: &mut Vec<Op>) -> Result<Ty> {
        if let Some(kind) = IntegerOp::binary(op) {
            if !ty.is_int() {
                return Err(at.error("bitwise operations require integers"));
            }
            ops.push(Op::Integer(kind));
            return Ok(ty);
        }
        if op == "/" && !ty.is_float() {
            return Err(at.error("use `//` for integer division; `/` requires floats"));
        }
        if matches!(op, "//" | "%") && ty.is_float() {
            return Err(at.error("floor division and modulo currently require integers"));
        }
        if ty.is_int()
            && let Some(operation) = crate::numeric::Checked::named(op)
            && self.checked_arith(operation, &ty, at, ops)?
        {
            return Ok(ty);
        }
        ops.push(match op {
            "+" => Op::Add,
            "-" => Op::Sub,
            "*" => Op::Mul,
            "/" => Op::Div,
            "//" => Op::FloorDiv(crate::numeric::Divisor::Any),
            "%" => Op::Modulo(crate::numeric::Divisor::Any),
            "==" => Op::Eq,
            "!=" => Op::Ne,
            "<" => Op::Lt,
            ">" => Op::Gt,
            "<=" => Op::Le,
            ">=" => Op::Ge,
            _ => unreachable!(),
        });
        Ok(if is_comparison(op) { Ty::Bool } else { ty })
    }
    fn number(&self, text: &str, negative: bool, at: &Token, ops: &mut Vec<Op>) -> Result<Ty> {
        if text.contains(['.', 'e', 'E']) || text.ends_with("f32") || text.ends_with("f64") {
            let (digits, ty) = if let Some(s) = text.strip_suffix("f32") {
                (s, Ty::F32)
            } else {
                (text.strip_suffix("f64").unwrap_or(text), Ty::F64)
            };
            let digits = digits.replace('_', "");
            let digits = if negative {
                format!("-{digits}")
            } else {
                digits
            };
            let bits = if ty == Ty::F32 {
                let value: f32 = digits
                    .parse()
                    .map_err(|_| at.error("invalid f32 literal"))?;
                if !value.is_finite() {
                    return Err(at.error("f32 literal is out of range"));
                }
                u64::from(value.to_bits())
            } else {
                let value: f64 = digits
                    .parse()
                    .map_err(|_| at.error("invalid f64 literal"))?;
                if !value.is_finite() {
                    return Err(at.error("f64 literal is out of range"));
                }
                value.to_bits()
            };
            ops.push(Op::PushFloat {
                bits,
                ty: ty.clone(),
            });
            Ok(ty)
        } else {
            let value = integer(text, negative, at)?;
            ops.push(Op::PushInt(value));
            Ok(Ty::from(value))
        }
    }
    fn same(&self, got: Type, expected: Type, at: &Token) -> Result<()> {
        if got == expected
            || matches!((&expected, &got), (Some(Ty::Iter(a, usize::MAX)), Some(Ty::Iter(b, _))) if a == b)
            || matches!((&expected, &got), (Some(a), Some(b)) if crate::generator::refine(a, b).is_some())
        {
            Ok(())
        } else {
            let can_propagate = got.as_ref().and_then(|ty| self.result_success(ty)).is_some_and(|ty| {
                let success = (ty != Ty::Unit).then_some(ty);
                success == expected
                    || matches!((&expected, &success), (Some(a), Some(b)) if crate::generator::refine(a, b).is_some())
            });
            let hint = if can_propagate {
                "; use `?` to propagate the error or match the result explicitly".into()
            } else if let (Some(Ty::Enum(expected)), Some(Ty::Enum(got))) = (&expected, &got)
                && expected.is_result()
                && got.is_result()
                && is_format_error(&expected.get().variants[1].fields[0])
                && got.get().variants[1].fields[0] == crate::sum::alloc_error()
            {
                "; an f-string whose fields use built-in spellings fails only with AllocError: declare Result[_, AllocError]".into()
            } else if let (Some(Ty::Enum(expected)), Some(Ty::Enum(got))) = (&expected, &got)
                && expected.is_result()
                && got.is_result()
                && is_format_error(&expected.get().variants[1].fields[0])
                && is_format_alloc_sum(&got.get().variants[1].fields[0])
            {
                "; an f-string whose field runs a display or debug method, or has a computed spec, fails with AllocError | FormatError: declare Result[_, AllocError | FormatError]".into()
            } else if let (Some(Ty::Ref(inner, _)), Some(expected)) = (&got, &expected)
                && **inner == *expected
                && copyable_through_reference(inner)
            {
                "; copy the value with `*`".into()
            } else {
                expected
                    .as_ref()
                    .zip(got.as_ref())
                    .and_then(|(expected, got)| callables::mismatch_hint(expected, got))
                    .unwrap_or_default()
            };
            Err(at.error(format!(
                "expected {}, got {}{hint}",
                type_name(expected),
                type_name(got),
            )))
        }
    }
    /// Success type of a Result that `?` can propagate in this function.
    /// Keep diagnostic advice subject to the same residual rules as lowering.
    fn result_success(&self, ty: &Ty) -> Option<Ty> {
        let Ty::Enum(source) = ty else { return None };
        let Some(Some(Ty::Enum(target))) = &self.return_type else {
            return None;
        };
        (self.yield_type.is_none() && target.accepts_result_error(source))
            .then(|| source.get().variants[0].fields[0].clone())
    }
    /// `?` returns its error, so like a returned closure it resolves a
    /// signature-only closure in the declared error (issue #336). The refined
    /// return type then constrains later paths; a second producer for the
    /// same member is rejected as a mismatch.
    fn infer_propagated_producers(
        &mut self,
        target: crate::nominal::Nominal<crate::sum::EnumType>,
        source: &crate::nominal::Nominal<crate::sum::EnumType>,
    ) -> crate::nominal::Nominal<crate::sum::EnumType> {
        if !target.is_result() || !source.is_result() {
            return target;
        }
        let Some(error) = crate::sum::infer_producers(
            &target.get().variants[1].fields[0],
            &source.get().variants[1].fields[0],
        ) else {
            return target;
        };
        let refined = crate::sum::result(target.get().variants[0].fields[0].clone(), error);
        self.return_type = Some(Some(refined.clone()));
        let Ty::Enum(refined) = refined else {
            unreachable!("Result is an enum")
        };
        refined
    }
    fn value(&mut self, e: &Expr, ops: &mut Vec<Op>) -> Result<Ty> {
        self.expr(e, ops)?
            .ok_or_else(|| e.at.error("expected a value, got ()"))
    }
    fn propagation_target(
        &self,
        e: &Expr,
    ) -> Result<crate::nominal::Nominal<crate::sum::EnumType>> {
        if self.yield_type.is_some() {
            return Err(e
                .at
                .error("`?` is not supported in generators; match the result explicitly"));
        }
        let Some(Some(Ty::Enum(target))) = self.return_type.clone() else {
            return Err(e
                .at
                .error("`?` requires a function returning Result or Option"));
        };
        if !target.propagatable() {
            return Err(e
                .at
                .error("`?` requires a function returning Result or Option"));
        }
        Ok(target)
    }

    fn propagate(
        &mut self,
        e: &Expr,
        value: &Expr,
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let target = self.propagation_target(e)?;
        // A collection literal's error is always AllocError, so its context keeps
        // that error and the mismatch is reported below rather than as a missing `?`.
        let literal = match &ungroup(value).kind {
            Expression::Collection { kind, .. } if kind != "generator" => {
                Some(FixedError::Collection)
            }
            Expression::Format(format) if matches!(**format, fstrings::Format::Literal(..)) => {
                Some(FixedError::Format)
            }
            _ => None,
        };
        let operand_context = expected.map(|ty| {
            if literal == Some(FixedError::Collection) {
                crate::sum::result(ty, crate::sum::alloc_error())
            } else if target.is_option()
                || matches!(&ungroup(value).kind, Expression::Call(name, _) if name == "Some")
            {
                crate::sum::option(ty)
            } else {
                crate::sum::result(ty, target.get().variants[1].fields[0].clone())
            }
        });
        let operand = self.expr_expected(value, operand_context, ops)?;
        if operand.as_ref().is_some_and(Ty::dictionary_view) {
            return Err(value.at.error("keys() returns a borrowed KeyView without AllocError; remove `?`, or use keys_snapshot(), into_keys(), or to_set() for an owned result"));
        }
        if matches!(ungroup(value).kind, Expression::Slice(..))
            && !matches!(&operand, Some(Ty::Enum(t)) if t.propagatable())
        {
            return Err(e.at.error(TUPLE_SLICE_TRY));
        }
        let Some(Ty::Enum(source)) = operand else {
            return Err(e.at.error("`?` requires a Result or Option operand"));
        };
        let target = self.check_propagation(e, &source, target, literal)?;
        let success = usize::from(source.is_option());
        let payload = source.get().variants[success].fields[0].clone();
        let mut cleanup = Vec::new();
        if !self.contexts.is_empty() {
            self.cleanup(0, &mut cleanup);
        }
        let carried = Ty::Enum(source.clone()).carries_loan();
        ops.push(Op::Try {
            source: Box::new(source),
            target: Box::new(target),
            cleanup: cleanup.into(),
        });
        if carried && payload.carries_loan() {
            // The payload reference takes over the operand's loan.
            let parent = self.reference_origin(value, ops)?;
            self.new_loan(
                self.loans[parent].root,
                payload.carries_mutable_loan(),
                Some(parent),
                ops,
            );
        }
        if payload == Ty::Unit {
            ops.push(Op::Drop);
            Ok(None)
        } else {
            Ok(Some(payload))
        }
    }

    /// `literal` names an operand whose error type is fixed, such as a
    /// collection literal's `AllocError`, so the diagnostic can say where the
    /// error comes from.
    fn check_propagation(
        &mut self,
        e: &Expr,
        source: &crate::nominal::Nominal<crate::sum::EnumType>,
        target: crate::nominal::Nominal<crate::sum::EnumType>,
        literal: Option<FixedError>,
    ) -> Result<crate::nominal::Nominal<crate::sum::EnumType>> {
        if !source.propagatable() || source.is_option() != target.is_option() {
            if source.propagatable() && source.is_option() {
                return Err(e.at.error(
                    "`?` on an Option cannot propagate from a function returning Result; convert absence to an error with `.ok_or(error)?`",
                ));
            }
            let note = match literal {
                Some(FixedError::Collection) => {
                    "; a collection literal returns Result[collection, AllocError]"
                }
                Some(FixedError::Format) => {
                    "; an f-string returns Result[str, AllocError], or Result[str, AllocError | FormatError] when a field runs a display or debug method or has a computed spec"
                }
                None => "",
            };
            return Err(e.at.error(format!(
                "`?` operand and function return must use the same Result or Option family{note}"
            )));
        }
        let target = self.infer_propagated_producers(target, source);
        if !source.is_option() && !target.accepts_result_error(source) {
            let target_error = &target.get().variants[1].fields[0];
            let source_error = &source.get().variants[1].fields[0];
            if matches!(target_error, Ty::Enum(t) if t.anonymous())
                || matches!(source_error, Ty::Enum(t) if t.anonymous())
            {
                let allowed = crate::sum::members(target_error);
                let missing = crate::sum::members(source_error)
                    .into_iter()
                    .filter(|t| !allowed.contains(t))
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                let hint = producer_conflict_hint(target_error, source_error);
                let origin = if literal == Some(FixedError::Format) {
                    format!("; this f-string fails with {source_error}")
                } else {
                    String::new()
                };
                return Err(e.at.error(format!("`?` operand error {source_error} cannot propagate as {target_error}; missing alternatives: {missing}{origin}{hint}")));
            }
            let hint = producer_conflict_hint(target_error, source_error);
            if !hint.is_empty() {
                return Err(e.at.error(format!(
                    "`?` operand error {source_error} cannot propagate as {target_error}; convert the error explicitly{hint}"
                )));
            }
            let member = source_error.to_string();
            let origin = match literal {
                Some(FixedError::Collection) => {
                    format!("; a collection literal fails with {member}")
                }
                Some(FixedError::Format) if is_format_error(target_error) => {
                    // A signature written before B92 phase 2: name the
                    // replacement rather than a sum the function may not need.
                    return Err(e.at.error(format!(
                        "`?` operand error {member} cannot propagate as FormatError; an f-string whose fields use built-in spellings fails only with AllocError: declare Result[_, AllocError], or Result[_, AllocError | FormatError] if the function also returns FormatError"
                    )));
                }
                Some(FixedError::Format) => format!("; an f-string fails with {member}"),
                None => String::new(),
            };
            let convert = if literal.is_some() {
                "match the result"
            } else {
                "match the operand"
            };
            return Err(e.at.error(format!(
                "`?` operand error {source_error} cannot propagate as {target_error}{origin}; the function's error type must include it: declare a sum such as Result[_, {member} | {target_error}], or {convert} and convert the error explicitly"
            )));
        }
        Ok(target)
    }
    fn expr(&mut self, e: &Expr, ops: &mut Vec<Op>) -> Result<Type> {
        if self.is_pipeline(e) && !self.is_dictionary_projection(e) {
            return Err(e.at.error("iterator pipelines cannot escape their creating scope; bind locally or consume with a loop or terminal"));
        }
        let ty = match &e.kind {
            Expression::NamedArgument(..) => {
                return Err(e.at.error("named arguments require a statically resolved function, method, or class constructor"));
            }
            Expression::Anonymous(function) => {
                if function.once || !function.captures.is_empty() || self.lexical_checked() {
                    return self.closure_value(function, &e.at, ops).map(Some);
                }
                let mut function = (**function).clone();
                function.name = format!("__plenty_anonymous_{}", self.sigs.declared_len());
                let name = function.name.clone();
                register_signature(&function, self.aliases, self.sigs)?;
                self.generics
                    .inherit_requirement(self.sigs, &self.function_name, &name);
                self.generics.declare(function, self.sigs);
                Some(self.function_value(&name, &e.at, ops)?)
            }
            Expression::GenericValue(name, types) => {
                if self.aliases.data.is_class(name) {
                    let ty = TypeRef {
                        concrete: None,
                        parameter: false,
                        at: e.at.clone(),
                        name: Some(name.clone()),
                        args: types.clone(),
                    }
                    .resolve(self.aliases)?
                    .unwrap();
                    return self.class_constructor_value(ty, &e.at, ops).map(Some);
                }
                let actual = types
                    .iter()
                    .map(|t| Ok(t.resolve(self.aliases)?.unwrap_or(Ty::Unit)))
                    .collect::<Result<_>>()?;
                let symbol = self.specialize(name, actual, &e.at)?;
                Some(self.function_value(&symbol, &e.at, ops)?)
            }
            Expression::GenericCall(name, types, args) => {
                if name == crate::atomic::ATOMIC {
                    return self.atomic_new(Some(types), args, None, &e.at, ops);
                }
                if name == "Shared" {
                    return self.shared_new(Some(types), args, &e.at, ops);
                }
                if self.aliases.data.contains(name) {
                    let ty = TypeRef {
                        concrete: None,
                        parameter: false,
                        at: e.at.clone(),
                        name: Some(name.clone()),
                        args: types.clone(),
                    }
                    .resolve(self.aliases)?
                    .unwrap();
                    let Ty::Class(t) = ty else {
                        return Err(e.at.error("select a variant to construct an enum"));
                    };
                    modules::check_member(self.access, &t.name, "__new__", &e.at)?;
                    self.call_named(&crate::record::method(&t.name, "new"), args, &e.at, ops)?
                } else {
                    self.generic_call(name, Some(types), args, &e.at, ops)?
                }
            }
            Expression::Try(value) => self.propagate(e, value, None, ops)?,
            Expression::Match(value, cases) => self.match_value(&e.at, value, cases, None, ops)?,
            Expression::Checked(body) => self.checked_value(&e.at, body, None, ops)?,
            Expression::If(body) => self.if_value(&e.at, body, None, ops)?,
            Expression::Type(ty) => {
                if let Some(ty @ Ty::Class(_)) = ty.resolve(self.aliases)? {
                    return self.class_constructor_value(ty, &e.at, ops).map(Some);
                }
                return Err(e.at.error("a type is not a value; select a variant"));
            }
            Expression::ClassNew(t) => {
                let op = crate::record::ClassOp::New(t.clone());
                let output = op.signature().unwrap().1;
                ops.push(Op::Class(op));
                Some(output)
            }
            Expression::ClassReady(t, receiver) => {
                self.value(receiver, ops)?;
                ops.push(Op::Class(crate::record::ClassOp::ArmDrop(t.clone())));
                ops.push(Op::Drop);
                None
            }
            Expression::Member(base, name, _) => {
                self.ide_member(e);
                match self.qualified_type(base)? {
                    Some(ty) => {
                        if ty.is_numeric() {
                            return self.numeric_constant(ty, name, &e.at, ops).map(Some);
                        }
                        self.variant(ty, name, None, &e.at, ops)?
                    }
                    _ => {
                        if let Expression::Name(owner) = &base.kind
                            && !self.names.contains_key(owner)
                            && self.aliases.data.is_enum(owner)
                        {
                            return Err(e.at.error(format!(
                                "generic enum `{owner}` needs type arguments here, such as `{owner}[i64].{name}`"
                            )));
                        }
                        let moving = self.place_type(e).is_some_and(|ty| ty.affine());
                        let owner = if moving {
                            self.field_owner(e)?
                        } else {
                            classes::FieldOwner::Other
                        };
                        if let classes::FieldOwner::Owned(field) = owner {
                            return self.move_field(e, field, ops).map(Some);
                        }
                        let (ty, loans) = self.field(e, ops)?;
                        if ty.affine() {
                            // Borrowing and replacement come first: they keep one
                            // owner. Copying allocates, so it is last and
                            // propagates its AllocError (issue #576).
                            let borrow = if matches!(ty, Ty::Enum(_)) && ty.recursive_data() {
                                "borrow it by matching &value.field (or &mut value.field)"
                            } else {
                                "borrow it with &value.field (or &mut value.field)"
                            };
                            let replace = if matches!(owner, classes::FieldOwner::Reference) {
                                "take it with replace(value.field, replacement) through a mutable reference"
                            } else {
                                "take it with replace(value.field, replacement) on a mutable owner"
                            };
                            let copies = ty.copy_facts();
                            let rest = if copies.copyable
                                && !copies.recursive
                                && !matches!(ty, Ty::Shared(_))
                            {
                                format!("{replace}, or copy it with copy(value.field)?")
                            } else {
                                format!("or {replace}")
                            };
                            let message = if matches!(owner, classes::FieldOwner::Reference) {
                                format!(
                                    "cannot move out of a field through a reference; {borrow}, {rest}"
                                )
                            } else {
                                format!(
                                    "cannot move out of a field here: fields move out only from a named class binding, not through a Box, Shared handle, element, capture, or temporary; {borrow}, {rest}"
                                )
                            };
                            return Err(self.value_demand_error(
                                &e.at,
                                e,
                                message,
                                crate::ownership::Demand::Copy(ty, "cannot move out of a field"),
                                false,
                            ));
                        }
                        Self::end_reads(loans, ops);
                        Some(ty)
                    }
                }
            }
            Expression::Tuple(values) => Some(self.tuple(values, None, &e.at, ops)?),
            Expression::Format(format) => self.format(format, &e.at, ops)?,
            Expression::Collection { .. } => Some(self.fallible_display(e, None, ops)?),
            Expression::Slice(..) => Some(self.slice(e, None, ops)?),
            Expression::Index(..) => {
                let ty = self.index(e, ops)?;
                if ty == Ty::Unit {
                    ops.push(Op::Drop);
                    None
                } else {
                    Some(ty)
                }
            }
            Expression::Method(base, name, args, pos) => {
                if name == "format"
                    && let Some(ty) = self.format_method(base, args, &e.at, ops)
                {
                    return ty;
                }
                if matches!(name.as_str(), "sort" | "sort_by_key")
                    && let Some(named) = args.iter().find(
                        |arg| matches!(&arg.kind, Expression::NamedArgument(label, _) if label == "reverse"),
                    )
                    && self.place_type(base).is_some_and(|ty| match ty {
                        Ty::Ref(inner, _) => matches!(&*inner, Ty::List(_)),
                        other => matches!(other, Ty::List(_)),
                    })
                {
                    return Err(named.at.error(if name == "sort" {
                        "`sort` takes the descending flag positionally: `items.sort(True)`"
                    } else {
                        "`sort_by_key` takes the descending flag positionally: `items.sort_by_key(key, True)`"
                    }));
                }
                ide::enter_method(*pos, name);
                let ty = self.method(base, name, args, ops);
                ide::leave_method();
                ty?
            }
            Expression::GenericMethod(base, name, types, args, pos) => {
                ide::enter_method(*pos, name);
                let ty = self.generic_method(base, name, types, args, ops);
                ide::leave_method();
                ty?
            }
            Expression::Constructor(ty, args) => {
                if ty.name.as_deref() == Some("channel") {
                    return self.channel_new(ty, args, ops);
                }
                let ty = ty.resolve(self.aliases)?.unwrap();
                Some(self.construct(ty, args, &e.at, ops)?)
            }
            Expression::Number(n) => Some(self.number(n, false, &e.at, ops)?),
            Expression::Text(s) => {
                ops.push(Op::PushStr(self.heap.add_str(s.clone())));
                Some(Ty::Str)
            }
            Expression::Bytes(b) => {
                ops.push(Op::PushBytes(self.heap.add_bytes(b.as_slice().into())));
                Some(Ty::ByteStr)
            }
            Expression::Bool(b) => {
                ops.push(Op::PushBool(*b));
                Some(Ty::Bool)
            }
            Expression::Unit => None,
            Expression::Ambiguous { .. } => unreachable!("resolved with module names"),
            Expression::Group(inner) => self.expr(inner, ops)?,
            Expression::Name(name) => {
                if self.captures.contains(name) {
                    let ty = self.place_type(e).expect("capture parameter");
                    if ty.affine() {
                        return Err(e
                            .at
                            .error("cannot move out of a closure capture; borrow it or use copy"));
                    }
                    let (ty, loans) = self.observe(e, ops)?;
                    Self::end_reads(loans, ops);
                    return Ok(Some(ty));
                }
                if enums::prelude_variant(name) && !self.names.contains_key(name) {
                    return self.prelude_constructor(name, None, None, &e.at, ops);
                }
                if !self.names.contains_key(name)
                    && let Some(Some(ty @ Ty::Class(_))) = lookup_type(name, self.aliases)
                {
                    return self.class_constructor_value(ty, &e.at, ops).map(Some);
                }
                if !self.names.contains_key(name) && self.sigs.contains_key(name) {
                    return self.function_value(name, &e.at, ops).map(Some);
                }
                if !self.names.contains_key(name) && self.aliases.data.is_class(name) {
                    return Err(e.at.error(format!("generic constructor `{name}` requires explicit type arguments or an expected Callable type")));
                }
                if !self.names.contains_key(name) && self.generics.templates.contains_key(name) {
                    return Err(e.at.error("generic function values require explicit type arguments, such as function[u8]"));
                }
                let local = self
                    .names
                    .get(name)
                    .ok_or_else(|| e.at.error(self.unknown_binding(name)))?;
                if matches!(local.ty, Ty::Task(_)) {
                    return Err(e.at.error("scoped tasks cannot escape or be copied; use task.join() inside its with block"));
                }
                ide::local_type(&e.at, &local.ty);
                self.mark(&e.at, ops);
                if let Some(loan) = self.reference_locals.get(&local.slot) {
                    ops.push(Op::UseLoan(*loan));
                }
                if let Ty::Closure(t) = &local.ty
                    && let Some(loans) = self.closure_loans.get(&t.name)
                {
                    ops.extend(loans.iter().copied().map(Op::UseLoan));
                }
                if local.ty.affine() {
                    ops.push(Op::MoveLocal(local.slot));
                } else {
                    if !matches!(local.ty, Ty::Ref(..)) {
                        ops.push(Op::Access(local.slot, false, None));
                    }
                    ops.push(Op::LoadLocal(local.slot));
                }
                if local.ty == Ty::Unit {
                    ops.push(Op::Drop);
                    None
                } else {
                    Some(local.ty.clone())
                }
            }
            Expression::Unary(op, value) => {
                if let Some(result) = self.overloaded_unary(op, value, ops)? {
                    return Ok(result);
                }
                if op == "&" || op == "&mut" {
                    return self
                        .borrow(value, op == "&mut", ops)
                        .map(|(ty, _)| Some(ty));
                }
                if op == "*" && matches!(self.place_type(value), Some(Ty::Shared(_))) {
                    return self.shared_read(e, ops);
                }
                if op == "*" {
                    let ty = match self.value(value, ops)? {
                        // Moving the content out consumes the box.
                        Ty::Box(content) => {
                            ops.push(Op::Box(crate::boxed::BoxOp::Take((*content).clone())));
                            return Ok(Some((*content).clone()));
                        }
                        Ty::Ref(ty, _) => ty,
                        Ty::Shared(_) => {
                            return Err(value.at.error(
                                "a temporary Shared owner has no place to borrow from; bind it to a name first",
                            ))
                        }
                        _ => {
                            return Err(value
                                .at
                                .error("dereference requires a reference, a Box, or a Shared handle"))
                        }
                    };
                    let loan = self.reference_origin(value, ops)?;
                    if let Ty::Atomic(_) = &*ty {
                        return Err(e.at.error("an atomic is read with load(), not through `*`"));
                    }
                    if ty.affine() {
                        return Err(self.value_demand_error(
                            &e.at,
                            e,
                            "cannot move out of a reference; use the reference instead of dereferencing it, or copy the value with copy(reference)?",
                            crate::ownership::Demand::Copy(
                                (*ty).clone(),
                                "cannot move out of a reference",
                            ),
                            false,
                        ));
                    }
                    ops.push(Op::Access(self.loans[loan].root, false, Some(loan)));
                    ops.push(Op::ReadRef((*ty).clone()));
                    ops.push(Op::UseLoan(loan));
                    if *ty == Ty::Unit {
                        ops.push(Op::Drop);
                        return Ok(None);
                    }
                    return Ok(Some((*ty).clone()));
                }
                if op == "-"
                    && let Expression::Number(n) = &value.kind
                {
                    return self.number(n, true, &e.at, ops).map(Some);
                }
                let mut body = Vec::new();
                let ty = self.value(value, &mut body)?;
                if op == "~" {
                    if !ty.is_int() {
                        return Err(e.at.error("bitwise complement requires an integer"));
                    }
                    ops.extend(body);
                    ops.push(Op::Integer(IntegerOp::Not));
                } else if op == "not" {
                    self.same(Some(ty.clone()), Some(Ty::Bool), &e.at)?;
                    ops.extend(body);
                    ops.push(Op::Not);
                } else {
                    if !ty.is_numeric() {
                        return Err(e.at.error("unary arithmetic requires a number"));
                    }
                    if ty.is_float() {
                        ops.extend(body);
                        if op == "-" {
                            ops.push(Op::FloatNeg);
                        }
                        return Ok(Some(ty));
                    }
                    if op == "-" {
                        reject_unsigned_negation(&ty, &e.at)?;
                        ops.push(Op::PushInt(integer(&format!("0{ty}"), false, &e.at)?));
                    }
                    ops.extend(body);
                    if op == "-" {
                        self.mark(&e.at, ops);
                        if !self.checked_arith(crate::numeric::Checked::Sub, &ty, &e.at, ops)? {
                            ops.push(Op::Sub);
                        }
                    }
                }
                Some(ty.clone())
            }
            Expression::Binary(op, left, right) if op == "in" || op == "not in" => {
                let (a, mut loans) = self.observe(left, ops)?;
                let (b, other_loans) = self.observe(right, ops)?;
                loans.extend(other_loans);
                if b == text_view() {
                    let needle_text = match a {
                        Ty::Str => true,
                        _ if a == text_view() => false,
                        _ => {
                            return Err(e.at.error(format!(
                                "`{op}` on a `&substr` looks for a `str` or `&substr`, not {a}"
                            )));
                        }
                    };
                    ops.push(Op::Swap);
                    let mode = crate::collection_opcodes::TEXT_SEARCH_CONTAINS << 1;
                    ops.push(Op::PushInt(Value::I64(mode | i64::from(needle_text))));
                    ops.push(Op::Collection(CollectionOp::TextView(
                        crate::text::TextViewOp::Search {
                            needle_text,
                            find: false,
                        },
                    )));
                    Self::end_reads(loans, ops);
                    if op == "not in" {
                        ops.push(Op::Not);
                    }
                    return Ok(Some(Ty::Bool));
                }
                if a == text_view() && b == Ty::Str {
                    return Err(e.at.error(format!(
                        "`{op}` on a `str` looks for a `str`; search for a `&substr` with `&s[:]`, as `v in &s[:]`"
                    )));
                }
                if let Ty::View(..) = b {
                    return Err(e.at.error(format!(
                        "slice views do not support `{op}` yet; compare the elements in a loop"
                    )));
                }
                if b.restricted_storage() && !matches!(b, Ty::KeyView(_)) {
                    return Err(e
                        .at
                        .error("membership does not consume generators; use a loop"));
                }
                let element = b
                    .element()
                    .ok_or_else(|| {
                        e.at.error(format!(
                            "membership is not defined for `{b}`; `in` and `not in` do not call user methods; search explicitly using the container's API"
                        ))
                    })?;
                if element.recursive_data() {
                    return Err(e.at.error("automatic equality is not supported for recursive data; search using selected fields"));
                }
                if element.facts().closure {
                    return Err(e
                        .at
                        .error("values containing closures do not support membership equality"));
                }
                if element.facts().concurrent_handle {
                    return Err(e.at.error(
                        "values containing channel endpoints, executors, or futures do not support membership equality",
                    ));
                }
                if element.facts().shared {
                    return Err(e.at.error(shared::EQUALITY));
                }
                if element.facts().atomic {
                    let subject = if mutex::holds_mutex(&element) {
                        "mutexes"
                    } else {
                        "atomics"
                    };
                    return Err(e.at.error(format!(
                        "values containing {subject} do not support membership equality"
                    )));
                }
                self.same(
                    Some(a),
                    Some(if b == Ty::Str { Ty::Str } else { element }),
                    &e.at,
                )?;
                ops.push(Op::Collection(CollectionOp::Contains(b)));
                Self::end_reads(loans, ops);
                if op == "not in" {
                    ops.push(Op::Not);
                }
                Some(Ty::Bool)
            }
            Expression::Binary(op, left, right) => {
                if let Some(result) = self.overloaded_binary(op, left, right, &e.at, ops)? {
                    return Ok(result);
                }
                if op == "*"
                    && let Some(display) = [left, right].into_iter().find_map(|e| list_display(e))
                {
                    return Err(display.at.error(LIST_REPETITION));
                }
                if op != "and"
                    && op != "or"
                    && let Some(hint) = self.numeric_hint(left).or_else(|| self.numeric_hint(right))
                {
                    return self
                        .numeric_binary(op, left, right, hint, &e.at, ops)
                        .map(Some);
                }
                let (a, mut loans) = self.observe(left, ops)?;
                let mut rhs = Vec::new();
                let (b, other_loans) = if a.is_numeric() {
                    self.observe_expected(right, Some(a.clone()), &mut rhs)?
                } else if matches!(a, Ty::KeyView(_))
                    && matches!(op.as_str(), "==" | "!=")
                    && list_display(right).is_none()
                {
                    self.observe_expected(
                        right,
                        Some(Ty::Set(Arc::new(a.element().unwrap()))),
                        &mut rhs,
                    )?
                } else {
                    self.observe(right, &mut rhs)?
                };
                if op == "and" || op == "or" {
                    Self::end_reads(other_loans, &mut rhs);
                } else {
                    loans.extend(other_loans);
                }
                if matches!(op.as_str(), "==" | "!=")
                    && (matches!(a, Ty::KeyView(_)) || matches!(b, Ty::KeyView(_)))
                {
                    if !matches!(a, Ty::Set(_) | Ty::KeyView(_))
                        || !matches!(b, Ty::Set(_) | Ty::KeyView(_))
                        || a.element() != b.element()
                    {
                        return Err(e.at.error(
                            "KeyView equality requires a set or KeyView with the same key type",
                        ));
                    }
                    ops.extend(rhs);
                    ops.push(Op::Collection(CollectionOp::key_set_relation(
                        a,
                        b,
                        crate::collection_opcodes::KEY_VIEW_EQUAL,
                    )));
                    Self::end_reads(loans, ops);
                    if op == "!=" {
                        ops.push(Op::Not);
                    }
                    return Ok(Some(Ty::Bool));
                }
                if matches!(a, Ty::KeyView(_)) || matches!(b, Ty::KeyView(_)) {
                    return Err(e.at.error(format!("KeyView does not support binary operator `{op}`; use ==/!= for set equality or the set relationship/algebra methods")));
                }
                if matches!(a, Ty::View(..)) || matches!(b, Ty::View(..)) {
                    return self.view_equality(op, (a, b), rhs, loans, &e.at, ops);
                }
                if a.restricted_storage() {
                    if matches!(a, Ty::Closure(_)) {
                        return Err(e.at.error("closures do not support binary operators"));
                    }
                    return Err(e.at.error("generators do not support binary operators"));
                }
                if a.is_numeric() && b.is_numeric() && a != b {
                    return Err(mixed_numeric(left, &a, right, &b));
                }
                if op == "*" {
                    match (&a, &b) {
                        (Ty::List(_), other) if other.is_int() => {
                            return Err(left.at.error(LIST_REPETITION));
                        }
                        (other, Ty::List(_)) if other.is_int() => {
                            return Err(right.at.error(LIST_REPETITION));
                        }
                        (Ty::Str, other) if other.is_int() => {
                            return Err(left.at.error(STR_REPETITION));
                        }
                        (other, Ty::Str) if other.is_int() => {
                            return Err(right.at.error(STR_REPETITION));
                        }
                        _ => {}
                    }
                }
                self.same(Some(b.clone()), Some(a.clone()), &e.at)?;
                if matches!(op.as_str(), "==" | "!=") && a.facts().closure {
                    return Err(e
                        .at
                        .error("values containing closures do not support equality"));
                }
                if matches!(op.as_str(), "==" | "!=") && a.facts().concurrent_handle {
                    return Err(e
                        .at
                        .error("values containing channel endpoints, executors, or futures do not support equality"));
                }
                if matches!(op.as_str(), "==" | "!=") && a.facts().shared {
                    return Err(e.at.error(shared::EQUALITY));
                }
                if matches!(op.as_str(), "==" | "!=") && a.facts().atomic {
                    if mutex::holds_mutex(&a) {
                        return Err(e.at.error(
                            "values containing mutexes do not support equality; compare values read inside `with mutex.lock() as value:`",
                        ));
                    }
                    return Err(e.at.error(
                        "values containing atomics do not support equality; compare values read with load()",
                    ));
                }
                if matches!(op.as_str(), "==" | "!=") && a.recursive_data() {
                    return Err(e.at.error("automatic equality is not supported for recursive data; compare fields explicitly"));
                }
                if op == "and" || op == "or" {
                    self.same(Some(a.clone()), Some(Ty::Bool), &e.at)?;
                    let constant = vec![Op::PushBool(op == "or")];
                    let (yes, no) = if op == "and" {
                        (rhs, constant)
                    } else {
                        (constant, rhs)
                    };
                    ops.push(branch(yes, no));
                    Self::end_reads(loans, ops);
                    Some(Ty::Bool)
                } else {
                    if !a.is_numeric()
                        && !(matches!(op.as_str(), "==" | "!=")
                            || (a == Ty::Str && op == "+")
                            || (a.is_orderable() && is_comparison(op)))
                    {
                        return Err(self.value_demand_error(
                            &e.at,
                            e,
                            format!(
                                "operator `{op}` does not accept {}{}",
                                type_name(Some(a.clone())),
                                operators::newtype_hint(&a)
                            ),
                            crate::ownership::Demand::Operator(op.clone(), a.clone()),
                            false,
                        ));
                    }
                    if op == "/" && !a.is_float() {
                        return Err(e
                            .at
                            .error("use `//` for integer division; `/` requires floats"));
                    }
                    if matches!(op.as_str(), "//" | "%") && a.is_float() {
                        return Err(e
                            .at
                            .error("floor division and modulo currently require integers"));
                    }
                    ops.extend(rhs);
                    if let Some(kind) = IntegerOp::binary(op) {
                        if !a.is_int() {
                            return Err(e.at.error("bitwise operations require integers"));
                        }
                        ops.push(Op::Integer(kind));
                        Self::end_reads(loans, ops);
                        return Ok(Some(a));
                    }
                    if op == "+" && a == Ty::Str {
                        ops.push(Op::Collection(CollectionOp::TextTryConcat));
                        Self::end_reads(loans, ops);
                        return Ok(Some(crate::sum::result(Ty::Str, crate::sum::alloc_error())));
                    }
                    self.mark_arithmetic(op, &a, &e.at, ops);
                    if a.is_int()
                        && let Some(operation) = crate::numeric::Checked::named(op)
                        && self.checked_arith(operation, &a, &e.at, ops)?
                    {
                        Self::end_reads(loans, ops);
                        return Ok(Some(a));
                    }
                    ops.push(match op.as_str() {
                        "+" => Op::Add,
                        "-" => Op::Sub,
                        "*" => Op::Mul,
                        "/" => Op::Div,
                        "//" => Op::FloorDiv(crate::numeric::Divisor::Any),
                        "%" => Op::Modulo(crate::numeric::Divisor::Any),
                        "==" => Op::Eq,
                        "!=" => Op::Ne,
                        "<" => Op::Lt,
                        ">" => Op::Gt,
                        "<=" => Op::Le,
                        ">=" => Op::Ge,
                        _ => unreachable!(),
                    });
                    Self::end_reads(loans, ops);
                    if is_comparison(op) {
                        Some(Ty::Bool)
                    } else {
                        Some(a.clone())
                    }
                }
            }
            Expression::Conditional { condition, yes, no } => {
                let cond = self.expr(condition, ops)?;
                self.same(cond, Some(Ty::Bool), &condition.at)?;
                let (mut a, mut b) = (Vec::new(), Vec::new());
                let ty = self.expr(yes, &mut a)?;
                let other = self.expr(no, &mut b)?;
                self.same(other, ty.clone(), &e.at)?;
                ops.push(branch(a, b));
                ty
            }
            Expression::Call(name, args) => {
                if name == "print"
                    && !self.names.contains_key(name)
                    && let Some(named) = args
                        .iter()
                        .find(|arg| matches!(arg.kind, Expression::NamedArgument(..)))
                {
                    return Err(named.at.error("`print` takes no named arguments such as `end` or `sep`: it separates values with one space and ends the line; build the line first, such as with an f-string, and print it once"));
                }
                if matches!(name.as_str(), "enumerate" | "zip" | "reversed")
                    && !self.names.contains_key(name)
                    && !self.sigs.contains_key(name)
                    && !self.generics.templates.contains_key(name)
                    && !self.aliases.contains_key(name)
                {
                    return Err(e.at.error(format!(
                        "{name} is supported directly in for loops and comprehensions"
                    )));
                }
                if matches!(name.as_str(), "any" | "all")
                    && !self.names.contains_key(name)
                    && !self.sigs.contains_key(name)
                    && !self.generics.templates.contains_key(name)
                    && !self.aliases.contains_key(name)
                {
                    // B75 replaced the free boolean reductions with pipeline terminals.
                    return Err(e.at.error(format!(
                        "{name} is not a builtin function; use the pipeline terminal: \
                         iter(flags).{name}() for bool items, or iter(source).{name}(predicate)"
                    )));
                }
                if matches!(name.as_str(), "ord" | "chr")
                    && !self.names.contains_key(name)
                    && !self.sigs.contains_key(name)
                {
                    let [argument] = args.as_slice() else {
                        return Err(e.at.error(format!("{name} takes one argument")));
                    };
                    let operation = if name == "ord" {
                        CollectionOp::TextOrd
                    } else {
                        CollectionOp::TextChr
                    };
                    let (inputs, output) = operation.signature();
                    let (actual, loans) =
                        self.observe_expected(argument, Some(inputs[0].clone()), ops)?;
                    self.same(Some(actual), Some(inputs[0].clone()), &argument.at)?;
                    Self::push_query(operation, ops);
                    Self::end_reads(loans, ops);
                    return Ok(Some(output));
                }
                if matches!(name.as_str(), "__min_replaces" | "__max_replaces") {
                    return self.extremum_call(name, args, &e.at, ops).map(Some);
                }
                if name == "CancellationToken" {
                    return self.control_new(args, &e.at, ops);
                }
                if name == crate::atomic::ATOMIC {
                    return self.atomic_new(None, args, None, &e.at, ops);
                }
                if name == crate::atomic::FENCE {
                    return self.atomic_fence(args, &e.at, ops);
                }
                if matches!(
                    name.as_str(),
                    "select_recv"
                        | "select_recv_nowait"
                        | "select_recv_timeout"
                        | "select_recv_timeout_ms"
                ) {
                    return self.channel_select(name, args, &e.at, ops);
                }
                if name == "ThreadPoolExecutor" {
                    return self.executor_new(args, &e.at, ops);
                }
                if name == "spawn" {
                    return Err(e.at.error("spawn is a scoped operation; use with spawn(worker, arguments...)? as task:"));
                }
                if matches!(name.as_str(), "__actor_run" | "__actor_forget")
                    && !self.names.contains_key(name)
                    && !self.sigs.contains_key(name)
                {
                    // Raw heap entry and release; std.actor wraps them.
                    let standard =
                        e.at.source()
                            .as_ref()
                            .is_some_and(|source| self.generics.source_facts(source).1);
                    if !standard {
                        return Err(e
                            .at
                            .error(format!("`{name}` is reserved for the standard library")));
                    }
                    return if name == "__actor_run" {
                        self.actor_run(args, &e.at, ops)
                    } else {
                        self.actor_forget(args, &e.at, ops)
                    };
                }
                if self.names.contains_key(name) {
                    let callee = Expr {
                        at: e.at.clone(),
                        kind: Expression::Name(name.clone()),
                    };
                    return self.call_value(&callee, args, ops);
                }
                if name == "print" {
                    let mut types = Vec::new();
                    let mut reads = Vec::new();
                    for argument in args {
                        let (ty, loans) = self.observe(argument, ops)?;
                        if ty.dictionary_view() {
                            return Err(argument.at.error("dictionary key views cannot be printed; iterate the view or materialise keys_snapshot(), into_keys(), or to_set()"));
                        }
                        if ty.prints_generator_frame() {
                            return Err(e.at.error("generators cannot be printed"));
                        }
                        if ty.facts().shared {
                            return Err(e.at.error(
                                "Shared owners cannot be printed; print values read through the handle",
                            ));
                        }
                        if ty.facts().atomic && mutex::holds_mutex(&ty) {
                            return Err(e.at.error(
                                "mutexes cannot be printed; print a value read inside `with mutex.lock() as value:`",
                            ));
                        }
                        if ty.facts().atomic {
                            return Err(e.at.error(
                                "atomics cannot be printed; print a value read with load()",
                            ));
                        }
                        if ty.recursive_data() {
                            return Err(e.at.error("automatic formatting is not supported for recursive data; print selected fields"));
                        }
                        if matches!(&ty, Ty::Enum(t) if t.is_result()) {
                            // Observation also accepts references and borrowed owners.
                            // Do not advise moving them with `?` based on type alone.
                            let unborrowed = loans.is_empty()
                                && !matches!(&ungroup(argument).kind, Expression::Name(name)
                                if self.names.get(name).is_some_and(|local| matches!(local.ty, Ty::Ref(..))));
                            let hint = self.result_success(&ty).filter(|success| {
                                unborrowed
                                    && *success != Ty::Unit
                                    && !matches!(success, Ty::Enum(t) if t.is_result())
                            });
                            return Err(argument.at.error(format!(
                            "print cannot print a Result directly; {}match the result explicitly, or use `str.repr` for intentional wrapper formatting",
                            if hint.is_some() { "use `?` on the argument to propagate its error, " } else { "" }
                        )));
                        }
                        types.push(ty);
                        reads.extend(loans);
                    }
                    let operation = if args.len() == 1 {
                        CollectionOp::TryPrint(types[0].clone())
                    } else {
                        CollectionOp::TryPrintMany(types.clone())
                    };
                    let operation = self.formatted(operation, &types, true, &e.at)?;
                    let output = operation.signature().1;
                    ops.push(Op::Collection(operation));
                    Self::end_reads(reads, ops);
                    return Ok(Some(output));
                }
                if name == "open" {
                    return self.open_file(args, &e.at, ops);
                }
                if let Some(operation) = crate::filesystem::FsOp::named(name) {
                    return self.filesystem_call(operation, args, &e.at, ops);
                }
                if let Some(operation) = crate::bytes::BytesOp::named(name) {
                    return self.bytes_call(operation, args, &e.at, ops);
                }
                if std_only_intrinsic(name) && !self.names.contains_key(name) {
                    // Native back ends of std modules; their std callers
                    // check ranges and map results to the public errors.
                    let standard =
                        e.at.source()
                            .as_ref()
                            .is_some_and(|source| self.generics.source_facts(source).1);
                    if !standard {
                        return Err(e
                            .at
                            .error(format!("`{name}` is reserved for the standard library")));
                    }
                    if name == BYTES_ADOPT {
                        return self.bytes_adopt(args, &e.at, ops);
                    }
                    if name == TRAP {
                        return Err(e
                            .at
                            .error("`__trap` is a statement; it does not produce a value"));
                    }
                }
                if let Some(operation) = crate::numeric::FloatBits::named(name) {
                    return self.float_bits_call(operation, args, &e.at, ops).map(Some);
                }
                if let Some(operation) = crate::stream::StreamOp::named(name) {
                    let exclusive = |index| operation.exclusive(index);
                    let call = CollectionOp::Stream(operation);
                    return self.exclusive_call(call, &exclusive, args, &e.at, ops);
                }
                if let Some(operation) = crate::crypto::CryptoOp::named(name) {
                    let exclusive = |index| operation.exclusive(index);
                    let call = CollectionOp::Crypto(operation);
                    return self.exclusive_call(call, &exclusive, args, &e.at, ops);
                }
                if let Some(operation) = crate::compress::CompressOp::named(name) {
                    let exclusive = |index| operation.exclusive(index);
                    let call = CollectionOp::Compress(operation);
                    return self.exclusive_call(call, &exclusive, args, &e.at, ops);
                }
                if let Some(operation) = crate::process::ProcessOp::named(name) {
                    return self.system_call(CollectionOp::Process(operation), args, &e.at, ops);
                }
                if let Some(operation) = crate::time::TimeOp::named(name) {
                    let output =
                        self.system_call(CollectionOp::Time(operation), args, &e.at, ops)?;
                    if output == Some(Ty::Unit) {
                        ops.push(Op::Drop);
                        return Ok(None);
                    }
                    return Ok(output);
                }
                if let Some(operation) = crate::unicode::UnicodeOp::named(name) {
                    return self.system_call(CollectionOp::Unicode(operation), args, &e.at, ops);
                }
                if let Some(operation) = crate::regex::RegexOp::named(name) {
                    return self.system_call(CollectionOp::Regex(operation), args, &e.at, ops);
                }
                if let Some(operation) = crate::text::TextOp::named(name) {
                    if operation == crate::text::TextOp::FindIn {
                        return self.text_find_in(args, &e.at, ops);
                    }
                    if args.len() == 2
                        && args.iter().all(|arg| self.place_type(arg) == Some(Ty::Str))
                    {
                        // Compare text places in place (issue #364).
                        let mut loans = Vec::new();
                        for arg in args {
                            loans.push(self.borrow(arg, false, ops)?.1);
                        }
                        ops.push(Op::Collection(CollectionOp::QueryRef(
                            crate::collection::Query::TextAsciiEqualFolded,
                        )));
                        Self::end_reads(loans, ops);
                        return Ok(Some(Ty::Bool));
                    }
                    return self.system_call(CollectionOp::Text(operation), args, &e.at, ops);
                }
                if let Some(operation) = crate::net::NetOp::named(name) {
                    let exclusive = |index| operation.exclusive(index);
                    let call = CollectionOp::Net(operation);
                    return self.exclusive_call(call, &exclusive, args, &e.at, ops);
                }
                if let Some(operation) = crate::sqlite::SqliteOp::named(name) {
                    return self.system_call(CollectionOp::Sqlite(operation), args, &e.at, ops);
                }
                if let Some(operation) = crate::tls::TlsOp::named(name) {
                    let exclusive = |index| operation.exclusive(index);
                    let call = CollectionOp::Tls(operation);
                    return self.exclusive_call(call, &exclusive, args, &e.at, ops);
                }
                if enums::prelude_variant(name) {
                    return self.prelude_constructor(name, Some(args), None, &e.at, ops);
                }
                if matches!(name.as_str(), "copy" | "drop") {
                    if name == "drop"
                        && let [value] = args.as_slice()
                        && self.is_pipeline(value)
                    {
                        let plan = self.pipeline(value, ops)?;
                        self.finish_pipeline(&plan, ops);
                        return Ok(None);
                    }
                    return self.copy_or_drop(name, args, &e.at, ops);
                }
                if name == "sorted" {
                    return self.sorted(args, &e.at, ops);
                }
                if name == "replace"
                    && !self.sigs.contains_key(name)
                    && !self.generics.templates.contains_key(name)
                    && !self.aliases.contains_key(name)
                {
                    return self.replace_field(args, &e.at, ops).map(Some);
                }
                if name == "BoxedCallable" {
                    return self.boxed_callable(args, &e.at, ops).map(Some);
                }
                if name == "Shared" {
                    return self.shared_new(None, args, &e.at, ops);
                }
                if name == "Box" {
                    let [content] = args.as_slice() else {
                        return Err(e.at.error("Box takes one value"));
                    };
                    let ty = self.value(content, ops)?;
                    if let Some(barrier) = ty.storage_barrier() {
                        return Err(content.at.error(barrier.message("a box")));
                    }
                    let operation = crate::boxed::BoxOp::New(ty);
                    let output = operation.signature().1;
                    ops.push(Op::Box(operation));
                    return Ok(Some(output));
                }
                if name == "next" {
                    return self.next(args, &e.at, ops);
                }
                if let Some(operation) = match name.as_str() {
                    "__caller_location" => Some(CollectionOp::CallerLocation),
                    "__test_fault" => Some(CollectionOp::TestFault),
                    "__actor_heap" => Some(CollectionOp::ActorHeap),
                    "__report_write" => Some(CollectionOp::ReportWrite),
                    "__io_is_terminal" => Some(CollectionOp::IsTerminal),
                    _ => None,
                } {
                    // Trusted std hooks are not user-visible API.
                    let standard =
                        e.at.source()
                            .as_ref()
                            .is_some_and(|source| self.generics.source_facts(source).1);
                    if !standard {
                        return Err(e
                            .at
                            .error(format!("`{name}` is reserved for the standard library")));
                    }
                    return self.system_call(operation, args, &e.at, ops);
                }
                if let Some(operation) = match name.as_str() {
                    "write_stdout" => Some(CollectionOp::WriteStdout),
                    "write_stderr" => Some(CollectionOp::WriteStderr),
                    "flush_stdout" => Some(CollectionOp::FlushStdout),
                    "flush_stderr" => Some(CollectionOp::FlushStderr),
                    "input" => Some(CollectionOp::Input),
                    "args" => Some(CollectionOp::Args),
                    "args_bytes" => Some(CollectionOp::ArgsBytes),
                    "input_bytes" => Some(CollectionOp::InputBytes),
                    "encode_utf8" => Some(CollectionOp::EncodeUtf8),
                    "decode_utf8" => Some(CollectionOp::DecodeUtf8),
                    "write_stdout_bytes" => Some(CollectionOp::WriteStdoutBytes),
                    "write_stderr_bytes" => Some(CollectionOp::WriteStderrBytes),
                    "read_bytes" => Some(CollectionOp::ReadBytes),
                    "write_bytes" => Some(CollectionOp::WriteBytes),
                    "read_text" => Some(CollectionOp::ReadText),
                    "write_text" => Some(CollectionOp::WriteText),
                    "append_text" => Some(CollectionOp::AppendText),
                    _ => None,
                } {
                    return self.system_call(operation, args, &e.at, ops);
                }
                if matches!(name.as_str(), "len" | "range" | "list" | "set" | "dict") {
                    return self.builtin_collection(name, args, &e.at, ops).map(Some);
                }
                if let Some(Some(Ty::Class(t))) = lookup_type(name, self.aliases) {
                    if Ty::Class(t.clone()).layout_depth() >= 64 {
                        return Err(e
                            .at
                            .error("type nesting exceeds the implementation limit of 64"));
                    }
                    modules::check_member(self.access, &t.name, "__new__", &e.at)?;
                    return self.call_named(
                        &crate::record::method(&t.name, "new"),
                        args,
                        &e.at,
                        ops,
                    );
                }
                match lookup_type(name, self.aliases) {
                    Some(target) => {
                        if let Some(target) = target
                            .as_ref()
                            .filter(|t| t.is_collection() || matches!(t, Ty::Clist(_)))
                        {
                            return self.construct(target.clone(), args, &e.at, ops).map(Some);
                        }
                        let target = target.filter(|ty| ty.is_numeric()).ok_or_else(|| {
                        e.at.error(if lookup_type(name, self.aliases) == Some(Some(Ty::Str)) {
                            "only numeric types support cast syntax; format a number or boolean as text with `str.from(value)?`"
                        } else {
                            "only numeric types support cast syntax"
                        })
                    })?;
                        if args.len() != 1 {
                            return Err(e.at.error("numeric casts take one argument"));
                        }
                        let source = self.value(&args[0], ops)?;
                        if !source.is_numeric() {
                            return Err(casts::operand_error(
                                "numeric casts require a number",
                                &source,
                                Ty::is_numeric,
                                &args[0],
                                &e.at,
                            ));
                        }
                        self.checked_cast(&source, &target, &args[0], &e.at, ops)?;
                        Some(target)
                    }
                    _ => {
                        if name == "contains" {
                            if args.len() != 2 {
                                return Err(e.at.error("contains takes two strings"));
                            }
                            let mut loans = Vec::new();
                            for arg in args {
                                let (ty, reads) = self.observe_expected(arg, Some(Ty::Str), ops)?;
                                self.same(Some(ty), Some(Ty::Str), &arg.at)?;
                                loans.extend(reads);
                            }
                            ops.push(Op::Contains);
                            Self::end_reads(loans, ops);
                            Some(Ty::Bool)
                        } else {
                            self.call_named(name, args, &e.at, ops)?
                        }
                    }
                }
            }
            Expression::Invoke(callee, args) => self.call_value(callee, args, ops)?,
        };
        Ok(ty)
    }
    fn call_named(
        &mut self,
        name: &str,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        self.call_named_with_receiver(name, args, at, false, ops)
    }
    fn call_named_with_receiver(
        &mut self,
        name: &str,
        args: &[Expr],
        at: &Token,
        method: bool,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        self.sync_data()?;
        if let Some((callee, arguments)) = self.prepare_named_call(name, args, at, method)? {
            return self.call_named_with_receiver(&callee, &arguments, at, method, ops);
        }
        if self.generics.templates.contains_key(name) {
            return self.generic_call(name, None, args, at, ops);
        }
        if self
            .sigs
            .get(name)
            .is_some_and(|s| s.inputs.iter().any(|(_, t)| t.unresolved_generator()))
        {
            return self.frame_call_with_receiver(name, args, at, method, ops);
        }
        self.ensure_concrete_output(name, at)?;
        let sig = self
            .sigs
            .get(name)
            .ok_or_else(|| at.error(unknown_function(name)))?
            .clone();
        if sig.inputs.len() != args.len() {
            return Err(at.error(format!(
                "`{name}` expects {} arguments, got {}",
                sig.inputs.len(),
                args.len()
            )));
        }
        if sig.outputs.len() > 1 {
            return Err(
                at.error("legacy multi-result functions cannot be called from modern Plenty")
            );
        }
        let start = ops.len();
        let argument_loans = if method && matches!(sig.inputs.first(), Some((_, Ty::Ref(_, true))))
        {
            let mut loans = self.call_arguments(&args[..1], &sig.inputs[..1], ops)?;
            self.reserve_receiver(loans[0], ops);
            loans.extend(self.call_arguments(&args[1..], &sig.inputs[1..], ops)?);
            self.mark(at, ops);
            loans[0] = self.activate_receiver(loans[0], ops);
            loans
        } else {
            self.call_arguments(args, &sig.inputs, ops)?
        };
        self.record_direct_call(name, at, &argument_loans, ops);
        self.passes(start, &sig.inputs, sig.outputs.first(), ops);
        self.push_call(
            Op::Call(name.to_owned()),
            &sig.inputs,
            &argument_loans,
            at,
            ops,
        )?;
        self.call_reference_result(name, &sig, &argument_loans, ops);
        Self::end_reads(argument_loans, ops);
        Ok(sig.outputs.first().cloned())
    }
    fn call_borrow(
        &mut self,
        arg: &Expr,
        mutable: bool,
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<(Ty, usize)> {
        let base = match &ungroup(arg).kind {
            Expression::Unary(op, base) if op == if mutable { "&mut" } else { "&" } => &**base,
            Expression::Name(name)
                if self
                    .names
                    .get(name)
                    .is_some_and(|l| matches!(l.ty, Ty::Ref(..))) =>
            {
                arg
            }
            _ if !mutable => arg,
            _ => {
                return Err(arg
                    .at
                    .error("mutable reference arguments require explicit &mut borrowing or an existing exclusive reference"))
            }
        };
        if mutable {
            if let Some(expected) = &expected
                && let Some(ty) = self.constructor_expected(base, expected, ops)?
            {
                let slot = self.slot(ty.clone(), &base.at)?;
                ops.push(Op::StoreLocal(slot));
                let loan = self.new_loan(slot, true, None, ops);
                ops.push(Op::BorrowLocal(slot, true));
                return Ok((Ty::Ref(Arc::new(ty), true), loan));
            }
            self.borrow(base, true, ops)
        } else {
            self.shared_argument(base, expected, ops)
        }
    }
    fn call_arguments(
        &mut self,
        args: &[Expr],
        inputs: &[(String, Ty)],
        ops: &mut Vec<Op>,
    ) -> Result<Vec<usize>> {
        let mut argument_loans = Vec::new();
        for (arg, (_, expected)) in args.iter().zip(inputs) {
            if let Ty::View(_, mutable) = expected {
                let text = *expected == text_view();
                let (ty, loan) = self.view_argument(arg, *mutable, text, ops)?;
                self.same(Some(ty), Some(expected.clone()), &arg.at)?;
                argument_loans.push(loan);
                continue;
            }
            if let Ty::Ref(inner, mutable) = expected {
                let (ty, loan) = self.call_borrow(arg, *mutable, Some((**inner).clone()), ops)?;
                // A borrowed Box or Shared owner also lends its content.
                let ty = if ty == *expected {
                    ty
                } else {
                    self.lend_content(ty, expected, ops)
                };
                if ty != *expected
                    && matches!(&ty, Ty::Ref(inner, true) if matches!(**inner, Ty::Shared(_)))
                {
                    return Err(arg.at.error(shared::MUTATION));
                }
                self.same(Some(ty), Some(expected.clone()), &arg.at)?;
                argument_loans.push(loan);
                continue;
            }
            let ty = self.expr_expected(arg, Some(expected.clone()), ops)?;
            self.same(ty, Some(expected.clone()), &arg.at)?;
        }

        Ok(argument_loans)
    }
    /// Name the future and pool slots that a call's arguments, `ops[start..]`,
    /// move or borrow, for a callee taking one of `inputs` (issue #349). A
    /// future or pool inside an aggregate is not named. The
    /// checker applies the waits and shutdowns the callee can reach to them.
    /// Goes before the call and its lifetime fact.
    pub(super) fn passes(
        &self,
        start: usize,
        inputs: &[(String, Ty)],
        output: Option<&Ty>,
        ops: &mut Vec<Op>,
    ) {
        self.forwards(start, inputs, output, ops);
        // Only a direct future or pool: inspecting aggregates at every call
        // made checking a long function quadratic in its type sizes.
        fn handed(ty: &Ty) -> bool {
            match ty {
                Ty::Future(_) | Ty::Executor => true,
                Ty::Ref(inner, _) => handed(inner),
                _ => false,
            }
        }
        fn slots(ops: &[Op], out: &mut Vec<u8>) {
            for op in ops {
                match op {
                    Op::MoveLocal(slot) | Op::LoadLocal(slot) | Op::BorrowLocal(slot, _)
                        if !out.contains(slot) =>
                    {
                        out.push(*slot)
                    }
                    Op::Match(arms) => arms.iter().for_each(|arm| slots(&arm.body, out)),
                    Op::If {
                        then,
                        elifs,
                        otherwise,
                    } => {
                        slots(then, out);
                        for arm in elifs.iter() {
                            slots(&arm.condition, out);
                            slots(&arm.body, out);
                        }
                        slots(otherwise, out);
                    }
                    _ => {}
                }
            }
        }
        if !inputs.iter().any(|(_, ty)| handed(ty)) {
            return;
        }
        let mut handed_slots = Vec::new();
        slots(&ops[start..], &mut handed_slots);
        handed_slots.retain(|&slot| self.slot_type(slot).is_some_and(|(ty, _)| handed(ty)));
        if !handed_slots.is_empty() {
            ops.push(Op::Passes(handed_slots.into()));
        }
    }
    /// Name the token slots that the arguments of a `spawn` or `submit`,
    /// `ops[start..]`, share, borrow, or move, for the skipped-cancel check
    /// (issue #355). Goes directly before the start or submission. A token
    /// reached through another value's field or a callable's captures is not
    /// named; neither is one in a failing `?`'s cleanup.
    pub(super) fn holds_tokens(&self, start: usize, ops: &mut Vec<Op>) {
        fn token(ty: &Ty) -> bool {
            match ty {
                Ty::CancellationToken => true,
                Ty::Ref(inner, _) => token(inner),
                _ => false,
            }
        }
        fn slots(ops: &[Op], out: &mut Vec<u8>) {
            for op in ops {
                match op {
                    Op::MoveLocal(slot) | Op::LoadLocal(slot) | Op::BorrowLocal(slot, _)
                        if !out.contains(slot) =>
                    {
                        out.push(*slot)
                    }
                    Op::Match(arms) => arms.iter().for_each(|arm| slots(&arm.body, out)),
                    Op::If {
                        then,
                        elifs,
                        otherwise,
                    } => {
                        slots(then, out);
                        for arm in elifs.iter() {
                            slots(&arm.condition, out);
                            slots(&arm.body, out);
                        }
                        slots(otherwise, out);
                    }
                    _ => {}
                }
            }
        }
        let mut held = Vec::new();
        slots(&ops[start..], &mut held);
        held.retain(|&slot| self.slot_type(slot).is_some_and(|(ty, _)| token(ty)));
        if !held.is_empty() {
            ops.push(Op::Tokens(crate::op::TokenMark::Holds(held.into())));
        }
    }
    /// Bracket the arguments, `ops[start..]`, of a call whose `output` can
    /// hold a future, so the checker gives that future the jobs of the
    /// futures the arguments carry and of the jobs the callee submits (issue
    /// #352). Matching is by result type, like the checks themselves.
    fn forwards(
        &self,
        start: usize,
        inputs: &[(String, Ty)],
        output: Option<&Ty>,
        ops: &mut Vec<Op>,
    ) {
        let Some(output) = output else {
            return;
        };
        let results = crate::ownership::future_results(output);
        if results.is_empty() {
            return;
        }
        let id = self.forwarded.get();
        self.forwarded.set(id + 1);
        let handed = inputs.iter().any(|(_, ty)| {
            crate::ownership::future_results(ty)
                .iter()
                .any(|result| results.contains(result))
        });
        if handed {
            ops.insert(start, Op::Forwards(Forward::Arguments(id)));
        }
        ops.push(Op::Forwards(Forward::Call(id, results.into())));
    }
    /// Emits a call, preceded by its lifetime fact when every reference it
    /// passes was borrowed through a reference parameter of this function.
    /// `loans` holds the loan of each reference input. A loan inherits its
    /// root from the loan it was derived from, so a reborrow, projection,
    /// payload, or alias of a reference parameter is rooted at that
    /// parameter's slot, while a borrow of a local, an owned parameter, or a
    /// temporary is rooted at the borrowed slot.
    fn push_call(
        &mut self,
        call: Op,
        inputs: &[(String, Ty)],
        loans: &[usize],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let parameters = self
            .sigs
            .get(&self.function_name)
            .map(|sig| &sig.inputs[..]);
        let references = inputs.iter().filter(|(_, ty)| ty.contains_reference());
        // A closure with borrowed captures carries references without a loan
        // of its own here, so it never qualifies.
        if !loans.is_empty()
            && references.clone().count() == loans.len()
            && references.clone().all(|(_, ty)| matches!(ty, Ty::Ref(..)))
            && loans.iter().all(|&loan| {
                let root = usize::from(self.loans[loan].root);
                root < self.parameters
                    && matches!(parameters.and_then(|p| p.get(root)), Some((_, Ty::Ref(..))))
            })
        {
            ops.push(Op::ForwardsReferences);
        }
        // Most programs have no lexical callback failure channel. Preserve
        // the ordinary call path without a name copy or extra signature walk.
        if self.generics.checked_errors.is_empty() {
            ops.push(call);
            return Ok(());
        }
        let direct = if let Op::Call(name) = &call {
            Some(name.clone())
        } else {
            None
        };
        ops.push(call);
        if let Some(name) = direct {
            let sig = self.sigs[&name].clone();
            self.checked_named_call(&name, &sig, at, ops)?;
        }
        Ok(())
    }
    /// Lowers `return` with its value, the function's cleanup, and the exit.
    fn return_statement(
        &mut self,
        e: Option<&Expr>,
        at: &Token,
        temporary_start: usize,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        if self.yield_type.is_some() && e.is_some() {
            return Err(at.error("a generator may only use bare return"));
        }
        let expected = self
            .return_type
            .clone()
            .ok_or_else(|| at.error("return outside a function"))?;
        let mut returned = Vec::new();
        let ty = match e {
            Some(e) => self.tail_expression(
                e,
                expected.clone(),
                &mut returned,
                self.yield_type.is_none(),
            )?,
            None => None,
        };
        // Only the value's own operations: an exit guard also moves its
        // binding, to drop it before the join.
        let marker = self.returning(at, &returned);
        self.refine_return(ty, at)?;
        let returned_loan = if self.return_origin.is_some() && !references::loan_free(e.unwrap()) {
            Some(self.check_return_reference(e.unwrap(), &returned)?)
        } else {
            None
        };
        // A returned reference normally stays borrowed across this
        // frame's cleanup. A forwarding call returns it from storage
        // that cleanup cannot reach, so it may still be a tail call.
        if self.contexts.is_empty()
            && (returned_loan.is_none() || forwarding_call(&returned).is_some())
        {
            returned.extend(returned_loan.map(Op::UseLoan));
            self.finish_tail_temporaries(temporary_start, &mut returned);
            if self.checked.first().is_some_and(|scope| scope.function) {
                returned.push(Op::Return);
            } else {
                finish_return(&mut returned);
            }
        } else {
            self.finish_temporaries(temporary_start, &mut returned);
            self.leaving(0, &mut returned);
            if let Some(loan) = returned_loan {
                returned.push(Op::UseLoan(loan));
            }
            returned.push(Op::Return);
        }
        if let Some(marker) = marker {
            returned.insert(0, marker);
        }
        ops.extend(returned);
        Ok(())
    }
    /// The marker naming the endpoint-holding slots that `value`, the
    /// operations of a returned value, moves. The value keeps their endpoints through
    /// the exit's joins and pool shutdowns. A move inside a `?` failure path
    /// leaves with the error instead, so those cleanups are skipped. A slot
    /// moved into a call that consumes it is still counted: the checker does
    /// not track which operand values reach the result.
    fn returning(&self, at: &Token, value: &[Op]) -> Option<Op> {
        let slots = self.endpoint_moves(value);
        (!slots.is_empty()).then(|| Op::Returning(site(at, false), slots.into()))
    }
    /// The endpoint-holding slots that `value` moves, outside `?` failure
    /// paths, in the order of their first move.
    fn endpoint_moves(&self, value: &[Op]) -> Vec<u8> {
        fn moves(ops: &[Op], out: &mut Vec<u8>) {
            for op in ops {
                match op {
                    Op::MoveLocal(slot) if !out.contains(slot) => out.push(*slot),
                    Op::Match(arms) => arms.iter().for_each(|arm| moves(&arm.body, out)),
                    Op::If {
                        then,
                        elifs,
                        otherwise,
                    } => {
                        moves(then, out);
                        for arm in elifs.iter() {
                            moves(&arm.condition, out);
                            moves(&arm.body, out);
                        }
                        moves(otherwise, out);
                    }
                    Op::Loop { condition, body } => {
                        moves(condition, out);
                        moves(body, out);
                    }
                    Op::CheckedBlock { body, .. } | Op::CheckedFunction { body, .. } => {
                        moves(body, out)
                    }
                    _ => {}
                }
            }
        }
        let mut slots = Vec::new();
        moves(value, &mut slots);
        slots.retain(|&slot| {
            self.slot_type(slot).is_some_and(|(ty, parameter)| {
                [true, false].into_iter().any(|sender| {
                    !crate::threading::endpoints([ty.clone()], sender, parameter).is_empty()
                })
            })
        });
        slots
    }
    /// The type of `slot`, and whether it is a parameter.
    fn slot_type(&self, slot: u8) -> Option<(&Ty, bool)> {
        let slot = slot as usize;
        match slot.checked_sub(self.parameters) {
            Some(local) => self.locals.get(local).map(|ty| (ty, false)),
            None => {
                let inputs = &self.sigs.get(&self.function_name)?.inputs;
                inputs.get(slot).map(|(_, ty)| (ty, true))
            }
        }
    }
    /// Bracket `ops[start..]`, the operations of one statement's expression
    /// and the store or drop of its value, as a value under construction
    /// (issue #347). An endpoint that the expression moves before a nested
    /// `result()`, `join()`, or pool shutdown is held by the unfinished value
    /// through that wait. `block` marks the right side of a binding that is a
    /// `match` block: each case's final expression holds its own moves, and
    /// the case's statements are statements of their own.
    pub(super) fn building(&self, at: &Token, start: usize, ops: &mut Vec<Op>, block: bool) {
        let (slots, open) = if block {
            (Vec::new(), holds(&ops[start..]))
        } else {
            let slots = self.endpoint_moves(&ops[start..]);
            let open = !slots.is_empty() && self.waits(&ops[start..]);
            (slots, open)
        };
        if open {
            ops.insert(
                start,
                Op::Building(Building::Open(site(at, false), slots.into())),
            );
            ops.push(Op::Building(Building::Close));
        }
    }
    /// The final expression of a `match` case used as a value, `ops[start..]`,
    /// holds the endpoints it moves until the binding statement ends: through
    /// the case's cleanup and the store.
    fn holding(&self, at: &Token, start: usize, ops: &mut Vec<Op>) {
        let slots = self.endpoint_moves(&ops[start..]);
        if !slots.is_empty() {
            ops.insert(
                start,
                Op::Building(Building::Hold(site(at, false), slots.into())),
            );
        }
    }
    /// `ops` can wait for another thread: a result, join, or pool shutdown.
    /// A `?` failure path drops the unfinished value first.
    fn waits(&self, ops: &[Op]) -> bool {
        let executor = |slot: &u8| {
            self.slot_type(*slot)
                .is_some_and(|(ty, _)| crate::threading::owns_executor(ty))
        };
        ops.iter().any(|op| match op {
            Op::Executor(ExecutorOp::Result(_) | ExecutorOp::Shutdown)
            | Op::Thread(ThreadOp::Join(_) | ThreadOp::Finish(..))
            | Op::Passes(_) => true,
            Op::MoveLocal(slot) | Op::StoreLocal(slot) | Op::DropLocal(slot) => executor(slot),
            Op::Match(arms) => arms.iter().any(|arm| self.waits(&arm.body)),
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                self.waits(then)
                    || elifs
                        .iter()
                        .any(|arm| self.waits(&arm.condition) || self.waits(&arm.body))
                    || self.waits(otherwise)
            }
            Op::Loop { condition, body } => self.waits(condition) || self.waits(body),
            Op::CheckedBlock { body, .. } | Op::CheckedFunction { body, .. } => self.waits(body),
            _ => false,
        })
    }
    fn if_value(
        &mut self,
        at: &Token,
        body: &[Stmt],
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let outer = std::mem::replace(&mut self.checked_value_tail, false);
        let result = self.block(body, ops, &Tail::Value(expected));
        self.checked_value_tail = outer;
        match result? {
            BlockResult::Continues(ty) => Ok(ty),
            BlockResult::Exits => Err(at.error(
                "every branch of this `if` exits, so it has no value; write it as a statement",
            )),
        }
    }
    fn block(&mut self, body: &[Stmt], ops: &mut Vec<Op>, tail: &Tail) -> Result<BlockResult> {
        let start = self.locals.len();
        let result = self.block_inner(body, ops, tail)?;
        if matches!(tail, Tail::Discard | Tail::Value(_))
            && matches!(result, BlockResult::Continues(_))
        {
            self.cleanup(start, ops);
        }
        Ok(result)
    }
    /// A suite of `return if` or `return match` without a value returns unit,
    /// so no selected arm continues past the statement.
    fn returning_block(
        &mut self,
        body: &[Stmt],
        at: &Token,
        ops: &mut Vec<Op>,
        tail: &Tail,
    ) -> Result<BlockResult> {
        let result = self.block(body, ops, tail)?;
        if !matches!(tail, Tail::Return) {
            return Ok(result);
        }
        if let BlockResult::Continues(ty) = result {
            debug_assert!(ty.is_none(), "a returning suite leaves no value");
            let temporary_start = self.expression_temps.len();
            self.return_statement(None, at, temporary_start, ops)?;
        }
        Ok(BlockResult::Exits)
    }
    fn block_inner(
        &mut self,
        body: &[Stmt],
        ops: &mut Vec<Op>,
        tail: &Tail,
    ) -> Result<BlockResult> {
        let mut result = None;
        for (i, stmt) in body.iter().enumerate() {
            let temporary_start = self.expression_temps.len();
            // A discarded expression statement's value is built until its drop.
            let mut discarded = None;
            let tail = if i + 1 == body.len() {
                tail
            } else {
                &Tail::Discard
            };
            let last = matches!(tail, Tail::Function);
            if !matches!(stmt.kind, Statement::Pass) {
                self.mark(&stmt.at, ops);
            }
            result = match &stmt.kind {
                Statement::With {
                    manager,
                    name,
                    body: inner,
                } => {
                    if matches!(
                        self.with_statement(manager, name.as_deref(), inner, ops)?,
                        BlockResult::Exits
                    ) {
                        return exited_block(body, i);
                    }
                    None
                }
                Statement::Yield(e) => {
                    let expected = self.yield_type.clone().ok_or_else(|| {
                        stmt.at
                            .error("yield requires a function returning Generator[T]")
                    })?;
                    let value_start = ops.len();
                    let expected = if self.function_name.starts_with("__plenty_genexpr_")
                        && expected == Ty::Unit
                    {
                        let actual = self.value(e, ops)?;
                        if actual.restricted_storage() {
                            return Err(e.at.error("generator expression elements cannot be references, generators, closures, or scoped tasks"));
                        }
                        self.yield_type = Some(actual.clone());
                        actual
                    } else {
                        let actual = self.expr_expected(e, Some(expected.clone()), ops)?;
                        self.same(actual, Some(expected.clone()), &e.at)?;
                        expected
                    };
                    self.building(&e.at, value_start, ops, false);
                    self.finish_temporaries(temporary_start, ops);
                    ops.push(Op::Yield(expected));
                    None
                }
                Statement::Match { value, cases } => {
                    match self.match_cases(value, cases, tail, ops)? {
                        BlockResult::Continues(ty) => ty,
                        BlockResult::Exits => return exited_block(body, i),
                    }
                }
                Statement::Expr(Expr {
                    kind: Expression::Call(name, args),
                    at,
                }) if name == TRAP
                    && !self.names.contains_key(name)
                    && !self.sigs.contains_key(name) =>
                {
                    self.trap_statement(args, at, ops)?;
                    return exited_block(body, i);
                }
                Statement::Expr(e) if matches!(tail, Tail::Return) => {
                    self.return_statement(Some(e), &e.at, temporary_start, ops)?;
                    return exited_block(body, i);
                }
                Statement::Expr(e) => {
                    let value_start = ops.len();
                    let ty = self.tail_expression(
                        e,
                        match tail {
                            Tail::Function => self.return_type.clone().flatten(),
                            Tail::Value(expected) => expected.clone(),
                            Tail::Discard | Tail::Return => None,
                        },
                        ops,
                        last && self.yield_type.is_none(),
                    )?;
                    if self.checked_value_tail
                        && matches!(tail, Tail::Value(_))
                        && ty.as_ref().is_some_and(Ty::checked_closure)
                    {
                        return Err(e.at.error("a closure defined in `checked:` cannot leave its block; define it outside the block instead"));
                    }
                    if matches!(tail, Tail::Value(_))
                        && ty.as_ref().is_some_and(Ty::contains_reference)
                    {
                        // The case's bindings and locals end before the value
                        // is used, and no loan follows it out of the suite.
                        return Err(e.at.error(if self.checked_value_tail {
                            "a `checked:` block cannot produce a reference or a closure with borrowed captures; produce an owned value"
                        } else {
                            "an `if` or `match` block used as a value cannot produce a reference or a closure with borrowed captures; produce an owned value, or assign in each arm"
                        }));
                    }
                    if last
                        && self.return_origin.is_some()
                        && ty.as_ref().is_some_and(Ty::carries_loan)
                        && !references::loan_free(e)
                    {
                        let loan = self.check_return_reference(e, ops)?;
                        ops.push(Op::UseLoan(loan));
                    }
                    if last && self.yield_type.is_none() {
                        self.refine_return(ty.clone(), &e.at)?;
                        // The function's result is in flight through the
                        // frame's release, which shuts its pools down.
                        if let Some(marker) = self.returning(&e.at, &ops[value_start..]) {
                            ops.insert(value_start, marker);
                        }
                    } else if matches!(tail, Tail::Value(_)) {
                        self.holding(&e.at, value_start, ops);
                    } else {
                        discarded = Some((&e.at, value_start));
                    }
                    ty
                }
                // `return match ...:` returns the final expression of the
                // selected case, so a call there is a tail call like any other
                // returned call.
                Statement::Return(Some(Expr {
                    kind: Expression::Match(value, cases),
                    ..
                })) => {
                    if self.yield_type.is_some() {
                        return Err(stmt.at.error("a generator may only use bare return"));
                    }
                    if self.return_type.is_none() {
                        return Err(stmt.at.error("return outside a function"));
                    }
                    self.match_cases(value, cases, &Tail::Return, ops)?;
                    return exited_block(body, i);
                }
                Statement::Return(Some(Expr {
                    kind: Expression::If(suite),
                    ..
                })) => {
                    if self.yield_type.is_some() {
                        return Err(stmt.at.error("a generator may only use bare return"));
                    }
                    if self.return_type.is_none() {
                        return Err(stmt.at.error("return outside a function"));
                    }
                    self.returning_block(suite, &stmt.at, ops, &Tail::Return)?;
                    return exited_block(body, i);
                }
                Statement::Return(e) => {
                    self.return_statement(e.as_ref(), &stmt.at, temporary_start, ops)?;
                    return exited_block(body, i);
                }
                Statement::Pass => None,
                Statement::Break | Statement::Continue => {
                    let is_continue = matches!(stmt.kind, Statement::Continue);
                    let step = self.loop_steps.last().ok_or_else(|| {
                        stmt.at.error(if is_continue {
                            "continue outside a loop"
                        } else {
                            "break outside a loop"
                        })
                    })?;
                    let scope = *self.loop_scopes.last().unwrap();
                    self.leaving(scope, ops);
                    if is_continue {
                        ops.extend(step.iter().cloned());
                    }
                    ops.push(if is_continue { Op::Continue } else { Op::Break });
                    return exited_block(body, i);
                }
                Statement::While { condition, body } => {
                    let mut test = Vec::new();
                    let ty = self.expr(condition, &mut test)?;
                    self.same(ty, Some(Ty::Bool), &condition.at)?;
                    self.building(&condition.at, 0, &mut test, false);
                    self.finish_temporaries(temporary_start, &mut test);
                    let saved = self.names.clone();
                    self.loop_steps.push(Vec::new());
                    self.loop_scopes.push(self.locals.len());
                    let mut lowered = Vec::new();
                    let result = self.block(body, &mut lowered, &Tail::Discard);
                    self.loop_steps.pop();
                    self.loop_scopes.pop();
                    self.names = saved;
                    result?;
                    ops.push(Op::Loop {
                        condition: test.into(),
                        body: lowered.into(),
                    });
                    None
                }
                Statement::For {
                    name,
                    iterable,
                    body,
                } => {
                    self.for_statement(name, iterable, body, ops)?;
                    None
                }
                Statement::SetIndex { target, value } => {
                    let start = ops.len();
                    if let Expression::Tuple(targets) = &ungroup(target).kind {
                        self.assign_targets(targets, value, ops)?;
                    } else if matches!(&target.kind, Expression::Unary(op, _) if op == "*") {
                        self.write_reference(target, value, ops)?;
                    } else if matches!(&target.kind, Expression::Member(..)) {
                        self.set_field(target, value, ops)?;
                    } else {
                        self.set_index(target, value, ops)?;
                    }
                    self.building(&value.at, start, ops, block_value(value));
                    None
                }
                Statement::Augment { target, op, value } => {
                    let start = ops.len();
                    self.augment(target, op, value, ops)?;
                    self.building(&value.at, start, ops, false);
                    None
                }
                Statement::Unpack {
                    names,
                    targets,
                    fresh,
                    mutable,
                    value,
                } => {
                    let start = ops.len();
                    if *fresh {
                        self.unpack_iteration_bindings(names, targets.as_deref(), value, ops)?;
                    } else if let Some(targets) = targets {
                        self.unpack_nested(targets, *mutable, value, ops)?;
                    } else {
                        self.unpack_tuple(names, *mutable, value, ops)?;
                    }
                    self.building(&value.at, start, ops, false);
                    None
                }
                Statement::Assign {
                    name,
                    mutable,
                    annotation,
                    value,
                } => {
                    if self.captures.contains(name) {
                        if *mutable || annotation.is_some() {
                            return Err(stmt.at.error("cannot redeclare a closure capture"));
                        }
                        let target = Expr {
                            at: stmt.at.clone(),
                            kind: Expression::Unary(
                                "*".into(),
                                Box::new(Expr {
                                    at: stmt.at.clone(),
                                    kind: Expression::Name(name.clone()),
                                }),
                            ),
                        };
                        let start = ops.len();
                        self.write_reference(&target, value, ops)?;
                        self.building(&value.at, start, ops, block_value(value));
                        self.finish_temporaries(temporary_start, ops);
                        continue;
                    }
                    let context = if let Some(ann) = annotation {
                        let context = ann.resolve(self.aliases)?;
                        if context.as_ref().is_some_and(Ty::unresolved_sum_member) {
                            return Err(ann.at.error(UNRESOLVED_SUM_MEMBER));
                        }
                        context
                    } else {
                        self.names.get(name).map(|l| l.ty.clone())
                    };
                    let value_start = ops.len();
                    let converted = match &context {
                        Some(view @ Ty::View(_, mutable)) => {
                            self.owner_view(value, *mutable, *view == text_view(), ops)?
                        }
                        _ => None,
                    };
                    let value = converted.as_ref().unwrap_or(value);
                    let (ty, origin) = self.binding_value(value, context, ops)?;
                    let diagnostic_origin = annotation
                        .as_ref()
                        .map(provenance::Origin::written)
                        .unwrap_or_else(|| self.value_origin(value));
                    if let Some(expected) = annotation {
                        let expected = expected.resolve(self.aliases)?.ok_or_else(|| {
                            expected.at.error("unit bindings are not supported yet")
                        })?;
                        self.same(Some(ty.clone()), Some(expected), &stmt.at)?;
                    }
                    let slot = if let Some(local) = self.names.get(name) {
                        if ty.checked_closure()
                            && self.checked.last().is_some_and(|scope| {
                                usize::from(local.slot) < self.parameters + scope.start
                            })
                        {
                            return Err(stmt.at.error("a closure defined in `checked:` cannot be assigned outside its block; define it outside the block instead"));
                        }
                        if *mutable || annotation.is_some() {
                            return Err(stmt.at.error(format!("duplicate binding `{name}`")));
                        }
                        if !local.mutable {
                            return Err(stmt
                                .at
                                .error(format!("`{name}` is immutable; declare it with `mut`")));
                        }
                        self.same(Some(ty.clone()), Some(local.ty.clone()), &stmt.at)?;
                        if let Some(origin) = origin
                            && self
                                .check_retargeted_reference(name, local.slot, origin, &stmt.at)?
                        {
                            ops.push(Op::EndLoan(self.reference_locals[&local.slot]));
                        }
                        local.slot
                    } else {
                        let slot =
                            u8::try_from(self.parameters + self.locals.len()).map_err(|_| {
                                stmt.at
                                    .error("at most 256 parameter/local slots are supported")
                            })?;
                        if *mutable && ty.carried_reference().is_some() {
                            // A later phase of book/src/proposals/returned-reference-enums.md.
                            return Err(stmt.at.error(format!(
                                "`mut {name}` cannot hold {ty}; bind each result to its own local"
                            )));
                        }
                        self.locals.push(ty.clone());
                        ide::local_type(&stmt.at, &ty);
                        if annotation.is_none() {
                            ide::inferred_binding(&stmt.at, name, &ty);
                        }
                        self.bind(
                            name.clone(),
                            Local {
                                slot,
                                ty,
                                mutable: *mutable,
                            },
                        );
                        if let Some(origin) = origin {
                            self.check_stored_reference(origin, &value.at)?;
                            self.bind_reference(slot, origin, *mutable, ops);
                        }
                        slot
                    };
                    self.record_value_origin(slot, diagnostic_origin);
                    self.mark(&stmt.at, ops);
                    ops.push(Op::StoreLocal(slot));
                    self.building(&value.at, value_start, ops, block_value(value));
                    None
                }
                Statement::If {
                    condition,
                    yes,
                    elifs,
                    no,
                } => {
                    let condition_start = ops.len();
                    let cond = self.expr(condition, ops)?;
                    self.same(cond, Some(Ty::Bool), &condition.at)?;
                    self.building(&condition.at, condition_start, ops, false);
                    self.finish_temporaries(temporary_start, ops);
                    let saved = self.names.clone();
                    let mut then = Vec::new();
                    let yes_result = self.returning_block(yes, &stmt.at, &mut then, tail)?;
                    let elif_start = self.locals.len();
                    let mut arms = Vec::with_capacity(elifs.len());
                    let mut results = Vec::with_capacity(elifs.len());
                    for elif in elifs {
                        self.names = saved.clone();
                        let (mut test, mut arm) = (Vec::new(), Vec::new());
                        let temporary_start = self.expression_temps.len();
                        self.mark(&elif.at, &mut test);
                        let condition_start = test.len();
                        let cond = self.expr(&elif.condition, &mut test)?;
                        self.same(cond, Some(Ty::Bool), &elif.condition.at)?;
                        self.building(&elif.condition.at, condition_start, &mut test, false);
                        self.finish_temporaries(temporary_start, &mut test);
                        results.push(self.returning_block(&elif.body, &elif.at, &mut arm, tail)?);
                        arms.push(ElifArm {
                            condition: test.into(),
                            body: arm.into(),
                        });
                    }
                    self.names = saved.clone();
                    let mut otherwise = Vec::new();
                    let mut result = self.returning_block(no, &stmt.at, &mut otherwise, tail)?;
                    self.names = saved;
                    ops.push(Op::If {
                        then: then.into(),
                        elifs: arms.into(),
                        otherwise: otherwise.into(),
                    });
                    // Each branch joins what follows it, as `elif` did when it
                    // was an `if` nested in `else`: a disagreement is reported
                    // at the last `if` or `elif` whose branch differs.
                    let branches = elifs
                        .iter()
                        .map(|elif| &elif.at)
                        .zip(results)
                        .rev()
                        .chain([(&stmt.at, yes_result)]);
                    for (at, branch) in branches {
                        result = match (branch, result) {
                            (BlockResult::Continues(ty), BlockResult::Continues(other)) => {
                                self.same(other, ty.clone(), at)?;
                                BlockResult::Continues(ty)
                            }
                            (BlockResult::Continues(ty), BlockResult::Exits)
                            | (BlockResult::Exits, BlockResult::Continues(ty)) => {
                                BlockResult::Continues(ty)
                            }
                            (BlockResult::Exits, BlockResult::Exits) => BlockResult::Exits,
                        };
                    }
                    let BlockResult::Continues(ty) = result else {
                        return exited_block(body, i);
                    };
                    if !last && !elifs.is_empty() {
                        // The scope of what the `elif` arms and `else` declare
                        // ends with the statement.
                        self.cleanup(elif_start, ops);
                    }
                    ty
                }
            };
            if matches!(tail, Tail::Discard) {
                if let Some(ty @ Ty::Enum(t)) = &result
                    && t.is_result()
                {
                    let can_propagate = self
                        .result_success(ty)
                        .is_some_and(|success| !matches!(success, Ty::Enum(t) if t.is_result()));
                    return Err(stmt.at.error(format!(
                            "cannot implicitly discard a Result; {}match the result explicitly, or use `drop(result)` to discard it deliberately",
                            if can_propagate { "use `?` to propagate the error, " } else { "" }
                        )));
                }
                if result.is_some() {
                    ops.push(Op::Drop);
                }
                result = None;
            }
            if let Some((at, start)) = discarded {
                self.building(at, start, ops, false);
            }
            if last && matches!(stmt.kind, Statement::Expr(_)) {
                self.finish_tail_temporaries(temporary_start, ops);
            } else {
                self.finish_temporaries(temporary_start, ops);
            }
        }
        Ok(BlockResult::Continues(result))
    }
}

/// Whether `ty` is `std.fmt.FormatError`, the prelude `FormatError`.
fn is_format_error(ty: &Ty) -> bool {
    matches!(ty, Ty::Enum(t) if &*t.name == "std.fmt.FormatError")
}

/// Whether `ty` is `AllocError | FormatError`, the error of an f-string that
/// runs user formatting or has a computed spec.
fn is_format_alloc_sum(ty: &Ty) -> bool {
    let members = crate::sum::members(ty);
    members.len() == 2
        && members.contains(&crate::sum::alloc_error())
        && members.iter().any(is_format_error)
}

/// An operand of `?` whose error type the language fixes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FixedError {
    /// A collection literal: `AllocError`.
    Collection,
    /// An f-string: `AllocError`, or `AllocError | FormatError` when user
    /// formatting runs or a spec is computed.
    Format,
}

const STR_REPETITION: &str = "strings do not support `*` repetition; use `text.repeat(count)?`";
const LIST_REPETITION: &str = "lists do not support `*` repetition; build the list with a comprehension, such as `[value for _ in range(count)]?`";

/// The list display that `e` is, possibly parenthesized or followed by `?`.
fn list_display(e: &Expr) -> Option<&Expr> {
    match &e.kind {
        Expression::Group(inner) | Expression::Try(inner) => list_display(inner),
        Expression::Method(inner, name, args, _) if name == "unwrap" && args.is_empty() => {
            list_display(inner)
        }
        Expression::Collection { kind, .. } if kind == "list" => Some(e),
        _ => None,
    }
}

/// Arithmetic never converts between numeric types. The suggested cast never
/// fails for a valid operand: to the float of an integer/float pair, to the
/// wider type of a same-signedness pair, and otherwise to the narrowest signed
/// type wider than the unsigned operand. When neither operand's type is that
/// signed type, both are converted; past `i64` no suggestion is made.
fn mixed_numeric(left: &Expr, a: &Ty, right: &Expr, b: &Ty) -> Box<dyn Error> {
    let bits = |ty: &Ty| ty.to_string()[1..].parse::<u32>().unwrap_or(0);
    let signed = |ty: &Ty| ty.to_string().starts_with('i');
    let spelled = |operand: &Expr| match &ungroup(operand).kind {
        Expression::Name(name) => name.as_str().to_string(),
        Expression::Number(text) => text.clone(),
        _ => "...".into(),
    };
    if a.is_float() || b.is_float() || signed(a) == signed(b) {
        let convert_left = if a.is_float() == b.is_float() {
            bits(a) < bits(b)
        } else {
            b.is_float()
        };
        let (operand, target) = if convert_left { (left, b) } else { (right, a) };
        return operand.at.error(format!(
            "cannot combine {a} with {b}; convert with `{target}({})`",
            spelled(operand)
        ));
    }
    let (unsigned, unsigned_ty, signed_operand, signed_ty) = if signed(a) {
        (right, b, left, a)
    } else {
        (left, a, right, b)
    };
    if bits(signed_ty) > bits(unsigned_ty) {
        return unsigned.at.error(format!(
            "cannot combine {a} with {b}; convert with `{signed_ty}({})`",
            spelled(unsigned)
        ));
    }
    match bits(unsigned_ty) * 2 {
        width @ ..=64 => left.at.error(format!(
            "cannot combine {a} with {b}; convert both with `i{width}({})` and `i{width}({})`",
            spelled(left),
            spelled(right)
        )),
        _ => signed_operand.at.error(format!(
            "cannot combine {a} with {b}; no integer type holds both"
        )),
    }
}

/// The type-reference names of `&[T]` and `&mut [T]`.
/// The element name of `&substr`, a view of part of a string (issue #595).
pub(crate) const SUBSTR: &str = "substr";

/// `&substr`: a shared view of a run of a string's scalars.
pub(crate) fn text_view() -> Ty {
    Ty::View(Arc::new(Ty::Text), false)
}

pub(crate) const VIEW_TYPE: &str = "&[]";
pub(crate) const MUT_VIEW_TYPE: &str = "&mut []";

/// `&[T]` (shared) or `&mut [T]` (exclusive): a view of list rows, or of
/// bytes when `T` is `u8`. Its elements are list elements, which never hold
/// references.
fn view_type(element: Ty, mutable: bool, at: &Token) -> Result<Ty> {
    if element.contains_reference() || element.restricted_storage() {
        return Err(at.error(format!(
            "a slice view's elements are list elements, and {element} cannot be stored in a list"
        )));
    }
    Ok(Ty::View(Arc::new(element), mutable))
}

/// `&xs[a:b]?` would propagate an error that a view cannot produce.
const VIEW_TRY: &str = "a slice view `&xs[a:b]` does not allocate, so it takes no `?`; write `&xs[a:b]`, or copy the elements with `xs[a:b]?`";

/// `t[a:b]?` on a tuple would propagate an error that the slice cannot
/// produce: its components are selected at compile time.
const TUPLE_SLICE_TRY: &str =
    "a tuple slice `t[a:b]` does not allocate, so it takes no `?`; write `t[a:b]`";

const SLICE_ASSIGNMENT: &str =
    "slice assignment is not supported; assign elements by index, or build a new list";

fn ungroup_mut(mut e: &mut Expr) -> &mut Expr {
    while matches!(e.kind, Expression::Group(_)) {
        let Expression::Group(inner) = &mut e.kind else {
            unreachable!()
        };
        e = inner;
    }
    e
}

fn ungroup(mut e: &Expr) -> &Expr {
    while let Expression::Group(inner) = &e.kind {
        e = inner;
    }
    e
}

/// Whether `*r` copies a `T` out of `r: &T`: the value needs no ownership
/// transfer and can itself be stored.
fn copyable_through_reference(ty: &Ty) -> bool {
    !ty.affine() && ty.storage_barrier().is_none()
}

/// A hint suggesting `*e` when `e` has type `&T` and `T` copies, for a
/// position that cannot hold the reference itself.
fn copy_hint(e: &Expr, ty: &Ty) -> Option<String> {
    let Ty::Ref(inner, _) = ty else { return None };
    if !copyable_through_reference(inner) {
        return None;
    }
    Some(match &ungroup(e).kind {
        Expression::Name(name) => format!("; store a copy with `*{name}`"),
        _ => "; store a copy with `*`".into(),
    })
}

fn type_name(ty: Type) -> String {
    match ty {
        None => "()".into(),
        Some(Ty::Bool) => "bool".into(),
        Some(Ty::Str) => "str".into(),
        Some(t) => t.to_string(),
    }
}
/// `e` is a block whose selected suite supplies a statement's value.
fn block_value(e: &Expr) -> bool {
    matches!(
        ungroup(e).kind,
        Expression::Match(..) | Expression::Checked(_) | Expression::If(_)
    )
}

/// `ops` contains a suite value's `Building::Hold`.
fn holds(ops: &[Op]) -> bool {
    ops.iter().any(|op| match op {
        Op::Building(Building::Hold(..)) => true,
        Op::Match(arms) => arms.iter().any(|arm| holds(&arm.body)),
        Op::If {
            then,
            elifs,
            otherwise,
        } => {
            holds(then)
                || elifs
                    .iter()
                    .any(|arm| holds(&arm.condition) || holds(&arm.body))
                || holds(otherwise)
        }
        Op::Loop { condition, body } => holds(condition) || holds(body),
        Op::CheckedBlock { body, .. } | Op::CheckedFunction { body, .. } => holds(body),
        _ => false,
    })
}
fn branch(yes: Vec<Op>, no: Vec<Op>) -> Op {
    Op::Match(
        vec![
            MatchArm {
                pattern: Pattern::Bool(true),
                body: yes.into(),
            },
            MatchArm {
                pattern: Pattern::Bool(false),
                body: no.into(),
            },
        ]
        .into(),
    )
}

/// Position of the lifetime fact of a forwarding call that ends `ops` except
/// for loan metadata: its argument loan uses and, for a reference result, the
/// result's loan.
fn forwarding_call(ops: &[Op]) -> Option<usize> {
    let call = ops
        .iter()
        .rposition(|op| !matches!(op, Op::Loan(_) | Op::UseLoan(_) | Op::EndLoan(_)))?;
    (matches!(ops[call], Op::Call(_) | Op::CallIndirect(_))
        && matches!(ops[..call].last(), Some(Op::ForwardsReferences)))
    .then(|| call - 1)
}

/// Removes the last direct call emitted at or after `begin`, with its lifetime
/// fact, for a construct that runs the callee itself.
fn take_call(ops: &mut Vec<Op>, begin: usize) -> Option<(usize, String)> {
    let mut index = (begin..ops.len()).rfind(|&i| matches!(ops[i], Op::Call(_)))?;
    let Op::Call(name) = ops.remove(index) else {
        unreachable!()
    };
    if index > begin && matches!(ops[index - 1], Op::ForwardsReferences) {
        index -= 1;
        ops.remove(index);
    }
    if index > begin && matches!(ops[index - 1], Op::Passes(_)) {
        index -= 1;
        ops.remove(index);
    }
    // The opening `Forward::Arguments`, if any, stays: without its call it
    // brackets nothing.
    if index > begin && matches!(ops[index - 1], Op::Forwards(Forward::Call(..))) {
        index -= 1;
        ops.remove(index);
    }
    Some((index, name))
}

/// Return expressions are tail positions even inside a guard branch. Make
/// every path exit, using a tail call where the last operation is a call.
fn finish_return(body: &mut Vec<Op>) {
    match body.last_mut() {
        Some(last @ Op::Call(_)) => {
            let Op::Call(name) = last else { unreachable!() };
            *last = Op::TailCall(std::mem::take(name));
        }
        Some(last @ Op::CallIndirect(_)) => {
            let Op::CallIndirect(signature) = last else {
                unreachable!()
            };
            *last = Op::TailCallIndirect(signature.clone());
        }
        Some(Op::Match(arms)) => {
            for arm in Arc::make_mut(arms) {
                let mut body = arm.body.to_vec();
                finish_return(&mut body);
                arm.body = body.into();
            }
        }
        _ => body.push(Op::Return),
    }
}

fn integer(text: &str, negative: bool, at: &Token) -> Result<Value> {
    let split = text
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(text.len());
    let (digits, suffix) = text.split_at(split);
    if digits.starts_with('_') || digits.ends_with('_') || digits.contains("__") {
        return Err(at.error("invalid integer separator"));
    }
    let ty = if suffix.is_empty() {
        Ty::I64
    } else {
        named_type(suffix)
            .filter(|t| t.is_int())
            .ok_or_else(|| at.error("invalid integer suffix"))?
    };
    if negative {
        reject_unsigned_negation(&ty, at)?;
    }
    let magnitude = digits
        .replace('_', "")
        .parse::<i128>()
        .map_err(|_| at.error("integer literal out of range"))?;
    let number = if negative { -magnitude } else { magnitude };
    let (min, max) = ty.int_range().unwrap();
    if number < min || number >= max {
        return Err(at.error(format!("integer literal out of range for {ty}")));
    }
    Ok(match ty {
        Ty::I8 => Value::I8(number as i8),
        Ty::I16 => Value::I16(number as i16),
        Ty::I32 => Value::I32(number as i32),
        Ty::I64 => Value::I64(number as i64),
        Ty::U8 => Value::U8(number as u8),
        Ty::U16 => Value::U16(number as u16),
        Ty::U32 => Value::U32(number as u32),
        Ty::U64 => Value::U64(number as u64),
        _ => unreachable!(),
    })
}

#[cfg_attr(feature = "profile", hotpath::measure)]
pub(crate) fn compile(source: &str, heap: &mut Heap) -> Result<Program> {
    lower(
        modules::single(source)?,
        heap,
        crate::CheckOptions::default(),
    )
}

/// [`compile`] with the checks `check` adds.
pub(crate) fn check(source: &str, heap: &mut Heap, check: crate::CheckOptions) -> Result<Program> {
    lower(modules::single(source)?, heap, check)
}

#[cfg_attr(feature = "profile", hotpath::measure)]
pub(crate) fn compile_file(
    path: &std::path::Path,
    root: Option<&std::path::Path>,
    require_main: bool,
    heap: &mut Heap,
    check: crate::CheckOptions,
) -> Result<Program> {
    lower(modules::load(path, root, require_main)?, heap, check)
}

#[cfg_attr(feature = "profile", hotpath::measure)]
pub(crate) fn compile_graph(
    path: &std::path::Path,
    graph: &crate::project::SourceGraph,
    require_main: bool,
    heap: &mut Heap,
    check: crate::CheckOptions,
) -> Result<Program> {
    lower(modules::load_graph(path, graph, require_main)?, heap, check)
}

pub(crate) fn compile_script(
    path: &std::path::Path,
    graph: &crate::project::SourceGraph,
    heap: &mut Heap,
) -> Result<Program> {
    lower(
        modules::load_graph_with(path, graph, true, None, true)?,
        heap,
        crate::CheckOptions::default(),
    )
}

/// The sources of one application build, as the program cache names them.
pub(crate) enum Sources<'a> {
    /// A standalone source, which can import only `std`.
    Text(&'a str),
    /// The entry file of `graph.packages[0]`, with text `plenty test` adds
    /// to its end.
    Graph {
        entry: std::path::PathBuf,
        graph: std::borrow::Cow<'a, crate::project::SourceGraph>,
        appendix: Option<String>,
        script: bool,
    },
}

impl<'a> Sources<'a> {
    /// A file and its imports below `root`, as [`compile_file`] loads them.
    pub(crate) fn file(
        path: &std::path::Path,
        root: Option<&std::path::Path>,
    ) -> Result<Sources<'a>> {
        let (entry, graph) = modules::file_graph(path, root)?;
        Ok(Sources::Graph {
            entry,
            graph: std::borrow::Cow::Owned(graph),
            appendix: None,
            script: false,
        })
    }

    pub(crate) fn compile(&self, heap: &mut Heap) -> Result<Program> {
        match self {
            Sources::Text(source) => compile(source, heap),
            Sources::Graph {
                entry,
                graph,
                appendix,
                script,
            } => lower(
                modules::load_graph_with(entry, graph, true, appendix.as_deref(), *script)?,
                heap,
                crate::CheckOptions::default(),
            ),
        }
    }

    /// Everything that names this build apart from the files it imports, or
    /// `None` when loading would fail.
    pub(crate) fn identity(&self) -> Option<Vec<u8>> {
        let mut identity = Vec::new();
        match self {
            Sources::Text(source) => {
                identity.extend_from_slice(b"text\n");
                identity.extend_from_slice(source.as_bytes());
            }
            Sources::Graph {
                entry,
                graph,
                appendix,
                script,
            } => {
                identity.extend_from_slice(b"graph\n");
                identity.extend_from_slice(modules::identity(entry, graph)?.as_bytes());
                identity.extend_from_slice(format!("\n{appendix:?} script={script}").as_bytes());
            }
        }
        Some(identity)
    }

    /// Whether a build now would read exactly `inputs`.
    pub(crate) fn current(&self, inputs: &Inputs) -> bool {
        match self {
            Sources::Text(_) => *inputs == Inputs::default(),
            Sources::Graph { graph, .. } => inputs.current(graph),
        }
    }
}

pub(crate) use modules::harness::{RETURNED_ERR, TestFile, TestReturn};

/// What `plenty test` runs from the test file at `path`; see [`modules::harness`].
pub(crate) fn discover_tests(path: &std::path::Path) -> Result<TestFile> {
    modules::harness::discover(path)
}

/// The `main` that `plenty test` adds to a file of test functions.
pub(crate) fn test_appendix(tests: &[(String, TestReturn)]) -> String {
    modules::harness::appendix(tests)
}

pub(crate) fn check_generated_interface(source: &str) -> Result<()> {
    let mut heap = Heap::default();
    let program = lower(
        modules::generated_interface(source)?,
        &mut heap,
        crate::CheckOptions::default(),
    )?;
    crate::op::check(&program.ops)
}

/// Explains why a second job closure cannot share an inferred error member,
/// or that the declared error names the job-free form of the source.
fn producer_conflict_hint(target: &Ty, source: &Ty) -> String {
    if let Some(free) = crate::sum::job_free(source)
        .filter(|free| free != source && crate::sum::members(target).contains(free))
    {
        return format!("; call `drop_job()` before `?` to propagate the declared job-free {free}");
    }
    crate::sum::producer_conflict(target, source)
        .map(|member| format!("; {member} already names another closure, and one declared closure type names one producer: call `drop_job()` on each start or submission and declare the job-free `SpawnError[()]` or `SubmitError[()]`, start jobs through a generic helper, or recover the job locally"))
        .unwrap_or_default()
}

/// The diagnostic for a reference-returning signature without a reference parameter.
fn no_origin(output: &Ty) -> String {
    if matches!(output, Ty::Ref(..)) {
        "returned references borrow from the first reference parameter; add a reference parameter"
            .into()
    } else {
        format!("a function returning {output} needs a reference parameter to borrow from")
    }
}

/// The diagnostic for an exclusive result whose origin is a shared reference.
fn immutable_origin(output: &Ty) -> String {
    if matches!(output, Ty::Ref(..)) {
        "a mutable returned reference borrows from the first reference parameter, which must be `&mut`".into()
    } else {
        format!("{output} requires the first reference parameter to be `&mut`")
    }
}

/// Rejects parameters that would hold a reference-carrying `Option` or
/// `Result`, or a reference to one. Such parameters are a later phase of
/// book/src/proposals/returned-reference-enums.md.
fn check_carried_parameters(f: &Function, inputs: &[(String, Ty)]) -> Result<()> {
    for ((name, ty), (_, written)) in inputs.iter().zip(&f.inputs) {
        let carried = match ty {
            Ty::Ref(inner, _) => inner.carried_reference().is_some(),
            ty => ty.carried_reference().is_some(),
        };
        if carried {
            return Err(written.at.error(format!(
                "parameter `{name}` cannot be {ty}: an Option or Result carrying a reference may only be returned or held in a local; pass the reference after matching"
            )));
        }
    }
    Ok(())
}
const UNRESOLVED_SUM_MEMBER: &str = "a closure signature in an anonymous sum names no concrete environment; only a function's declared return type can infer it, from the closure the body returns or propagates; use a generic parameter bounded by OnceCallable or Callable";

fn register_signature(
    f: &Function,
    aliases: &TypeAliases,
    sigs: &mut functions::Functions,
) -> Result<Arc<FnSig>> {
    let symbol = sigs.intern(&f.name);
    register_resolved(symbol, f, None, aliases, sigs)
}

/// [`register_signature`] for a function whose parameter and return types
/// `generics::prepare` already resolved, or that resolves them when
/// `resolved` is `None`. Resolving a type again would find the same type, so
/// only the cost differs.
fn register_resolved(
    symbol: functions::Symbol,
    f: &Function,
    resolved: Option<generics::Signature>,
    aliases: &TypeAliases,
    sigs: &mut functions::Functions,
) -> Result<Arc<FnSig>> {
    register_with_body(symbol, f, &f.body, resolved, aliases, sigs)
}

/// [`register_resolved`] reading whether `f` yields, and what reference it
/// returns, from `body`: a generic instance's template body until the
/// instance is lowered.
#[cfg_attr(feature = "profile", hotpath::measure)]
fn register_with_body(
    symbol: functions::Symbol,
    f: &Function,
    body: &[Stmt],
    resolved: Option<generics::Signature>,
    aliases: &TypeAliases,
    sigs: &mut functions::Functions,
) -> Result<Arc<FnSig>> {
    if f.inputs.len() > 256 {
        return Err(f
            .at
            .error("at most 256 parameter/local slots are supported"));
    }
    if aliases.contains_key(&f.name) {
        return Err(f
            .at
            .error(format!("function `{}` conflicts with a type alias", f.name)));
    }
    // The entry is found once and filled below: a whole-program table costs
    // a cache miss per probe once it outgrows the caches (issue #57).
    if sigs.sig(symbol).is_some() {
        return Err(f.at.error(format!(
            "function `{}` is already defined; redefinition is not supported",
            f.name
        )));
    }
    let (types, mut output) = match resolved {
        Some(resolved) => resolved,
        None => protocols::signature(f, aliases)?,
    };
    // Aggregates, `Option` and `Result` included, reject views when built.
    if matches!(output, Some(Ty::View(..))) {
        return Err(f.at.error(views::RETURN));
    }
    if types.iter().any(Ty::dictionary_view) || output.as_ref().is_some_and(Ty::dictionary_view) {
        return Err(f.at.error("dictionary key views are local-only; user-function parameters, returns, and captures require an owned keys_snapshot(), into_keys(), or to_set()"));
    }
    let pipeline = |ty: &Ty| {
        matches!(ty, Ty::Iter(..)) || matches!(ty, Ty::Ref(t, _) if matches!(**t, Ty::Iter(..)))
    };
    if output.as_ref().is_some_and(pipeline) {
        return Err(f.at.error(
            "an iterator pipeline cannot be returned; return a collected value or lend the pipeline as `&mut Iter[T]`",
        ));
    }
    if let Some(ty) = types
        .iter()
        .find(|ty| pipeline(ty) && !matches!(ty, Ty::Ref(_, true)))
    {
        return Err(f.at.error(format!(
            "an iterator pipeline parameter must be `&mut {}`; a pipeline is lent, never moved or shared",
            match ty {
                Ty::Ref(t, _) => t.to_string(),
                t => t.to_string(),
            }
        )));
    }
    if (f.foreign.is_some() || f.export.is_some()) && types.iter().any(pipeline) {
        return Err(f
            .at
            .error("foreign and exported functions cannot take iterator pipelines"));
    }
    if let Some((_, written)) = f
        .inputs
        .iter()
        .zip(&types)
        .find(|(_, ty)| ty.unresolved_sum_member())
        .map(|(input, _)| input)
    {
        return Err(written.at.error(UNRESOLVED_SUM_MEMBER));
    }
    let inputs: Vec<_> = f
        .inputs
        .iter()
        .map(|(name, _)| name.clone())
        .zip(types)
        .collect();
    if f.foreign.is_some() {
        foreign::check_signature(f, &inputs, &output)?;
    }
    if f.export.is_some() {
        crate::exports::check_signature(&inputs, &output).map_err(|message| f.at.error(message))?;
    }
    check_carried_parameters(f, &inputs)?;
    if let Some(output) = output.as_ref().filter(|ty| ty.carries_loan()) {
        // The first reference or view parameter is the result's origin; later ones
        // are ordinary inputs whose loans end with the call.
        let Some((_, origin)) = inputs.iter().find(|(_, ty)| references::lends(ty)) else {
            return Err(f.at.error(no_origin(output)));
        };
        if output.carries_mutable_loan() && !matches!(origin, Ty::Ref(_, true) | Ty::View(_, true))
        {
            return Err(f.at.error(immutable_origin(output)));
        }
    }
    if generators::yields(body)
        && let Some(Ty::Generator(t)) = &output
    {
        output = Some(crate::generator::ty(
            t.element.clone(),
            Some(f.name.clone()),
        ));
    }
    let outputs = output.into_iter().collect();
    let sig = Arc::new(FnSig { inputs, outputs });
    if let Some(fields) = references::returned_fields(body, &sig) {
        sigs.set_returned_fields(symbol, fields);
    }
    sigs.register(symbol, &sig);
    Ok(sig)
}

/// Lowers one function. An error in the body of a generic instance is reported
/// at the use that first required the instance, with the body position in a
/// note: see `ownership::Instantiation`.
#[cfg_attr(feature = "profile", hotpath::measure)]
fn lower_function(
    symbol: functions::Symbol,
    f: Arc<Function>,
    heap: &mut Heap,
    sigs: &mut functions::Functions,
    generics: &mut generics::Engine,
    aliases: &TypeAliases,
    access: &modules::AccessMap,
) -> Result<()> {
    if sigs.lowered(symbol) {
        return Ok(());
    }
    let required = generics.requirement(symbol).cloned();
    let outer = aliases
        .data
        .attribute(required.clone().map(data::Requiring::Instance));
    let lowered = generics
        .complete_instance(symbol, f, sigs)
        .and_then(|f| lower_body(symbol, f, heap, sigs, generics, aliases, access));
    aliases.data.attribute(outer);
    lowered.map_err(|error| -> Box<dyn Error> {
        let error = Diagnostic::from_boxed(error);
        if error.attributed {
            return error;
        }
        let Some(required) = &required else {
            return Box::new(Diagnostic {
                attributed: true,
                ..*error
            });
        };
        let body = error.position.as_ref();
        let attributed = error
            .demand
            .as_ref()
            .filter(|_| body.is_some())
            .and_then(|demand| {
                let detail = demand.detail().unwrap_or(&error.message);
                required.explain(body, detail, demand)
            })
            .unwrap_or_else(|| required.report(body, &error.message));
        Box::new(attributed)
    })
}

fn lower_body(
    symbol: functions::Symbol,
    f: Arc<Function>,
    heap: &mut Heap,
    sigs: &mut functions::Functions,
    generics: &mut generics::Engine,
    aliases: &TypeAliases,
    access: &modules::AccessMap,
) -> Result<()> {
    let (mut sig, mut yield_type, mut lower, mut body, lent_exit) = crate::profile_block!(
        "frontend::setup_function",
        {
            if !generics.active.insert(symbol) {
                return Err(f.at.error("recursive generator factory requires a concrete return type; recursive inline frames are not supported"));
            }
            let sig = Arc::clone(
                sigs.sig(symbol)
                    .expect("a lowered function has a registered signature"),
            );
            if let Some(declaration) = &f.foreign {
                let mut body: Vec<_> = (0..sig.inputs.len())
                    .map(|i| Op::LoadLocal(i as u8))
                    .collect();
                body.push(Op::ForeignCall {
                    declaration: declaration.clone(),
                    sig: sig.clone(),
                });
                body.push(Op::Return);
                let locals = sig
                    .inputs
                    .iter()
                    .map(|(_, t)| t.clone())
                    .collect::<Vec<_>>();
                generics.active.remove(&symbol);
                if sigs.complete(symbol, &sig) {
                    generics.retention.complete(symbol, &mut generics.templates);
                }
                let standard_library =
                    f.at.source()
                        .is_some_and(|source| generics.source_facts(&source).1);
                generics.compiled.push(Op::DefineFn(
                    f.name.clone(),
                    Box::new(CompiledFn {
                        location: None,
                        // Foreign wrappers need the same pruning eligibility as
                        // ordinary std bodies; otherwise unused imports retain
                        // their native symbols in COFF (issue #688).
                        origins: Arc::new(crate::ownership::Origins {
                            standard_library,
                            ..Default::default()
                        }),
                        generator: None,
                        sig,
                        doc: "".into(),
                        body: body.into(),
                        locals: locals.into(),
                    }),
                ));
                return Ok(());
            }
            let yield_type = if generators::yields(&f.body) {
                let Some(Ty::Generator(element)) = sig.outputs.first() else {
                    return Err(f
                        .at
                        .error("yield requires a function returning Generator[T]"));
                };
                Some(element.element.clone())
            } else {
                None
            };
            let mut lower = Lower {
                function_name: f.name.clone(),
                symbol,
                bare_self_receiver: f.inputs.first().is_some_and(|(name, ty)| {
                    name == "self" && ty.name.as_deref() == Some("&") && ty.at.is("self")
                }),
                init_fields: (crate::record::is_method(&f.name) && f.name.ends_with(".__init__"))
                    .then(HashSet::default),
                source_tail: Vec::new(),
                tail_candidates: Vec::new(),
                closure_loans: HashMap::default(),
                captures: f
                    .captures
                    .iter()
                    .filter(|capture| !f.once || capture.borrowed)
                    .map(|c| c.name.clone())
                    .collect(),
                heap,
                sigs,
                generics,
                aliases,
                access,
                names: HashMap::default(),
                pipelines: Vec::new(),
                pipeline_ends: Default::default(),
                resumed_ends: Default::default(),
                binding_pipeline: false,
                lent_parameters: HashMap::default(),
                pipeline_callbacks: HashMap::default(),
                lent_item_loans: HashSet::default(),
                pipeline_generators: 0,
                function_places: HashMap::default(),
                function_initializers: Vec::new(),
                locals: Vec::new(),
                parameters: sig.inputs.len(),
                return_type: Some(if yield_type.is_some() {
                    None
                } else {
                    sig.outputs.first().cloned()
                }),
                loop_steps: Vec::new(),
                loop_scopes: Vec::new(),
                checked: Vec::new(),
                checked_value_tail: false,
                yield_type: yield_type.clone(),
                consuming_pipeline: false,
                loans: Vec::new(),
                reference_locals: HashMap::default(),
                indexed_lists: Vec::new(),
                expression_temps: Vec::new(),
                contexts: Vec::new(),
                return_origin: if sig.outputs.first().is_some_and(Ty::carries_loan) {
                    sig.inputs
                        .iter()
                        .position(|(_, ty)| references::lends(ty))
                        .map(|i| i as u8)
                } else {
                    None
                },
                slot_names: Vec::new(),
                value_origins: HashMap::default(),
                site: Default::default(),
                forwarded: Default::default(),
            };
            for ((_, ty), (_, written)) in sig.inputs.iter().zip(&f.inputs) {
                ide::local_type(&written.at, ty);
            }
            for (i, (name, ty)) in sig.inputs.iter().enumerate() {
                let origin = if name == "self" {
                    lower
                        .self_origin(ty)
                        .unwrap_or_else(|| provenance::Origin::written(&f.inputs[i].1))
                } else {
                    provenance::Origin::written(&f.inputs[i].1)
                };
                lower.record_value_origin(i as u8, origin);
                lower.bind(
                    name.clone(),
                    Local {
                        slot: i as u8,
                        ty: ty.clone(),
                        mutable: f.once && f.captures.get(i).is_some_and(|c| c.mutable),
                    },
                );
            }
            lower.validate_defaults(&f)?;
            let mut body = Vec::new();
            // Parameter loans start at the function header.
            lower.mark(&f.at, &mut body);
            for (i, (_, ty)) in sig.inputs.iter().enumerate() {
                if let Ty::Ref(_, mutable) | Ty::View(_, mutable) = ty {
                    if yield_type.is_some() {
                        return Err(f.at.error("generators cannot capture references"));
                    }
                    let loan = lower.new_loan(i as u8, *mutable, None, &mut body);
                    lower.reference_locals.insert(i as u8, loan);
                }
            }
            let lent_exit = if sig
                .inputs
                .iter()
                .any(|(_, ty)| pipelines::lent_identity(ty).is_some())
            {
                lower.receive_lent(&f.at, &mut body)?
            } else {
                Vec::new()
            };
            (sig, yield_type, lower, body, lent_exit)
        }
    );
    let lexical = lower.generics.checked_closures.contains(&f.name);
    let failures = checked::input_errors(&sig.inputs);
    let effectful = lexical || !failures.is_empty();
    if effectful {
        lower.checked.push(checked::Scope {
            start: 0,
            function: true,
            arithmetic: lexical,
            failures,
        });
    }
    let lowered = crate::profile_block!(
        "frontend::lower_function_body",
        lower.block(
            &f.body,
            &mut body,
            if yield_type.is_none() {
                &Tail::Function
            } else {
                &Tail::Discard
            },
        )
    )?;
    crate::profile_block!("frontend::finish_function", {
        if let BlockResult::Continues(output) = lowered {
            if let Some(doc_at) = f
                .doc_at
                .as_ref()
                .filter(|_| f.body.is_empty() && lower.return_type == Some(Some(Ty::Str)))
            {
                return Err(doc_at.error(
                "expected str, got (); a string at the beginning of a function is its documentation; write `return` before it to return the text",
            ));
            }
            lower.refine_return(output, &f.at)?;
            body.extend(lent_exit);
        }
        if f.name.starts_with("__plenty_genexpr_") {
            yield_type = lower.yield_type.clone();
            sig = Arc::new(FnSig {
                inputs: sig.inputs.clone(),
                outputs: vec![crate::generator::ty(
                    yield_type
                        .clone()
                        .expect("generator expression yields a value"),
                    Some(f.name.clone()),
                )],
            });
            lower.sigs.insert(f.name.clone(), sig.clone());
        }
        if yield_type.is_none() {
            let output = lower.return_type.clone().flatten();
            match output.as_ref().and_then(Ty::unresolved_producer) {
                Some(Ty::Closure(closure)) => {
                    let kind = if closure.once {
                        "one-shot closure"
                    } else {
                        "closure"
                    };
                    return Err(f.at.error(format!(
                    "cannot infer the concrete {kind} returned by this function; a callable signature does not specify the closure's captured environment, so at least one path must return a closure value"
                )));
                }
                Some(_) => {
                    return Err(f
                        .at
                        .error("cannot infer the concrete generator returned by this function"));
                }
                None => {}
            }
            let inferred_frame = sig.outputs.iter().any(Ty::unresolved_generator);
            // Most bodies return exactly the declared type; the registered
            // signature then stands, sparing a copy and a table probe (issue #57).
            if inferred_frame || sig.outputs.as_slice() != output.as_slice() {
                sig = Arc::new(FnSig {
                    inputs: sig.inputs.clone(),
                    outputs: output.into_iter().collect(),
                });
                *lower.sigs.sig_mut(symbol) = sig.clone();
            }
            if inferred_frame {
                // Check empty variants and propagation paths again with the resolved
                // return context. Calls/specializations are cached across this pass.
                drop(lower);
                generics.active.remove(&symbol);
                return lower_function(symbol, f, heap, sigs, generics, aliases, access);
            }
        } else if let Some(Ty::Generator(t)) = sig.outputs.first() {
            t.set_slots(
                sig.inputs
                    .iter()
                    .map(|(_, t)| t.clone())
                    .chain(lower.locals.iter().cloned())
                    .collect(),
            )
            .map_err(|message| f.at.error(message))?;
        }
        if effectful {
            let scope = lower.checked.pop().expect("function effect scope");
            if !scope.failures.is_empty() {
                let error = crate::sum::anonymous(scope.failures);
                lower
                    .generics
                    .checked_errors
                    .insert(f.name.clone(), error.clone());
                let result =
                    crate::sum::result(sig.outputs.first().cloned().unwrap_or(Ty::Unit), error);
                sig = Arc::new(FnSig {
                    inputs: sig.inputs.clone(),
                    outputs: vec![result.clone()],
                });
                body = vec![Op::CheckedFunction {
                    body: body.into(),
                    result,
                }];
            }
        } else if yield_type.is_none() {
            mark_tail_calls(&mut body);
        }
        // A declaration's borrowed address has one immutable slot per function.
        // Initialize it at entry so a first borrow inside a branch dominates later uses.
        body.splice(0..0, std::mem::take(&mut lower.function_initializers));
        // Most functions have no call in source tail position; `validate` reads
        // a missing entry as no candidates.
        if !lower.tail_candidates.is_empty() {
            lower
                .generics
                .tail_candidates
                .insert(f.name.clone(), std::mem::take(&mut lower.tail_candidates));
        }
        let source = f.at.source();
        let facts = source
            .as_ref()
            .map(|source| lower.generics.source_facts(source));
        let parameter_values = lower.parameter_values();
        let compiled = CompiledFn {
            location: source.as_ref().map(|_| f.at.position()),
            origins: Arc::new(crate::ownership::Origins {
                standard_library: facts.as_ref().is_some_and(|(_, standard)| *standard),
                source,
                trap_file: facts.map(|(relative, _)| relative),
                names: lower.slot_names,
                parameter_values,
                required: lower.generics.requirement(symbol).cloned(),
            }),
            generator: yield_type,
            sig,
            doc: f.doc.as_str().into(),
            body: body.into(),
            locals: lower.locals.into(),
        };
        generics.active.remove(&symbol);
        if sigs.complete(symbol, &compiled.sig) {
            generics.retention.complete(symbol, &mut generics.templates);
        }
        generics
            .compiled
            .push(Op::DefineFn(f.name.clone(), Box::new(compiled)));
        Ok(())
    })
}

/// Checks every function's native frame layout and generator operations on
/// several threads, reporting the first failure in program order.
#[cfg_attr(feature = "profile", hotpath::measure)]
fn validate_frames(ops: &[Op]) -> std::result::Result<(), String> {
    let validate = |f: &crate::op::CompiledFn| -> std::result::Result<(), String> {
        let mut frame_bytes = 0usize;
        for ty in f
            .sig
            .inputs
            .iter()
            .map(|(_, t)| t)
            .chain(&f.sig.outputs)
            .chain(f.locals.iter())
        {
            let bytes = crate::generator::layout(ty, &mut Vec::new())?;
            frame_bytes = frame_bytes
                .checked_add(16 + bytes)
                .filter(|n| *n <= i32::MAX as usize)
                .ok_or("inline values exceed the native stack-layout limit")?;
        }
        crate::generator::validate_ops(&f.body)
    };
    crate::parallel::map(ops.len(), crate::parallel::workers(), |i| match &ops[i] {
        Op::DefineFn(_, f) => validate(f),
        _ => Ok(()),
    })
    .into_iter()
    .collect()
}

#[cfg_attr(feature = "profile", hotpath::measure)]
fn lower(
    resolved: modules::Resolved,
    heap: &mut Heap,
    check: crate::CheckOptions,
) -> Result<Program> {
    ide::declare_program(&resolved);
    let modules::Resolved {
        standard_sources,
        source_paths,
        source_root,
        mut functions,
        declarations,
        enums,
        classes,
        protocols,
        access,
        public_api,
        at,
        require_main,
        inputs,
    } = resolved;
    let aliases = enums::resolve_types(&declarations, &enums, &classes)?;
    modules::check_api(&public_api, &aliases, &access)?;
    let (concrete_enums, generic_enums): (Vec<_>, Vec<_>) =
        enums.into_iter().partition(|e| e.type_params.is_empty());
    let (concrete_classes, generic_classes): (Vec<_>, Vec<_>) =
        classes.into_iter().partition(|c| c.type_params.is_empty());
    functions.extend(enums::expand(concrete_enums, &aliases)?);
    functions.extend(classes::expand(concrete_classes, &aliases)?);
    // One entry per function: sized up front, the table is not rehashed key by
    // key at every doubling while the program is lowered (issue #57).
    let mut sigs = functions::Functions::with_capacity(functions.len());
    // Generic data methods are expanded from the type tables' copies of these
    // declarations whenever an instance is created, so the generic functions
    // they call keep their bodies.
    let kept: Vec<&Function> = generic_enums
        .iter()
        .flat_map(|e| &e.methods)
        .chain(generic_classes.iter().flat_map(|c| &c.methods))
        .collect();
    let (mut generics, prepared) =
        generics::prepare(functions, &aliases, protocols, &kept, &mut sigs)?;
    drop(kept);
    drop((generic_enums, generic_classes));
    generics.source_root = source_root;
    generics.standard_sources = standard_sources;
    // Queued functions are registered below, in queue order; unused data
    // methods are not queued, so they are registered after them.
    let mut unused_methods = Vec::new();
    loop {
        let functions = generics.expand_data(&aliases, &mut sigs)?;
        if functions.is_empty() {
            break;
        }
        unused_methods.extend(
            functions
                .into_iter()
                .filter(|f| generics.is_unused_method(&f.name)),
        );
    }
    generics.compiled.reserve(generics.pending.len());
    // Every function is visited once here. A pass over the whole program
    // costs cache misses per function once the program outgrows the caches,
    // so the checks below visit only the functions they concern (issue #57).
    let mut boundary = Vec::new();
    let mut generator_parameters = false;
    // `prepare` resolved the types of the functions it queued, in queue
    // order. Those `expand_data` queued after them are resolved here.
    let mut prepared = prepared.into_iter();
    for (symbol, f) in &generics.pending {
        let sig = register_resolved(*symbol, f, prepared.next(), &aliases, &mut sigs)?;
        generator_parameters |= sig.inputs.iter().any(|(_, t)| t.unresolved_generator());
        if f.foreign.is_some() || f.export.is_some() {
            boundary.push(f.as_ref());
        }
    }
    // The emptied list is released before lowering, which sets peak memory.
    drop(prepared);
    for f in unused_methods {
        register_signature(&f, &aliases, &mut sigs)?;
    }
    foreign::check_symbols(boundary.iter().copied(), &sigs)?;
    let mut exports = Vec::new();
    let mut symbols = HashSet::default();
    let imported_symbols: HashSet<_> = boundary
        .iter()
        .filter_map(|f| {
            f.foreign
                .as_ref()
                .and_then(|d| d.symbol())
                .map(str::to_owned)
        })
        .collect();
    let mut interface_names = HashSet::default();
    for f in &boundary {
        if let Some(symbol) = &f.export {
            let name = f.name.rsplit('.').next().unwrap().to_owned();
            if imported_symbols.contains(symbol.as_str()) || !symbols.insert(symbol.clone()) {
                return Err(f
                    .at
                    .error(format!("duplicate or imported C export symbol `{symbol}`")));
            }
            if !interface_names.insert(name.clone()) {
                return Err(f
                    .at
                    .error(format!("duplicate generated interface name `{name}`")));
            }
            exports.push(crate::exports::Export {
                function: f.name.clone(),
                name,
                symbol: symbol.clone(),
                signature: sigs[&f.name].clone(),
                documentation: f.doc.clone(),
            });
        }
    }
    exports.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    let returns_status = if require_main {
        const MAIN_RETURN_TYPES: &str = "(), i32, Result[(), E], or Result[i32, E]";
        let (yields, entry) = sigs
            .declared("main")
            .and_then(|declared| Some((declared.yields, declared.function.as_ref()?)))
            .ok_or_else(|| {
                at.error(format!(
                    "binary application requires a module-level main taking no parameters and returning {MAIN_RETURN_TYPES}; \
                     a module without main is a library: import it from a program, or check it with `plenty --check-module FILE`"
                ))
            })?;
        let entry_sig = &sigs["main"];
        if !entry_sig.inputs.is_empty()
            || !matches!(entry_sig.outputs.as_slice(), [] | [Ty::I32])
                && !matches!(entry_sig.outputs.as_slice(), [Ty::Enum(t)] if t.propagatable() && !t.is_option() && matches!(t.get().variants[0].fields.as_slice(), [Ty::Unit | Ty::I32]))
            || yields
        {
            return Err(entry.at.error(format!(
                "main must take no parameters and return {MAIN_RETURN_TYPES}; it cannot be a generator"
            )));
        }
        !entry_sig.outputs.is_empty()
    } else {
        false
    };
    // The reporting contract is optional: types without its exact public
    // implementation retain the existing structural entrypoint diagnostic.
    // Its canonical types imply that the trusted std.fmt module is loaded.
    let entry_report = if require_main {
        match (
            sigs["main"].outputs.as_slice(),
            generics.reporting_protocol().cloned(),
        ) {
            ([Ty::Enum(result)], Some(protocol)) => {
                let error = &result.get().variants[1].fields[0];
                if protocols::check(
                    &protocol,
                    error,
                    &generics.methods,
                    &generics.templates,
                    &aliases,
                    &access,
                    &generics.standard_sources,
                    &protocol.at,
                    &protocol.at,
                )
                .is_ok()
                {
                    let main = sigs.intern("main");
                    let (symbol, function) = generics.instantiate(
                        "std.fmt.entry_report",
                        vec![error.clone()],
                        &protocol.at,
                        &aliases,
                        &access,
                        main,
                    )?;
                    if let Some(instance) = function {
                        let depth = instance.depth;
                        let function = instance.complete()?;
                        let instance = sigs.intern(&function.name);
                        generics.specialization_depths.insert(instance, depth);
                        register_signature(&function, &aliases, &mut sigs)?;
                        generics.declare(function, &mut sigs);
                    }
                    Some(symbol)
                } else {
                    None
                }
            }
            _ => None,
        }
    } else {
        None
    };
    if generator_parameters {
        generics.pending.retain(|(symbol, _)| {
            !sigs
                .sig(*symbol)
                .expect("a queued function has a registered signature")
                .inputs
                .iter()
                .any(|(_, t)| t.unresolved_generator())
        });
    }
    loop {
        // Resolving a specialized return type can introduce data instances even
        // when its body only returns Err. Drain those methods before finishing.
        loop {
            let functions = generics.expand_data(&aliases, &mut sigs)?;
            if functions.is_empty() {
                break;
            }
            for f in functions {
                register_signature(&f, &aliases, &mut sigs)?;
            }
        }
        let Some((symbol, f)) = generics.pending.pop_front() else {
            // Methods no code used are checked last, so each that a use
            // reached keeps that use as its reported site.
            if check.all_methods && generics.demand_unused_methods() {
                continue;
            }
            break;
        };
        if ide::active() && ide::skipped(&f.name) {
            continue;
        }
        let start = generics.compiled.len();
        let lowered = lower_function(symbol, f, heap, &mut sigs, &mut generics, &aliases, &access);
        generics.demand_references(start);
        if let Err(error) = lowered {
            // An editor check reports the error and goes on to the next
            // function; nothing is being lowered between functions.
            if !ide::recover(&*error) {
                return Err(error);
            }
            generics.active.clear();
        }
    }
    let mut ops = std::mem::take(&mut generics.compiled);
    validate_frames(&ops).map_err(|message| at.error(message))?;
    tail_calls::validate(&ops, &generics, &sigs)?;
    if require_main {
        ops.push(Op::Call("main".into()));
        if let [Ty::Enum(t)] = sigs["main"].outputs.as_slice() {
            // An entry failure is reported on stderr, then its payload is
            // dropped like any other value, and the process exits with 1.
            ops.extend([
                Op::Dup,
                Op::Enum(crate::sum::EnumOp::Tag(t.clone())),
                Op::PushInt(Value::I64(0)),
                Op::Eq,
                branch(
                    if t.get().variants[0].fields[0] == Ty::I32 {
                        vec![Op::Enum(crate::sum::EnumOp::Take(t.clone(), 0, 0))]
                    } else {
                        vec![Op::Drop, Op::PushInt(Value::I32(0))]
                    },
                    vec![
                        Op::Enum(crate::sum::EnumOp::Take(t.clone(), 1, 0)),
                        entry_report.as_ref().map_or_else(
                            || {
                                Op::Collection(CollectionOp::ReportEntryError(
                                    t.get().variants[1].fields[0].clone(),
                                ))
                            },
                            |symbol| Op::Call(symbol.clone()),
                        ),
                    ],
                ),
            ]);
        }
    }
    Ok(Program {
        inputs,
        source_paths,
        imported_symbols,
        ops,
        returns_status,
        exports,
    })
}
