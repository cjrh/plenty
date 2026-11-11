//! The syntax tree the parser builds and lowering consumes. Resolving a
//! written type (`TypeRef::resolve`) belongs to lowering and stays there.

use super::*;

#[derive(Clone)]
pub(super) struct Function {
    pub(super) once: bool,
    pub(super) captures: Vec<Capture>,
    pub(super) foreign: Option<crate::foreign::Declaration>,
    pub(super) export: Option<String>,
    pub(super) export_thread_safe: bool,
    pub(super) export_exact: bool,
    pub(super) name: String,
    pub(super) type_params: Vec<(String, Option<TypeRef>)>,
    pub(super) at: Token,
    pub(super) inputs: Vec<(String, TypeRef)>,
    /// Aligned with inputs; synthetic declarations may leave this empty.
    pub(super) defaults: Vec<Option<Expr>>,
    /// Where each parameter is named, aligned with inputs, for diagnostics
    /// about a parameter rather than its type. Synthetic declarations may
    /// leave this empty.
    pub(super) input_at: Vec<Token>,
    pub(super) output: TypeRef,
    pub(super) doc: String,
    /// Where the docstring was written, for diagnostics about a body that is only a docstring.
    pub(super) doc_at: Option<Token>,
    pub(super) body: Vec<Stmt>,
}

#[derive(Clone)]
pub(super) struct Capture {
    pub(super) name: String,
    pub(super) mutable: bool,
    pub(super) borrowed: bool,
}

#[derive(Clone)]
pub(super) struct TypeRef {
    /// Internal substitutions preserve inferred frame identities directly.
    /// Boxing keeps parsed type references and statement nodes compact.
    pub(super) concrete: Option<Box<Ty>>,
    /// Rare declaration/provenance facts stay boxed so adding a std ceiling
    /// does not enlarge every syntax node or statement annotation.
    pub(super) metadata: Option<Box<TypeMetadata>>,
    pub(super) at: Token,
    /// None denotes unit, spelled `()`.
    pub(super) name: Option<String>,
    pub(super) args: Vec<TypeRef>,
}

#[derive(Clone, Default)]
pub(super) struct TypeMetadata {
    pub(super) value: Option<Box<Expr>>,
    pub(super) value_parameter: bool,
    /// Substitutes a source type parameter, rather than a written concrete type.
    pub(super) parameter: bool,
    /// Std-only check of a statically known argument; resolved types and
    /// layout are unchanged.
    pub(super) check: Option<StaticCheck>,
}

/// A std-only declaration marker, checked where a direct call, default or
/// field update supplies a literal or module constant (see `ceilings.rs`).
#[derive(Clone, Copy)]
pub(super) enum StaticCheck {
    /// `__max(N)`: a fixed upper bound on an integer domain value.
    Ceiling(i64),
    /// `__open_mode`: a mode string that the runtime's `open` accepts.
    OpenMode,
}

impl TypeRef {
    pub(super) fn static_check(&self) -> Option<StaticCheck> {
        self.metadata.as_ref().and_then(|m| m.check)
    }
}

pub(super) struct TypeAlias {
    pub(super) dimension: bool,
    pub(super) at: Token,
    pub(super) name: String,
    pub(super) target: TypeRef,
}

#[derive(Clone)]
pub(super) struct Stmt {
    pub(super) at: Token,
    pub(super) kind: Statement,
}
#[derive(Clone)]
pub(super) enum Statement {
    Unpack {
        /// Leaf bindings, also used by scope and capture visitors.
        names: Vec<String>,
        /// Recursive declaration targets contain only names and tuples;
        /// executable place destinations use `SetIndex` instead.
        targets: Option<Vec<Expr>>,
        /// Iteration bindings shadow outer locals; user assignments may reuse them.
        fresh: bool,
        mutable: bool,
        value: Expr,
    },
    With {
        manager: Expr,
        name: Option<String>,
        body: Vec<Stmt>,
    },
    Yield(Expr),
    Match {
        value: Expr,
        cases: Vec<enums::Case>,
    },
    Expr(Expr),
    Assign {
        name: String,
        mutable: bool,
        annotation: Option<TypeRef>,
        value: Expr,
    },
    Return(Option<Expr>),
    If {
        condition: Expr,
        yes: Vec<Stmt>,
        // A list, not an `if` nested in `no`: a pass that recursed once per
        // arm overflowed the stack on a long chain (issue #70).
        elifs: Vec<Elif>,
        no: Vec<Stmt>,
    },
    Pass,
    Break,
    Continue,
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    For {
        name: Vec<String>,
        iterable: Expr,
        body: Vec<Stmt>,
    },
    SetIndex {
        target: Expr,
        value: Expr,
    },
    Augment {
        target: Expr,
        op: String,
        value: Expr,
    },
}

#[derive(Clone)]
pub(super) struct Elif {
    pub(super) at: Token,
    pub(super) condition: Expr,
    pub(super) body: Vec<Stmt>,
}

#[derive(Clone)]
pub(super) struct Expr {
    pub(super) at: Token,
    pub(super) kind: Expression,
}
#[derive(Clone)]
pub(super) enum Expression {
    /// A call-site label; binding removes it before ordinary expression lowering.
    NamedArgument(String, Box<Expr>),
    Anonymous(Box<Function>),
    GenericValue(String, Vec<TypeRef>),
    GenericCall(String, Vec<TypeRef>, Vec<Expr>),
    Tuple(Vec<Expr>),
    Try(Box<Expr>),
    ClassNew(crate::nominal::Nominal<crate::record::ClassType>),
    ClassReady(crate::nominal::Nominal<crate::record::ClassType>, Box<Expr>),
    // Boxed: a type reference inline would more than double every expression node.
    Type(Box<TypeRef>),
    /// The position is the member name's, for editor queries; see [`Pos`].
    Member(Box<Expr>, String, Pos),
    Number(String),
    Text(String),
    /// An f-string, or one of its fields inside a generated function.
    Format(Box<fstrings::Format>),
    Bytes(Vec<u8>),
    Bool(bool),
    Collection {
        kind: String,
        entries: Vec<(Expr, Option<Expr>)>,
        clauses: Vec<Clause>,
    },
    Index(Box<Expr>, Box<Expr>),
    /// `base[start:stop:step]`, a copying slice; see
    /// book/src/design/language/04-expressions/slices.md. Its own variant, so
    /// no pass can mistake it for an index.
    Slice(Box<Expr>, Box<SliceBounds>),
    Method(Box<Expr>, String, Vec<Expr>, Pos),
    // The name is boxed to keep `Expr` within its size bound with the position.
    GenericMethod(Box<Expr>, Box<str>, Vec<TypeRef>, Vec<Expr>, Pos),
    /// `base[...]` whose brackets parse both as type arguments and as an index.
    /// `generic` is the Generic* reading, with any call arguments; `index` is
    /// `Index(base, ...)` without them, or its nesting diagnostic. Module name
    /// resolution distinguishes locals from declarations and keeps one reading;
    /// no later pass sees this variant.
    Ambiguous {
        generic: Box<Expr>,
        index: IndexReading,
    },
    Constructor(Box<TypeRef>, Vec<Expr>),
    Unit,
    Group(Box<Expr>),
    Name(String),
    Call(String, Vec<Expr>),
    Invoke(Box<Expr>, Vec<Expr>),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Conditional {
        condition: Box<Expr>,
        yes: Box<Expr>,
        no: Box<Expr>,
    },
    /// A `match` block used as a value. The parser accepts it only as the
    /// whole value of a binding, an assignment, or a `return`, where the
    /// statement can end with the block.
    Match(Box<Expr>, Vec<enums::Case>),
    /// A `checked:` block, accepted where a `match` block is. Its value is
    /// `Ok` of its final expression, or `Err` of the first arithmetic error
    /// written inside it.
    /// Arithmetic suite: `checked:`, `unchecked:` or `unchecked trap:`.
    Checked(Suite, Vec<Stmt>),
    /// One conditional statement used as a whole statement value; retaining
    /// its suites shares branch joins, lazy tests, and cleanup with tail `if`.
    If(Vec<Stmt>),
}

/// The arithmetic contract an explicit suite selects; see
/// `book/src/design/language/03-types/numbers.md#unchecked-blocks`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Suite {
    /// `checked:`: local recoverable arithmetic.
    Checked,
    /// `unchecked:`: the programmer guarantees the arithmetic preconditions;
    /// a violation traps.
    Unchecked,
    /// `unchecked trap:`: the same as `unchecked:`.
    Trap,
}

/// The bounds of a slice, each optional as in Python.
#[derive(Clone)]
pub(super) struct SliceBounds {
    pub(super) start: Option<Expr>,
    pub(super) stop: Option<Expr>,
    pub(super) step: Option<Expr>,
}

impl SliceBounds {
    pub(super) fn iter(&self) -> impl Iterator<Item = &Expr> {
        [&self.start, &self.stop, &self.step].into_iter().flatten()
    }
    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Expr> {
        [&mut self.start, &mut self.stop, &mut self.step]
            .into_iter()
            .flatten()
    }
}

/// Where a member name was written. Expressions carry only the position of
/// their first token, so `a.b.c` would otherwise not say where `c` is. A
/// synthetic member has the default position, which no source token has.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Pos {
    pub(super) line: u32,
    pub(super) column: u32,
}

impl Pos {
    pub(super) fn of(at: &Token) -> Self {
        Pos {
            line: at.line,
            column: at.column,
        }
    }
}

#[derive(Clone)]
pub(super) enum Clause {
    For(Vec<String>, Expr),
    /// Irrefutable nested targets, bound immediately after their for source.
    Unpack(Vec<String>, Vec<Expr>, Expr),
    If(Expr),
}

/// A speculative index parse, retaining its structured nesting diagnostic.
pub(super) type IndexReading = std::result::Result<Box<Expr>, Box<crate::diagnostic::Diagnostic>>;
