//! `f"..."` literals. The lexer splits a template into literal text and
//! fields, the parser checks each field's restricted expression and format
//! spec, and lowering calls a generated generic function that writes the
//! pieces to a `std.fmt` string formatter. Specializing that function gives
//! every field a concrete type, so each spec is checked against its type at
//! compile time and nothing interprets a template at run time.
use super::*;

/// The module binding of the `std.fmt` import that every module using an
/// f-string receives. `__plenty_` names cannot be spelled in source.
pub(super) const FMT_BINDING: &str = "__plenty_fmt";
/// Nesting depth that f-string fields may use through `display`/`debug`.
const DEPTH: &str = "32";
/// f-strings have no byte budget; only allocation failure bounds them.
const NO_BUDGET: &str = "9223372036854775807";
const OUT: &str = "__plenty_out";
/// Widths and precisions are static; this keeps generated spellings bounded.
const SPEC_LIMIT: usize = 4096;

/// A field as lexed: its expression's tokens and what follows them.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LexedField {
    tokens: Vec<Token>,
    /// Where the expression's text ends, for the parser's end-of-field token.
    end: Token,
    /// `{expr=}`: the source text through `=` and any spaces after it.
    debug: Option<String>,
    conversion: Option<char>,
    spec: Option<(String, Token)>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Part {
    Literal(String),
    Field(LexedField),
}

#[derive(Clone, Copy, PartialEq)]
enum Conversion {
    None,
    Repr,
    Str,
}

#[derive(Clone)]
struct Spec {
    text: String,
    fill: Option<char>,
    align: Option<char>,
    sign: Option<char>,
    alternate: bool,
    zero: bool,
    width: usize,
    grouping: Option<char>,
    precision: Option<usize>,
    kind: Option<char>,
}

#[derive(Clone)]
struct Field {
    at: Token,
    conversion: Conversion,
    spec: Option<Spec>,
}

#[derive(Clone)]
enum Piece {
    Literal(String),
    Field(Field),
}

pub(super) struct Template {
    pieces: Vec<Piece>,
}

#[derive(Clone)]
pub(super) enum Format {
    /// The literal: its template and one value per field, in order.
    Literal(Arc<Template>, Vec<Expr>),
    /// Inside a generated function only: write field `index` of the template,
    /// whose value is `value`, to the formatter `__plenty_out`.
    Field(Arc<Template>, usize, Expr),
}

impl Format {
    pub(super) fn operands(&self) -> Vec<&Expr> {
        match self {
            Format::Literal(_, values) => values.iter().collect(),
            Format::Field(_, _, value) => vec![value],
        }
    }
    pub(super) fn operands_mut(&mut self) -> Vec<&mut Expr> {
        match self {
            Format::Literal(_, values) => values.iter_mut().collect(),
            Format::Field(_, _, value) => vec![value],
        }
    }
}

/// The literal-text escapes of ordinary strings.
fn escape(c: char) -> Option<char> {
    Some(match c {
        '0' => '\0',
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        '\\' => '\\',
        '\'' => '\'',
        '"' => '"',
        _ => return None,
    })
}

impl Lexer<'_> {
    fn here(&self) -> Token {
        Token {
            kind: Kind::Eof,
            line: self.line,
            column: self.column,
            source: None,
        }
    }
    fn char_at(&self, offset: usize) -> Option<char> {
        self.source[self.pos..].chars().nth(offset)
    }
    /// Consumes one character, keeping the position current.
    fn advance(&mut self) -> Option<char> {
        let c = self.source[self.pos..].chars().next()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(c)
    }

    /// Lexes `f'...'` or `f"..."`, single or triple quoted, after the `f`.
    pub(super) fn format_string(&mut self, start: &Token) -> Result<Kind> {
        let quote = self.advance().unwrap();
        let triple = self.char_at(0) == Some(quote) && self.char_at(1) == Some(quote);
        if triple {
            self.advance();
            self.advance();
        }
        let closes = |lexer: &Self| {
            lexer.char_at(0) == Some(quote)
                && (!triple || (lexer.char_at(1) == Some(quote) && lexer.char_at(2) == Some(quote)))
        };
        let mut parts = Vec::new();
        let mut text = String::new();
        loop {
            let at = self.here();
            if closes(self) {
                for _ in 0..if triple { 3 } else { 1 } {
                    self.advance();
                }
                break;
            }
            let c = self
                .advance()
                .ok_or_else(|| start.error("unterminated string"))?;
            match c {
                '\n' if !triple => return Err(start.error("newline in a quoted string")),
                '\\' => {
                    let escaped = self
                        .advance()
                        .ok_or_else(|| start.error("unterminated escape"))?;
                    text.push(
                        escape(escaped).ok_or_else(|| {
                            start.error(format!("unsupported escape \\{escaped}"))
                        })?,
                    );
                }
                '{' if self.char_at(0) == Some('{') => {
                    self.advance();
                    text.push('{');
                }
                '}' if self.char_at(0) == Some('}') => {
                    self.advance();
                    text.push('}');
                }
                '}' => {
                    return Err(
                        at.error("single `}` in an f-string; write `}}` for a literal brace")
                    )
                }
                '{' => {
                    if !text.is_empty() {
                        parts.push(Part::Literal(std::mem::take(&mut text)));
                    }
                    parts.push(Part::Field(self.format_field(quote, triple, &at)?));
                }
                _ => text.push(c),
            }
        }
        if !text.is_empty() {
            parts.push(Part::Literal(text));
        }
        Ok(Kind::Format(parts))
    }

    /// Lexes one field after its `{`, through its `}`.
    fn format_field(&mut self, quote: char, triple: bool, open: &Token) -> Result<LexedField> {
        let unterminated = || open.error("unterminated f-string field; expected `}`");
        let begin = self.pos;
        let first = self.here();
        let mut depth = 0usize;
        let mut previous = ' ';
        let terminator = loop {
            let at = self.here();
            let c = self.char_at(0).ok_or_else(unterminated)?;
            let next = self.char_at(1);
            match c {
                '\n' if !triple => return Err(unterminated()),
                '\\' => return Err(at.error("an f-string field cannot contain a backslash")),
                '#' => return Err(at.error("an f-string field cannot contain a comment")),
                '\'' | '"' if c == quote => {
                    return Err(open.error(
                        "unterminated f-string field; expected `}`, and a field cannot reuse the enclosing quote",
                    ))
                }
                '\'' | '"' => {
                    self.advance();
                    loop {
                        match self.advance() {
                            None | Some('\n') => return Err(at.error("unterminated string")),
                            Some('\\') => {
                                self.advance();
                            }
                            Some(close) if close == c => break,
                            Some(_) => {}
                        }
                    }
                    previous = c;
                    continue;
                }
                '(' | '[' | '{' => depth += 1,
                ')' | ']' if depth == 0 => {
                    return Err(at.error(format!("unmatched `{c}` in an f-string field")))
                }
                ')' | ']' => depth -= 1,
                '}' if depth > 0 => depth -= 1,
                '}' => break '}',
                ':' if depth == 0 => break ':',
                '!' if depth == 0 && next != Some('=') => break '!',
                '=' if depth == 0 && next != Some('=') && !matches!(previous, '=' | '!' | '<' | '>') => {
                    break '='
                }
                _ => {}
            }
            previous = c;
            self.advance();
        };
        let expression = &self.source[begin..self.pos];
        if expression.trim().is_empty() {
            return Err(first.error("empty f-string field; write an expression between the braces"));
        }
        let end = self.here();
        let tokens = field_tokens(expression, &first)?;
        let mut field = LexedField {
            tokens,
            end,
            debug: None,
            conversion: None,
            spec: None,
        };
        let mut terminator = terminator;
        if terminator == '=' {
            self.advance();
            while self.char_at(0) == Some(' ') {
                self.advance();
            }
            field.debug = Some(self.source[begin..self.pos].into());
            terminator = self.char_at(0).ok_or_else(unterminated)?;
            if !matches!(terminator, '!' | ':' | '}') {
                return Err(self
                    .here()
                    .error("expected `!`, `:`, or `}` after `=` in an f-string field"));
            }
        }
        if terminator == '!' {
            self.advance();
            let at = self.here();
            field.conversion = match self.advance() {
                Some(c @ ('r' | 's')) => Some(c),
                Some('a') => return Err(at.error("the `!a` conversion is not supported; use `!r`")),
                _ => return Err(at.error("expected the conversion `r` or `s` after `!`")),
            };
            terminator = self.char_at(0).ok_or_else(unterminated)?;
            if !matches!(terminator, ':' | '}') {
                return Err(self
                    .here()
                    .error("expected `:` or `}` after an f-string conversion"));
            }
        }
        if terminator == ':' {
            self.advance();
            let at = self.here();
            let begin = self.pos;
            loop {
                match self.char_at(0) {
                    None => return Err(unterminated()),
                    Some('}') => break,
                    Some('{') => return Err(self.here().error(
                        "nested replacement fields in a format spec are not supported; write the width and precision as numbers",
                    )),
                    Some('\n') => return Err(unterminated()),
                    Some(c) if c == quote => return Err(unterminated()),
                    Some(_) => {
                        self.advance();
                    }
                }
            }
            field.spec = Some((self.source[begin..self.pos].into(), at));
        }
        self.advance(); // }
        Ok(field)
    }
}

/// Lexes a field's expression as if it were parenthesized, so that line breaks
/// inside a triple-quoted template are not statement ends.
fn field_tokens(expression: &str, first: &Token) -> Result<Vec<Token>> {
    let text = format!("({expression})");
    let mut lexer = Lexer::new(&text);
    lexer.line = first.line;
    lexer.column = first.column - 1;
    lexer.line_start = false;
    let mut tokens = VecDeque::new();
    lexer.item(&mut tokens)?;
    let mut tokens = Vec::from(tokens);
    // `(`, then the expression, then `)`, the line end, and the module end.
    tokens.truncate(tokens.len().saturating_sub(3));
    tokens.remove(0);
    Ok(tokens)
}

fn is_align(c: char) -> bool {
    matches!(c, '<' | '>' | '^' | '=')
}

fn parse_spec(text: &str, at: &Token) -> Result<Spec> {
    let invalid = |reason: &str| at.error(format!("invalid format spec `{text}`: {reason}"));
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut spec = Spec {
        text: text.into(),
        fill: None,
        align: None,
        sign: None,
        alternate: false,
        zero: false,
        width: 0,
        grouping: None,
        precision: None,
        kind: None,
    };
    if chars.len() >= 2 && is_align(chars[1]) {
        spec.fill = Some(chars[0]);
        spec.align = Some(chars[1]);
        i = 2;
    } else if chars.first().is_some_and(|c| is_align(*c)) {
        spec.align = Some(chars[0]);
        i = 1;
    }
    if let Some(c @ ('+' | '-' | ' ')) = chars.get(i) {
        spec.sign = Some(*c);
        i += 1;
    }
    if chars.get(i) == Some(&'z') {
        return Err(invalid("the `z` option is not supported"));
    }
    if chars.get(i) == Some(&'#') {
        spec.alternate = true;
        i += 1;
    }
    if chars.get(i) == Some(&'0') {
        spec.zero = true;
        i += 1;
    }
    let number = |i: &mut usize| -> Result<Option<usize>> {
        let start = *i;
        while chars.get(*i).is_some_and(char::is_ascii_digit) {
            *i += 1;
        }
        if start == *i {
            return Ok(None);
        }
        let value: String = chars[start..*i].iter().collect();
        match value.parse::<usize>() {
            Ok(n) if n <= SPEC_LIMIT => Ok(Some(n)),
            _ => Err(invalid(&format!(
                "widths and precisions are limited to {SPEC_LIMIT}"
            ))),
        }
    };
    spec.width = number(&mut i)?.unwrap_or(0);
    if let Some(c @ (',' | '_')) = chars.get(i) {
        spec.grouping = Some(*c);
        i += 1;
    }
    if chars.get(i) == Some(&'.') {
        i += 1;
        spec.precision = Some(number(&mut i)?.ok_or_else(|| invalid("expected digits after `.`"))?);
    }
    match chars.get(i) {
        Some(c) if "sdxXobeEfFgG%".contains(*c) => {
            spec.kind = Some(*c);
            i += 1;
        }
        Some(c @ ('n' | 'c')) => {
            return Err(invalid(&format!("the type `{c}` is not supported")));
        }
        _ => {}
    }
    if i != chars.len() {
        return Err(invalid(&format!(
            "unexpected `{}`; the form is [[fill]align][sign][#][0][width][grouping][.precision][type]",
            chars[i]
        )));
    }
    if spec.zero && spec.align.is_some() {
        spec.fill.get_or_insert('0');
        spec.zero = false;
    }
    Ok(spec)
}

/// The first slice accepts names and literals combined with field access,
/// indexing, and calls; call arguments and indices may also borrow.
fn restricted(e: &Expr, argument: bool) -> bool {
    match &e.kind {
        Expression::Name(_)
        | Expression::Number(_)
        | Expression::Text(_)
        | Expression::Bytes(_)
        | Expression::Bool(_)
        | Expression::Unit
        | Expression::GenericValue(..) => true,
        Expression::Unary(op, inner) if op == "-" => {
            matches!(inner.kind, Expression::Number(_))
        }
        Expression::Unary(op, inner) if op == "&" || op == "&mut" => {
            argument && restricted(inner, false)
        }
        Expression::NamedArgument(_, inner) => argument && restricted(inner, true),
        Expression::Member(base, _, _) => restricted(base, false),
        Expression::Index(base, index) => restricted(base, false) && restricted(index, true),
        Expression::Ambiguous { generic, index } => {
            restricted(generic, false) && restricted(index, false)
        }
        Expression::Call(_, args) | Expression::GenericCall(_, _, args) => {
            args.iter().all(|a| restricted(a, true))
        }
        Expression::Method(base, _, args, _)
        | Expression::GenericMethod(base, _, _, args, _)
        | Expression::Invoke(base, args) => {
            restricted(base, false) && args.iter().all(|a| restricted(a, true))
        }
        _ => false,
    }
}

impl Parser {
    /// Parses each field's tokens and spec into a template.
    pub(super) fn format_literal(&mut self, parts: &[Part]) -> Result<Expression> {
        let mut pieces = Vec::new();
        let mut values = Vec::new();
        let literal = |pieces: &mut Vec<Piece>, text: &str| match pieces.last_mut() {
            Some(Piece::Literal(last)) => last.push_str(text),
            _ => pieces.push(Piece::Literal(text.into())),
        };
        for part in parts {
            let field = match part {
                Part::Literal(text) => {
                    literal(&mut pieces, text);
                    continue;
                }
                Part::Field(field) => field,
            };
            let located = |token: &Token| Token {
                source: self.end.source.clone(),
                ..token.clone()
            };
            let mut parser = Parser::new(self.end.source.clone());
            parser.tokens = field.tokens.iter().map(located).collect();
            parser.end = located(&field.end);
            parser.in_brackets = parser.tokens.iter().map(|_| true).collect();
            parser.complete = true;
            parser.depth = self.depth;
            let value = parser.expr(0)?;
            if parser.peek().kind != Kind::Eof {
                return Err(parser.peek().error("unexpected token in an f-string field"));
            }
            if !restricted(&value, false) {
                return Err(value.at.error(
                    "an f-string field is a name or literal with field access, indexing, and calls; bind other expressions to a name first",
                ));
            }
            let spec = field
                .spec
                .as_ref()
                .map(|(text, at)| parse_spec(text, &located(at)))
                .transpose()?;
            let mut conversion = match field.conversion {
                Some('r') => Conversion::Repr,
                Some('s') => Conversion::Str,
                _ => Conversion::None,
            };
            if let Some(debug) = &field.debug {
                literal(&mut pieces, debug);
                if conversion == Conversion::None && spec.is_none() {
                    conversion = Conversion::Repr;
                }
            }
            pieces.push(Piece::Field(Field {
                at: field
                    .spec
                    .as_ref()
                    .map_or_else(|| value.at.clone(), |(_, at)| located(at)),
                conversion,
                spec,
            }));
            values.push(value);
        }
        self.formats = true;
        Ok(Expression::Format(Box::new(Format::Literal(
            Arc::new(Template { pieces }),
            values,
        ))))
    }
}

fn expr(at: &Token, kind: Expression) -> Expr {
    Expr {
        at: at.clone(),
        kind,
    }
}
fn name(at: &Token, name: &str) -> Expr {
    expr(at, Expression::Name(name.into()))
}
fn text(at: &Token, text: &str) -> Expr {
    expr(at, Expression::Text(text.into()))
}
fn number(at: &Token, number: &str) -> Expr {
    expr(at, Expression::Number(number.into()))
}
fn call(at: &Token, function: &str, args: Vec<Expr>) -> Expr {
    expr(at, Expression::Call(function.into(), args))
}
fn unary(at: &Token, op: &str, value: Expr) -> Expr {
    expr(at, Expression::Unary(op.into(), Box::new(value)))
}
fn method(at: &Token, receiver: Expr, method: &str, args: Vec<Expr>) -> Expr {
    expr(
        at,
        Expression::Method(Box::new(receiver), method.into(), args, Pos::of(at)),
    )
}
fn try_(value: Expr) -> Expr {
    let at = value.at.clone();
    expr(&at, Expression::Try(Box::new(value)))
}
fn statement(value: Expr) -> Stmt {
    Stmt {
        at: value.at.clone(),
        kind: Statement::Expr(value),
    }
}
fn type_ref(at: &Token, name: &str, args: Vec<TypeRef>) -> TypeRef {
    TypeRef {
        concrete: None,
        at: at.clone(),
        name: Some(name.into()),
        args,
    }
}
fn fmt(name: &str) -> String {
    format!("std.fmt.{name}")
}

/// What a field's value is formatted as.
enum Category {
    Text,
    Bool,
    Signed,
    Unsigned,
    Float(bool),
    /// A user type with the `display` or `debug` method the conversion needs.
    Method,
    /// Anything else: the `str.repr` spelling that `print` uses.
    Repr,
}

impl Spec {
    fn arguments(&self, at: &Token) -> Vec<Expr> {
        let char_text = |c: Option<char>| text(at, &c.map(String::from).unwrap_or_default());
        let flag = |b: bool| expr(at, Expression::Bool(b));
        vec![
            text(at, &self.fill.unwrap_or(' ').to_string()),
            char_text(self.align),
            char_text(self.sign),
            flag(self.alternate),
            flag(self.zero),
            number(at, &self.width.to_string()),
            char_text(self.grouping),
            match self.precision {
                Some(p) => number(at, &p.to_string()),
                None => unary(at, "-", number(at, "1")),
            },
            char_text(self.kind),
        ]
    }

    /// Checks the options against what the field is spelled as.
    fn check(&self, category: &Category, ty: &Ty, at: &Token) -> Result<()> {
        let invalid = |reason: String| {
            at.error(format!(
                "format spec `{}` does not apply to {ty}: {reason}",
                self.text
            ))
        };
        let kind = self.kind;
        match category {
            Category::Signed | Category::Unsigned => {
                if self.precision.is_some() {
                    return Err(invalid(
                        "integers take no precision; convert with f64(...) to format a float"
                            .into(),
                    ));
                }
                match kind {
                    None | Some('d' | 'x' | 'X' | 'o' | 'b') => {}
                    Some('s') => {
                        return Err(invalid(
                            "type `s` formats text; use `!s` to format the value's text".into(),
                        ))
                    }
                    Some(k) => {
                        return Err(invalid(format!(
                            "type `{k}` formats floats; convert with f64(...)"
                        )))
                    }
                }
                if self.grouping == Some(',') && !matches!(kind, None | Some('d')) {
                    return Err(invalid(format!(
                        "`,` grouping applies to decimal output; use `_` with `{}`",
                        kind.unwrap()
                    )));
                }
            }
            Category::Float(_) => match kind {
                None | Some('e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%') => {}
                Some('s') => {
                    return Err(invalid(
                        "type `s` formats text; use `!s` to format the value's text".into(),
                    ))
                }
                Some(k) => {
                    return Err(invalid(format!(
                        "type `{k}` formats integers; convert with i64(...)"
                    )))
                }
            },
            _ => {
                let numeric = if self.sign.is_some() {
                    Some("a sign")
                } else if self.alternate {
                    Some("`#`")
                } else if self.zero {
                    Some("`0` padding")
                } else if self.grouping.is_some() {
                    Some("grouping")
                } else if self.align == Some('=') {
                    Some("`=` alignment")
                } else {
                    None
                };
                if let Some(option) = numeric {
                    return Err(invalid(format!("{option} requires an integer or float")));
                }
                if let Some(k) = kind.filter(|k| *k != 's') {
                    return Err(invalid(format!("type `{k}` requires a number")));
                }
            }
        }
        Ok(())
    }
}

/// A stable symbol for the template at `at`, shared by every instance of a
/// generic function that contains it.
fn symbol(at: &Token) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in at.location().bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("__plenty_fstring_{hash:016x}")
}

impl Lower<'_> {
    pub(super) fn format(
        &mut self,
        format: &Format,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        match format {
            Format::Literal(template, values) => self.format_literal(template, values, at, ops),
            Format::Field(template, index, value) => {
                let Piece::Field(field) = template
                    .pieces
                    .iter()
                    .filter(|p| matches!(p, Piece::Field(_)))
                    .nth(*index)
                    .unwrap()
                else {
                    unreachable!()
                };
                let write = self.format_field(field, value)?;
                self.expr(&write, ops)
            }
        }
    }

    /// Calls the generated function, registering it on first use.
    fn format_literal(
        &mut self,
        template: &Arc<Template>,
        values: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let name = symbol(at);
        if !self.generics.templates.contains_key(&name) {
            let function = self.format_function(&name, template, at);
            self.generics.validate_template(&function, self.aliases)?;
            self.generics
                .templates
                .insert(name.clone(), Arc::new(function));
        }
        // Every field is lent; a reference value is lent again.
        let args: Vec<Expr> = values
            .iter()
            .map(|value| unary(&value.at, "&", value.clone()))
            .collect();
        self.generic_call(&name, None, &args, at, ops)
    }

    fn format_function(&self, symbol: &str, template: &Arc<Template>, at: &Token) -> Function {
        let out = || name(at, OUT);
        let mut body = vec![Stmt {
            at: at.clone(),
            kind: Statement::Assign {
                name: OUT.into(),
                mutable: true,
                annotation: None,
                value: try_(call(
                    at,
                    &fmt("string_formatter"),
                    vec![number(at, NO_BUDGET), number(at, DEPTH)],
                )),
            },
        }];
        let mut inputs = Vec::new();
        let mut type_params = Vec::new();
        for piece in &template.pieces {
            match piece {
                Piece::Literal(literal) => body.push(statement(try_(method(
                    at,
                    out(),
                    "text",
                    vec![text(at, literal)],
                )))),
                Piece::Field(field) => {
                    let index = inputs.len();
                    let parameter = format!("__plenty_v{index}");
                    let ty = format!("__plenty_T{index}");
                    inputs.push((
                        parameter.clone(),
                        type_ref(at, "&", vec![type_ref(at, &ty, vec![])]),
                    ));
                    type_params.push((ty, None));
                    body.push(statement(try_(expr(
                        &field.at,
                        Expression::Format(Box::new(Format::Field(
                            template.clone(),
                            index,
                            name(&field.at, &parameter),
                        ))),
                    ))));
                }
            }
        }
        body.push(statement(call(
            at,
            &fmt("finish"),
            vec![unary(at, "&", out())],
        )));
        Function {
            once: false,
            captures: vec![],
            foreign: None,
            export: None,
            name: symbol.into(),
            type_params,
            at: at.clone(),
            defaults: vec![None; inputs.len()],
            inputs,
            output: type_ref(
                at,
                "Result",
                vec![
                    type_ref(at, "str", vec![]),
                    type_ref(at, &fmt("FormatError"), vec![]),
                ],
            ),
            doc: String::new(),
            doc_at: None,
            body,
        }
    }

    fn has_method(&self, ty: &Ty, name: &str) -> bool {
        let owner = match ty {
            Ty::Class(class) => &class.name,
            Ty::Enum(sum) => &sum.name,
            _ => return false,
        };
        let symbol = crate::record::method(owner, name);
        self.generics.templates.contains_key(&symbol) || self.sigs.contains_key(&symbol)
    }

    /// The call that writes one field, chosen by the value's concrete type.
    fn format_field(&mut self, field: &Field, value: &Expr) -> Result<Expr> {
        let at = &field.at;
        let Some(Ty::Ref(mut ty, _)) = self.expression_type_hint(value) else {
            unreachable!("generated f-string parameters are references")
        };
        // `value` has type `&T`; a borrowed reference field adds one level.
        let mut value = value.clone();
        while let Ty::Ref(inner, _) = &*ty {
            value = unary(at, "*", value);
            ty = inner.clone();
        }
        let ty = (*ty).clone();
        let wanted = if field.conversion == Conversion::Repr {
            "debug"
        } else {
            "display"
        };
        let category = match &ty {
            Ty::Str => Category::Text,
            Ty::Bool => Category::Bool,
            t if t.is_int() && t.int_range().is_some_and(|(min, _)| min < 0) => Category::Signed,
            t if t.is_int() => Category::Unsigned,
            Ty::F32 => Category::Float(true),
            Ty::F64 => Category::Float(false),
            t if self.has_method(t, wanted) => Category::Method,
            _ => Category::Repr,
        };
        // A conversion spells the value as text first.
        let category = match (field.conversion, category) {
            (Conversion::None, category)
            | (Conversion::Str, category @ (Category::Text | Category::Method | Category::Repr)) => {
                category
            }
            (Conversion::Repr, Category::Text) => Category::Text,
            (Conversion::Repr, Category::Method) => Category::Method,
            _ => Category::Repr,
        };
        let repr_text = field.conversion == Conversion::Repr && matches!(ty, Ty::Str);
        if let Some(spec) = &field.spec {
            let checked = if repr_text {
                Category::Text
            } else {
                Category::Repr
            };
            spec.check(
                match category {
                    Category::Method | Category::Repr | Category::Bool => &checked,
                    ref other => other,
                },
                &ty,
                at,
            )?;
        }
        let out = name(at, OUT);
        let out_mut = || unary(at, "&mut", name(at, OUT));
        let scalar = || unary(at, "*", value.clone());
        let Some(spec) = &field.spec else {
            return Ok(match category {
                Category::Text if repr_text => method(at, out, "debug_text", vec![value]),
                Category::Text => method(at, out, "text", vec![value]),
                Category::Bool => method(at, out, "boolean", vec![scalar()]),
                Category::Signed => method(
                    at,
                    out,
                    "integer",
                    vec![call(at, "i64", vec![scalar()]), number(at, "0")],
                ),
                Category::Unsigned => method(
                    at,
                    out,
                    "unsigned",
                    vec![
                        call(at, "u64", vec![scalar()]),
                        number(at, "10"),
                        number(at, "0"),
                    ],
                ),
                Category::Float(single) => method(
                    at,
                    out,
                    if single { "float32_spec" } else { "float_spec" },
                    vec![scalar(), unary(at, "&", call(at, &fmt("spec"), vec![]))],
                ),
                Category::Method => call(at, &fmt(wanted), vec![value, out_mut()]),
                Category::Repr => call(at, &fmt("represent"), vec![value, out_mut()]),
            });
        };
        let spec_value = unary(at, "&", call(at, &fmt("Spec"), spec.arguments(at)));
        let padded = |text_value: Expr| {
            method(
                at,
                name(at, OUT),
                "text_spec",
                vec![text_value, spec_value.clone()],
            )
        };
        let temporary =
            |function: &str, args: Vec<Expr>| unary(at, "&", try_(call(at, &fmt(function), args)));
        Ok(match category {
            Category::Text if repr_text => padded(temporary("debug_string", vec![value])),
            Category::Text => padded(value),
            Category::Bool => padded(expr(
                at,
                Expression::Conditional {
                    condition: Box::new(scalar()),
                    yes: Box::new(text(at, "True")),
                    no: Box::new(text(at, "False")),
                },
            )),
            Category::Signed => method(
                at,
                out,
                "signed_spec",
                vec![call(at, "i64", vec![scalar()]), spec_value],
            ),
            Category::Unsigned => method(
                at,
                out,
                "unsigned_spec",
                vec![call(at, "u64", vec![scalar()]), spec_value],
            ),
            Category::Float(single) => method(
                at,
                out,
                if single { "float32_spec" } else { "float_spec" },
                vec![scalar(), spec_value],
            ),
            Category::Method => padded(temporary(
                if wanted == "debug" {
                    "to_debug"
                } else {
                    "to_string"
                },
                vec![value, number(at, NO_BUDGET), number(at, DEPTH)],
            )),
            Category::Repr => padded(temporary("repr_string", vec![value])),
        })
    }
}
