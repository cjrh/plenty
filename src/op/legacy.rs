//! The historical stack syntax, compiled straight to operations. Backend
//! tests are written in it; the CLI reaches it through `--legacy`.

pub(crate) mod lexer;

use super::*;
use lexer::Tok;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_unsuffixed_literal_must_fit_i64() {
        for source in [
            "18446744073709551615 .",
            "-9223372036854775809 .",
            ": f { -> i64 } 9223372036854775808 ;",
        ] {
            let toks = lexer::lex(source).unwrap();
            let mut heap = crate::value::Heap::default();
            let error = compile(&toks, &mut heap).unwrap_err().to_string();
            assert!(error.contains("does not fit i64"), "{source}: {error}");
        }
    }
}
