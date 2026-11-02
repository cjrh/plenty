//! The core statement and expression parser. It reads tokens only, never
//! lowering state. Feature modules add their own `impl Parser` blocks beside
//! their lowering, which is deliberate.

use super::*;

/// Indented blocks and expressions nest to this many levels in total. Every
/// pass over the tree, and the tree's own clone and drop, recurses once per
/// level, so an unbounded depth overflows the compiler's stack (issue #70).
pub(super) const MAX_NESTING: usize = 64;
pub(super) const NESTING: &str =
    "block and expression nesting exceeds the implementation limit of 64";

pub(super) struct Parser {
    /// The item being parsed and the items buffered after it, behind the
    /// last token of the item before.
    pub(super) tokens: VecDeque<Token>,
    /// Whether each token lies inside `()`, `[]`, or `{}`. The lexer drops
    /// layout tokens there, so the parser cannot recover this from `tokens`.
    pub(super) in_brackets: VecDeque<bool>,
    pub(super) pos: usize,
    /// Tokens released ahead of `tokens`.
    pub(super) released: usize,
    /// The token counts, from the start of the module, at which the buffered
    /// items end.
    pub(super) items: VecDeque<usize>,
    /// Whether `tokens` reaches the end of the module.
    pub(super) complete: bool,
    /// Set when the parser asks for a token past the buffered items. What it
    /// then parsed is not what the whole module holds.
    pub(super) starved: Cell<bool>,
    /// Stands in for a token past the buffered items.
    pub(super) end: Token,
    pub(super) type_depth: usize,
    /// Blocks and expressions open around the one being parsed.
    pub(super) depth: usize,
    /// Level of the deepest node in the expression being parsed. An operator
    /// parsed after its left operand moves that operand down a level, so the
    /// depth of the calls into `expr` alone does not bound the tree.
    pub(super) deepest: usize,
    /// Whether the module has an f-string, which needs `std.fmt`.
    pub(super) formats: bool,
    /// Whether the module has a slice, which can produce `std.bytes.Bytes`.
    pub(super) slices: bool,
    /// Whether the module can introduce a `bytes` value: it has a `b"..."`
    /// literal, `include_bytes`, or the word `bytes`. See
    /// [`modules::Loader::slice_bytes`].
    pub(super) mentions_bytes: bool,
    /// Whether the module has the word `FormatError`, the prelude name that
    /// `std.fmt` declares.
    pub(super) mentions_format_error: bool,
    /// Elements of the lists being parsed; see [`Parser::exact_list`].
    pub(super) scratch_stmts: Vec<Stmt>,
    pub(super) scratch_exprs: Vec<Expr>,
    pub(super) scratch_inputs: Vec<(String, TypeRef)>,
    pub(super) scratch_type_params: Vec<(String, Option<TypeRef>)>,
    pub(super) scratch_type_args: Vec<TypeRef>,
}
impl Parser {
    /// A parser for a module whose tokens name `origin` as their source file.
    pub(super) fn new(origin: Option<FileId>) -> Self {
        Parser {
            tokens: VecDeque::new(),
            in_brackets: VecDeque::new(),
            pos: 0,
            released: 0,
            items: VecDeque::new(),
            complete: false,
            starved: Cell::new(false),
            end: Token {
                kind: Kind::Eof,
                line: 0,
                column: 0,
                file: origin,
            },
            type_depth: 0,
            depth: 0,
            deepest: 0,
            formats: false,
            slices: false,
            mentions_bytes: false,
            mentions_format_error: false,
            scratch_stmts: Vec::new(),
            scratch_exprs: Vec::new(),
            scratch_inputs: Vec::new(),
            scratch_type_params: Vec::new(),
            scratch_type_args: Vec::new(),
        }
    }
    /// Runs `parse`, which pushes a list's elements onto the scratch stack
    /// that `scratch` selects, and moves them into a vector of exactly their
    /// number. A vector grown element by element reserves four, over 1.6 KB
    /// for a one-statement block, and shrinking it copies them again; the
    /// whole tree is live when lowering starts (issues #54, #422). Lists
    /// nest, so each takes only the elements above where it started, also
    /// after a failed parse that a caller discards.
    pub(super) fn exact_list<T>(
        &mut self,
        scratch: fn(&mut Self) -> &mut Vec<T>,
        parse: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<Vec<T>> {
        let base = scratch(self).len();
        let parsed = parse(self);
        let elements = scratch(self).drain(base..);
        parsed.map(|()| elements.collect())
    }
    /// Releases the tokens of the items already parsed and buffers the next
    /// item with `lookahead` items after it. Call between top-level items.
    pub(super) fn fill(&mut self, lexer: &mut Lexer, lookahead: usize) -> Result<()> {
        // The token before `pos` tells how the last statement ended.
        let parsed = self.pos.saturating_sub(1);
        self.tokens.drain(..parsed);
        self.in_brackets.drain(..parsed);
        self.pos -= parsed;
        self.released += parsed;
        let next = self.released + self.pos;
        while self.items.front().is_some_and(|&end| end <= next) {
            self.items.pop_front();
        }
        while self.items.len() <= lookahead && !lexer.done {
            let lexed = self.tokens.len();
            lexer.item(&mut self.tokens)?;
            // An item ends outside brackets, so each starts at depth zero.
            let mut depth = 0usize;
            for token in self.tokens.range_mut(lexed..) {
                token.file = self.end.file;
                // Brackets are symbols, so other tokens need no comparison.
                let symbol = match &token.kind {
                    Kind::Symbol(s) => *s,
                    _ => "",
                };
                if matches!(symbol, ")" | "]" | "}") {
                    depth = depth.saturating_sub(1);
                }
                self.in_brackets.push_back(depth > 0);
                if matches!(symbol, "(" | "[" | "{") {
                    depth += 1;
                }
                self.mentions_bytes |= match &token.kind {
                    Kind::Bytes(_) => true,
                    Kind::Word(word) => matches!(&**word, "bytes" | "include_bytes"),
                    _ => false,
                };
                self.mentions_format_error |=
                    matches!(&token.kind, Kind::Word(word) if &**word == "FormatError");
            }
            // An annotation line belongs to the declaration below it, so
            // stacked annotations do not use up the lookahead.
            if !lexer.done && self.tokens.get(lexed).is_some_and(|t| t.is("@")) {
                continue;
            }
            self.items.push_back(self.released + self.tokens.len());
        }
        self.complete = lexer.done;
        Ok(())
    }
    /// Token `index` of the buffer, or None past the end of the module.
    pub(super) fn token(&self, index: usize) -> Option<&Token> {
        let token = self.tokens.get(index);
        if token.is_none() && !self.complete {
            self.starved.set(true);
        }
        token
    }
    /// Parse a block or an expression one level below the current one.
    pub(super) fn nested<T>(&mut self, parse: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let around = self.deepest;
        self.depth += 1;
        self.deepest = self.depth;
        let result = if self.depth > MAX_NESTING {
            Err(self.peek().error(NESTING))
        } else {
            parse(self)
        };
        self.depth -= 1;
        // A failed parse may be a discarded reading (`index_reading`), which
        // must not count against the expression around it.
        self.deepest = if result.is_ok() {
            self.deepest.max(around)
        } else {
            around
        };
        result
    }
    /// The expression parsed so far becomes an operand of the operator just
    /// taken: `a + b + c` is `(a + b) + c`, one level per operator.
    fn deepen(&mut self) -> Result<()> {
        self.deepest += 1;
        if self.deepest > MAX_NESTING {
            return Err(self.tokens[self.pos - 1].error(NESTING));
        }
        Ok(())
    }
    /// Parse the expression of the comprehension clause whose keyword is token
    /// `keyword` and which follows `outer` others. A clause is a level around
    /// the clauses after it and around the element, whose deepest node is at
    /// level `element`.
    pub(super) fn clause(
        &mut self,
        keyword: usize,
        outer: usize,
        element: &mut usize,
    ) -> Result<Expr> {
        *element += 1;
        if *element > MAX_NESTING {
            return Err(self.tokens[keyword].error(NESTING));
        }
        self.depth += outer;
        let expression = self.expr(1);
        self.depth -= outer;
        expression
    }
    pub(super) fn peek(&self) -> &Token {
        self.token(self.pos).unwrap_or(&self.end)
    }
    pub(super) fn take(&mut self) -> Token {
        let t = self.peek().clone();
        self.pos += 1;
        t
    }
    pub(super) fn eat(&mut self, text: &str) -> bool {
        if self.peek().is(text) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    pub(super) fn expect(&mut self, text: &str) -> Result<()> {
        if self.eat(text) {
            Ok(())
        } else {
            Err(self.peek().error(format!("expected `{text}`")))
        }
    }
    pub(super) fn kind(&mut self, kind: Kind, label: &str) -> Result<()> {
        if self.peek().kind == kind {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.peek().error(format!("expected {label}")))
        }
    }
    pub(super) fn name(&mut self) -> Result<String> {
        let t = self.take();
        if let Kind::Word(ref s) = t.kind
            && !reserved(s)
            && !s.starts_with("__plenty_")
        {
            return Ok(String::from(&**s));
        }
        Err(t.error("expected an identifier (keywords and __plenty_ names are reserved)"))
    }
    pub(super) fn ty(&mut self) -> Result<TypeRef> {
        if self.type_depth >= 64 {
            return Err(self
                .peek()
                .error("type nesting exceeds the implementation limit of 64"));
        }
        self.type_depth += 1;
        let result = self.ty_sum();
        self.type_depth -= 1;
        result
    }
    fn ty_sum(&mut self) -> Result<TypeRef> {
        let first = self.ty_inner()?;
        if !self.eat("|") {
            return Ok(first);
        }
        let at = first.at.clone();
        let mut args = vec![first, self.ty_inner()?];
        while self.eat("|") {
            args.push(self.ty_inner()?);
        }
        Ok(TypeRef {
            concrete: None,
            parameter: false,
            at,
            name: Some("|".into()),
            args,
        })
    }
    fn ty_inner(&mut self) -> Result<TypeRef> {
        let at = self.peek().clone();
        if self.eat("&") {
            let mutable = self.eat("mut");
            // `&[T]` and `&mut [T]` are slice views; see `view_type`.
            let view = self.eat("[");
            let name = match (view, mutable) {
                (false, false) => "&",
                (false, true) => "&mut",
                (true, false) => VIEW_TYPE,
                (true, true) => MUT_VIEW_TYPE,
            };
            if self.type_depth >= 64 {
                return Err(at.error("type nesting exceeds the implementation limit of 64"));
            }
            self.type_depth += 1;
            let inner = if view {
                self.ty().and_then(|inner| self.expect("]").map(|_| inner))
            } else {
                self.ty_inner()
            };
            self.type_depth -= 1;
            return Ok(TypeRef {
                concrete: None,
                parameter: false,
                at,
                name: Some(name.into()),
                args: vec![inner?],
            });
        }
        if self.eat("(") {
            let mut args = Vec::new();
            let mut comma = false;
            while !self.eat(")") {
                args.push(self.ty()?);
                if self.eat(")") {
                    break;
                }
                self.expect(",")?;
                comma = true;
            }
            if args.len() == 1 && !comma {
                return Ok(args.pop().unwrap());
            }
            return Ok(TypeRef {
                concrete: None,
                parameter: false,
                at,
                name: if args.is_empty() {
                    None
                } else {
                    Some("tuple".into())
                },
                args,
            });
        }
        let t = self.take();
        if let Kind::Word(ref name) = t.kind
            && !reserved(name)
        {
            let mut name = String::from(&**name);
            while self.eat(".") {
                name.push('.');
                name.push_str(&self.name()?);
            }
            if matches!(
                name.as_str(),
                "Callable" | "BoxedCallable" | "Closure" | "OnceClosure" | "OnceCallable"
            ) {
                if !(self.eat("[") && self.eat("[")) {
                    return Err(self
                        .peek()
                        .error(format!("use {name}[[parameter types], return type]")));
                }
                let mut args = Vec::new();
                while !self.eat("]") {
                    args.push(self.ty()?);
                    if self.eat("]") {
                        break;
                    }
                    self.expect(",")?;
                }
                self.expect(",")?;
                args.push(self.ty()?);
                self.eat(",");
                self.expect("]")?;
                return Ok(TypeRef {
                    concrete: None,
                    parameter: false,
                    at,
                    name: Some(name),
                    args,
                });
            }
            return Ok(TypeRef {
                concrete: None,
                parameter: false,
                at,
                name: Some(name),
                args: self.type_arguments()?,
            });
        }
        if t.is("None") {
            return Err(t.error(NONE_TYPE));
        }
        Err(t.error("expected a type name or ()"))
    }
    fn type_arguments(&mut self) -> Result<Vec<TypeRef>> {
        if !self.eat("[") {
            return Ok(Vec::new());
        }
        self.exact_list(
            |p| &mut p.scratch_type_args,
            |p| loop {
                let ty = p.ty()?;
                p.scratch_type_args.push(ty);
                if p.eat("]") {
                    return Ok(());
                }
                p.expect(",")?;
            },
        )
    }
    /// Reparses brackets already accepted as type arguments as `base[index]`.
    /// `start` is the opening bracket and the parser sits just past the closing
    /// one, where it stays. Returns None when the contents are not one expression.
    fn index_reading(&mut self, start: usize, base: &Expr) -> Option<Expr> {
        let end = std::mem::replace(&mut self.pos, start + 1);
        let deepest = self.deepest;
        let index = self
            .expr(0)
            .ok()
            .filter(|_| self.eat("]") && self.pos == end);
        self.pos = end;
        if index.is_none() {
            self.deepest = deepest;
        }
        Some(Expr {
            at: base.at.clone(),
            kind: Expression::Index(Box::new(base.clone()), Box::new(index?)),
        })
    }
    /// The rest of `[start:stop:step]` after `start`, from its first colon
    /// through the closing bracket.
    fn slice_bounds(&mut self, start: Option<Expr>) -> Result<SliceBounds> {
        self.expect(":")?;
        self.slices = true;
        let bound = |p: &mut Self| -> Result<Option<Expr>> {
            if p.peek().is(":") || p.peek().is("]") || p.peek().is(",") {
                Ok(None)
            } else {
                p.expr(0).map(Some)
            }
        };
        let stop = bound(self)?;
        let step = if self.eat(":") { bound(self)? } else { None };
        if self.peek().is(",") {
            return Err(self.peek().error("a slice cannot have several dimensions"));
        }
        self.expect("]")?;
        Ok(SliceBounds { start, stop, step })
    }
    pub(super) fn type_parameters(&mut self) -> Result<Vec<(String, Option<TypeRef>)>> {
        if !self.eat("[") {
            return Ok(Vec::new());
        }
        self.exact_list(
            |p| &mut p.scratch_type_params,
            |p| {
                let base = p.scratch_type_params.len();
                loop {
                    let at = p.peek().clone();
                    let name = p.name()?;
                    if builtin(&name)
                        || p.scratch_type_params[base..]
                            .iter()
                            .any(|(n, _)| n == &name)
                    {
                        return Err(at.error("duplicate or builtin type parameter"));
                    }
                    let bound = if p.eat(":") { Some(p.ty()?) } else { None };
                    p.scratch_type_params.push((name, bound));
                    if p.eat("]") {
                        return Ok(());
                    }
                    p.expect(",")?;
                }
            },
        )
    }
    pub(super) fn alias(&mut self) -> Result<TypeAlias> {
        let at = self.take(); // type
        let name = self.name()?;
        self.expect("=")?;
        let target = self.ty()?;
        self.kind(Kind::Newline, "the end of the type alias declaration")?;
        Ok(TypeAlias { at, name, target })
    }
    pub(super) fn function(&mut self) -> Result<Function> {
        self.function_in(None)
    }
    pub(super) fn function_in(&mut self, class: Option<&str>) -> Result<Function> {
        self.function_header(class, false, false, false)
    }
    pub(super) fn function_header(
        &mut self,
        class: Option<&str>,
        foreign: bool,
        exported: bool,
        anonymous: bool,
    ) -> Result<Function> {
        let at = self.take(); // def
        let once = anonymous && self.eat("once");
        let name = if anonymous {
            "__plenty_anonymous".into()
        } else {
            self.name()?
        };
        if class.is_none() && builtin(&name) {
            return Err(at.error("cannot redefine a builtin"));
        }
        let mut captures = Vec::new();
        if anonymous && self.eat("[") {
            while !self.eat("]") {
                let borrowed = self.eat("&");
                let mutable = self.eat("mut");
                let name_at = self.peek().clone();
                let name = self.name()?;
                if captures.iter().any(|c: &Capture| c.name == name) {
                    return Err(name_at.error("duplicate closure capture"));
                }
                captures.push(Capture {
                    name,
                    mutable,
                    borrowed,
                });
                if self.eat("]") {
                    break;
                }
                self.expect(",")?;
            }
        }
        let type_params = if anonymous {
            Vec::new()
        } else {
            if class.is_some()
                && matches!(name.as_str(), "__init__" | "__del__")
                && self.peek().is("[")
            {
                return Err(at.error("lifecycle methods cannot declare their own type parameters"));
            }
            self.type_parameters()?
        };
        self.expect("(")?;
        // Empty unless a parameter has a default; then aligned with inputs.
        let mut defaults = Vec::new();
        let mut adapters = Vec::new();
        let inputs = self.exact_list(
            |parser| &mut parser.scratch_inputs,
            |parser| {
                parser.parameters(
                    &at,
                    &name,
                    class,
                    &captures,
                    (foreign, exported, anonymous),
                    &mut defaults,
                    &mut adapters,
                )
            },
        )?;
        if inputs.len() > 256 {
            return Err(at.error("at most 256 parameter/local slots are supported"));
        }
        defaults::validate_dependencies(&inputs, &defaults)?;
        self.expect("->")?;
        let output = self.ty()?;
        let mut address_parameter = None;
        let symbol = if foreign || exported {
            if !type_params.is_empty() {
                return Err(at.error(if exported {
                    "C exports cannot be generic"
                } else {
                    "C imports cannot be generic"
                }));
            }
            self.expect("=")?;
            let symbol = self.take();
            if let Kind::Text(ref symbol_name) = symbol.kind {
                if !(foreign && foreign::loader_symbol(symbol_name)) {
                    foreign::check_symbol(symbol_name, &symbol)?;
                }
                Some(symbol_name.to_string())
            } else if foreign {
                address_parameter = Some(
                    inputs
                        .iter()
                        .position(|(name, _)| symbol.is(name))
                        .ok_or_else(|| {
                            symbol
                                .error("expected a C symbol string or a function-address parameter")
                        })?,
                );
                None
            } else {
                return Err(symbol.error("expected a C symbol string"));
            }
        } else {
            None
        };
        if foreign {
            self.kind(Kind::Newline, "the end of the C declaration")?;
            return Ok(Function {
                once,
                captures,
                foreign: Some(crate::foreign::Declaration {
                    target: match address_parameter {
                        Some(index) => crate::foreign::Target::Parameter(index),
                        None => crate::foreign::Target::Symbol(symbol.unwrap()),
                    },
                    arguments: adapters,
                }),
                export: None,
                type_params,
                name,
                at,
                inputs,
                defaults,
                output,
                doc: String::new(),
                doc_at: None,
                body: vec![],
            });
        }
        if anonymous && self.in_brackets[self.pos] {
            let body = self.inline_body()?;
            return Ok(Function {
                once,
                captures,
                foreign: None,
                export: None,
                type_params,
                name,
                at,
                inputs,
                defaults,
                output,
                doc: String::new(),
                doc_at: None,
                body,
            });
        }
        if anonymous
            && self.peek().is(":")
            && self
                .token(self.pos + 1)
                .is_some_and(|t| t.kind != Kind::Newline)
        {
            return Err(self.tokens[self.pos + 1].error(
                "outside brackets an anonymous function needs an indented body on the next line; the one-expression body is only for brackets",
            ));
        }
        let mut body = self.suite()?;
        let (doc, doc_at) = if matches!(
            body.first(),
            Some(Stmt {
                kind: Statement::Expr(Expr {
                    kind: Expression::Text(_),
                    ..
                }),
                ..
            })
        ) {
            let Stmt {
                kind:
                    Statement::Expr(Expr {
                        kind: Expression::Text(s),
                        at,
                    }),
                ..
            } = body.remove(0)
            else {
                unreachable!()
            };
            (s, Some(at))
        } else {
            (String::new(), None)
        };
        Ok(Function {
            once,
            captures,
            foreign: None,
            export: symbol,
            type_params,
            name,
            at,
            inputs,
            defaults,
            output,
            doc,
            doc_at,
            body,
        })
    }
    /// Pushes a function's parameters up to the closing `)` onto
    /// `scratch_inputs`. `at` and `name` are the function's; `flags` are
    /// whether it is foreign, exported and anonymous.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn parameters(
        &mut self,
        at: &Token,
        name: &str,
        class: Option<&str>,
        captures: &[Capture],
        (foreign, exported, anonymous): (bool, bool, bool),
        defaults: &mut Vec<Option<Expr>>,
        adapters: &mut Vec<crate::foreign::Argument>,
    ) -> Result<()> {
        let base = self.scratch_inputs.len();
        while !self.eat(")") {
            let param_at = self.peek().clone();
            if param_at.is("*") {
                return Err(param_at.error("variadic parameters (`*args`, `**kwargs`) are not supported; take a list parameter, such as `values: &list[i64]`"));
            }
            let param = self.name()?;
            if captures.iter().any(|c| c.name == param) {
                return Err(param_at.error("a parameter cannot shadow a closure capture"));
            }
            let inputs = &self.scratch_inputs[base..];
            if inputs.iter().any(|(n, _)| n == &param) {
                return Err(param_at.error("duplicate parameter"));
            }
            let first = inputs.is_empty();
            let ty = if let Some(class) =
                class.filter(|_| param == "self" && first && !self.peek().is(":"))
            {
                TypeRef {
                    concrete: None,
                    parameter: false,
                    // Preserve the shorthand's provenance for receiver diagnostics.
                    at: param_at.clone(),
                    name: Some(
                        if matches!(name, "__init__" | "__del__") {
                            "&mut"
                        } else {
                            "&"
                        }
                        .into(),
                    ),
                    args: vec![TypeRef {
                        concrete: None,
                        parameter: false,
                        at: at.clone(),
                        name: Some(class.into()),
                        args: vec![],
                    }],
                }
            } else {
                self.expect(":")?;
                self.ty()?
            };
            let adapter = if foreign && self.eat("as") {
                let token = self.take();
                if token.is("utf8") {
                    crate::foreign::Argument::Utf8
                } else if token.is("c_string") {
                    crate::foreign::Argument::CString
                } else if token.is("strided") {
                    crate::foreign::Argument::Strided
                } else {
                    return Err(
                        token.error("expected utf8, c_string or strided C argument adapter")
                    );
                }
            } else {
                crate::foreign::Argument::Direct
            };
            if foreign {
                adapters.push(adapter);
            }
            if self.eat("=") {
                if anonymous {
                    return Err(
                        param_at.error("anonymous functions cannot declare parameter defaults")
                    );
                }
                if foreign || exported {
                    return Err(
                        param_at.error("C imports and exports cannot declare parameter defaults")
                    );
                }
                if param == "self" && class.is_some() {
                    return Err(param_at.error("self cannot have a default value"));
                }
                let default = self.expr(0)?;
                defaults.resize_with(self.scratch_inputs.len() - base, || None);
                defaults.push(Some(default));
            } else if !defaults.is_empty() {
                return Err(
                    param_at.error("required parameters must precede parameters with defaults")
                );
            }
            self.scratch_inputs.push((param, ty));
            if self.eat(")") {
                break;
            }
            self.expect(",")?;
        }
        Ok(())
    }
    /// The body of an anonymous function written inside brackets: one
    /// expression, parsed as the tail expression of an indented body so both
    /// forms share every later rule. Brackets suspend layout, so statements
    /// cannot be delimited here.
    pub(super) fn inline_body(&mut self) -> Result<Vec<Stmt>> {
        const BIND_FIRST: &str = "inside brackets an anonymous function body is one expression; bind a function with statements to a name first";
        self.expect(":")?;
        let at = self.peek().clone();
        if at.is("return") {
            return Err(at.error(
                "inside brackets an anonymous function body is one expression, and its value is returned; remove `return`",
            ));
        }
        let keyword = [
            "mut", "pass", "if", "while", "for", "with", "match", "yield", "break", "continue",
        ]
        .iter()
        .any(|keyword| at.is(keyword));
        let binding = matches!(at.kind, Kind::Word(_))
            && self
                .token(self.pos + 1)
                .is_some_and(|t| t.is("=") || t.is(":"));
        if keyword || binding {
            return Err(at.error(BIND_FIRST));
        }
        let value = self.expr(0)?;
        let after = self.peek();
        // A complete expression inside brackets is followed by one of these.
        // Anything else on a later line starts a second statement; on the
        // same line the enclosing construct reports what it expected.
        let ends = [",", ")", "]", "}", ":", "for"]
            .iter()
            .any(|end| after.is(end));
        if after.is("=") || !ends && after.line > self.tokens[self.pos - 1].line {
            return Err(after.error(BIND_FIRST));
        }
        Ok(vec![Stmt {
            at,
            kind: Statement::Expr(value),
        }])
    }
    pub(super) fn suite(&mut self) -> Result<Vec<Stmt>> {
        self.nested(Self::suite_inner)
    }
    fn suite_inner(&mut self) -> Result<Vec<Stmt>> {
        self.expect(":")?;
        self.kind(Kind::Newline, "a newline after `:`")?;
        self.kind(Kind::Indent, "an indented block")?;
        let body = self.exact_list(
            |parser| &mut parser.scratch_stmts,
            |parser| {
                while !matches!(parser.peek().kind, Kind::Dedent | Kind::Eof) {
                    if parser.at_const_if() {
                        let at = parser.peek().clone();
                        let selected = parser.const_statements()?;
                        if selected.is_empty() {
                            // No arm holds: the chain stands for nothing,
                            // but the suite must not become empty.
                            parser.scratch_stmts.push(Stmt {
                                at,
                                kind: Statement::Pass,
                            });
                        }
                        parser.scratch_stmts.extend(selected);
                        continue;
                    }
                    let statement = parser.statement()?;
                    parser.scratch_stmts.push(statement);
                }
                Ok(())
            },
        )?;
        self.kind(Kind::Dedent, "the end of an indented block")?;
        if body.is_empty() {
            return Err(self.peek().error("empty block; use `pass`"));
        }
        Ok(body)
    }
    fn conditional_statement(&mut self) -> Result<Stmt> {
        let at = self.take(); // if
        let condition = self.expr(0)?;
        let yes = self.suite()?;
        let mut elifs = Vec::new();
        while self.peek().is("elif") {
            let at = self.take();
            let condition = self.expr(0)?;
            let body = self.suite()?;
            elifs.push(Elif {
                at,
                condition,
                body,
            });
        }
        elifs.shrink_to_fit();
        let no = if self.eat("else") {
            self.suite()?
        } else {
            Vec::new()
        };
        Ok(Stmt {
            at,
            kind: Statement::If {
                condition,
                yes,
                elifs,
                no,
            },
        })
    }
    /// A Python exception statement at the start of a statement. Only `try`
    /// is reserved, so the others are recognized only where no valid Plenty
    /// statement could begin with that name: before a block colon ending the
    /// line, or (`except` and `raise`) before a name, as in `raise Error(...)`.
    fn exception_statement(&self) -> Option<&'static str> {
        let Kind::Word(word) = &self.peek().kind else {
            return None;
        };
        let next = self.token(self.pos + 1)?;
        let block = next.is(":")
            && self
                .token(self.pos + 2)
                .is_some_and(|t| t.kind == Kind::Newline);
        match &**word {
            "try" => Some("try"),
            "finally" if block => Some("finally"),
            "except" if block || matches!(next.kind, Kind::Word(_)) => Some("except"),
            "raise" if matches!(next.kind, Kind::Word(_)) => Some("raise"),
            _ => None,
        }
    }
    /// A Python statement keyword that Plenty does not reserve, recognized
    /// only before another word on the same line, where no valid Plenty
    /// statement could follow a name: `assert x`, `del xs[0]`, `global x`.
    fn python_statement(&self) -> Option<&'static str> {
        let Kind::Word(word) = &self.peek().kind else {
            return None;
        };
        let next = self.token(self.pos + 1)?;
        if !matches!(next.kind, Kind::Word(_)) || next.line != self.peek().line {
            return None;
        }
        Some(match &**word {
            "assert" => {
                "Plenty has no run-time `assert` statement; return an error with `if not condition: return Err(...)`, or use `std.test.check(condition, message)?` in tests; a module-scope `assert` checks constants at compile time"
            }
            "del" => {
                "`del` is not supported; remove a list element with `items.pop(index)` or a dictionary entry with `table.pop(key)`, and handle the returned Option"
            }
            "global" | "nonlocal" => {
                "Plenty has no global variables; pass the value as a parameter, or declare a `const` at module scope"
            }
            _ => return None,
        })
    }
    pub(super) fn statement(&mut self) -> Result<Stmt> {
        self.misplaced_annotations("statements")?;
        if let Some(keyword) = self.exception_statement() {
            return Err(self.peek().error(format!(
                "`{keyword}` is not supported; Plenty has no exceptions: return `Result[T, E]`, propagate an error with `?`, and handle it with `match`"
            )));
        }
        if let Some(message) = self.python_statement() {
            return Err(self.peek().error(message));
        }
        if self.peek().is("with") {
            let at = self.take();
            let mut managers = Vec::new();
            loop {
                let manager = self.expr(0)?;
                let name = if self.eat("as") {
                    Some(self.name()?)
                } else {
                    None
                };
                managers.push((manager, name));
                if !self.eat(",") {
                    break;
                }
            }
            // Each further manager is a `with` nested in the previous one.
            let inner = managers.len() - 1;
            self.depth += inner;
            let body = self.suite();
            self.depth -= inner;
            let mut body = body?;
            for (manager, name) in managers.into_iter().rev() {
                body = vec![Stmt {
                    at: at.clone(),
                    kind: Statement::With {
                        manager,
                        name,
                        body,
                    },
                }];
            }
            return Ok(body.pop().unwrap());
        }
        if self.peek().is("enum") {
            return Err(self.peek().error("enums must be declared at module scope"));
        }
        if self.peek().is("match") {
            return self.match_statement();
        }
        if self.checked_block() {
            let value = self.statement_value()?;
            return Ok(Stmt {
                at: value.at.clone(),
                kind: Statement::Expr(value),
            });
        }
        if self.peek().is("type") {
            return Err(self
                .peek()
                .error("type aliases must be declared at module scope"));
        }
        if self.peek().is("if") {
            return self.conditional_statement();
        }
        if self.peek().is("while") {
            let at = self.take();
            let condition = self.expr(0)?;
            let body = self.suite()?;
            if self.peek().is("else") {
                return Err(self.peek().error("loop else is not supported"));
            }
            return Ok(Stmt {
                at,
                kind: Statement::While { condition, body },
            });
        }
        if self.peek().is("for") {
            let at = self.take();
            let (names, mut prefix) = self.loop_bindings(&at)?;
            self.expect("in")?;
            let iterable = self.expr(0)?;
            let mut body = self.suite()?;
            prefix.append(&mut body);
            let body = prefix;
            if self.peek().is("else") {
                return Err(self.peek().error("loop else is not supported"));
            }
            return Ok(Stmt {
                at,
                kind: Statement::For {
                    name: names,
                    iterable,
                    body,
                },
            });
        }
        let at = self.peek().clone();
        let kind = if self.eat("return") {
            Statement::Return(if self.peek().kind == Kind::Newline {
                None
            } else {
                Some(self.statement_value()?)
            })
        } else if self.eat("yield") {
            Statement::Yield(self.expr(0)?)
        } else if self.eat("break") {
            Statement::Break
        } else if self.eat("continue") {
            Statement::Continue
        } else if self.eat("pass") {
            Statement::Pass
        } else {
            let mutable = self.eat("mut");
            if mutable
                && (self.peek().is("(") || self.token(self.pos + 1).is_some_and(|t| t.is(",")))
            {
                let mut first = self.expr(0)?;
                while matches!(first.kind, Expression::Group(_)) {
                    let Expression::Group(inner) = first.kind else {
                        unreachable!()
                    };
                    first = *inner;
                }
                if let Expression::Name(name) = &first.kind
                    && !self.peek().is(",")
                {
                    self.expect("=")?;
                    let value = self.statement_value()?;
                    if !matches!(self.tokens[self.pos - 1].kind, Kind::Dedent) {
                        self.kind(Kind::Newline, "the end of the statement")?;
                    }
                    return Ok(Stmt {
                        at,
                        kind: Statement::Assign {
                            name: name.clone(),
                            mutable,
                            annotation: None,
                            value,
                        },
                    });
                }
                let mut targets = if self.peek().is(",") {
                    vec![first]
                } else {
                    match first.kind {
                        Expression::Tuple(targets) => targets,
                        _ => vec![first],
                    }
                };
                while self.eat(",") {
                    targets.push(self.expr(0)?);
                }
                let names = Self::unpacking_names(&targets)?;
                let targets = (!targets
                    .iter()
                    .all(|target| matches!(&target.kind, Expression::Name(_))))
                .then_some(targets);
                self.expect("=")?;
                let value = self.unpacked_value()?;
                self.kind(Kind::Newline, "the end of the statement")?;
                return Ok(Stmt {
                    at,
                    kind: Statement::Unpack {
                        names,
                        targets,
                        fresh: false,
                        mutable,
                        value,
                    },
                });
            }
            let assignment = mutable
                || matches!(&self.peek().kind, Kind::Word(_))
                    && self
                        .token(self.pos + 1)
                        .is_some_and(|t| t.is("=") || t.is(":"));
            if assignment {
                let name = self.name()?;
                if self.eat(",") {
                    let mut names = vec![name];
                    loop {
                        names.push(self.name()?);
                        if !self.eat(",") {
                            break;
                        }
                    }
                    self.expect("=")?;
                    let value = self.unpacked_value()?;
                    self.kind(Kind::Newline, "the end of the statement")?;
                    return Ok(Stmt {
                        at,
                        kind: Statement::Unpack {
                            names,
                            targets: None,
                            fresh: false,
                            mutable,
                            value,
                        },
                    });
                }
                let annotation = if self.eat(":") {
                    Some(self.ty()?)
                } else {
                    None
                };
                self.expect("=")?;
                Statement::Assign {
                    name,
                    mutable,
                    annotation,
                    value: self.statement_value()?,
                }
            } else {
                let target = self.expr(0)?;
                if self.peek().is(",") {
                    let mut targets = vec![target];
                    while self.eat(",") {
                        targets.push(self.expr(0)?);
                    }
                    if let Some(slice) = targets
                        .iter()
                        .find(|t| matches!(&ungroup(t).kind, Expression::Slice(..)))
                    {
                        return Err(slice.at.error(SLICE_ASSIGNMENT));
                    }
                    self.expect("=")?;
                    let value = self.unpacked_value()?;
                    let names = targets
                        .iter()
                        .map(|target| match &target.kind {
                            Expression::Name(name) => Some(name.clone()),
                            _ => None,
                        })
                        .collect::<Option<Vec<_>>>();
                    match names {
                        Some(names) => Statement::Unpack {
                            names,
                            targets: None,
                            fresh: false,
                            mutable: false,
                            value,
                        },
                        // A tuple is never a place, so a tuple target marks an
                        // assignment to several places.
                        None => Statement::SetIndex {
                            target: Expr {
                                at: at.clone(),
                                kind: Expression::Tuple(targets),
                            },
                            value,
                        },
                    }
                } else if self.eat("=") {
                    if matches!(&ungroup(&target).kind, Expression::Slice(..)) {
                        return Err(target.at.error(SLICE_ASSIGNMENT));
                    }
                    let value = self.statement_value()?;
                    if let Expression::Name(name) = &ungroup(&target).kind {
                        Statement::Assign {
                            name: name.clone(),
                            mutable: false,
                            annotation: None,
                            value,
                        }
                    } else {
                        Statement::SetIndex { target, value }
                    }
                } else if matches!(&self.peek().kind, Kind::Symbol(s)
                    if matches!(&**s, "+=" | "-=" | "*=" | "/=" | "//=" | "%=" | "&=" | "|=" | "^=" | "<<=" | ">>="))
                {
                    if matches!(&ungroup(&target).kind, Expression::Slice(..)) {
                        return Err(target.at.error(SLICE_ASSIGNMENT));
                    }
                    let operator = self.take();
                    let Kind::Symbol(symbol) = operator.kind else {
                        unreachable!()
                    };
                    Statement::Augment {
                        target,
                        op: symbol.trim_end_matches('=').into(),
                        value: self.expr(0)?,
                    }
                } else {
                    Statement::Expr(target)
                }
            }
        };
        if !matches!(self.tokens[self.pos - 1].kind, Kind::Dedent) {
            self.kind(Kind::Newline, "the end of the statement")?;
        }
        Ok(Stmt { at, kind })
    }
    /// The value that ends a binding, an assignment, or a `return`: an
    /// expression, or a value-producing `if`/`match`/`checked` block.
    /// `checked:` followed by a newline starts a `checked:` block; `checked`
    /// is otherwise an ordinary name, so `checked: i64 = 0` still declares
    /// one.
    fn checked_block(&self) -> bool {
        self.peek().is("checked")
            && self.tokens.get(self.pos + 1).is_some_and(|t| t.is(":"))
            && self
                .tokens
                .get(self.pos + 2)
                .is_some_and(|t| t.kind == Kind::Newline)
    }
    fn statement_value(&mut self) -> Result<Expr> {
        if self.peek().is("if") {
            if let Some(at) = self.token(self.pos + 1).filter(|at| at.is("const")) {
                return Err(at.error("`if const` selects statements at compile time and cannot be used as a block value; put the binding or return in its selected arm"));
            }
            let statement = self.conditional_statement()?;
            return Ok(Expr {
                at: statement.at.clone(),
                kind: Expression::If(vec![statement]),
            });
        }
        if self.checked_block() {
            let at = self.take();
            let body = self.suite()?;
            return Ok(Expr {
                at,
                kind: Expression::Checked(body),
            });
        }
        if !self.peek().is("match") {
            return self.expr(0);
        }

        let (at, value, cases) = self.match_block()?;
        Ok(Expr {
            at,
            kind: Expression::Match(Box::new(value), cases),
        })
    }
    pub(super) fn loop_bindings(&mut self, at: &Token) -> Result<(Vec<String>, Vec<Stmt>)> {
        let mut targets = self.loop_targets()?;
        if targets.len() == 1
            && matches!(&targets[0].kind, Expression::Tuple(fields) if fields.len() > 1)
        {
            let Expression::Tuple(fields) = targets.pop().unwrap().kind else {
                unreachable!()
            };
            targets = fields;
        }
        Self::unpacking_names(&targets)?;
        let mut names = Vec::new();
        let mut prefix = Vec::new();
        for (position, target) in targets.into_iter().enumerate() {
            if let Expression::Name(name) = target.kind {
                names.push(name);
            } else {
                let name = format!("<loop {} {} {position}>", at.line, at.column);
                let Expression::Tuple(targets) = target.kind else {
                    unreachable!()
                };
                prefix.push(Stmt {
                    at: target.at.clone(),
                    kind: Statement::Unpack {
                        names: Self::unpacking_names(&targets)?,
                        targets: Some(targets),
                        fresh: true,
                        mutable: false,
                        value: Expr {
                            at: target.at,
                            kind: Expression::Name(name.clone()),
                        },
                    },
                });
                names.push(name);
            }
        }
        Ok((names, prefix))
    }
    pub(super) fn unpacking_names(targets: &[Expr]) -> Result<Vec<String>> {
        fn walk(target: &Expr, names: &mut Vec<String>, seen: &mut HashSet<String>) -> Result<()> {
            match &ungroup(target).kind {
                Expression::Tuple(targets) => {
                    for target in targets {
                        walk(target, names, seen)?;
                    }
                }
                Expression::Name(name) => {
                    if name != "_" && !seen.insert(name.clone()) {
                        return Err(target.at.error("duplicate unpacking binding"));
                    }
                    names.push(name.clone());
                }
                _ => {
                    return Err(target
                        .at
                        .error("unpacking declarations and loop targets require names or tuples"));
                }
            }
            Ok(())
        }
        let mut names = Vec::new();
        let mut seen = HashSet::default();
        for target in targets {
            walk(target, &mut names, &mut seen)?;
        }
        Ok(names)
    }
    fn loop_targets(&mut self) -> Result<Vec<Expr>> {
        fn target(parser: &mut Parser) -> Result<Expr> {
            parser.nested(|parser| {
                let at = parser.peek().clone();
                let kind = if parser.eat("(") {
                    let mut targets = parser.loop_targets()?;
                    let singleton = parser.token(parser.pos - 1).is_some_and(|t| t.is(","));
                    parser.expect(")")?;
                    if targets.len() == 1 && !singleton {
                        targets.remove(0).kind
                    } else {
                        Expression::Tuple(targets)
                    }
                } else {
                    Expression::Name(parser.name()?)
                };
                Ok(Expr { at, kind })
            })
        }
        let mut targets = vec![target(self)?];
        while self.eat(",") {
            if self.peek().is(")") {
                break;
            }
            targets.push(target(self)?);
        }
        Ok(targets)
    }
    /// The right side of an assignment to several targets: one expression, or
    /// a tuple written without its parentheses.
    fn unpacked_value(&mut self) -> Result<Expr> {
        let first = self.expr(0)?;
        if !self.peek().is(",") {
            return Ok(first);
        }
        let at = first.at.clone();
        let mut values = vec![first];
        while self.eat(",") {
            values.push(self.expr(0)?);
        }
        Ok(Expr {
            at,
            kind: Expression::Tuple(values),
        })
    }
    pub(super) fn parenthesized(&mut self) -> Result<Expression> {
        if self.eat(")") {
            return Ok(Expression::Unit);
        }
        let first = self.expr(0)?;
        if self.peek().is("for") {
            let generator = self.generator_display(first)?;
            self.expect(")")?;
            return Ok(generator);
        }
        if self.eat(")") {
            return Ok(Expression::Group(Box::new(first)));
        }
        self.expect(",")?;
        let mut values = vec![first];
        while !self.eat(")") {
            values.push(self.expr(0)?);
            if self.eat(")") {
                break;
            }
            self.expect(",")?;
        }
        Ok(Expression::Tuple(values))
    }
    pub(super) fn expr(&mut self, min: u8) -> Result<Expr> {
        self.nested(|parser| parser.expr_inner(min))
    }
    fn expr_inner(&mut self, min: u8) -> Result<Expr> {
        if self.peek().is("def") {
            if self
                .token(self.pos + 1)
                .is_some_and(|t| !t.is("(") && !t.is("[") && !t.is("once"))
            {
                return Err(self.peek().error("nested named functions are not supported; assign an anonymous def expression instead"));
            }
            if min != 0 {
                return Err(self.peek().error(
                    "an anonymous function must be a complete expression; wrap it in parentheses to use it as an operand",
                ));
            }
            let at = self.peek().clone();
            let function = self.function_header(None, false, false, true)?;
            return Ok(Expr {
                at,
                kind: Expression::Anonymous(Box::new(function)),
            });
        }
        let at = self.take();
        let kind = match &at.kind {
            Kind::Number(n) => Expression::Number(String::from(&**n)),
            Kind::Text(s) => Expression::Text(String::from(&**s)),
            Kind::Format(parts) => self.format_literal(parts)?,
            Kind::Bytes(b) => Expression::Bytes(b.to_vec()),
            Kind::Word(s) if matches!(&**s, "True" | "False") => Expression::Bool(&**s == "True"),
            Kind::Word(s) if &**s == "not" => {
                Expression::Unary(s.to_string(), Box::new(self.expr(3)?))
            }
            Kind::Word(s) if &**s == "lambda" => {
                return Err(at.error(if self.in_brackets[self.pos - 1] {
                    "`lambda` is not supported; write an anonymous function as `def(name: T) -> U: expression`"
                } else {
                    "`lambda` is not supported; write an anonymous function as `def(name: T) -> U:` with an indented body"
                }));
            }
            Kind::Word(s) if &**s == "try" => {
                return Err(at.error("collection literals already return Result; remove `try` and use postfix `?` to propagate errors"));
            }
            Kind::Symbol(s) if &**s == "&" => {
                let op = if self.eat("mut") { "&mut" } else { "&" };
                let operand = self.expr(10)?;
                if matches!(&ungroup(&operand).kind, Expression::Try(inner) if matches!(ungroup(inner).kind, Expression::Slice(..)))
                {
                    return Err(at.error(VIEW_TRY));
                }
                Expression::Unary(op.into(), Box::new(operand))
            }
            Kind::Symbol(s) if matches!(&**s, "-" | "+" | "*" | "~") => {
                Expression::Unary(s.to_string(), Box::new(self.expr(10)?))
            }
            Kind::Symbol(s) if &**s == "(" => self.parenthesized()?,
            Kind::Symbol(s) if matches!(&**s, "[" | "{") => self.collection_display(s)?,
            Kind::Word(s) if !reserved(s) && !s.starts_with("__plenty_") => {
                if matches!(
                    &**s,
                    "Option"
                        | "Result"
                        | "SpawnError"
                        | "SendError"
                        | "AppendError"
                        | "SendTimeoutError"
                        | "Selected"
                        | "SubmitError"
                        | "ParallelError"
                        | "Sender"
                        | "Receiver"
                        | "Future"
                ) && self.peek().is("[")
                {
                    self.pos -= 1;
                    Expression::Type(Box::new(self.ty()?))
                } else if matches!(
                    &**s,
                    "list" | "dict" | "set" | "range" | "channel" | "clist"
                ) && self.peek().is("[")
                {
                    self.pos -= 1;
                    let ty = self.ty()?;
                    if self.peek().is(".") {
                        Expression::Type(Box::new(ty))
                    } else {
                        self.expect("(")?;
                        Expression::Constructor(Box::new(ty), self.arguments()?)
                    }
                } else if self.eat("(") {
                    Expression::Call(String::from(&**s), self.arguments()?)
                } else {
                    Expression::Name(String::from(&**s))
                }
            }
            _ if at.is("match") => return Err(at.error(MATCH_VALUE_POSITION)),
            _ if at.is("if") => return Err(at.error(IF_VALUE_POSITION)),
            _ if at.is("None") => return Err(at.error(NONE_VALUE)),
            Kind::Word(s) if unused_keyword(s).is_some() => {
                return Err(at.error(unused_keyword(s).unwrap_or_default()));
            }
            _ => return Err(at.error("expected an expression")),
        };
        let mut left = Expr { at, kind };
        loop {
            if self.eat("(") {
                self.deepen()?;
                let args = self.arguments()?;
                left = Expr {
                    at: left.at.clone(),
                    kind: Expression::Invoke(Box::new(left), args),
                };
                continue;
            }
            if self.peek().is("[") {
                let saved = self.pos;
                if let Expression::Member(base, name, pos) = &left.kind {
                    if let Ok(types) = self.type_arguments()
                        && self.peek().is("(")
                    {
                        // The index reading holds `left` as its base.
                        self.deepen()?;
                        let index = self.index_reading(saved, &left);
                        self.take();
                        let args = self.arguments()?;
                        let generic = Expression::GenericMethod(
                            base.clone(),
                            name.as_str().into(),
                            types,
                            args,
                            *pos,
                        );
                        left.kind = bracketed(&left.at, generic, index);
                        continue;
                    }
                    self.pos = saved;
                }
                fn path(e: &Expr) -> Option<String> {
                    match &e.kind {
                        Expression::Name(n) => Some(n.clone()),
                        Expression::Member(base, n, _) => Some(format!("{}.{n}", path(base)?)),
                        _ => None,
                    }
                }
                if let Some(name) = path(&left) {
                    if let Ok(types) = self.type_arguments() {
                        self.deepen()?;
                        let index = self.index_reading(saved, &left);
                        let generic = if self.eat("(") {
                            Expression::GenericCall(name, types, self.arguments()?)
                        } else {
                            Expression::GenericValue(name, types)
                        };
                        left.kind = bracketed(&left.at, generic, index);
                        continue;
                    }
                    self.pos = saved;
                }
            }
            if self.peek().is("?") {
                let at = self.take();
                self.deepen()?;
                left = Expr {
                    at,
                    kind: Expression::Try(Box::new(left)),
                };
                continue;
            }
            if self.eat("[") {
                self.deepen()?;
                let start = if self.peek().is(":") {
                    None
                } else {
                    Some(self.expr(0)?)
                };
                if self.peek().is(":") {
                    let bounds = self.slice_bounds(start)?;
                    left = Expr {
                        at: left.at.clone(),
                        kind: Expression::Slice(Box::new(left), Box::new(bounds)),
                    };
                    continue;
                }
                let index = start.expect("an index");
                self.expect("]")?;
                left = Expr {
                    at: left.at.clone(),
                    kind: Expression::Index(Box::new(left), Box::new(index)),
                };
                continue;
            }
            if self.eat(".") {
                self.deepen()?;
                let pos = Pos::of(self.peek());
                let name = if self.eat("from") {
                    "from".into()
                } else {
                    self.name()?
                };
                let at = left.at.clone();
                left = if self.eat("(") {
                    let args = self.arguments()?;
                    Expr {
                        at,
                        kind: Expression::Method(Box::new(left), name, args, pos),
                    }
                } else {
                    Expr {
                        at,
                        kind: Expression::Member(Box::new(left), name, pos),
                    }
                };
                continue;
            }
            if min == 0 && self.eat("if") {
                self.deepen()?;
                let condition = self.expr(1)?;
                self.expect("else")?;
                let no = self.expr(0)?;
                let at = left.at.clone();
                left = Expr {
                    at,
                    kind: Expression::Conditional {
                        condition: Box::new(condition),
                        yes: Box::new(left),
                        no: Box::new(no),
                    },
                };
                continue;
            }
            let not_in =
                self.peek().is("not") && self.token(self.pos + 1).is_some_and(|t| t.is("in"));
            let token = if not_in {
                Token {
                    kind: Kind::Word("not in".into()),
                    ..self.peek().clone()
                }
            } else {
                self.peek().clone()
            };
            let Some((op, prec)) = binary(&token) else {
                if token.is("is") && left.at.line == token.line {
                    return Err(token.error(IS_OPERATOR));
                }
                break;
            };
            if prec < min {
                break;
            }
            if prec == 3 && matches!(&left.kind, Expression::Binary(op, _, _) if is_comparison(op))
            {
                return Err(self.peek().error(
                    "chained comparisons are not supported; combine comparisons with `and`",
                ));
            }
            self.take();
            if not_in {
                self.take();
            }
            // Adjacent stars are always rejected: reading them as `a * (*b)`
            // compiled the Python power habit to a product whenever `b` was
            // a reference (issue #612). Multiplying by a dereference needs a
            // space, `a * *b`.
            if op == "*"
                && self.peek().is("*")
                && self.peek().line == token.line
                && self.peek().column == token.column + 1
            {
                return Err(token
                    .error("exponentiation with `**` is not supported; use `base.pow(exponent)`"));
            }
            self.deepen()?;
            let right = self.expr(prec + 1)?;
            let at = left.at.clone();
            left = Expr {
                at,
                kind: Expression::Binary(op, Box::new(left), Box::new(right)),
            };
        }
        Ok(left)
    }
}
/// Keeps the index reading beside the generic one when the brackets have both.
fn bracketed(at: &Token, generic: Expression, index: Option<Expr>) -> Expression {
    match index {
        Some(index) => Expression::Ambiguous {
            generic: Box::new(Expr {
                at: at.clone(),
                kind: generic,
            }),
            index: Box::new(index),
        },
        None => generic,
    }
}
const IS_OPERATOR: &str = "Plenty has no `is` operator; test an Option with `.is_none()` or `.is_some()`, or `match` it, and compare values with `==`";
