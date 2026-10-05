//! Modern Plenty frontend: indentation → syntax tree → typed operations.
//!
//! Signatures are collected before bodies. Inference is local, expressions
//! have zero (unit) or one result, and no backend participates in inference.
use std::collections::HashMap;
use std::error::Error;
use std::rc::Rc;

use crate::op::{mark_tail_calls, CompiledFn, FnSig, MatchArm, Op, Pattern, Ty};
use crate::value::{Heap, Value};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Type = Option<Ty>; // Unit has no runtime representation in this milestone.
pub(crate) const ENTRY: &str = "__plenty_entry";

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
}

impl Token {
    fn is(&self, text: &str) -> bool {
        matches!(&self.kind, Kind::Word(s) | Kind::Symbol(s) if s == text)
    }

    fn error(&self, message: impl std::fmt::Display) -> Box<dyn Error> {
        format!("{}:{}: {message}", self.line, self.column).into()
    }
}

fn lex(source: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = source.chars().collect();
    let (mut pos, mut line, mut column) = (0, 1, 1);
    let mut line_start = true;
    let mut indents = vec![0];
    let mut parens = 0usize;
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
                    });
                } else {
                    while spaces < *indents.last().unwrap() {
                        indents.pop();
                        out.push(Token {
                            kind: Kind::Dedent,
                            line,
                            column,
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
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            '\\' => '\\',
                            '\'' => '\'',
                            '"' => '"',
                            _ => return Err(start.error(format!("unsupported escape \\{escaped}"))),
                        });
                    } else {
                        if ch == '\0' {
                            return Err(start.error("NUL bytes are not supported in strings yet"));
                        }
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
            c if c.is_ascii_digit() => {
                let begin = pos;
                while chars
                    .get(pos)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
                {
                    pos += 1;
                    column += 1;
                }
                out.push(Token {
                    kind: Kind::Number(chars[begin..pos].iter().collect()),
                    ..start
                });
            }
            '(' | ')' | ':' | ',' | '+' | '-' | '*' | '/' | '=' | '!' | '<' | '>' => {
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
                if c == '(' {
                    parens += 1;
                }
                if c == ')' {
                    parens = parens
                        .checked_sub(1)
                        .ok_or_else(|| start.error("unmatched `)`"))?;
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
        });
    }
    for _ in 1..indents.len() {
        out.push(Token {
            kind: Kind::Dedent,
            line,
            column,
        });
    }
    out.push(Token {
        kind: Kind::Eof,
        line,
        column,
    });
    Ok(out)
}

struct Function {
    name: String,
    at: Token,
    sig: FnSig,
    doc: String,
    body: Vec<Stmt>,
}

struct Stmt {
    at: Token,
    kind: Statement,
}
enum Statement {
    Expr(Expr),
    Assign {
        name: String,
        mutable: bool,
        annotation: Type,
        value: Expr,
    },
    Return(Option<Expr>),
    If {
        condition: Expr,
        yes: Vec<Stmt>,
        no: Vec<Stmt>,
    },
    Pass,
}

struct Expr {
    at: Token,
    kind: Expression,
}
enum Expression {
    Number(String),
    Text(String),
    Bool(bool),
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

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
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
    fn ty(&mut self) -> Result<Type> {
        if self.eat("(") {
            self.expect(")")?;
            return Ok(None);
        }
        let t = self.take();
        if let Kind::Word(ref name) = t.kind {
            if let Some(ty) = named_type(name) {
                return Ok(Some(ty));
            }
        }
        Err(t.error("expected a type: i8…i64, u8…u64, int, bool, str, or ()"))
    }
    fn function(&mut self) -> Result<Function> {
        let at = self.take(); // def
        let name = self.name()?;
        if builtin(&name) {
            return Err(at.error("cannot redefine a builtin"));
        }
        self.expect("(")?;
        let mut inputs = Vec::new();
        while !self.eat(")") {
            let param = self.name()?;
            if inputs.iter().any(|(n, _)| n == &param) {
                return Err(self.peek().error("duplicate parameter"));
            }
            self.expect(":")?;
            let ty = self
                .ty()?
                .ok_or_else(|| at.error("unit parameters are not supported yet"))?;
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
            name,
            at,
            sig: FnSig {
                inputs,
                outputs: output.into_iter().collect(),
            },
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
        if self.peek().is("if") {
            return self.conditional_statement();
        }
        let at = self.peek().clone();
        let kind = if self.eat("return") {
            Statement::Return(if self.peek().kind == Kind::Newline {
                None
            } else {
                Some(self.expr(0)?)
            })
        } else if self.eat("pass") {
            Statement::Pass
        } else {
            let mutable = self.eat("mut");
            let assignment = mutable
                || matches!(&self.peek().kind, Kind::Word(_))
                    && self
                        .tokens
                        .get(self.pos + 1)
                        .is_some_and(|t| t.is("=") || t.is(":"));
            if assignment {
                let name = self.name()?;
                let annotation = if self.eat(":") {
                    Some(
                        self.ty()?
                            .ok_or_else(|| at.error("unit bindings are not supported yet"))?,
                    )
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
                Statement::Expr(self.expr(0)?)
            }
        };
        self.kind(Kind::Newline, "the end of the statement")?;
        Ok(Stmt { at, kind })
    }
    fn expr(&mut self, min: u8) -> Result<Expr> {
        let at = self.take();
        let kind = match &at.kind {
            Kind::Number(n) => Expression::Number(n.clone()),
            Kind::Text(s) => Expression::Text(s.clone()),
            Kind::Word(s) if s == "True" || s == "False" => Expression::Bool(s == "True"),
            Kind::Word(s) if s == "not" => Expression::Unary(s.clone(), Box::new(self.expr(3)?)),
            Kind::Symbol(s) if s == "-" || s == "+" => {
                Expression::Unary(s.clone(), Box::new(self.expr(7)?))
            }
            Kind::Symbol(s) if s == "(" => {
                if self.eat(")") {
                    Expression::Unit
                } else {
                    let e = self.expr(0)?;
                    self.expect(")")?;
                    Expression::Group(Box::new(e))
                }
            }
            Kind::Word(s) if !reserved(s) && !s.starts_with("__plenty_") => {
                if self.eat("(") {
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
            let Some((op, prec)) = binary(self.peek()) else {
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
        "i64" | "int" => Ty::I64,
        "u8" => Ty::U8,
        "u16" => Ty::U16,
        "u32" => Ty::U32,
        "u64" => Ty::U64,
        "bool" => Ty::Bool,
        "str" => Ty::Str,
        _ => return None,
    })
}
fn builtin(name: &str) -> bool {
    named_type(name).is_some() || matches!(name, "print" | "contains")
}
fn reserved(name: &str) -> bool {
    matches!(
        name,
        "def"
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
            | "break"
            | "continue"
            | "import"
    )
}
fn is_comparison(op: &str) -> bool {
    matches!(op, "==" | "!=" | "<" | ">" | "<=" | ">=")
}
fn binary(t: &Token) -> Option<(String, u8)> {
    let s = match &t.kind {
        Kind::Word(s) | Kind::Symbol(s) => s,
        _ => return None,
    };
    let precedence = match s.as_str() {
        "or" => 1,
        "and" => 2,
        "==" | "!=" | "<" | ">" | "<=" | ">=" => 3,
        "+" | "-" => 4,
        "*" | "/" | "//" => 5,
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
struct Lower<'a> {
    heap: &'a mut Heap,
    sigs: &'a HashMap<String, Rc<FnSig>>,
    names: HashMap<String, Local>,
    locals: Vec<Ty>,
    parameters: usize,
}
impl Lower<'_> {
    fn same(&self, got: Type, expected: Type, at: &Token) -> Result<()> {
        if got == expected {
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
    fn expr(&mut self, e: &Expr, ops: &mut Vec<Op>) -> Result<Type> {
        let ty = match &e.kind {
            Expression::Number(n) => {
                let v = integer(n, false, &e.at)?;
                ops.push(Op::PushInt(v));
                Some(Ty::from(v))
            }
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
                let local = self
                    .names
                    .get(name)
                    .ok_or_else(|| e.at.error(format!("unknown binding `{name}`")))?;
                ops.push(Op::LoadLocal(local.slot));
                Some(local.ty)
            }
            Expression::Unary(op, value) => {
                if op == "-" {
                    if let Expression::Number(n) = &value.kind {
                        let v = integer(n, true, &e.at)?;
                        ops.push(Op::PushInt(v));
                        return Ok(Some(Ty::from(v)));
                    }
                }
                let mut body = Vec::new();
                let ty = self.value(value, &mut body)?;
                if op == "not" {
                    self.same(Some(ty), Some(Ty::Bool), &e.at)?;
                    ops.extend(body);
                    ops.push(Op::Not);
                } else {
                    if !ty.is_int() {
                        return Err(e.at.error("unary arithmetic requires an integer"));
                    }
                    if op == "-" {
                        ops.push(Op::PushInt(integer(&format!("0{ty}"), false, &e.at)?));
                    }
                    ops.extend(body);
                    if op == "-" {
                        ops.push(Op::Sub);
                    }
                }
                Some(ty)
            }
            Expression::Binary(op, left, right) => {
                let a = self.value(left, ops)?;
                let mut rhs = Vec::new();
                let b = self.value(right, &mut rhs)?;
                self.same(Some(b), Some(a), &e.at)?;
                if op == "and" || op == "or" {
                    self.same(Some(a), Some(Ty::Bool), &e.at)?;
                    let constant = vec![Op::PushBool(op == "or")];
                    let (yes, no) = if op == "and" {
                        (rhs, constant)
                    } else {
                        (constant, rhs)
                    };
                    ops.push(branch(yes, no));
                    Some(Ty::Bool)
                } else {
                    if !a.is_int()
                        && !(matches!(op.as_str(), "==" | "!=") || (op == "+" && a == Ty::Str))
                    {
                        return Err(e.at.error(format!(
                            "operator `{op}` does not accept {}",
                            type_name(Some(a))
                        )));
                    }
                    if op == "/" {
                        return Err(e.at.error(
                            "use `//` for integer division; floating-point `/` is not implemented",
                        ));
                    }
                    ops.extend(rhs);
                    ops.push(match op.as_str() {
                        "+" => Op::Add,
                        "-" => Op::Sub,
                        "*" => Op::Mul,
                        "//" => Op::FloorDiv,
                        "==" => Op::Eq,
                        "!=" => Op::Ne,
                        "<" => Op::Lt,
                        ">" => Op::Gt,
                        "<=" => Op::Le,
                        ">=" => Op::Ge,
                        _ => unreachable!(),
                    });
                    if is_comparison(op) {
                        Some(Ty::Bool)
                    } else {
                        Some(a)
                    }
                }
            }
            Expression::Conditional { condition, yes, no } => {
                let cond = self.expr(condition, ops)?;
                self.same(cond, Some(Ty::Bool), &condition.at)?;
                let (mut a, mut b) = (Vec::new(), Vec::new());
                let ty = self.expr(yes, &mut a)?;
                let other = self.expr(no, &mut b)?;
                self.same(other, ty, &e.at)?;
                ops.push(branch(a, b));
                ty
            }
            Expression::Call(name, args) => {
                if self.names.contains_key(name) {
                    return Err(e.at.error(format!("binding `{name}` is not callable")));
                }
                if let Some(target) = named_type(name).filter(|ty| ty.is_int()) {
                    if args.len() != 1 {
                        return Err(e.at.error("integer casts take one argument"));
                    }
                    let source = self.value(&args[0], ops)?;
                    if !source.is_int() {
                        return Err(e.at.error("integer casts require an integer"));
                    }
                    ops.push(Op::Cast(target));
                    Some(target)
                } else if name == "print" {
                    if args.len() != 1 {
                        return Err(e.at.error("print takes one argument"));
                    }
                    self.value(&args[0], ops)?;
                    ops.push(Op::PrintLine);
                    None
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
                    let sig = self
                        .sigs
                        .get(name)
                        .ok_or_else(|| e.at.error(format!("unknown function `{name}`")))?;
                    if sig.inputs.len() != args.len() {
                        return Err(e.at.error(format!(
                            "`{name}` expects {} arguments, got {}",
                            sig.inputs.len(),
                            args.len()
                        )));
                    }
                    if sig.outputs.len() > 1 {
                        return Err(e.at.error(
                            "legacy multi-result functions cannot be called from modern Plenty",
                        ));
                    }
                    for (arg, (_, expected)) in args.iter().zip(&sig.inputs) {
                        let ty = self.expr(arg, ops)?;
                        self.same(ty, Some(*expected), &arg.at)?;
                    }
                    ops.push(Op::Call(name.clone()));
                    sig.outputs.first().copied()
                }
            }
        };
        Ok(ty)
    }
    fn block(
        &mut self,
        body: &[Stmt],
        ops: &mut Vec<Op>,
        tail: bool,
        in_function: bool,
    ) -> Result<Type> {
        let mut result = None;
        for (i, stmt) in body.iter().enumerate() {
            let last = tail && i + 1 == body.len();
            result = match &stmt.kind {
                Statement::Expr(e) => self.expr(e, ops)?,
                Statement::Return(e) => {
                    if !in_function {
                        return Err(stmt.at.error("return outside a function"));
                    }
                    if !last {
                        return Err(stmt.at.error("early return is not supported yet; put return in the final expression or final if/else"));
                    }
                    match e {
                        Some(e) => self.expr(e, ops)?,
                        None => None,
                    }
                }
                Statement::Pass => None,
                Statement::Assign {
                    name,
                    mutable,
                    annotation,
                    value,
                } => {
                    let ty = self.value(value, ops)?;
                    if let Some(expected) = annotation {
                        self.same(Some(ty), Some(*expected), &stmt.at)?;
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
                        self.same(Some(ty), Some(local.ty), &stmt.at)?;
                        local.slot
                    } else {
                        let slot =
                            u8::try_from(self.parameters + self.locals.len()).map_err(|_| {
                                stmt.at
                                    .error("at most 256 parameter/local slots are supported")
                            })?;
                        self.locals.push(ty);
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
                    None
                }
                Statement::If { condition, yes, no } => {
                    let cond = self.expr(condition, ops)?;
                    self.same(cond, Some(Ty::Bool), &condition.at)?;
                    let saved = self.names.clone();
                    let (mut a, mut b) = (Vec::new(), Vec::new());
                    let ty = self.block(yes, &mut a, last, in_function)?;
                    self.names = saved.clone();
                    let other = self.block(no, &mut b, last, in_function)?;
                    self.names = saved;
                    self.same(other, ty, &stmt.at)?;
                    ops.push(branch(a, b));
                    ty
                }
            };
            if !last {
                if result.is_some() {
                    ops.push(Op::Drop);
                }
                result = None;
            }
        }
        Ok(result)
    }
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

pub(crate) fn compile(
    source: &str,
    heap: &mut Heap,
    prior: &HashMap<String, Rc<FnSig>>,
) -> Result<Vec<Op>> {
    let mut parser = Parser {
        tokens: lex(source)?,
        pos: 0,
    };
    let (mut functions, mut statements) = (Vec::new(), Vec::new());
    while parser.peek().kind != Kind::Eof {
        if parser.peek().is("def") {
            functions.push(parser.function()?);
        } else {
            statements.push(parser.statement()?);
        }
    }
    let mut sigs = prior.clone();
    for f in &functions {
        if sigs.contains_key(&f.name) {
            return Err(f.at.error(format!(
                "function `{}` is already defined; redefinition is not supported",
                f.name
            )));
        }
        sigs.insert(f.name.clone(), Rc::new(f.sig.clone()));
    }
    let mut ops = Vec::new();
    for f in functions {
        let mut lower = Lower {
            heap,
            sigs: &sigs,
            names: HashMap::new(),
            locals: Vec::new(),
            parameters: f.sig.inputs.len(),
        };
        for (i, (name, ty)) in f.sig.inputs.iter().enumerate() {
            lower.names.insert(
                name.clone(),
                Local {
                    slot: i as u8,
                    ty: *ty,
                    mutable: false,
                },
            );
        }
        let mut body = Vec::new();
        let output = lower.block(&f.body, &mut body, true, true)?;
        lower.same(output, f.sig.outputs.first().copied(), &f.at)?;
        mark_tail_calls(&mut body);
        ops.push(Op::DefineFn(
            f.name,
            CompiledFn {
                sig: Rc::new(f.sig),
                doc: f.doc.into(),
                body: body.into(),
                locals: lower.locals.into(),
            },
        ));
    }
    if !statements.is_empty() {
        let mut lower = Lower {
            heap,
            sigs: &sigs,
            names: HashMap::new(),
            locals: Vec::new(),
            parameters: 0,
        };
        let mut body = Vec::new();
        let output = lower.block(&statements, &mut body, true, false)?;
        mark_tail_calls(&mut body);
        ops.push(Op::DefineFn(
            ENTRY.into(),
            CompiledFn {
                sig: Rc::new(FnSig {
                    inputs: Vec::new(),
                    outputs: output.into_iter().collect(),
                }),
                doc: "".into(),
                body: body.into(),
                locals: lower.locals.into(),
            },
        ));
        ops.push(Op::Call(ENTRY.into()));
    }
    Ok(ops)
}

/// REPL input: a blank line submits a compound suite; single expressions
/// submit immediately. Syntax errors are submitted for normal diagnostics.
pub fn input_complete(source: &str) -> bool {
    let tokens = match lex(source) {
        Ok(tokens) => tokens,
        Err(error) => {
            return !(error.to_string().contains("unterminated")
                || error.to_string().contains("unclosed parenthesis"))
        }
    };
    let compound = tokens.iter().enumerate().any(|(i, t)| {
        t.is("def")
            || (t.is("if")
                && (i == 0
                    || matches!(
                        tokens[i - 1].kind,
                        Kind::Newline | Kind::Indent | Kind::Dedent
                    )))
    });
    !compound
        || source.ends_with("\n\n")
        || source
            .lines()
            .last()
            .is_some_and(|line| line.trim().is_empty())
}
