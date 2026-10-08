//! Typed operations and independent checking before native code generation.
//! Also contains the historical stack-syntax lowering used by backend tests.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::rc::Rc;

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
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Ty {
    Closure(Rc<crate::closure::ClosureType>),
    Callable(Rc<CallableSig>),
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
    File,
    /// Nominal opaque C pointer. No dereference or integer conversions.
    ForeignPtr(Rc<str>),
    Bool,
    List(Rc<Ty>),
    Set(Rc<Ty>),
    Dict(Rc<Ty>, Rc<Ty>),
    Range(Rc<Ty>),
    Enum(Rc<crate::sum::EnumType>),
    Class(Rc<crate::record::ClassType>),
    Generator(Rc<crate::generator::GeneratorType>),
    Ref(Rc<Ty>, bool),
}

impl Ty {
    /// Range payloads also occur inside standard sums. Tags remain in the
    /// ordinary 128-bit representation; slot_bytes includes owner-local payloads.
    pub fn has_inline_range(&self) -> bool {
        matches!(self, Self::Range(_)) || matches!(self, Self::Enum(t) if t.inline_range)
    }
    pub fn slot_bytes(&self) -> usize {
        16 + self.inline_bytes()
    }
    pub fn inline_sum(&self) -> bool {
        matches!(self, Self::Enum(t) if t.inline())
    }
    pub fn is_float(&self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }
    pub fn is_numeric(&self) -> bool {
        self.is_int() || self.is_float()
    }
    pub fn layout_depth(&self) -> usize {
        match self {
            Self::Closure(t) => {
                1 + t
                    .captures
                    .iter()
                    .map(|(_, t)| t.layout_depth())
                    .max()
                    .unwrap_or(0)
            }
            Ty::Callable(sig) => {
                1 + sig
                    .inputs
                    .iter()
                    .chain(sig.output.iter())
                    .map(Ty::layout_depth)
                    .max()
                    .unwrap_or(0)
            }
            Self::List(t) | Self::Set(t) => 1 + t.layout_depth(),
            Self::Generator(t) => 1 + t.element.layout_depth(),
            Self::Dict(k, v) => 1 + k.layout_depth().max(v.layout_depth()),
            Self::Enum(t) => t.depth,
            Self::Class(t) => t.depth,
            _ => 0,
        }
    }
    pub fn affine(&self) -> bool {
        matches!(
            self,
            Self::List(_)
                | Self::Closure(_)
                | Self::Set(_)
                | Self::Dict(_, _)
                | Self::Generator(_)
                | Self::Class(_)
                | Self::File
        ) || matches!(self, Self::Enum(t) if t.affine)
    }
    pub fn restricted_storage(&self) -> bool {
        match self {
            Self::Generator(_) | Self::Closure(_) | Self::Ref(..) => true,
            Self::Enum(t) => t.restricted_storage,
            _ => false,
        }
    }
    pub fn contains_reference(&self) -> bool {
        // Composite references are rejected at their construction boundary.
        matches!(self, Self::Ref(..))
    }
    pub fn can_copy(&self) -> bool {
        match self {
            Self::Generator(_) | Self::Closure(_) | Self::Ref(..) | Self::File => false,
            Self::Class(t) => t.copyable,
            Self::Enum(t) => t.copyable,
            Self::List(t) | Self::Set(t) => t.can_copy(),
            Self::Dict(k, v) => k.can_copy() && v.can_copy(),
            _ => true,
        }
    }
    pub fn has_destructor(&self) -> bool {
        match self {
            Self::Generator(_) | Self::Closure(_) | Self::File => true,
            Self::Class(t) => t.has_destructor,
            Self::Enum(t) => t.has_destructor,
            Self::List(t) | Self::Set(t) => t.has_destructor(),
            Self::Dict(k, v) => k.has_destructor() || v.has_destructor(),
            _ => false,
        }
    }
    /// Values requiring cleanup have one owner per operand/local. This includes
    /// inline generators as well as heap-backed values; scalars copy as bits.
    pub fn managed(&self) -> bool {
        if let Self::Enum(t) = self {
            if t.inline() {
                return t.managed;
            }
        }
        matches!(
            self,
            Self::Str
                | Self::Closure(_)
                | Self::File
                | Self::List(_)
                | Self::Set(_)
                | Self::Dict(_, _)
                | Self::Enum(_)
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

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Ty::Closure(t) => return write!(f, "closure {} from `{}`", t.signature, t.name),
            Ty::Callable(sig) => return write!(f, "{sig}"),
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
            Ty::File => "File",
            Ty::ForeignPtr(name) => return f.write_str(name),
            Ty::Bool => "bool",
            Ty::List(t) => return write!(f, "list[{t}]"),
            Ty::Set(t) => return write!(f, "set[{t}]"),
            Ty::Dict(k, v) => return write!(f, "dict[{k}, {v}]"),
            Ty::Range(t) => {
                return if **t == Ty::I64 {
                    f.write_str("range")
                } else {
                    write!(f, "range[{t}]")
                }
            }
            Ty::Enum(t) => return f.write_str(&t.name),
            Ty::Class(t) => return f.write_str(&t.name),
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

/// A typed operation lowered into native code.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    ClosureNew(Rc<crate::closure::ClosureType>),
    ClosureCall(Rc<crate::closure::ClosureType>),
    FunctionAddress(String, Rc<CallableSig>),
    CallIndirect(Rc<CallableSig>),
    TailCallIndirect(Rc<CallableSig>),
    /// Unwrap a standard sum or return its residual, releasing pending operands.
    Try {
        source: Rc<crate::sum::EnumType>,
        target: Rc<crate::sum::EnumType>,
        cleanup: Rc<[Op]>,
    },
    Class(crate::record::ClassOp),
    BorrowLocal(u8, bool),
    ReadRef(Ty),
    Reborrow(Ty),
    WriteRef(Ty),
    Loan(crate::ownership::Loan),
    UseLoan(usize),
    Access(u8, bool, Option<usize>),
    Yield(Ty),
    Next(u8, String),
    MoveLocal(u8, String),
    DropLocal(u8),
    Enum(crate::sum::EnumOp),
    /// Defensive trap after an exhaustive finite-domain match.
    Unreachable,
    Collection(crate::collection::CollectionOp),
    /// The condition leaves bool; a continuing body preserves the operand stack.
    Loop {
        condition: Rc<[Op]>,
        body: Rc<[Op]>,
    },
    /// Push an integer literal onto the stack. The payload is always an
    /// integer `Value`; retaining its width makes suffixed literals direct.
    PushInt(Value),
    PushFloat {
        bits: u64,
        ty: Ty,
    },
    PushUnit,
    FloatNeg,
    /// Push a string literal — already stored in the heap — onto the stack.
    PushStr(StrId),
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
    DefineFn(String, CompiledFn),
    /// Invoke a user-defined function by name. Non-tail position.
    Call(String),
    ForeignCall {
        declaration: crate::foreign::Declaration,
        sig: Rc<FnSig>,
    },
    ForeignNull(Ty),
    /// Invoke a user-defined function by name from tail position (§11.8).
    /// Native lowering reuses the caller's frame. Emitted by tail-call marking.
    TailCall(String),
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
    Match(Rc<[MatchArm]>),
    /// Pop an integer of any width; push its representation at the target
    /// integer width. Surface syntax is `:as-i8` ... `:as-u64`. Conversion
    /// follows Rust's `as` semantics: widening sign-extends signed sources
    /// and zero-extends unsigned ones; narrowing truncates; equal-width
    /// signedness change reinterprets the bit pattern. Casts that would
    /// silently change a value's mathematical meaning are still allowed —
    /// that is the whole point of an explicit cast word.
    Cast(Ty),
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

/// One arm of a [`Op::Match`]. The pattern is matched against the popped
/// value; if the match succeeds, `body` is executed against the current
/// data stack and the enclosing call's locals frame.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Rc<[Op]>,
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
    pub location: Option<Rc<str>>,
    pub generator: Option<Ty>,
    pub sig: Rc<FnSig>,
    pub doc: Rc<str>,
    pub body: Rc<[Op]>,
    /// Types of local slots after the parameters. Allocated once per call.
    pub locals: Rc<[Ty]>,
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
                return Err("':' must be followed by a function name".into())
            }
            Some(Tok::Text(_)) => {
                return Err("a function name must be a plain word, not a text literal".into())
            }
        };
        self.pos += 1;
        if is_reserved_function_name(&name) {
            return Err(format!("function name `{name}` is reserved for a builtin word").into());
        }
        let sig: Rc<FnSig> = self.compile_sig(&name)?.into();
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
        let doc: Rc<str> = match self.toks.get(self.pos).cloned() {
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
            CompiledFn {
                location: None,
                sig,
                doc,
                generator: None,
                body: body.into(),
                locals: Rc::from([]),
            },
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
                    return Err("match arm is missing a pattern before `[`".into())
                }
                Some(Tok::Word(";")) | Some(Tok::Word("]")) | None => {
                    return Err("`match` has no matching `end`".into())
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
                    )
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
                .into())
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
                    .into())
                }
                Some(Tok::Word(w)) if parse_type(w).is_some() => {
                    return Err(format!(
                        "function `{fn_name}` type header: input requires a name \
                         before the type `{w}` (write `{{ x {w} -> ... }}`)"
                    )
                    .into())
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
                    .into())
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
                    .into())
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
/// `Rc<[Op]>`, so mutating an arm body means rebuilding it; we only do that
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
        _ => {}
    }
}

// --- type checking (§11.6) -------------------------------------------------

/// Independently check a complete module's operations and function signatures.
/// Abstract operand stacks track concrete types without executing the program.
pub fn check(ops: &[Op]) -> Result<()> {
    let mut sigs = HashMap::new();
    collect_sigs(ops, &mut sigs);
    // Top-level: locals are empty (the compiler will never have emitted a
    // `LoadLocal` here either), and there is no end-of-stream invariant.
    let mut stack = Vec::new();
    check_sequence(ops, &mut stack, &[], &sigs, None, None, None)?;
    Ok(())
}

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
    sigs: &HashMap<String, Rc<FnSig>>,
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
fn collect_sigs(ops: &[Op], out: &mut HashMap<String, Rc<FnSig>>) {
    for op in ops {
        match op {
            Op::DefineFn(name, f) => {
                out.insert(name.clone(), Rc::clone(&f.sig));
                collect_sigs(&f.body, out);
            }
            Op::Loop { condition, body } => {
                collect_sigs(condition, out);
                collect_sigs(body, out);
            }
            Op::Match(arms) => {
                for arm in arms.iter() {
                    collect_sigs(&arm.body, out);
                }
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
    sigs: &HashMap<String, Rc<FnSig>>,
    returns: Option<&[Ty]>,
    loop_stack: Option<&[Ty]>,
    yield_ty: Option<&Ty>,
) -> Result<Flow> {
    match op {
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
                || (!source.is_option()
                    && !target.discards_error()
                    && source.variants[1].fields != target.variants[1].fields)
                || yield_ty.is_some()
                || returns != Some(&[Ty::Enum(target.clone())][..])
                || stack.pop() != Some(Ty::Enum(source.clone()))
            {
                return Err("invalid Result/Option propagation".into());
            }
            stack.push(source.variants[usize::from(source.is_option())].fields[0].clone());
        }
        Op::Loan(_) | Op::UseLoan(_) | Op::Access(..) => {}
        Op::BorrowLocal(i, mutable) => {
            let ty = locals.get(*i as usize).ok_or("invalid borrowed place")?;
            stack.push(Ty::Ref(Rc::new(ty.clone()), *mutable));
        }
        Op::ReadRef(ty) => {
            if !matches!(stack.pop(), Some(Ty::Ref(t, _)) if t.as_ref() == ty) {
                return Err("invalid reference read".into());
            }
            stack.push(ty.clone());
        }
        Op::Reborrow(target) => {
            let Some(Ty::Ref(source, writable)) = stack.pop() else {
                return Err("invalid reborrow".into());
            };
            let Ty::Ref(inner, mutable) = target else {
                return Err("invalid reborrow target".into());
            };
            if source != *inner || (*mutable && !writable) {
                return Err("invalid reborrow permissions".into());
            }
            stack.push(target.clone());
        }
        Op::WriteRef(ty) => {
            if stack.pop() != Some(Ty::Ref(Rc::new(ty.clone()), true))
                || stack.pop().as_ref() != Some(ty)
            {
                return Err("invalid reference write".into());
            }
        }
        Op::Collection(operation) => {
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
        Op::Unreachable => return Ok(Flow::Exits),
        Op::Yield(ty) => {
            if yield_ty != Some(ty) || stack.pop().as_ref() != Some(ty) || !stack.is_empty() {
                return Err(
                    "yield requires its declared element type and an empty residual operand stack"
                        .into(),
                );
            }
        }
        Op::Next(slot, _) => {
            let Some(Ty::Generator(element)) = locals.get(*slot as usize) else {
                return Err("next requires a generator local".into());
            };
            stack.push(crate::sum::option(element.element.clone()));
        }
        Op::DropLocal(i) => {
            locals.get(*i as usize).ok_or("invalid drop local")?;
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
        Op::FloatNeg => {
            let ty = stack.last().ok_or("stack underflow on float negation")?;
            if !ty.is_float() {
                return Err("float negation requires a float".into());
            }
        }
        Op::PushStr(_) => stack.push(Ty::Str),
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
                    .into())
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
        Op::LoadLocal(i) | Op::MoveLocal(i, _) => {
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
        Op::DefineFn(name, f) => {
            check_body(name, &f.sig, &f.body, &f.locals, sigs, f.generator.as_ref()).map_err(
                |e| -> Box<dyn std::error::Error> {
                    match &f.location {
                        Some(at) => format!("{at}: {e}").into(),
                        None => e,
                    }
                },
            )?
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
                    .map(|(i, (n, ty))| (n.clone(), Ty::Ref(Rc::new(ty.clone()), t.writable[i])))
                    .chain(t.signature.function().inputs)
                    .collect(),
                outputs: t.signature.output.iter().cloned().collect(),
            };
            let actual = sigs.get(&t.name).ok_or("undefined closure body")?;
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
            if stack.pop() != Some(Ty::Ref(Rc::new(Ty::Closure(t.clone())), t.mutable)) {
                return Err("closure call requires an environment borrow".into());
            }
            stack.extend(t.signature.output.iter().cloned());
        }
        Op::FunctionAddress(name, signature) => {
            let actual = sigs.get(name).ok_or("undefined function value")?;
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
            return check_match(arms, stack, locals, sigs, returns, loop_stack, yield_ty)
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

/// Stack effect for numeric ordering: same-typed numbers in, Bool out.
fn cmp_int(stack: &mut Vec<Ty>, op_label: &str) -> Result<()> {
    let (a, b) = pop2(stack, op_label)?;
    if !a.is_numeric() || a != b {
        return Err(format!("`{op_label}` requires same-typed numbers, got ({a} {b})").into());
    }
    stack.push(Ty::Bool);
    Ok(())
}

/// Stack effect for a `Call(name)`: verify the top of the stack matches
/// the function's declared inputs in declaration order, then replace them
/// with the declared outputs.
fn check_call(name: &str, stack: &mut Vec<Ty>, sigs: &HashMap<String, Rc<FnSig>>) -> Result<()> {
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
        if actual != *expected {
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
    sigs: &HashMap<String, Rc<FnSig>>,
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

/// Check one function body against its declared sig.
///
/// The body's abstract data stack starts **empty** — inputs are drained
/// into the locals frame by `Op::Call`, not left on the stack — and the
/// inputs become the body's `locals` for `LoadLocal` to resolve against.
/// At end of body the abstract stack must equal the declared outputs
/// exactly; anything else is a type error.
fn check_body(
    fn_name: &str,
    sig: &FnSig,
    body: &[Op],
    extra_locals: &[Ty],
    sigs: &HashMap<String, Rc<FnSig>>,
    yield_ty: Option<&Ty>,
) -> Result<()> {
    let locals: Vec<Ty> = sig
        .inputs
        .iter()
        .map(|(_, t)| t.clone())
        .chain(extra_locals.iter().cloned())
        .collect();
    let mut stack: Vec<Ty> = Vec::new();
    let returns = if yield_ty.is_some() {
        &[][..]
    } else {
        &sig.outputs
    };
    crate::ownership::check(body, &locals, sig.inputs.len())?;
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
