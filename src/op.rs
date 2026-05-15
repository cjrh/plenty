//! The operation layer: the instruction set the VM executes, and the step that
//! turns lexed words into instructions.
//!
//! An [`Op`] is fully resolved — numbers parsed, string literals already in the
//! heap, function bodies compiled to nested `Op` sequences. A compiled program
//! is just a `Vec<Op>`, run without ever re-lexing its source.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::rc::Rc;

use crate::lexer::Tok;
use crate::value::{Heap, StrId};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// A Plenty type, as it appears in a function's type header (§11.2).
///
/// Monomorphic by design: `Int`, `Str`, `Bool` are the entire user-visible
/// vocabulary. No type variables, no parametric types. Arrays and sum types
/// are deferred (§12.7, §12.14).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ty {
    Int,
    Str,
    Bool,
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Ty::Int => "Int",
            Ty::Str => "Str",
            Ty::Bool => "Bool",
        })
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

/// A single instruction for the Plenty VM.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// Push an integer literal onto the stack.
    PushInt(i64),
    /// Push a string literal — already stored in the heap — onto the stack.
    PushStr(StrId),
    /// Pop two values; push their sum (integers) or concatenation (text).
    Add,
    /// Pop two integers `a b`; push `a - b`.
    Sub,
    /// Pop two integers `a b`; push `a * b`.
    Mul,
    /// Pop two integers `a b`; push `a / b`.
    Div,
    /// Print the whole stack — the `.` word.
    Display,
    /// Discard every value on the stack.
    Clear,
    /// Print the names in the current directory.
    ListDir,
    /// Define a function: bind `name` to an already-compiled body and docstring.
    ///
    /// The body is carved out of the token stream at compile time, so running
    /// this op never touches the runtime stack — whatever is on it stays put.
    DefineFn(String, CompiledFn),
    /// Invoke a user-defined function by name.
    Call(String),
    /// Push the value of the `i`-th input local of the enclosing call's frame
    /// (§11.5). Only emitted inside function bodies, so the VM always has at
    /// least one frame on its frame stack when it runs one.
    LoadLocal(u8),
}

/// A compiled function: the signature (§11.2), the docstring (§11.7), and
/// the body.
///
/// All three fields are `Rc`-shared so that defining a function — at either
/// compile time (`Op::DefineFn` carries one) or run time (the VM stores it
/// in the dictionary) — never copies the body, the docstring, or the sig.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledFn {
    pub sig: Rc<FnSig>,
    pub doc: Rc<str>,
    pub body: Rc<[Op]>,
}

/// Compile lexed words into ops, interning string literals into `heap`.
///
/// This is the only path from `Tok` to `Op`. It is used both for top-level
/// source and, recursively, for function bodies, so it depends on nothing but
/// the `Heap`.
pub fn compile(toks: &[Tok], heap: &mut Heap) -> Result<Vec<Op>> {
    Compiler { toks, pos: 0, heap, local_scopes: Vec::new() }
        .compile_seq(Stop::EndOfInput)
}

/// What ends the run of tokens a [`Compiler::compile_seq`] call is reading.
#[derive(Clone, Copy, PartialEq)]
enum Stop {
    /// The top level: stop at end of input; a `;` here is an error.
    EndOfInput,
    /// A function body: stop at — and consume — the matching `;`.
    Semicolon,
}

/// A cursor over a token slice that compiles it to ops.
///
/// Bundled into a struct because the four things — the tokens, the position
/// within them, the heap that literals are interned into, and the stack of
/// enclosing functions' input-name lists — all travel together through the
/// recursion that handles nested `: ... ;` definitions.
///
/// `local_scopes` is a stack only so that nested definitions can push and pop
/// cleanly; per §11.5, **only the innermost (topmost) scope is visible** at
/// any point. Outer scopes are inaccessible by design: nested functions do
/// not see their enclosing function's locals.
struct Compiler<'t, 'src> {
    toks: &'t [Tok<'src>],
    pos: usize,
    heap: &'t mut Heap,
    local_scopes: Vec<Vec<String>>,
}

impl Compiler<'_, '_> {
    /// Compile tokens from the current position until `stop` is reached,
    /// consuming the terminating `;` if there is one.
    fn compile_seq(&mut self, stop: Stop) -> Result<Vec<Op>> {
        let mut ops = Vec::new();
        while let Some(tok) = self.toks.get(self.pos).copied() {
            self.pos += 1;
            match tok {
                Tok::Word(";") if stop == Stop::Semicolon => return Ok(ops),
                Tok::Word(";") => return Err("';' has no matching ':'".into()),
                Tok::Word(":") => ops.push(self.compile_definition()?),
                Tok::Word(w) => match self.lookup_local(w) {
                    Some(ix) => ops.push(Op::LoadLocal(ix)),
                    None => ops.push(compile_word(w, self.heap)?),
                },
                Tok::Text(s) => ops.push(Op::PushStr(self.heap.add_str(unescape(s)?))),
            }
        }
        match stop {
            Stop::Semicolon => Err("':' has no matching ';'".into()),
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
        let name = match self.toks.get(self.pos).copied() {
            Some(Tok::Word(w)) if w != ":" && w != ";" => w.to_string(),
            Some(Tok::Word(_)) | None => {
                return Err("':' must be followed by a function name".into())
            }
            Some(Tok::Text(_)) => {
                return Err("a function name must be a plain word, not a text literal".into())
            }
        };
        self.pos += 1;
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
        let doc: Rc<str> = match self.toks.get(self.pos).copied() {
            Some(Tok::Text(s)) => {
                self.pos += 1;
                unescape(s)?.into()
            }
            Some(_) | None => {
                return Err(format!(
                    "function `{name}` is missing a docstring \
                     (expected \"...\" after the type header)"
                )
                .into())
            }
        };
        // The input names are in scope for the duration of the body. Pushing
        // a fresh scope per definition is what gives nested definitions their
        // own (non-inheriting) frame; pop on every exit, success or error, so
        // the scope stack tracks the lexical structure faithfully.
        let locals: Vec<String> = sig.inputs.iter().map(|(n, _)| n.clone()).collect();
        self.local_scopes.push(locals);
        let body_result = self.compile_seq(Stop::Semicolon);
        self.local_scopes.pop();
        let body = body_result?;
        Ok(Op::DefineFn(name, CompiledFn { sig, doc, body: body.into() }))
    }

    /// Compile a `{ name Type ... -> Type ... }` header (§11.2).
    ///
    /// Inputs are `name Type` pairs; outputs are either bare `Type`s or
    /// `name Type` pairs (the name is documentation-only and discarded).
    /// The `->` is mandatory; both sides may be empty. `fn_name` is used for
    /// error messages only.
    fn compile_sig(&mut self, fn_name: &str) -> Result<FnSig> {
        match self.toks.get(self.pos).copied() {
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
            match self.toks.get(self.pos).copied() {
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
            match self.toks.get(self.pos).copied() {
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
        match self.toks.get(self.pos).copied() {
            Some(Tok::Word(w)) => match parse_type(w) {
                Some(ty) => {
                    self.pos += 1;
                    Ok(ty)
                }
                None => Err(format!(
                    "function `{fn_name}` type header: `{w}` is not a known type \
                     (expected `Int`, `Str`, or `Bool`)"
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
        "Int" => Some(Ty::Int),
        "Str" => Some(Ty::Str),
        "Bool" => Some(Ty::Bool),
        _ => None,
    }
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
/// — into a number, a built-in, a function call (`:name`), or bare text.
fn compile_word(word: &str, heap: &mut Heap) -> Result<Op> {
    if let Ok(n) = word.parse::<i64>() {
        return Ok(Op::PushInt(n));
    }
    Ok(match word {
        "+" => Op::Add,
        "-" => Op::Sub,
        "*" => Op::Mul,
        "/" => Op::Div,
        "." => Op::Display,
        ":clear" => Op::Clear,
        ":listdir" => Op::ListDir,
        _ => match word.strip_prefix(':') {
            Some(name) => Op::Call(name.to_string()),
            None => Op::PushStr(heap.add_str(word.to_string())),
        },
    })
}

// --- type checking (§11.6) -------------------------------------------------

/// Type-check a compiled op stream against a side table of function sigs.
///
/// Forward abstract interpretation of `ops` over a tiny type lattice
/// (§11.6). Each op is treated as a stack effect: pop its declared inputs,
/// error on underflow or mismatch, push its outputs. Every function body
/// inside `ops` is recursively checked against its declared sig; top-level
/// ops have no declared sig, so they are checked op-by-op without an
/// end-of-stream invariant (the REPL case).
///
/// `prior_sigs` is the caller's already-known dictionary — typically the
/// VM's `functions` map. The checker also collects sigs from every
/// `DefineFn` reachable from `ops` (top-level and nested) into a single
/// table, so forward references *within* this source resolve cleanly.
/// References to functions that are neither in `prior_sigs` nor defined
/// in `ops` are rejected here, before any op executes.
///
/// Returns `Ok(())` if the program is well-typed; otherwise a stringly
/// error per §12.10. Error messages are name-bearing where they can be —
/// stack-language errors are hard to localise, so anchoring them to a
/// function name helps.
pub fn check(ops: &[Op], prior_sigs: &HashMap<String, Rc<FnSig>>) -> Result<()> {
    let mut sigs = prior_sigs.clone();
    collect_sigs(ops, &mut sigs);
    // Top-level: locals are empty (the compiler will never have emitted a
    // `LoadLocal` here either), and there is no end-of-stream invariant.
    let mut stack: Vec<Ty> = Vec::new();
    for op in ops {
        step(op, &mut stack, &[], &sigs)?;
    }
    Ok(())
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
        if let Op::DefineFn(name, f) = op {
            out.insert(name.clone(), Rc::clone(&f.sig));
            collect_sigs(&f.body, out);
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
) -> Result<()> {
    match op {
        Op::PushInt(_) => stack.push(Ty::Int),
        Op::PushStr(_) => stack.push(Ty::Str),
        Op::Add => {
            let (a, b) = pop2(stack, "+")?;
            let out = match (a, b) {
                (Ty::Int, Ty::Int) => Ty::Int,
                (Ty::Str, Ty::Str) => Ty::Str,
                _ => {
                    return Err(format!(
                        "`+` requires (Int Int) or (Str Str), got ({a} {b})"
                    )
                    .into())
                }
            };
            stack.push(out);
        }
        Op::Sub => arith(stack, "-")?,
        Op::Mul => arith(stack, "*")?,
        Op::Div => arith(stack, "/")?,
        Op::Display | Op::ListDir => {}
        Op::Clear => stack.clear(),
        Op::LoadLocal(i) => {
            let ty = locals.get(*i as usize).copied().ok_or_else(|| {
                format!("LoadLocal({i}) has no matching input in the enclosing function")
            })?;
            stack.push(ty);
        }
        Op::DefineFn(name, f) => check_body(name, &f.sig, &f.body, sigs)?,
        Op::Call(name) => check_call(name, stack, sigs)?,
    }
    Ok(())
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

/// Stack effect for the three integer-only arithmetic ops.
fn arith(stack: &mut Vec<Ty>, op_label: &str) -> Result<()> {
    let (a, b) = pop2(stack, op_label)?;
    if a != Ty::Int || b != Ty::Int {
        return Err(format!(
            "`{op_label}` requires (Int Int), got ({a} {b})"
        )
        .into());
    }
    stack.push(Ty::Int);
    Ok(())
}

/// Stack effect for a `Call(name)`: verify the top of the stack matches
/// the function's declared inputs in declaration order, then replace them
/// with the declared outputs.
fn check_call(
    name: &str,
    stack: &mut Vec<Ty>,
    sigs: &HashMap<String, Rc<FnSig>>,
) -> Result<()> {
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
    // `inputs[0]` is the deepest value on the stack at call time — same
    // direction as the runtime drain in `Vm::call`. So the type at
    // `stack[split + i]` must match `inputs[i]`.
    let split = stack.len() - n;
    for (i, (param, expected)) in sig.inputs.iter().enumerate() {
        let actual = stack[split + i];
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
        stack.push(*out);
    }
    Ok(())
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
    sigs: &HashMap<String, Rc<FnSig>>,
) -> Result<()> {
    let locals: Vec<Ty> = sig.inputs.iter().map(|(_, t)| *t).collect();
    let mut stack: Vec<Ty> = Vec::new();
    for op in body {
        step(op, &mut stack, &locals, sigs)
            .map_err(|e| -> Box<dyn Error> { format!("in `{fn_name}`: {e}").into() })?;
    }
    if stack != sig.outputs {
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
