//! `f"..."` literals. The lexer splits a template into literal text and
//! fields, the parser parses each field's expression and static format spec,
//! and lowering calls a generated generic function that writes the pieces to a
//! `std.fmt` string formatter, or to the formatter of `out.format(f"...")`.
//! Specializing that function gives every field a concrete type, so each spec
//! is checked against its type at compile time and nothing interprets a
//! template at run time. Nested fields in a spec supply only a fill,
//! alignment, width, or precision; their values are checked when formatting.
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
    // Boxed: an inline spec would make every lexed part as large as a field.
    spec: Option<Box<LexedSpec>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct LexedSpec {
    /// The spec's text with each nested field written `{}`, which cannot
    /// otherwise occur in a spec.
    text: String,
    at: Token,
    fields: Vec<LexedField>,
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
    Ascii,
}

/// A spec option written in the spec, or supplied by its nested field `n`.
#[derive(Clone, Copy, PartialEq)]
enum Opt<T> {
    Static(T),
    Nested(usize),
}

/// What a nested field's type lets it supply.
#[derive(Clone, Copy, PartialEq)]
enum Nested {
    /// An integer: a width or precision.
    Count,
    /// A `str`: a fill or alignment.
    Text,
}

#[derive(Clone)]
struct Spec {
    text: String,
    fill: Option<Opt<char>>,
    align: Option<Opt<char>>,
    sign: Option<char>,
    coerce_zero: bool,
    alternate: bool,
    zero: bool,
    width: Opt<usize>,
    grouping: Option<char>,
    precision: Option<Opt<usize>>,
    kind: Option<char>,
}

#[derive(Clone)]
enum FieldSpec {
    Static(Spec),
    /// Parsed when the nested fields' types are known; see [`parse_spec`].
    Nested(String, Token),
}

#[derive(Clone)]
struct Field {
    at: Token,
    conversion: Conversion,
    spec: Option<FieldSpec>,
    /// Where each nested field of the spec was written.
    nested: Vec<Token>,
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
    /// Inside a generated function only: write field `index` of the template
    /// to the formatter `__plenty_out`. The values are the field's, then
    /// those of its spec's nested fields.
    Field(Arc<Template>, usize, Vec<Expr>),
}

impl Format {
    pub(super) fn operands(&self) -> Vec<&Expr> {
        match self {
            Format::Literal(_, values) | Format::Field(_, _, values) => values.iter().collect(),
        }
    }
    pub(super) fn operands_mut(&mut self) -> Vec<&mut Expr> {
        match self {
            Format::Literal(_, values) | Format::Field(_, _, values) => values.iter_mut().collect(),
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
            file: None,
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
                        escape(escaped)
                            .ok_or_else(|| at.error(format!("unsupported escape \\{escaped}")))?,
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
                    );
                }
                '{' => {
                    if !text.is_empty() {
                        parts.push(Part::Literal(std::mem::take(&mut text)));
                    }
                    parts.push(Part::Field(self.format_field(quote, triple, &at, false)?));
                }
                _ => text.push(c),
            }
        }
        if !text.is_empty() {
            parts.push(Part::Literal(text));
        }
        Ok(Kind::Format(parts.into()))
    }

    /// Lexes one field after its `{`, through its `}`. A `nested` field is
    /// inside a spec and cannot contain another.
    fn format_field(
        &mut self,
        quote: char,
        triple: bool,
        open: &Token,
        nested: bool,
    ) -> Result<LexedField> {
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
                Some(c @ ('r' | 's' | 'a')) => Some(c),
                _ => return Err(at.error("expected the conversion `r`, `s`, or `a` after `!`")),
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
            let mut text = String::new();
            let mut fields = Vec::new();
            loop {
                match self.char_at(0) {
                    None => return Err(unterminated()),
                    Some('}') => break,
                    Some('{') if nested => {
                        return Err(self
                            .here()
                            .error("a nested f-string field cannot contain another field"))
                    }
                    Some('{') => {
                        let open = self.here();
                        self.advance();
                        let inner = self.format_field(quote, triple, &open, true)?;
                        if inner.debug.is_some()
                            || inner.conversion.is_some()
                            || inner.spec.is_some()
                        {
                            return Err(open.error(
                                "a nested f-string field is an expression without `=`, a conversion, or a spec",
                            ));
                        }
                        fields.push(inner);
                        text.push_str("{}");
                    }
                    Some('\n') if !triple => return Err(unterminated()),
                    Some(c) if c == quote => {
                        return Err(open.error(
                            "unterminated f-string field; expected `}`, and a field cannot reuse the enclosing quote",
                        ))
                    }
                    Some(c) => {
                        self.advance();
                        text.push(c);
                    }
                }
            }
            field.spec = Some(Box::new(LexedSpec { text, at, fields }));
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

/// One item of a spec: a character, or nested field `n`.
#[derive(Clone, Copy, PartialEq)]
enum Item {
    Char(char),
    Nested(usize),
}

/// Parses a spec whose nested fields, written `{}`, have the given kinds.
/// Each nested field supplies exactly one option, chosen by its position and
/// type: a `str` is the fill or alignment, an integer the width or precision.
fn parse_spec(text: &str, at: &Token, kinds: &[Nested]) -> Result<Spec> {
    let invalid = |reason: &str| at.error(format!("invalid format spec `{text}`: {reason}"));
    let mut items = Vec::new();
    let mut chars = text.chars().peekable();
    let mut count = 0;
    while let Some(c) = chars.next() {
        if c == '{' && chars.peek() == Some(&'}') {
            chars.next();
            items.push(Item::Nested(count));
            count += 1;
        } else {
            items.push(Item::Char(c));
        }
    }
    let char_at = |i: usize| match items.get(i) {
        Some(Item::Char(c)) => Some(*c),
        _ => None,
    };
    let text_at = |i: usize| match items.get(i) {
        Some(Item::Nested(n)) if kinds[*n] == Nested::Text => Some(Opt::Nested(*n)),
        _ => None,
    };
    let align_at = |i: usize| {
        char_at(i)
            .filter(|c| is_align(*c))
            .map(Opt::Static)
            .or_else(|| text_at(i))
    };
    let fill_at = |i: usize| char_at(i).map(Opt::Static).or_else(|| text_at(i));
    let mut i = 0;
    let mut spec = Spec {
        text: text.into(),
        fill: None,
        align: None,
        sign: None,
        coerce_zero: false,
        alternate: false,
        zero: false,
        width: Opt::Static(0),
        grouping: None,
        precision: None,
        kind: None,
    };
    if let (Some(fill), Some(align)) = (fill_at(0), align_at(1)) {
        spec.fill = Some(fill);
        spec.align = Some(align);
        i = 2;
    } else if let Some(align) = align_at(0) {
        spec.align = Some(align);
        i = 1;
    }
    if let Some(c @ ('+' | '-' | ' ')) = char_at(i) {
        spec.sign = Some(c);
        i += 1;
    }
    if char_at(i) == Some('z') {
        spec.coerce_zero = true;
        i += 1;
    }
    if char_at(i) == Some('#') {
        spec.alternate = true;
        i += 1;
    }
    if char_at(i) == Some('0') {
        spec.zero = true;
        i += 1;
    }
    let number = |i: &mut usize| -> Result<Option<Opt<usize>>> {
        if let Some(Item::Nested(n)) = items.get(*i)
            && kinds[*n] == Nested::Count
        {
            *i += 1;
            return Ok(Some(Opt::Nested(*n)));
        }
        let start = *i;
        while char_at(*i).is_some_and(|c| c.is_ascii_digit()) {
            *i += 1;
        }
        if start == *i {
            return Ok(None);
        }
        let value: String = (start..*i).filter_map(char_at).collect();
        match value.parse::<usize>() {
            Ok(n) if n <= SPEC_LIMIT => Ok(Some(Opt::Static(n))),
            _ => Err(invalid(&format!(
                "widths and precisions are limited to {SPEC_LIMIT}"
            ))),
        }
    };
    spec.width = number(&mut i)?.unwrap_or(Opt::Static(0));
    if let Some(c @ (',' | '_')) = char_at(i) {
        spec.grouping = Some(c);
        i += 1;
    }
    if char_at(i) == Some('.') {
        i += 1;
        spec.precision = Some(number(&mut i)?.ok_or_else(|| invalid("expected digits after `.`"))?);
    }
    if let Some(c) = char_at(i).filter(|c| "sdxXobcneEfFgG%".contains(*c)) {
        spec.kind = Some(c);
        i += 1;
    }
    match items.get(i) {
        None => {}
        Some(Item::Nested(n)) if kinds[*n] == Nested::Text => {
            return Err(invalid(
                "a nested `str` field is the fill or the alignment, at the start of the spec",
            ));
        }
        Some(Item::Nested(_)) => {
            return Err(invalid(
                "a nested integer field is the width or, after `.`, the precision",
            ));
        }
        Some(Item::Char(c)) => {
            return Err(invalid(&format!(
                "unexpected `{c}`; the form is [[fill]align][sign][z][#][0][width][grouping][.precision][type]"
            )));
        }
    }
    if spec.zero {
        match spec.align {
            Some(Opt::Static(_)) => {
                spec.fill.get_or_insert(Opt::Static('0'));
                spec.zero = false;
            }
            Some(Opt::Nested(_)) => {
                return Err(invalid(
                    "`0` cannot follow a nested alignment field; write the fill instead",
                ));
            }
            None => {}
        }
    }
    Ok(spec)
}

impl Parser {
    /// Parses a field's expression from its lexed tokens.
    fn field_expression(&self, field: &LexedField) -> Result<Expr> {
        let mut parser = Parser::new(self.end.file);
        parser.tokens = field.tokens.iter().map(|t| self.located(t)).collect();
        parser.end = self.located(&field.end);
        parser.in_brackets = parser.tokens.iter().map(|_| true).collect();
        parser.complete = true;
        parser.depth = self.depth;
        let mut value = parser.expr(0)?;
        // As in Python, `{a, b}` is a tuple without its parentheses.
        if parser.peek().is(",") {
            let at = value.at.clone();
            let mut values = vec![value];
            while parser.eat(",") && parser.peek().kind != Kind::Eof {
                values.push(parser.expr(0)?);
            }
            value = Expr {
                at,
                kind: Expression::Tuple(values),
            };
        }
        if parser.peek().kind != Kind::Eof {
            return Err(parser.peek().error("unexpected token in an f-string field"));
        }
        Ok(value)
    }

    fn located(&self, token: &Token) -> Token {
        Token {
            file: self.end.file,
            ..token.clone()
        }
    }

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
            let value = self.field_expression(field)?;
            let mut at = value.at.clone();
            values.push(value);
            let mut nested = Vec::new();
            let spec = match &field.spec {
                None => None,
                Some(spec) => {
                    at = self.located(&spec.at);
                    for inner in &spec.fields {
                        let value = self.field_expression(inner)?;
                        nested.push(value.at.clone());
                        values.push(value);
                    }
                    Some(if spec.fields.is_empty() {
                        FieldSpec::Static(parse_spec(&spec.text, &at, &[])?)
                    } else {
                        FieldSpec::Nested(spec.text.clone(), at.clone())
                    })
                }
            };
            let mut conversion = match field.conversion {
                Some('r') => Conversion::Repr,
                Some('s') => Conversion::Str,
                Some('a') => Conversion::Ascii,
                _ => Conversion::None,
            };
            if let Some(debug) = &field.debug {
                literal(&mut pieces, debug);
                if conversion == Conversion::None && spec.is_none() {
                    conversion = Conversion::Repr;
                }
            }
            pieces.push(Piece::Field(Field {
                at,
                conversion,
                spec,
                nested,
            }));
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
        parameter: false,
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
    /// The `std.fmt.Spec` constructor arguments. `nested` holds the values of
    /// the spec's nested fields, each a reference to a `str` or an integer.
    fn arguments(&self, at: &Token, nested: &[Expr]) -> Vec<Expr> {
        let char_text = |c: Option<char>| text(at, &c.map(String::from).unwrap_or_default());
        let flag = |b: bool| expr(at, Expression::Bool(b));
        let source = |part: Option<Opt<char>>| match part {
            Some(Opt::Nested(n)) => nested[n].clone(),
            Some(Opt::Static(c)) => char_text(Some(c)),
            None => char_text(None),
        };
        let count = |part: Opt<usize>| match part {
            Opt::Static(n) => number(at, &n.to_string()),
            // Wrapping maps a `u64` beyond `i64.MAX` to a negative count,
            // which `spec_count` rejects like any other negative count.
            Opt::Nested(n) => try_(call(
                at,
                &fmt("spec_count"),
                vec![method(
                    at,
                    name(at, "i64"),
                    "wrapping_from",
                    vec![unary(at, "*", nested[n].clone())],
                )],
            )),
        };
        let dynamic = |part: Option<Opt<char>>| matches!(part, Some(Opt::Nested(_)));
        let (fill, align) = if dynamic(self.fill) || dynamic(self.align) {
            (
                try_(call(
                    at,
                    &fmt("spec_fill"),
                    vec![source(self.fill), source(self.align)],
                )),
                try_(call(at, &fmt("spec_align"), vec![source(self.align)])),
            )
        } else {
            let fill = match self.fill {
                Some(Opt::Static(c)) => c,
                _ => ' ',
            };
            (text(at, &fill.to_string()), source(self.align))
        };
        vec![
            fill,
            align,
            char_text(self.sign),
            flag(self.coerce_zero),
            flag(self.alternate),
            flag(self.zero),
            count(self.width),
            char_text(self.grouping),
            match self.precision {
                Some(p) => count(p),
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
        if self.coerce_zero && !matches!(category, Category::Float(_)) {
            return Err(invalid("the `z` option applies to floats".into()));
        }
        if kind == Some('n') && self.grouping.is_some() {
            return Err(invalid(
                "type `n` takes no grouping; it groups nothing without a locale".into(),
            ));
        }
        match category {
            Category::Signed | Category::Unsigned => {
                if self.precision.is_some() {
                    return Err(invalid(
                        "integers take no precision; convert with f64(...) to format a float"
                            .into(),
                    ));
                }
                match kind {
                    None | Some('d' | 'x' | 'X' | 'o' | 'b' | 'n') => {}
                    Some('c') => {
                        let option = if self.sign.is_some() {
                            Some("sign")
                        } else if self.alternate {
                            Some("`#`")
                        } else if self.grouping.is_some() {
                            Some("grouping")
                        } else {
                            None
                        };
                        if let Some(option) = option {
                            return Err(invalid(format!("type `c` takes no {option}")));
                        }
                    }
                    Some('s') => {
                        return Err(invalid(
                            "type `s` formats text; use `!s` to format the value's text".into(),
                        ));
                    }
                    Some(k) => {
                        return Err(invalid(format!(
                            "type `{k}` formats floats; convert with f64(...)"
                        )));
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
                None | Some('e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%' | 'n') => {}
                Some('s') => {
                    return Err(invalid(
                        "type `s` formats text; use `!s` to format the value's text".into(),
                    ));
                }
                Some(k) => {
                    return Err(invalid(format!(
                        "type `{k}` formats integers; convert with i64(...)"
                    )));
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
                } else if self.align == Some(Opt::Static('=')) {
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
/// generic function that contains it. `write` names the form that writes to
/// a caller's formatter.
fn symbol(at: &Token, write: bool) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in at.location().bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    let form = if write { "write_" } else { "" };
    format!("__plenty_fstring_{form}{hash:016x}")
}

/// Whether `ty` is a `std.fmt.Formatter`, directly or through a reference.
fn formatter(ty: &Ty) -> Option<bool> {
    match ty {
        Ty::Class(class) if class.name.starts_with("std.fmt.Formatter[") => Some(false),
        Ty::Ref(inner, _) => formatter(inner).map(|_| true),
        _ => None,
    }
}

impl Lower<'_> {
    pub(super) fn format(
        &mut self,
        format: &Format,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        match format {
            Format::Literal(template, values) => {
                self.format_literal(template, values, None, at, ops)
            }
            Format::Field(template, index, values) => {
                let Piece::Field(field) = template
                    .pieces
                    .iter()
                    .filter(|p| matches!(p, Piece::Field(_)))
                    .nth(*index)
                    .unwrap()
                else {
                    unreachable!()
                };
                let write = self.format_field(field, values)?;
                self.expr(&write, ops)
            }
        }
    }

    /// `out.format(f"...")` when `out` is a `std.fmt.Formatter` or a
    /// reference to one: writes the f-string to it without building a `str`.
    /// Returns `None` when `base` is not a formatter.
    pub(super) fn format_method(
        &mut self,
        base: &Expr,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Option<Result<Type>> {
        let reference = formatter(self.expression_type_hint(base).as_ref()?)?;
        let [
            Expr {
                kind: Expression::Format(format),
                at: literal_at,
            },
        ] = args
        else {
            return Some(Err(at.error(
                "`Formatter.format` takes one f-string literal, as in `out.format(f\"{x}\")`",
            )));
        };
        let Format::Literal(template, values) = &**format else {
            unreachable!("only generated functions contain field writes")
        };
        let target = if reference {
            base.clone()
        } else {
            unary(&base.at, "&mut", base.clone())
        };
        Some(self.format_literal(template, values, Some(target), literal_at, ops))
    }

    /// Calls the generated function, registering it on first use. With a
    /// `target` formatter, the function writes to it instead of a new string.
    fn format_literal(
        &mut self,
        template: &Arc<Template>,
        values: &[Expr],
        target: Option<Expr>,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let name = symbol(at, target.is_some());
        if !self.generics.templates.contains_key(&name) {
            let function = format_function(&name, template, target.is_some(), at);
            self.generics.validate_template(&function, self.aliases)?;
            self.generics.retention.keep(&function.body);
            self.generics
                .templates
                .insert(name.clone(), Arc::new(function));
        }
        // Every field is lent; a reference value is lent again.
        let args: Vec<Expr> = target
            .into_iter()
            .chain(
                values
                    .iter()
                    .map(|value| unary(&value.at, "&", value.clone())),
            )
            .collect();
        self.generic_call(&name, None, &args, at, ops)
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

    /// A generated parameter `&T` with any further reference levels of `T`
    /// removed, and `T` itself.
    fn field_value(&self, value: &Expr, at: &Token) -> (Expr, Ty) {
        let Some(Ty::Ref(mut ty, _)) = self.expression_type_hint(value) else {
            unreachable!("generated f-string parameters are references")
        };
        let mut value = value.clone();
        while let Ty::Ref(inner, _) = &*ty {
            value = unary(at, "*", value);
            ty = inner.clone();
        }
        (value, (*ty).clone())
    }

    /// The call that writes one field, chosen by the value's concrete type.
    fn format_field(&mut self, field: &Field, values: &[Expr]) -> Result<Expr> {
        let at = &field.at;
        let (value, ty) = self.field_value(&values[0], at);
        let mut nested = Vec::new();
        let mut kinds = Vec::new();
        for (written, parameter) in field.nested.iter().zip(&values[1..]) {
            let (value, ty) = self.field_value(parameter, at);
            kinds.push(match ty {
                Ty::Str => Nested::Text,
                t if t.is_int() => Nested::Count,
                t => {
                    return Err(written.error(format!(
                        "a nested field in a format spec is an integer width or precision, or a `str` fill or alignment, not {t}"
                    )))
                }
            });
            nested.push(value);
        }
        let spec = match &field.spec {
            None => None,
            Some(FieldSpec::Static(spec)) => Some(spec.clone()),
            Some(FieldSpec::Nested(text, spec_at)) => Some(parse_spec(text, spec_at, &kinds)?),
        };
        let wanted = if matches!(field.conversion, Conversion::Repr | Conversion::Ascii) {
            "debug"
        } else {
            "display"
        };
        if ty.facts().shared {
            return Err(at.error(super::shared::FORMAT));
        }
        if ty.facts().atomic && super::mutex::holds_mutex(&ty) {
            return Err(at.error(super::mutex::FORMAT));
        }
        if ty.facts().atomic {
            return Err(at.error("atomics cannot be formatted; format a value read with load()"));
        }
        // `NonZero[T]` formats as its integer.
        let nonzero = ty.nonzero_element().is_some();
        let ty = ty.nonzero_element().unwrap_or(ty);
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
        let out = name(at, OUT);
        let out_mut = || unary(at, "&mut", name(at, OUT));
        let temporary =
            |function: &str, args: Vec<Expr>| unary(at, "&", try_(call(at, &fmt(function), args)));
        let spec_value =
            |spec: &Spec| unary(at, "&", call(at, &fmt("Spec"), spec.arguments(at, &nested)));
        let padded = |text_value: Expr, spec: &Spec| {
            method(
                at,
                name(at, OUT),
                "text_spec",
                vec![text_value, spec_value(spec)],
            )
        };
        if field.conversion == Conversion::Ascii {
            // Python's `ascii()`: the `!r` text with non-ASCII scalars escaped.
            let repr = match category {
                Category::Text => temporary("debug_string", vec![value]),
                Category::Method => temporary(
                    "to_debug",
                    vec![value, number(at, NO_BUDGET), number(at, DEPTH)],
                ),
                _ => temporary("repr_string", vec![value]),
            };
            return Ok(match &spec {
                None => method(at, out, "ascii_text", vec![repr]),
                Some(spec) => {
                    spec.check(&Category::Text, &ty, at)?;
                    padded(temporary("ascii_string", vec![repr]), spec)
                }
            });
        }
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
        if let Some(spec) = &spec {
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
        let scalar = || {
            if nonzero {
                method(at, value.clone(), "get", vec![])
            } else {
                unary(at, "*", value.clone())
            }
        };
        let Some(spec) = &spec else {
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
        Ok(match category {
            Category::Text if repr_text => padded(temporary("debug_string", vec![value]), spec),
            Category::Text => padded(value, spec),
            Category::Bool => padded(
                expr(
                    at,
                    Expression::Conditional {
                        condition: Box::new(scalar()),
                        yes: Box::new(text(at, "True")),
                        no: Box::new(text(at, "False")),
                    },
                ),
                spec,
            ),
            Category::Signed | Category::Unsigned if spec.kind == Some('c') => method(
                at,
                out,
                "char_spec",
                vec![
                    method(at, name(at, "i64"), "wrapping_from", vec![scalar()]),
                    spec_value(spec),
                ],
            ),
            Category::Signed => method(
                at,
                out,
                "signed_spec",
                vec![call(at, "i64", vec![scalar()]), spec_value(spec)],
            ),
            Category::Unsigned => method(
                at,
                out,
                "unsigned_spec",
                vec![call(at, "u64", vec![scalar()]), spec_value(spec)],
            ),
            Category::Float(single) => method(
                at,
                out,
                if single { "float32_spec" } else { "float_spec" },
                vec![scalar(), spec_value(spec)],
            ),
            Category::Method => padded(
                temporary(
                    if wanted == "debug" {
                        "to_debug"
                    } else {
                        "to_string"
                    },
                    vec![value, number(at, NO_BUDGET), number(at, DEPTH)],
                ),
                spec,
            ),
            Category::Repr => padded(temporary("repr_string", vec![value]), spec),
        })
    }
}

/// The generic function an f-string calls. It takes each field value, and
/// each nested spec field's value, by reference. With `write`, its first
/// parameter is the caller's formatter and it returns `Result[(), FormatError]`;
/// otherwise it builds and returns a new `str`.
fn format_function(symbol: &str, template: &Arc<Template>, write: bool, at: &Token) -> Function {
    let out = || name(at, OUT);
    let mut inputs = Vec::new();
    let mut type_params = Vec::new();
    let mut body = Vec::new();
    if write {
        let writer = "__plenty_W";
        type_params.push((writer.into(), None));
        inputs.push((
            OUT.into(),
            type_ref(
                at,
                "&mut",
                vec![type_ref(
                    at,
                    &fmt("Formatter"),
                    vec![type_ref(at, writer, vec![])],
                )],
            ),
        ));
    } else {
        body.push(Stmt {
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
        });
    }
    let mut index = 0;
    for piece in &template.pieces {
        match piece {
            Piece::Literal(literal) => body.push(statement(try_(method(
                at,
                out(),
                "text",
                vec![text(at, literal)],
            )))),
            Piece::Field(field) => {
                let mut values = Vec::new();
                for _ in 0..=field.nested.len() {
                    let parameter = format!("__plenty_v{}", type_params.len());
                    let ty = format!("__plenty_T{}", type_params.len());
                    inputs.push((
                        parameter.clone(),
                        type_ref(at, "&", vec![type_ref(at, &ty, vec![])]),
                    ));
                    type_params.push((ty, None));
                    values.push(name(&field.at, &parameter));
                }
                body.push(statement(try_(expr(
                    &field.at,
                    Expression::Format(Box::new(Format::Field(template.clone(), index, values))),
                ))));
                index += 1;
            }
        }
    }
    let result = if write {
        call(at, "Ok", vec![expr(at, Expression::Unit)])
    } else {
        call(at, &fmt("finish"), vec![unary(at, "&", out())])
    };
    body.push(statement(result));
    let output = if write {
        TypeRef {
            concrete: None,
            parameter: false,
            at: at.clone(),
            name: None,
            args: vec![],
        }
    } else {
        type_ref(at, "str", vec![])
    };
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
            vec![output, type_ref(at, &fmt("FormatError"), vec![])],
        ),
        doc: String::new(),
        doc_at: None,
        body,
    }
}
