//! Modern Plenty frontend: indentation → syntax tree → typed operations.
//!
//! Signatures are collected before bodies. Inference is local, expressions
//! have zero (unit) or one result, and no backend participates in inference.
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::rc::Rc;

use crate::collection::CollectionOp;
use crate::op::{mark_tail_calls, CompiledFn, FnSig, MatchArm, Op, Pattern, Ty};
use crate::value::{Heap, Value};
mod classes;
mod collections;
mod contexts;
mod enums;
mod files;
mod generators;
mod generics;
mod modules;
mod protocols;
mod references;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Type = Option<Ty>; // Unit expressions have no operand; enum slots use Ty::Unit.
pub(crate) type TypeAliases = HashMap<String, Type>;

/// A checked binary's source entrypoint determines the native process result.
pub(crate) struct Program {
    pub(crate) ops: Vec<Op>,
    pub(crate) returns_status: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Word(String),
    Number(String),
    Text(String),
    Symbol(String),
    Newline,
    Indent,
    Dedent,
    Eof,
}

#[derive(Clone, Debug)]
struct Token {
    kind: Kind,
    line: usize,
    column: usize,
    source: Option<Rc<str>>,
}

impl Token {
    fn is(&self, text: &str) -> bool {
        matches!(&self.kind, Kind::Word(s) | Kind::Symbol(s) if s == text)
    }

    fn error(&self, message: impl std::fmt::Display) -> Box<dyn Error> {
        let source = self
            .source
            .as_ref()
            .map(|s| format!("{s}:"))
            .unwrap_or_default();
        format!("{source}{}:{}: {message}", self.line, self.column).into()
    }
}

fn lex(source: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = source.chars().collect();
    let (mut pos, mut line, mut column) = (0, 1, 1);
    let mut line_start = true;
    let mut indents = vec![0];
    let mut parens = 0usize;
    let mut delimiters = Vec::new();
    let mut out = Vec::new();
    while pos < chars.len() {
        if line_start {
            let mut spaces = 0;
            while chars.get(pos) == Some(&' ') {
                spaces += 1;
                pos += 1;
                column += 1;
            }
            if chars.get(pos) == Some(&'\t') {
                return Err(
                    format!("{line}:{column}: use spaces, not tabs, for indentation").into(),
                );
            }
            if parens == 0 && !matches!(chars.get(pos), None | Some('\n' | '\r' | '#')) {
                let last = *indents.last().unwrap();
                if spaces > last {
                    indents.push(spaces);
                    out.push(Token {
                        kind: Kind::Indent,
                        line,
                        column,
                        source: None,
                    });
                } else {
                    while spaces < *indents.last().unwrap() {
                        indents.pop();
                        out.push(Token {
                            kind: Kind::Dedent,
                            line,
                            column,
                            source: None,
                        });
                    }
                    if spaces != *indents.last().unwrap() {
                        return Err(format!(
                            "{line}:{column}: indentation does not match an outer block"
                        )
                        .into());
                    }
                }
            }
            line_start = false;
            if pos == chars.len() {
                break;
            }
        }
        let start = Token {
            kind: Kind::Eof,
            line,
            column,
            source: None,
        };
        let c = chars[pos];
        match c {
            ' ' | '\r' => {
                pos += 1;
                column += 1;
            }
            '\t' => return Err(start.error("tabs are not supported; use spaces")),
            '\n' => {
                if parens == 0
                    && out
                        .last()
                        .is_some_and(|t| !matches!(t.kind, Kind::Newline | Kind::Dedent))
                {
                    out.push(Token {
                        kind: Kind::Newline,
                        ..start
                    });
                }
                pos += 1;
                line += 1;
                column = 1;
                line_start = true;
            }
            '#' => {
                while pos < chars.len() && chars[pos] != '\n' {
                    pos += 1;
                    column += 1;
                }
            }
            '\'' | '"' => {
                let triple = chars.get(pos + 1) == Some(&c) && chars.get(pos + 2) == Some(&c);
                let width = if triple { 3 } else { 1 };
                pos += width;
                column += width;
                let mut text = String::new();
                loop {
                    let ch = *chars
                        .get(pos)
                        .ok_or_else(|| start.error("unterminated string"))?;
                    if ch == c
                        && (!triple
                            || (chars.get(pos + 1) == Some(&c) && chars.get(pos + 2) == Some(&c)))
                    {
                        pos += width;
                        column += width;
                        break;
                    }
                    if ch == '\n' && !triple {
                        return Err(start.error("newline in a quoted string"));
                    }
                    pos += 1;
                    column += 1;
                    if ch == '\\' {
                        let escaped = *chars
                            .get(pos)
                            .ok_or_else(|| start.error("unterminated escape"))?;
                        pos += 1;
                        column += 1;
                        text.push(match escaped {
                            '0' => '\0',
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            '\\' => '\\',
                            '\'' => '\'',
                            '"' => '"',
                            _ => return Err(start.error(format!("unsupported escape \\{escaped}"))),
                        });
                    } else {
                        if ch == '\n' {
                            line += 1;
                            column = 1;
                        }
                        text.push(ch);
                    }
                }
                out.push(Token {
                    kind: Kind::Text(text),
                    ..start
                });
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let begin = pos;
                while chars
                    .get(pos)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
                {
                    pos += 1;
                    column += 1;
                }
                out.push(Token {
                    kind: Kind::Word(chars[begin..pos].iter().collect()),
                    ..start
                });
            }
            c if c.is_ascii_digit()
                || (c == '.' && chars.get(pos + 1).is_some_and(char::is_ascii_digit)) =>
            {
                let begin = pos;
                while chars
                    .get(pos)
                    .is_some_and(|c| c.is_ascii_digit() || *c == '_')
                {
                    pos += 1;
                }
                if chars.get(pos) == Some(&'.') {
                    pos += 1;
                    while chars
                        .get(pos)
                        .is_some_and(|c| c.is_ascii_digit() || *c == '_')
                    {
                        pos += 1;
                    }
                }
                if matches!(chars.get(pos), Some('e' | 'E')) {
                    pos += 1;
                    if matches!(chars.get(pos), Some('+' | '-')) {
                        pos += 1;
                    }
                    while chars
                        .get(pos)
                        .is_some_and(|c| c.is_ascii_digit() || *c == '_')
                    {
                        pos += 1;
                    }
                }
                // Keep a suffix (including malformed suffixes) in the same token.
                while chars
                    .get(pos)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
                {
                    pos += 1;
                }
                column += pos - begin;
                out.push(Token {
                    kind: Kind::Number(chars[begin..pos].iter().collect()),
                    ..start
                });
            }
            '(' | ')' | '[' | ']' | '{' | '}' | '.' | ':' | ',' | '+' | '-' | '*' | '%' | '/'
            | '=' | '!' | '<' | '>' | '&' | '?' => {
                let mut symbol = c.to_string();
                if let Some(next) = chars.get(pos + 1) {
                    if matches!(
                        (c, *next),
                        ('-', '>') | ('=', '=') | ('!', '=') | ('<', '=') | ('>', '=') | ('/', '/')
                    ) {
                        symbol.push(*next);
                        pos += 1;
                        column += 1;
                    }
                }
                if matches!(c, '(' | '[' | '{') {
                    parens += 1;
                    delimiters.push(c);
                }
                if matches!(c, ')' | ']' | '}') {
                    let expected = match c {
                        ')' => '(',
                        ']' => '[',
                        _ => '{',
                    };
                    if delimiters.pop() != Some(expected) {
                        return Err(start.error(format!("unmatched or mismatched `{c}`")));
                    }
                    parens -= 1;
                }
                pos += 1;
                column += 1;
                out.push(Token {
                    kind: Kind::Symbol(symbol),
                    ..start
                });
            }
            _ => return Err(start.error(format!("unexpected character `{c}`"))),
        }
    }
    if parens != 0 {
        return Err(format!("{line}:{column}: unclosed parenthesis").into());
    }
    if out
        .last()
        .is_some_and(|t| !matches!(t.kind, Kind::Newline | Kind::Dedent))
    {
        out.push(Token {
            kind: Kind::Newline,
            line,
            column,
            source: None,
        });
    }
    for _ in 1..indents.len() {
        out.push(Token {
            kind: Kind::Dedent,
            line,
            column,
            source: None,
        });
    }
    out.push(Token {
        kind: Kind::Eof,
        line,
        column,
        source: None,
    });
    Ok(out)
}

#[derive(Clone)]
struct Function {
    name: String,
    type_params: Vec<(String, Option<TypeRef>)>,
    at: Token,
    inputs: Vec<(String, TypeRef)>,
    output: TypeRef,
    doc: String,
    body: Vec<Stmt>,
}

#[derive(Clone)]
struct TypeRef {
    /// Internal substitutions preserve inferred frame identities directly.
    concrete: Type,
    at: Token,
    /// None denotes unit, spelled `()`.
    name: Option<String>,
    args: Vec<TypeRef>,
}

impl TypeRef {
    fn resolve(&self, aliases: &TypeAliases) -> Result<Type> {
        let ty = self.resolve_inner(aliases)?;
        if ty.as_ref().is_some_and(|t| t.layout_depth() > 64) {
            return Err(self
                .at
                .error("type nesting exceeds the implementation limit of 64"));
        }
        Ok(ty)
    }
    fn resolve_inner(&self, aliases: &TypeAliases) -> Result<Type> {
        if let Some(ty) = &self.concrete {
            return Ok(Some(ty.clone()));
        }
        let Some(name) = &self.name else {
            return Ok(None);
        };
        if name == "tuple" {
            if self.args.is_empty() {
                return Err(self.at.error("use () for the empty tuple"));
            }
            let fields = self
                .args
                .iter()
                .map(|t| Ok(t.resolve(aliases)?.unwrap_or(Ty::Unit)))
                .collect::<Result<Vec<_>>>()?;
            if fields.iter().any(Ty::restricted_storage) {
                return Err(self
                    .at
                    .error("references and generators cannot be stored in tuples"));
            }
            if fields.iter().map(|t| t.to_string().len()).sum::<usize>() > 16_384 {
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
            return Ok(Some(Ty::Range(Rc::new(ty))));
        }
        if matches!(name.as_str(), "&" | "&mut") {
            let inner = self.args[0]
                .resolve(aliases)?
                .ok_or_else(|| self.at.error("cannot borrow unit"))?;
            if matches!(inner, Ty::Ref(..)) {
                return Err(self.at.error("references to references are not supported"));
            }
            return Ok(Some(Ty::Ref(Rc::new(inner), name == "&mut")));
        }
        if name == "Generator" {
            if self.args.len() != 1 {
                return Err(self.at.error("Generator requires 1 type argument"));
            }
            let element = self.args[0]
                .resolve(aliases)?
                .ok_or_else(|| self.at.error("generator elements cannot be unit"))?;
            if element.restricted_storage() {
                return Err(self.at.error("generators cannot yield generators"));
            }
            return Ok(Some(crate::generator::ty(element, None)));
        }
        if matches!(name.as_str(), "Option" | "Result") {
            let count = if name == "Option" { 1 } else { 2 };
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
            if args.iter().map(|t| t.to_string().len()).sum::<usize>() > 16_384 {
                return Err(self
                    .at
                    .error("concrete type name exceeds the implementation limit"));
            }
            if args.iter().any(Ty::contains_reference) {
                return Err(self
                    .at
                    .error("references cannot be stored in enum payloads"));
            }
            return Ok(Some(if name == "Option" {
                crate::sum::option(args[0].clone())
            } else {
                crate::sum::result(args[0].clone(), args[1].clone())
            }));
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
            if args.iter().any(Ty::restricted_storage) {
                return Err(self.at.error("generators cannot be stored in collections"));
            }
            return Ok(Some(match name.as_str() {
                "list" => Ty::List(Rc::new(args[0].clone())),
                "set" => {
                    if !args[0].hashable() {
                        return Err(self.at.error("set elements must be integers, bool, or str"));
                    }
                    Ty::Set(Rc::new(args[0].clone()))
                }
                _ => {
                    if !args[0].hashable() {
                        return Err(self
                            .at
                            .error("dictionary keys must be integers, bool, or str"));
                    }
                    Ty::Dict(Rc::new(args[0].clone()), Rc::new(args[1].clone()))
                }
            }));
        }
        if !self.args.is_empty() {
            return Err(self.at.error("this type does not take type arguments"));
        }
        lookup_type(name, aliases).ok_or_else(|| self.at.error(format!("unknown type `{name}`")))
    }
}

struct TypeAlias {
    at: Token,
    name: String,
    target: TypeRef,
}

#[derive(Clone)]
struct Stmt {
    at: Token,
    kind: Statement,
}
#[derive(Clone)]
enum Statement {
    Unpack {
        names: Vec<String>,
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
}

#[derive(Clone)]
struct Expr {
    at: Token,
    kind: Expression,
}
#[derive(Clone)]
enum Expression {
    GenericCall(String, Vec<TypeRef>, Vec<Expr>),
    Tuple(Vec<Expr>, bool),
    Try(Box<Expr>),
    ClassNew(Rc<crate::record::ClassType>),
    ClassReady(Rc<crate::record::ClassType>, Box<Expr>),
    Type(TypeRef),
    Member(Box<Expr>, String),
    Number(String),
    Text(String),
    Bool(bool),
    Collection {
        kind: String,
        entries: Vec<(Expr, Option<Expr>)>,
        clauses: Vec<Clause>,
    },
    Index(Box<Expr>, Box<Expr>),
    Method(Box<Expr>, String, Vec<Expr>),
    Constructor(TypeRef, Vec<Expr>),
    Unit,
    Group(Box<Expr>),
    Name(String),
    Call(String, Vec<Expr>),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Conditional {
        condition: Box<Expr>,
        yes: Box<Expr>,
        no: Box<Expr>,
    },
}

#[derive(Clone)]
enum Clause {
    For(Vec<String>, Expr),
    If(Expr),
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    type_depth: usize,
}
impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }
    fn take(&mut self) -> Token {
        let t = self.peek().clone();
        self.pos += 1;
        t
    }
    fn eat(&mut self, text: &str) -> bool {
        if self.peek().is(text) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, text: &str) -> Result<()> {
        if self.eat(text) {
            Ok(())
        } else {
            Err(self.peek().error(format!("expected `{text}`")))
        }
    }
    fn kind(&mut self, kind: Kind, label: &str) -> Result<()> {
        if self.peek().kind == kind {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.peek().error(format!("expected {label}")))
        }
    }
    fn name(&mut self) -> Result<String> {
        let t = self.take();
        if let Kind::Word(ref s) = t.kind {
            if !reserved(s) && !s.starts_with("__plenty_") {
                return Ok(s.clone());
            }
        }
        Err(t.error("expected an identifier (keywords and __plenty_ names are reserved)"))
    }
    fn ty(&mut self) -> Result<TypeRef> {
        if self.type_depth >= 64 {
            return Err(self
                .peek()
                .error("type nesting exceeds the implementation limit of 64"));
        }
        self.type_depth += 1;
        let result = self.ty_inner();
        self.type_depth -= 1;
        result
    }
    fn ty_inner(&mut self) -> Result<TypeRef> {
        let at = self.peek().clone();
        if self.eat("&") {
            let name = if self.eat("mut") { "&mut" } else { "&" };
            return Ok(TypeRef {
                concrete: None,
                at,
                name: Some(name.into()),
                args: vec![self.ty()?],
            });
        }
        if self.eat("(") {
            let mut args = Vec::new();
            while !self.eat(")") {
                args.push(self.ty()?);
                if self.eat(")") {
                    break;
                }
                self.expect(",")?;
            }
            return Ok(TypeRef {
                concrete: None,
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
        if let Kind::Word(ref name) = t.kind {
            if !reserved(name) {
                let mut name = name.clone();
                while self.eat(".") {
                    name.push('.');
                    name.push_str(&self.name()?);
                }
                return Ok(TypeRef {
                    concrete: None,
                    at,
                    name: Some(name),
                    args: self.type_arguments()?,
                });
            }
        }
        Err(t.error("expected a type name or ()"))
    }
    fn type_arguments(&mut self) -> Result<Vec<TypeRef>> {
        let mut args = Vec::new();
        if self.eat("[") {
            loop {
                args.push(self.ty()?);
                if self.eat("]") {
                    break;
                }
                self.expect(",")?;
            }
        }
        Ok(args)
    }
    fn alias(&mut self) -> Result<TypeAlias> {
        let at = self.take(); // type
        let name = self.name()?;
        self.expect("=")?;
        let target = self.ty()?;
        self.kind(Kind::Newline, "the end of the type alias declaration")?;
        Ok(TypeAlias { at, name, target })
    }
    fn function(&mut self) -> Result<Function> {
        self.function_in(None)
    }
    fn function_in(&mut self, class: Option<&str>) -> Result<Function> {
        let at = self.take(); // def
        let name = self.name()?;
        if class.is_none() && builtin(&name) {
            return Err(at.error("cannot redefine a builtin"));
        }
        let mut type_params = Vec::new();
        if self.eat("[") {
            if class.is_some() {
                return Err(at.error("generic methods are not supported yet"));
            }
            loop {
                let param = self.name()?;
                if builtin(&param) || type_params.iter().any(|(n, _)| n == &param) {
                    return Err(at.error("duplicate or builtin type parameter"));
                }
                let bound = if self.eat(":") {
                    Some(self.ty()?)
                } else {
                    None
                };
                type_params.push((param, bound));
                if self.eat("]") {
                    break;
                }
                self.expect(",")?;
            }
        }
        self.expect("(")?;
        let mut inputs = Vec::new();
        while !self.eat(")") {
            let param = self.name()?;
            if inputs.iter().any(|(n, _)| n == &param) {
                return Err(self.peek().error("duplicate parameter"));
            }
            let ty = if let Some(class) =
                class.filter(|_| param == "self" && inputs.is_empty() && !self.peek().is(":"))
            {
                TypeRef {
                    concrete: None,
                    at: at.clone(),
                    name: Some(
                        if matches!(name.as_str(), "__init__" | "__del__") {
                            "&mut"
                        } else {
                            "&"
                        }
                        .into(),
                    ),
                    args: vec![TypeRef {
                        concrete: None,
                        at: at.clone(),
                        name: Some(class.into()),
                        args: vec![],
                    }],
                }
            } else {
                self.expect(":")?;
                self.ty()?
            };
            inputs.push((param, ty));
            if self.eat(")") {
                break;
            }
            self.expect(",")?;
        }
        if inputs.len() > 256 {
            return Err(at.error("at most 256 parameter/local slots are supported"));
        }
        self.expect("->")?;
        let output = self.ty()?;
        let mut body = self.suite()?;
        let doc = if matches!(
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
                        ..
                    }),
                ..
            } = body.remove(0)
            else {
                unreachable!()
            };
            s
        } else {
            String::new()
        };
        Ok(Function {
            type_params,
            name,
            at,
            inputs,
            output,
            doc,
            body,
        })
    }
    fn suite(&mut self) -> Result<Vec<Stmt>> {
        self.expect(":")?;
        self.kind(Kind::Newline, "a newline after `:`")?;
        self.kind(Kind::Indent, "an indented block")?;
        let mut body = Vec::new();
        while !matches!(self.peek().kind, Kind::Dedent | Kind::Eof) {
            body.push(self.statement()?);
        }
        self.kind(Kind::Dedent, "the end of an indented block")?;
        if body.is_empty() {
            return Err(self.peek().error("empty block; use `pass`"));
        }
        Ok(body)
    }
    fn conditional_statement(&mut self) -> Result<Stmt> {
        let at = self.take(); // if or elif
        let condition = self.expr(0)?;
        let yes = self.suite()?;
        let no = if self.eat("else") {
            self.suite()?
        } else if self.peek().is("elif") {
            vec![self.conditional_statement()?]
        } else {
            Vec::new()
        };
        Ok(Stmt {
            at,
            kind: Statement::If { condition, yes, no },
        })
    }
    fn statement(&mut self) -> Result<Stmt> {
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
            let mut body = self.suite()?;
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
            let mut names = vec![self.name()?];
            while self.eat(",") {
                names.push(self.name()?);
            }
            self.expect("in")?;
            let iterable = self.expr(0)?;
            let body = self.suite()?;
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
                Some(self.expr(0)?)
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
            let assignment = mutable
                || matches!(&self.peek().kind, Kind::Word(_))
                    && self
                        .tokens
                        .get(self.pos + 1)
                        .is_some_and(|t| t.is("=") || t.is(":") || t.is(","));
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
                    let value = self.expr(0)?;
                    self.kind(Kind::Newline, "the end of the statement")?;
                    return Ok(Stmt {
                        at,
                        kind: Statement::Unpack {
                            names,
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
                    value: self.expr(0)?,
                }
            } else {
                let target = self.expr(0)?;
                if self.eat("=") {
                    Statement::SetIndex {
                        target,
                        value: self.expr(0)?,
                    }
                } else {
                    Statement::Expr(target)
                }
            }
        };
        self.kind(Kind::Newline, "the end of the statement")?;
        Ok(Stmt { at, kind })
    }
    fn parenthesized(&mut self) -> Result<Expression> {
        if self.eat(")") {
            return Ok(Expression::Unit);
        }
        let first = self.expr(0)?;
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
        Ok(Expression::Tuple(values, true))
    }
    fn expr(&mut self, min: u8) -> Result<Expr> {
        let at = self.take();
        let kind = match &at.kind {
            Kind::Number(n) => Expression::Number(n.clone()),
            Kind::Text(s) => Expression::Text(s.clone()),
            Kind::Word(s) if s == "True" || s == "False" => Expression::Bool(s == "True"),
            Kind::Word(s) if s == "not" => Expression::Unary(s.clone(), Box::new(self.expr(3)?)),
            Kind::Word(s) if s == "try" => {
                return Err(at.error("collection literals already return Result; remove `try` and use postfix `?` to propagate errors"));
            }
            Kind::Symbol(s) if s == "&" => {
                let op = if self.eat("mut") { "&mut" } else { "&" };
                Expression::Unary(op.into(), Box::new(self.expr(7)?))
            }
            Kind::Symbol(s) if s == "-" || s == "+" || s == "*" => {
                Expression::Unary(s.clone(), Box::new(self.expr(7)?))
            }
            Kind::Symbol(s) if s == "(" => self.parenthesized()?,
            Kind::Symbol(s) if s == "[" || s == "{" => self.collection_display(s)?,
            Kind::Word(s) if !reserved(s) && !s.starts_with("__plenty_") => {
                if matches!(s.as_str(), "Option" | "Result") && self.peek().is("[") {
                    self.pos -= 1;
                    Expression::Type(self.ty()?)
                } else if matches!(s.as_str(), "list" | "dict" | "set" | "range")
                    && self.peek().is("[")
                {
                    self.pos -= 1;
                    let ty = self.ty()?;
                    if self.peek().is(".") {
                        Expression::Type(ty)
                    } else {
                        self.expect("(")?;
                        Expression::Constructor(ty, self.arguments()?)
                    }
                } else if self.eat("(") {
                    let mut args = Vec::new();
                    while !self.eat(")") {
                        args.push(self.expr(0)?);
                        if self.eat(")") {
                            break;
                        }
                        self.expect(",")?;
                    }
                    Expression::Call(s.clone(), args)
                } else {
                    Expression::Name(s.clone())
                }
            }
            _ => return Err(at.error("expected an expression")),
        };
        let mut left = Expr { at, kind };
        loop {
            if self.peek().is("[") {
                fn path(e: &Expr) -> Option<String> {
                    match &e.kind {
                        Expression::Name(n) => Some(n.clone()),
                        Expression::Member(base, n) => Some(format!("{}.{n}", path(base)?)),
                        _ => None,
                    }
                }
                if let Some(name) = path(&left) {
                    let saved = self.pos;
                    if let Ok(types) = self.type_arguments() {
                        if self.eat("(") {
                            let args = self.arguments()?;
                            left.kind = Expression::GenericCall(name, types, args);
                            continue;
                        }
                    }
                    self.pos = saved;
                }
            }
            if self.peek().is("?") {
                let at = self.take();
                left = Expr {
                    at,
                    kind: Expression::Try(Box::new(left)),
                };
                continue;
            }
            if self.eat("[") {
                let index = self.expr(0)?;
                self.expect("]")?;
                left = Expr {
                    at: left.at.clone(),
                    kind: Expression::Index(Box::new(left), Box::new(index)),
                };
                continue;
            }
            if self.eat(".") {
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
                        kind: Expression::Method(Box::new(left), name, args),
                    }
                } else {
                    Expr {
                        at,
                        kind: Expression::Member(Box::new(left), name),
                    }
                };
                continue;
            }
            if min == 0 && self.eat("if") {
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
                self.peek().is("not") && self.tokens.get(self.pos + 1).is_some_and(|t| t.is("in"));
            let token = if not_in {
                Token {
                    kind: Kind::Word("not in".into()),
                    ..self.peek().clone()
                }
            } else {
                self.peek().clone()
            };
            let Some((op, prec)) = binary(&token) else {
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
        "File" => Ty::File,
        "range" => Ty::Range(Rc::new(Ty::I64)),
        "AllocError" => crate::sum::alloc_error(),
        "ParseError" => crate::sum::parse_error(),
        "DataError" => crate::sum::data_error(),
        "IoError" => crate::sum::io_error(),
        "Failure" => crate::sum::failure(),
        _ => return None,
    })
}
fn lookup_type(name: &str, aliases: &TypeAliases) -> Option<Type> {
    named_type(name)
        .map(Some)
        .or_else(|| aliases.get(name).cloned())
}

fn builtin(name: &str) -> bool {
    named_type(name).is_some()
        || matches!(
            name,
            "print"
                | "open"
                | "write_stdout"
                | "write_stderr"
                | "flush_stdout"
                | "flush_stderr"
                | "input"
                | "args"
                | "read_text"
                | "write_text"
                | "append_text"
                | "contains"
                | "list"
                | "dict"
                | "set"
                | "len"
                | "range"
                | "Ok"
                | "Err"
                | "Some"
                | "Nothing"
                | "Option"
                | "Result"
                | "Generator"
                | "tuple"
                | "IntType"
                | "next"
                | "copy"
                | "drop"
        )
}
fn reserved(name: &str) -> bool {
    matches!(
        name,
        "def"
            | "try"
            | "with"
            | "type"
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
            | "trait"
            | "protocol"
            | "break"
            | "continue"
            | "import"
            | "from"
            | "as"
            | "pub"
    )
}
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
fn binary(t: &Token) -> Option<(String, u8)> {
    let s = match &t.kind {
        Kind::Word(s) | Kind::Symbol(s) => s,
        _ => return None,
    };
    let precedence = match s.as_str() {
        "or" => 1,
        "and" => 2,
        "==" | "!=" | "<" | ">" | "<=" | ">=" | "in" | "not in" => 3,
        "+" | "-" => 4,
        "*" | "%" | "/" | "//" => 5,
        _ => return None,
    };
    Some((s.clone(), precedence))
}

#[derive(Clone)]
struct Local {
    slot: u8,
    ty: Ty,
    mutable: bool,
}

/// An exited branch has no value to unify with paths that continue.
enum BlockResult {
    Continues(Type),
    Exits,
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
    heap: &'a mut Heap,
    sigs: &'a mut HashMap<String, Rc<FnSig>>,
    generics: &'a mut generics::Engine,
    aliases: &'a TypeAliases,
    access: &'a modules::AccessMap,
    names: HashMap<String, Local>,
    locals: Vec<Ty>,
    parameters: usize,
    /// None at module scope; Some(None) for a unit-returning function.
    return_type: Option<Type>,
    /// Code executed before continuing the innermost loop (for-loop increment).
    loop_steps: Vec<Vec<Op>>,
    loop_scopes: Vec<usize>,
    yield_type: Type,
    loans: Vec<crate::ownership::Loan>,
    reference_locals: HashMap<u8, usize>,
    expression_temps: Vec<u8>,
    contexts: Vec<contexts::Context>,
    return_origin: Option<u8>,
    returned_fields: &'a mut HashMap<String, Vec<usize>>,
}
impl Lower<'_> {
    fn numeric_hint(&self, e: &Expr) -> Type {
        match &ungroup(e).kind {
            Expression::GenericCall(name, types, _) => self
                .generics
                .explicit_output(name, types, self.aliases)
                .filter(Ty::is_numeric),
            Expression::Number(n) if numeric_suffix(n) => {
                self.number(n, false, &e.at, &mut Vec::new()).ok()
            }
            Expression::Name(n) => self
                .names
                .get(n)
                .map(|l| l.ty.clone())
                .filter(Ty::is_numeric),
            Expression::Member(..) | Expression::Index(..) => {
                self.place_type(e).filter(Ty::is_numeric)
            }
            Expression::Call(n, _) => self
                .sigs
                .get(n)
                .and_then(|s| s.outputs.first().cloned())
                .or_else(|| lookup_type(n, self.aliases).flatten())
                .filter(Ty::is_numeric),
            Expression::Unary(op, inner) if op == "+" || op == "-" => self.numeric_hint(inner),
            Expression::Binary(op, a, b)
                if matches!(op.as_str(), "+" | "-" | "*" | "/" | "//" | "%") =>
            {
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
        let a = self.expr_expected(left, Some(ty.clone()), ops)?;
        self.same(a, Some(ty.clone()), &left.at)?;
        let b = self.expr_expected(right, Some(ty.clone()), ops)?;
        self.same(b, Some(ty.clone()), &right.at)?;
        if op == "/" && !ty.is_float() {
            return Err(at.error("use `//` for integer division; `/` requires floats"));
        }
        if matches!(op, "//" | "%") && ty.is_float() {
            return Err(at.error("floor division and modulo currently require integers"));
        }
        ops.push(match op {
            "+" => Op::Add,
            "-" => Op::Sub,
            "*" => Op::Mul,
            "/" => Op::Div,
            "//" => Op::FloorDiv,
            "%" => Op::Modulo,
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
            || matches!((&expected, &got), (Some(a), Some(b)) if crate::generator::refine(a, b).is_some())
        {
            Ok(())
        } else {
            Err(at.error(format!(
                "expected {}, got {}",
                type_name(expected),
                type_name(got)
            )))
        }
    }
    fn value(&mut self, e: &Expr, ops: &mut Vec<Op>) -> Result<Ty> {
        self.expr(e, ops)?
            .ok_or_else(|| e.at.error("expected a value, got ()"))
    }
    fn propagate(
        &mut self,
        e: &Expr,
        value: &Expr,
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
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
        let operand_context = expected.map(|ty| {
            if target.is_option()
                || matches!(&ungroup(value).kind, Expression::Call(name, _) if name == "Some")
            {
                crate::sum::option(ty)
            } else {
                crate::sum::result(ty, target.variants[1].fields[0].clone())
            }
        });
        let Some(Ty::Enum(source)) = self.expr_expected(value, operand_context, ops)? else {
            return Err(e.at.error("`?` requires a Result or Option operand"));
        };
        if !source.propagatable() || source.is_option() != target.is_option() {
            return Err(e.at.error(
                "`?` operand and function return must use the same Result or Option family",
            ));
        }
        if !source.is_option()
            && !target.discards_error()
            && source.variants[1].fields != target.variants[1].fields
        {
            return Err(e
                .at
                .error("`?` requires identical Result error types; convert the error explicitly"));
        }
        let success = usize::from(source.is_option());
        let payload = source.variants[success].fields[0].clone();
        let mut cleanup = Vec::new();
        if !self.contexts.is_empty() {
            self.cleanup(0, &mut cleanup);
        }
        ops.push(Op::Try {
            source,
            target,
            cleanup: cleanup.into(),
        });
        if payload == Ty::Unit {
            ops.push(Op::Drop);
            Ok(None)
        } else {
            Ok(Some(payload))
        }
    }
    fn expr(&mut self, e: &Expr, ops: &mut Vec<Op>) -> Result<Type> {
        let ty = match &e.kind {
            Expression::GenericCall(name, types, args) => {
                self.generic_call(name, Some(types), args, &e.at, ops)?
            }
            Expression::Try(value) => self.propagate(e, value, None, ops)?,
            Expression::Type(_) => {
                return Err(e.at.error("a type is not a value; select a variant"))
            }
            Expression::ClassNew(t) => {
                let op = crate::record::ClassOp::TryNew(t.clone());
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
            Expression::Member(base, name) => {
                if let Some(ty) = self.qualified_type(base)? {
                    self.variant(ty, name, None, &e.at, ops)?
                } else {
                    let (ty, loans) = self.field(e, ops)?;
                    if ty.affine() {
                        return Err(e.at.error("cannot move out of a field; use copy or borrow"));
                    }
                    Self::end_reads(loans, ops);
                    Some(ty)
                }
            }
            Expression::Tuple(values, fallible) => {
                Some(self.tuple(values, *fallible, None, &e.at, ops)?)
            }
            Expression::Collection { .. } => Some(self.fallible_display(e, None, ops)?),
            Expression::Index(base, index) => {
                let ty = self.index(base, index, ops)?;
                if ty == Ty::Unit {
                    ops.push(Op::Drop);
                    None
                } else {
                    Some(ty)
                }
            }
            Expression::Method(base, name, args) => self.method(base, name, args, ops)?,
            Expression::Constructor(ty, args) => {
                let ty = ty.resolve(self.aliases)?.unwrap();
                Some(self.construct(ty, args, &e.at, ops)?)
            }
            Expression::Number(n) => Some(self.number(n, false, &e.at, ops)?),
            Expression::Text(s) => {
                ops.push(Op::PushStr(self.heap.add_str(s.clone())));
                Some(Ty::Str)
            }
            Expression::Bool(b) => {
                ops.push(Op::PushBool(*b));
                Some(Ty::Bool)
            }
            Expression::Unit => None,
            Expression::Group(inner) => self.expr(inner, ops)?,
            Expression::Name(name) => {
                if enums::prelude_variant(name) && !self.names.contains_key(name) {
                    return self.prelude_constructor(name, None, None, &e.at, ops);
                }
                let local = self
                    .names
                    .get(name)
                    .ok_or_else(|| e.at.error(format!("unknown binding `{name}`")))?;
                if let Some(loan) = self.reference_locals.get(&local.slot) {
                    ops.push(Op::UseLoan(*loan));
                }
                if local.ty.affine() {
                    ops.push(Op::MoveLocal(
                        local.slot,
                        format!("{}:{}: `{name}`", e.at.line, e.at.column),
                    ));
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
                if op == "&" || op == "&mut" {
                    return self
                        .borrow(value, op == "&mut", ops)
                        .map(|(ty, _)| Some(ty));
                }
                if op == "*" {
                    let Ty::Ref(ty, _) = self.value(value, ops)? else {
                        return Err(value.at.error("dereference requires a reference"));
                    };
                    let loan = self.reference_origin(value, ops)?;
                    if ty.affine() {
                        return Err(e
                            .at
                            .error("cannot move out of a reference; use copy or borrow"));
                    }
                    ops.push(Op::Access(self.loans[loan].root, false, Some(loan)));
                    ops.push(Op::ReadRef((*ty).clone()));
                    ops.push(Op::UseLoan(loan));
                    return Ok(Some((*ty).clone()));
                }
                if op == "-" {
                    if let Expression::Number(n) = &value.kind {
                        return self.number(n, true, &e.at, ops).map(Some);
                    }
                }
                let mut body = Vec::new();
                let ty = self.value(value, &mut body)?;
                if op == "not" {
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
                        ops.push(Op::PushInt(integer(&format!("0{ty}"), false, &e.at)?));
                    }
                    ops.extend(body);
                    if op == "-" {
                        ops.push(Op::Sub);
                    }
                }
                Some(ty.clone())
            }
            Expression::Binary(op, left, right) if op == "in" || op == "not in" => {
                let (a, mut loans) = self.observe(left, ops)?;
                let (b, other_loans) = self.observe(right, ops)?;
                loans.extend(other_loans);
                if b.restricted_storage() {
                    return Err(e
                        .at
                        .error("membership does not consume generators; use a loop"));
                }
                let element = b
                    .element()
                    .ok_or_else(|| e.at.error("membership requires an iterable"))?;
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
                if op != "and" && op != "or" {
                    if let Some(hint) = self.numeric_hint(left).or_else(|| self.numeric_hint(right))
                    {
                        return self
                            .numeric_binary(op, left, right, hint, &e.at, ops)
                            .map(Some);
                    }
                }
                let (a, mut loans) = self.observe(left, ops)?;
                let mut rhs = Vec::new();
                let (b, other_loans) = if a.is_numeric() {
                    (
                        self.expr_expected(right, Some(a.clone()), &mut rhs)?
                            .ok_or_else(|| right.at.error("expected a numeric value"))?,
                        vec![],
                    )
                } else {
                    self.observe(right, &mut rhs)?
                };
                if op == "and" || op == "or" {
                    Self::end_reads(other_loans, &mut rhs);
                } else {
                    loans.extend(other_loans);
                }
                if a.restricted_storage() {
                    return Err(e.at.error("generators do not support binary operators"));
                }
                self.same(Some(b.clone()), Some(a.clone()), &e.at)?;
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
                        && !(matches!(op.as_str(), "==" | "!=") || (op == "+" && a == Ty::Str))
                    {
                        return Err(e.at.error(format!(
                            "operator `{op}` does not accept {}",
                            type_name(Some(a.clone()))
                        )));
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
                    if op == "+" && a == Ty::Str {
                        ops.push(Op::Collection(CollectionOp::TextTryConcat));
                        Self::end_reads(loans, ops);
                        return Ok(Some(crate::sum::result(Ty::Str, crate::sum::alloc_error())));
                    }
                    ops.push(match op.as_str() {
                        "+" => Op::Add,
                        "-" => Op::Sub,
                        "*" => Op::Mul,
                        "/" => Op::Div,
                        "//" => Op::FloorDiv,
                        "%" => Op::Modulo,
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
                if name == "print" {
                    if self.names.contains_key(name) {
                        return Err(e.at.error(format!("binding `{name}` is not callable")));
                    }
                    if args.len() != 1 {
                        return Err(e.at.error("print takes one argument"));
                    }
                    let (ty, loans) = self.observe(&args[0], ops)?;
                    if ty.restricted_storage() {
                        return Err(e.at.error("generators cannot be printed"));
                    }
                    let operation = CollectionOp::TryPrint(ty);
                    let output = operation.signature().1;
                    ops.push(Op::Collection(operation));
                    Self::end_reads(loans, ops);
                    return Ok(Some(output));
                }
                if name == "open" {
                    return self.open_file(args, &e.at, ops);
                }
                if self.names.contains_key(name) {
                    return Err(e.at.error(format!("binding `{name}` is not callable")));
                }
                if enums::prelude_variant(name) {
                    return self.prelude_constructor(name, Some(args), None, &e.at, ops);
                }
                if matches!(name.as_str(), "copy" | "drop") {
                    return self.copy_or_drop(name, args, &e.at, ops);
                }
                if name == "next" {
                    return self.next(args, &e.at, ops);
                }
                if let Some(operation) = match name.as_str() {
                    "write_stdout" => Some(CollectionOp::WriteStdout),
                    "write_stderr" => Some(CollectionOp::WriteStderr),
                    "flush_stdout" => Some(CollectionOp::FlushStdout),
                    "flush_stderr" => Some(CollectionOp::FlushStderr),
                    "input" => Some(CollectionOp::Input),
                    "args" => Some(CollectionOp::Args),
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
                    if t.depth >= 64 {
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
                if let Some(target) = lookup_type(name, self.aliases) {
                    if let Some(target) = target.as_ref().filter(|t| t.is_collection()) {
                        return self.construct(target.clone(), args, &e.at, ops).map(Some);
                    }
                    let target = target
                        .filter(|ty| ty.is_numeric())
                        .ok_or_else(|| e.at.error("only numeric types support cast syntax"))?;
                    if args.len() != 1 {
                        return Err(e.at.error("numeric casts take one argument"));
                    }
                    let source = self.value(&args[0], ops)?;
                    if !source.is_numeric() {
                        return Err(e.at.error("numeric casts require a number"));
                    }
                    ops.push(Op::Cast(target.clone()));
                    Some(target)
                } else if name == "contains" {
                    if args.len() != 2 {
                        return Err(e.at.error("contains takes two strings"));
                    }
                    for arg in args {
                        let ty = self.expr(arg, ops)?;
                        self.same(ty, Some(Ty::Str), &arg.at)?;
                    }
                    ops.push(Op::Contains);
                    Some(Ty::Bool)
                } else {
                    self.call_named(name, args, &e.at, ops)?
                }
            }
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
        if self.generics.templates.contains_key(name) {
            return self.generic_call(name, None, args, at, ops);
        }
        if self
            .sigs
            .get(name)
            .is_some_and(|s| s.inputs.iter().any(|(_, t)| t.unresolved_generator()))
        {
            return self.frame_call(name, args, at, ops);
        }
        self.ensure_concrete_output(name, at)?;
        let sig = self
            .sigs
            .get(name)
            .ok_or_else(|| at.error(format!("unknown function `{name}`")))?
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
        let argument_loans = self.call_arguments(args, &sig.inputs, ops)?;
        ops.push(Op::Call(name.to_owned()));
        self.call_reference_result(name, &sig, &argument_loans, ops);
        Self::end_reads(argument_loans, ops);
        Ok(sig.outputs.first().cloned())
    }
    fn call_borrow(&mut self, arg: &Expr, mutable: bool, ops: &mut Vec<Op>) -> Result<(Ty, usize)> {
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
            _ => {
                return Err(arg
                    .at
                    .error("reference arguments require explicit & or &mut borrowing"))
            }
        };
        self.borrow(base, mutable, ops)
    }
    fn call_arguments(
        &mut self,
        args: &[Expr],
        inputs: &[(String, Ty)],
        ops: &mut Vec<Op>,
    ) -> Result<Vec<usize>> {
        let mut argument_loans = Vec::new();
        for (arg, (_, expected)) in args.iter().zip(inputs) {
            if let Ty::Ref(_, mutable) = expected {
                let (ty, loan) = self.call_borrow(arg, *mutable, ops)?;
                self.same(Some(ty), Some(expected.clone()), &arg.at)?;
                argument_loans.push(loan);
                continue;
            }
            let ty = self.expr_expected(arg, Some(expected.clone()), ops)?;
            self.same(ty, Some(expected.clone()), &arg.at)?;
        }

        Ok(argument_loans)
    }
    fn block(&mut self, body: &[Stmt], ops: &mut Vec<Op>, tail: bool) -> Result<BlockResult> {
        let start = self.locals.len();
        let result = self.block_inner(body, ops, tail)?;
        if !tail && matches!(result, BlockResult::Continues(_)) {
            self.cleanup(start, ops);
        }
        Ok(result)
    }
    fn block_inner(&mut self, body: &[Stmt], ops: &mut Vec<Op>, tail: bool) -> Result<BlockResult> {
        let mut result = None;
        for (i, stmt) in body.iter().enumerate() {
            let temporary_start = self.expression_temps.len();
            let last = tail && i + 1 == body.len();
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
                    let actual = self.expr_expected(e, Some(expected.clone()), ops)?;
                    self.same(actual, Some(expected.clone()), &e.at)?;
                    self.finish_temporaries(temporary_start, ops);
                    ops.push(Op::Yield(expected));
                    None
                }
                Statement::Match { value, cases } => {
                    match self.match_cases(value, cases, last, ops)? {
                        BlockResult::Continues(ty) => ty,
                        BlockResult::Exits => return exited_block(body, i),
                    }
                }
                Statement::Expr(e) => {
                    let ty = self.expr_expected(
                        e,
                        if last {
                            self.return_type.clone().flatten()
                        } else {
                            None
                        },
                        ops,
                    )?;
                    if last && self.return_origin.is_some() && matches!(ty, Some(Ty::Ref(..))) {
                        let loan = self.check_return_reference(e, ops)?;
                        ops.push(Op::UseLoan(loan));
                    }
                    if last && self.yield_type.is_none() {
                        self.refine_return(ty.clone(), &e.at)?;
                    }
                    ty
                }
                Statement::Return(e) => {
                    if self.yield_type.is_some() && e.is_some() {
                        return Err(stmt.at.error("a generator may only use bare return"));
                    }
                    let expected = self
                        .return_type
                        .clone()
                        .ok_or_else(|| stmt.at.error("return outside a function"))?;
                    let mut returned = Vec::new();
                    let ty = match e {
                        Some(e) => self.expr_expected(e, expected.clone(), &mut returned)?,
                        None => None,
                    };
                    self.refine_return(ty, &stmt.at)?;
                    let returned_loan = if self.return_origin.is_some() {
                        Some(self.check_return_reference(e.as_ref().unwrap(), &returned)?)
                    } else {
                        None
                    };
                    self.finish_temporaries(temporary_start, &mut returned);
                    if self.contexts.is_empty() && returned_loan.is_none() {
                        finish_return(&mut returned);
                    } else {
                        self.cleanup(0, &mut returned);
                        if let Some(loan) = returned_loan {
                            returned.push(Op::UseLoan(loan));
                        }
                        returned.push(Op::Return);
                    }
                    ops.extend(returned);
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
                    self.cleanup(*self.loop_scopes.last().unwrap(), ops);
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
                    self.finish_temporaries(temporary_start, &mut test);
                    let saved = self.names.clone();
                    self.loop_steps.push(Vec::new());
                    self.loop_scopes.push(self.locals.len());
                    let mut lowered = Vec::new();
                    let result = self.block(body, &mut lowered, false);
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
                    if matches!(&target.kind, Expression::Unary(op, _) if op == "*") {
                        self.write_reference(target, value, ops)?;
                    } else if matches!(&target.kind, Expression::Member(..)) {
                        self.set_field(target, value, ops)?;
                    } else {
                        self.set_index(target, value, ops)?;
                    }
                    None
                }
                Statement::Unpack {
                    names,
                    mutable,
                    value,
                } => {
                    self.unpack_tuple(names, *mutable, value, ops)?;
                    None
                }
                Statement::Assign {
                    name,
                    mutable,
                    annotation,
                    value,
                } => {
                    if self
                        .names
                        .get(name)
                        .is_some_and(|l| matches!(l.ty, Ty::Ref(..)))
                    {
                        return Err(stmt.at.error(
                            "reference bindings cannot be reassigned; create a new borrow",
                        ));
                    }
                    let context = if let Some(ann) = annotation {
                        ann.resolve(self.aliases)?
                    } else {
                        self.names.get(name).map(|l| l.ty.clone())
                    };
                    let ty = self
                        .expr_expected(value, context, ops)?
                        .ok_or_else(|| value.at.error("expected a value, got ()"))?;
                    if let Some(expected) = annotation {
                        let expected = expected.resolve(self.aliases)?.ok_or_else(|| {
                            expected.at.error("unit bindings are not supported yet")
                        })?;
                        self.same(Some(ty.clone()), Some(expected), &stmt.at)?;
                    }
                    let reference = matches!(ty, Ty::Ref(..));
                    if reference
                        && (*mutable
                            || !matches!(&ungroup(value).kind, Expression::Unary(op, _) if op == "&" || op == "&mut")
                                && !matches!(
                                    &ungroup(value).kind,
                                    Expression::Call(..)
                                        | Expression::GenericCall(..)
                                        | Expression::Method(..)
                                ))
                    {
                        return Err(stmt.at.error(
                            "reference bindings require a direct borrow or reference-returning call and cannot be mut",
                        ));
                    }
                    let slot = if let Some(local) = self.names.get(name) {
                        if *mutable || annotation.is_some() {
                            return Err(stmt.at.error(format!("duplicate binding `{name}`")));
                        }
                        if !local.mutable {
                            return Err(stmt
                                .at
                                .error(format!("`{name}` is immutable; declare it with `mut`")));
                        }
                        self.same(Some(ty.clone()), Some(local.ty.clone()), &stmt.at)?;
                        local.slot
                    } else {
                        let slot =
                            u8::try_from(self.parameters + self.locals.len()).map_err(|_| {
                                stmt.at
                                    .error("at most 256 parameter/local slots are supported")
                            })?;
                        self.locals.push(ty.clone());
                        self.names.insert(
                            name.clone(),
                            Local {
                                slot,
                                ty,
                                mutable: *mutable,
                            },
                        );
                        slot
                    };
                    ops.push(Op::StoreLocal(slot));
                    if reference {
                        let origin = self.reference_origin(value, ops)?;
                        self.reference_locals.insert(slot, origin);
                    }
                    None
                }
                Statement::If { condition, yes, no } => {
                    let cond = self.expr(condition, ops)?;
                    self.same(cond, Some(Ty::Bool), &condition.at)?;
                    self.finish_temporaries(temporary_start, ops);
                    let saved = self.names.clone();
                    let (mut a, mut b) = (Vec::new(), Vec::new());
                    let yes_result = self.block(yes, &mut a, last)?;
                    self.names = saved.clone();
                    let no_result = self.block(no, &mut b, last)?;
                    self.names = saved;
                    ops.push(branch(a, b));
                    match (yes_result, no_result) {
                        (BlockResult::Continues(ty), BlockResult::Continues(other)) => {
                            self.same(other, ty.clone(), &stmt.at)?;
                            ty
                        }
                        (BlockResult::Continues(ty), BlockResult::Exits)
                        | (BlockResult::Exits, BlockResult::Continues(ty)) => ty,
                        (BlockResult::Exits, BlockResult::Exits) => {
                            return exited_block(body, i);
                        }
                    }
                }
            };
            if !last {
                if result.is_some() {
                    ops.push(Op::Drop);
                }
                result = None;
            }
            self.finish_temporaries(temporary_start, ops);
        }
        Ok(BlockResult::Continues(result))
    }
}

fn ungroup(mut e: &Expr) -> &Expr {
    while let Expression::Group(inner) = &e.kind {
        e = inner;
    }
    e
}

fn type_name(ty: Type) -> String {
    match ty {
        None => "()".into(),
        Some(Ty::Bool) => "bool".into(),
        Some(Ty::Str) => "str".into(),
        Some(t) => t.to_string(),
    }
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

/// Return expressions are tail positions even inside a guard branch. Make
/// every path exit, using a tail call where the last operation is a call.
fn finish_return(body: &mut Vec<Op>) {
    match body.last_mut() {
        Some(last @ Op::Call(_)) => {
            let Op::Call(name) = last else { unreachable!() };
            *last = Op::TailCall(std::mem::take(name));
        }
        Some(Op::Match(arms)) => {
            for arm in Rc::make_mut(arms) {
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
    let magnitude = digits
        .replace('_', "")
        .parse::<i128>()
        .map_err(|_| at.error("integer literal out of range"))?;
    let number = if negative { -magnitude } else { magnitude };
    let ty = if suffix.is_empty() {
        Ty::I64
    } else {
        named_type(suffix)
            .filter(|t| t.is_int())
            .ok_or_else(|| at.error("invalid integer suffix"))?
    };
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

pub(crate) fn compile(source: &str, heap: &mut Heap) -> Result<Program> {
    lower(modules::single(source)?, heap)
}

pub(crate) fn compile_file(
    path: &std::path::Path,
    root: Option<&std::path::Path>,
    require_main: bool,
    heap: &mut Heap,
) -> Result<Program> {
    lower(modules::load(path, root, require_main)?, heap)
}

fn register_signature(
    f: &Function,
    aliases: &TypeAliases,
    sigs: &mut HashMap<String, Rc<FnSig>>,
    returned_fields: &mut HashMap<String, Vec<usize>>,
) -> Result<()> {
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
    if sigs.contains_key(&f.name) {
        return Err(f.at.error(format!(
            "function `{}` is already defined; redefinition is not supported",
            f.name
        )));
    }
    let mut inputs = Vec::with_capacity(f.inputs.len());
    for (name, ty) in &f.inputs {
        let resolved = ty
            .resolve(aliases)?
            .ok_or_else(|| ty.at.error("unit parameters are not supported yet"))?;
        inputs.push((name.clone(), resolved));
    }
    let mut output = f.output.resolve(aliases)?;
    if let Some(Ty::Ref(_, mutable)) = &output {
        let references: Vec<_> = inputs
            .iter()
            .filter(|(_, ty)| matches!(ty, Ty::Ref(..)))
            .collect();
        if references.len() != 1 {
            return Err(f
                .at
                .error("returned references require exactly one reference parameter"));
        }
        if *mutable && !matches!(&references[0].1, Ty::Ref(_, true)) {
            return Err(f
                .at
                .error("mutable returned references require a mutable reference parameter"));
        }
    }
    if generators::yields(&f.body) {
        if let Some(Ty::Generator(t)) = &output {
            output = Some(crate::generator::ty(
                t.element.clone(),
                Some(f.name.clone()),
            ));
        }
    }
    let outputs = output.into_iter().collect();
    sigs.insert(f.name.clone(), Rc::new(FnSig { inputs, outputs }));

    if let Some(fields) = references::returned_fields(f, &sigs[&f.name]) {
        returned_fields.insert(f.name.clone(), fields);
    }
    Ok(())
}

fn lower_function(
    f: Function,
    heap: &mut Heap,
    sigs: &mut HashMap<String, Rc<FnSig>>,
    generics: &mut generics::Engine,
    aliases: &TypeAliases,
    access: &modules::AccessMap,
    returned_fields: &mut HashMap<String, Vec<usize>>,
) -> Result<()> {
    if generics.completed.contains(&f.name) {
        return Ok(());
    }
    if !generics.active.insert(f.name.clone()) {
        return Err(f.at.error("recursive generator factory requires a concrete return type; recursive inline frames are not supported"));
    }
    let mut sig = Rc::clone(&sigs[&f.name]);
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
        returned_fields,
        heap,
        sigs,
        generics,
        aliases,
        access,
        names: HashMap::new(),
        locals: Vec::new(),
        parameters: sig.inputs.len(),
        return_type: Some(if yield_type.is_some() {
            None
        } else {
            sig.outputs.first().cloned()
        }),
        loop_steps: Vec::new(),
        loop_scopes: Vec::new(),
        yield_type: yield_type.clone(),
        loans: Vec::new(),
        reference_locals: HashMap::new(),
        expression_temps: Vec::new(),
        contexts: Vec::new(),
        return_origin: if matches!(sig.outputs.first(), Some(Ty::Ref(..))) {
            sig.inputs
                .iter()
                .position(|(_, ty)| matches!(ty, Ty::Ref(..)))
                .map(|i| i as u8)
        } else {
            None
        },
    };
    for (i, (name, ty)) in sig.inputs.iter().enumerate() {
        lower.names.insert(
            name.clone(),
            Local {
                slot: i as u8,
                ty: ty.clone(),
                mutable: false,
            },
        );
    }
    let mut body = Vec::new();
    for (i, (_, ty)) in sig.inputs.iter().enumerate() {
        if let Ty::Ref(_, mutable) = ty {
            if yield_type.is_some() {
                return Err(f.at.error("generators cannot capture references"));
            }
            let loan = lower.new_loan(i as u8, *mutable, None, &mut body);
            lower.reference_locals.insert(i as u8, loan);
        }
    }
    if let BlockResult::Continues(output) = lower.block(&f.body, &mut body, yield_type.is_none())? {
        lower.refine_return(output, &f.at)?;
    }
    if yield_type.is_none() {
        let output = lower.return_type.clone().flatten();
        if output.as_ref().is_some_and(Ty::unresolved_generator) {
            return Err(f
                .at
                .error("cannot infer the concrete generator returned by this function"));
        }
        let inferred_frame = sig.outputs.iter().any(Ty::unresolved_generator);
        sig = Rc::new(FnSig {
            inputs: sig.inputs.clone(),
            outputs: output.into_iter().collect(),
        });
        lower.sigs.insert(f.name.clone(), sig.clone());
        if inferred_frame {
            // Check empty variants and propagation paths again with the resolved
            // return context. Calls/specializations are cached across this pass.
            drop(lower);
            generics.active.remove(&f.name);
            return lower_function(f, heap, sigs, generics, aliases, access, returned_fields);
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
    if yield_type.is_none() {
        mark_tail_calls(&mut body);
    }
    if sig.inputs.iter().any(|(_, t)| t.has_destructor())
        || lower.locals.iter().any(Ty::has_destructor)
    {
        classes::preserve_drop_order(&mut body);
    }
    let compiled = CompiledFn {
        location: f
            .at
            .source
            .as_ref()
            .map(|source| format!("{source}:{}:{}", f.at.line, f.at.column).into()),
        generator: yield_type,
        sig,
        doc: f.doc.into(),
        body: body.into(),
        locals: lower.locals.into(),
    };
    generics.active.remove(&f.name);
    generics.completed.insert(f.name.clone());
    generics.compiled.push(Op::DefineFn(f.name, compiled));
    Ok(())
}

fn lower(resolved: modules::Resolved, heap: &mut Heap) -> Result<Program> {
    let modules::Resolved {
        mut functions,
        declarations,
        enums,
        classes,
        protocols,
        access,
        public_api,
        at,
        require_main,
    } = resolved;
    let aliases = enums::resolve_types(&declarations, &enums, &classes)?;
    modules::check_api(&public_api, &aliases, &access)?;
    functions.extend(classes::expand(classes, &aliases)?);
    let mut generics = generics::prepare(functions, &aliases, protocols)?;
    let mut sigs = HashMap::new();
    let mut returned_fields = HashMap::new();
    for f in &generics.pending {
        register_signature(f, &aliases, &mut sigs, &mut returned_fields)?;
    }
    let returns_status = if require_main {
        let entry = generics
            .pending
            .iter()
            .find(|f| f.name == "main")
            .ok_or_else(|| {
                at.error("binary application requires `def main() -> ()` or `def main() -> i32`")
            })?;
        let entry_sig = &sigs["main"];
        if !entry_sig.inputs.is_empty()
            || !matches!(entry_sig.outputs.as_slice(), [] | [Ty::I32])
                && !matches!(entry_sig.outputs.as_slice(), [Ty::Enum(t)] if t.propagatable() && !t.is_option() && matches!(t.variants[0].fields.as_slice(), [Ty::Unit | Ty::I32]))
            || generators::yields(&entry.body)
        {
            return Err(entry.at.error(
                "main must take no parameters and return (), i32, Result[(), E], or Result[i32, E]; it cannot be a generator",
            ));
        }
        !entry_sig.outputs.is_empty()
    } else {
        false
    };
    generics.pending.retain(|f| {
        !sigs[&f.name]
            .inputs
            .iter()
            .any(|(_, t)| t.unresolved_generator())
    });
    while let Some(f) = generics.pending.pop_front() {
        lower_function(
            f,
            heap,
            &mut sigs,
            &mut generics,
            &aliases,
            &access,
            &mut returned_fields,
        )?;
    }
    let mut ops = std::mem::take(&mut generics.compiled);
    for op in &ops {
        if let Op::DefineFn(_, f) = op {
            let mut frame_bytes = 0usize;
            for ty in f
                .sig
                .inputs
                .iter()
                .map(|(_, t)| t)
                .chain(&f.sig.outputs)
                .chain(f.locals.iter())
            {
                let bytes = crate::generator::layout(ty, &mut Vec::new())
                    .map_err(|message| at.error(message))?;
                frame_bytes = frame_bytes
                    .checked_add(16 + bytes)
                    .filter(|n| *n <= i32::MAX as usize)
                    .ok_or_else(|| {
                        at.error("inline values exceed the native stack-layout limit")
                    })?;
            }
            crate::generator::validate_ops(&f.body).map_err(|message| at.error(message))?;
        }
    }
    if require_main {
        ops.push(Op::Call("main".into()));
        if let [Ty::Enum(t)] = sigs["main"].outputs.as_slice() {
            // Entry failures map to exit status 1 without allocating a diagnostic.
            ops.extend([
                Op::Dup,
                Op::Enum(crate::sum::EnumOp::Tag(t.clone())),
                Op::PushInt(Value::I64(0)),
                Op::Eq,
                branch(
                    if t.variants[0].fields[0] == Ty::I32 {
                        vec![Op::Enum(crate::sum::EnumOp::Take(t.clone(), 0, 0))]
                    } else {
                        vec![Op::Drop, Op::PushInt(Value::I32(0))]
                    },
                    vec![Op::Drop, Op::PushInt(Value::I32(1))],
                ),
            ]);
        }
    }
    Ok(Program {
        ops,
        returns_status,
    })
}
