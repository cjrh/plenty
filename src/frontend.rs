//! Modern Plenty frontend: indentation → syntax tree → typed operations.
//!
//! Signatures are collected before bodies. Inference is local, expressions
//! have zero (unit) or one result, and no backend participates in inference.
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
mod atomic;
mod byte_buffers;
mod callables;
mod calls;
mod casts;
mod channels;
mod classes;
mod closures;
mod collections;
mod contexts;
mod control;
mod data;
mod defaults;
mod enums;
mod executors;
mod files;
mod foreign;
mod fstrings;
mod generators;
mod generics;
pub(crate) mod ide;
mod modules;
mod mutex;
mod numeric;
mod patterns;
mod pipelines;
mod protocols;
mod references;
mod shared;
mod sorting;
mod tail_calls;
mod threads;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Type = Option<Ty>; // Unit expressions have no operand; enum slots use Ty::Unit.
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

/// Every symbol the lexer accepts, so a symbol token needs no allocation.
const SYMBOLS: [&str; 37] = [
    "(", ")", "[", "]", "{", "}", ".", ":", ",", "+", "-", "*", "%", "/", "=", "!", "<", ">", "&",
    "|", "^", "~", "?", "->", "==", "!=", "<=", ">=", "//", "<<", ">>", "+=", "-=", "*=", "/=",
    "//=", "%=",
];

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    /// Word, number and text tokens share their text: the parser clones a
    /// token for every position it records.
    Word(Arc<str>),
    Number(Arc<str>),
    Text(Arc<str>),
    /// An `f"..."` literal's text and fields; see `fstrings`.
    Format(Vec<fstrings::Part>),
    Bytes(Vec<u8>),
    Symbol(&'static str),
    Newline,
    Indent,
    Dedent,
    Eof,
}

/// A diagnostic that keeps its position apart from its text, so that an error
/// in a generic instance's body can move to the use that required the
/// instance. It displays as the plain `location: message` text.
#[derive(Debug)]
struct Located {
    location: String,
    message: String,
}

impl std::fmt::Display for Located {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

impl Error for Located {}

/// A body diagnostic in its final form. Lowering one function can lower
/// another on demand; the marker stops the outer function from attributing the
/// inner one's error to its own instantiation.
#[derive(Debug)]
struct Attributed(String);

impl std::fmt::Display for Attributed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for Attributed {}

/// A diagnostic that can leave a worker thread. Located and Attributed keep
/// their types, so callers that inspect a diagnostic see the same value as
/// after a serial pass.
fn sendable(error: Box<dyn Error>) -> Box<dyn Error + Send + Sync> {
    let error = match error.downcast::<Located>() {
        Ok(located) => return located,
        Err(error) => error,
    };
    match error.downcast::<Attributed>() {
        Ok(attributed) => attributed,
        Err(error) => error.to_string().into(),
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Token {
    kind: Kind,
    line: usize,
    column: usize,
    source: Option<Arc<str>>,
}

impl Token {
    fn is(&self, text: &str) -> bool {
        self.text() == Some(text)
    }

    /// The spelling of a word or symbol.
    fn text(&self) -> Option<&str> {
        match &self.kind {
            Kind::Word(s) => Some(s),
            Kind::Symbol(s) => Some(s),
            _ => None,
        }
    }

    fn error(&self, message: impl std::fmt::Display) -> Box<dyn Error> {
        Box::new(Located {
            location: self.location(),
            message: message.to_string(),
        })
    }

    fn location(&self) -> String {
        let source = self
            .source
            .as_ref()
            .map(|s| format!("{s}:"))
            .unwrap_or_default();
        format!("{source}{}:{}", self.line, self.column)
    }
}

/// Lexes a module one top-level item at a time. The parser releases an item's
/// tokens once it has the item's tree, so a module's tokens never all exist
/// beside its syntax tree (issue #54).
struct Lexer<'a> {
    source: &'a str,
    /// Byte offset of the next character.
    pos: usize,
    line: usize,
    column: usize,
    line_start: bool,
    indents: Vec<usize>,
    /// Open brackets with their positions, so an unclosed one is reported where it opened.
    delimiters: Vec<(u8, Token)>,
    /// Whether the last token leaves a logical line for a line break to end.
    open: bool,
    /// No tokens remain: the module ended or has a lexical error.
    done: bool,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Lexer {
            source,
            pos: 0,
            line: 1,
            column: 1,
            line_start: true,
            indents: vec![0],
            delimiters: Vec::new(),
            open: false,
            done: false,
        }
    }

    /// Appends the next top-level item's tokens: everything up to the next
    /// line that starts at the left margin outside brackets, or to the end of
    /// the module, where `Eof` is the last token.
    fn item(&mut self, out: &mut VecDeque<Token>) -> Result<()> {
        let lexed = self.lex_item(out);
        self.done |= lexed.is_err();
        lexed
    }

    /// Lexes the rest of the module for its errors alone.
    fn rest(&mut self) -> Result<()> {
        let mut tokens = VecDeque::new();
        while !self.done {
            tokens.clear();
            self.item(&mut tokens)?;
        }
        Ok(())
    }

    fn push(&mut self, out: &mut VecDeque<Token>, kind: Kind, line: usize, column: usize) {
        self.open = !matches!(kind, Kind::Newline | Kind::Dedent);
        out.push_back(Token {
            kind,
            line,
            column,
            source: None,
        });
    }

    /// The string prefix at the cursor, when a known spelling is directly
    /// followed by a quote, and its width. Any other word before a quote stays
    /// a word.
    fn literal_prefix(&self) -> Option<(std::result::Result<Prefix, &'static str>, usize)> {
        let bytes = self.source.as_bytes();
        let width = bytes[self.pos..]
            .iter()
            .take_while(|c| c.is_ascii_alphabetic())
            .count();
        if !matches!(bytes.get(self.pos + width), Some(b'\'' | b'"')) {
            return None;
        }
        let prefix = match &bytes[self.pos..self.pos + width] {
            b"b" => Ok(Prefix::Bytes),
            b"B" => Err("byte literals use a lowercase `b` prefix"),
            b"f" => Ok(Prefix::Format),
            b"F" => Err("f-strings use a lowercase `f` prefix"),
            b"bf" | b"fb" | b"Bf" | b"bF" | b"fB" | b"Fb" | b"BF" | b"FB" => Err(
                "byte f-strings are not supported; build text with an f-string, then convert it to bytes",
            ),
            _ => return None,
        };
        Some((prefix, width))
    }

    /// The body of `b"..."` from its opening quote. Only ASCII characters and
    /// escapes may appear, so each byte has exactly one spelling.
    fn byte_literal(&mut self, start: &Token) -> Result<Vec<u8>> {
        let source = self.source;
        let bytes = source.as_bytes();
        let quote = bytes[self.pos];
        let closes = |pos: usize| bytes.get(pos) == Some(&quote);
        let triple = closes(self.pos + 1) && closes(self.pos + 2);
        let width = if triple { 3 } else { 1 };
        self.pos += width;
        self.column += width;
        let mut value = Vec::new();
        loop {
            let here = Token {
                kind: Kind::Eof,
                line: self.line,
                column: self.column,
                source: None,
            };
            let ch = source[self.pos..]
                .chars()
                .next()
                .ok_or_else(|| start.error("unterminated byte literal"))?;
            if ch == quote as char && (!triple || (closes(self.pos + 1) && closes(self.pos + 2))) {
                self.pos += width;
                self.column += width;
                return Ok(value);
            }
            if ch == '\n' && !triple {
                return Err(start.error("newline in a quoted byte literal"));
            }
            if !ch.is_ascii() {
                return Err(here.error(format!(
                    "a byte literal holds only ASCII characters; write `{ch}` with `\\x` escapes or convert text with `std.bytes.from_text`"
                )));
            }
            self.pos += 1;
            self.column += 1;
            if ch == '\n' {
                self.line += 1;
                self.column = 1;
            }
            if ch != '\\' {
                value.push(ch as u8);
                continue;
            }
            if self.pos == bytes.len() {
                return Err(start.error("unterminated escape"));
            }
            let (byte, width) = byte_escape(&source[self.pos..]).map_err(|m| here.error(m))?;
            value.push(byte);
            self.pos += width;
            self.column += width;
        }
    }

    fn lex_item(&mut self, out: &mut VecDeque<Token>) -> Result<()> {
        // Every character with a meaning outside strings and comments is
        // ASCII, and no byte of a longer UTF-8 sequence is, so the bytes are
        // compared directly. Columns count characters.
        let source = self.source;
        let bytes = source.as_bytes();
        let before = out.len();
        while self.pos < bytes.len() {
            if self.line_start {
                let mut spaces = 0;
                while bytes.get(self.pos) == Some(&b' ') {
                    spaces += 1;
                    self.pos += 1;
                    self.column += 1;
                }
                let (line, column) = (self.line, self.column);
                if bytes.get(self.pos) == Some(&b'\t') {
                    return Err(
                        format!("{line}:{column}: use spaces, not tabs, for indentation").into(),
                    );
                }
                let code = self.delimiters.is_empty()
                    && !matches!(bytes.get(self.pos), None | Some(b'\n' | b'\r' | b'#'));
                if code {
                    let last = *self.indents.last().unwrap();
                    if spaces > last {
                        self.indents.push(spaces);
                        self.push(out, Kind::Indent, line, column);
                    } else {
                        while spaces < *self.indents.last().unwrap() {
                            self.indents.pop();
                            self.push(out, Kind::Dedent, line, column);
                        }
                        if spaces != *self.indents.last().unwrap() {
                            return Err(format!(
                                "{line}:{column}: indentation does not match an outer block"
                            )
                            .into());
                        }
                    }
                }
                self.line_start = false;
                if self.pos == bytes.len() {
                    break;
                }
                if code && spaces == 0 && out.len() > before {
                    return Ok(());
                }
            }
            let (line, column) = (self.line, self.column);
            let start = Token {
                kind: Kind::Eof,
                line,
                column,
                source: None,
            };
            let c = bytes[self.pos];
            match c {
                b' ' | b'\r' => {
                    self.pos += 1;
                    self.column += 1;
                }
                b'\t' => return Err(start.error("tabs are not supported; use spaces")),
                b'\n' => {
                    if self.delimiters.is_empty() && self.open {
                        self.push(out, Kind::Newline, line, column);
                    }
                    self.pos += 1;
                    self.line += 1;
                    self.column = 1;
                    self.line_start = true;
                }
                b'#' => {
                    let end = bytes[self.pos..]
                        .iter()
                        .position(|&b| b == b'\n')
                        .map_or(bytes.len(), |length| self.pos + length);
                    self.column += source[self.pos..end].chars().count();
                    self.pos = end;
                }
                b'\'' | b'"' => {
                    let quote = c as char;
                    let closes = |pos: usize| bytes.get(pos) == Some(&c);
                    let triple = closes(self.pos + 1) && closes(self.pos + 2);
                    let width = if triple { 3 } else { 1 };
                    self.pos += width;
                    self.column += width;
                    let mut text = String::new();
                    loop {
                        let ch = source[self.pos..]
                            .chars()
                            .next()
                            .ok_or_else(|| start.error("unterminated string"))?;
                        if ch == quote
                            && (!triple || (closes(self.pos + 1) && closes(self.pos + 2)))
                        {
                            self.pos += width;
                            self.column += width;
                            break;
                        }
                        if ch == '\n' && !triple {
                            return Err(start.error("newline in a quoted string"));
                        }
                        let here = Token {
                            kind: Kind::Eof,
                            line: self.line,
                            column: self.column,
                            source: None,
                        };
                        self.pos += ch.len_utf8();
                        self.column += 1;
                        if ch == '\\' {
                            let escaped = source[self.pos..]
                                .chars()
                                .next()
                                .ok_or_else(|| start.error("unterminated escape"))?;
                            self.pos += escaped.len_utf8();
                            self.column += 1;
                            text.push(escape(escaped).ok_or_else(|| {
                                here.error(format!("unsupported escape \\{escaped}"))
                            })?);
                        } else {
                            if ch == '\n' {
                                self.line += 1;
                                self.column = 1;
                            }
                            text.push(ch);
                        }
                    }
                    self.push(out, Kind::Text(text.into()), line, column);
                }
                c if c.is_ascii_alphabetic() && self.literal_prefix().is_some() => {
                    let (prefix, width) = self.literal_prefix().expect("checked prefix");
                    self.pos += width;
                    self.column += width;
                    match prefix.map_err(|message| start.error(message))? {
                        Prefix::Bytes => {
                            let value = self.byte_literal(&start)?;
                            self.push(out, Kind::Bytes(value), line, column);
                        }
                        Prefix::Format => {
                            let kind = self.format_string(&start)?;
                            self.push(out, kind, line, column);
                        }
                    }
                }
                c if c.is_ascii_alphabetic() || c == b'_' => {
                    let begin = self.pos;
                    while bytes
                        .get(self.pos)
                        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
                    {
                        self.pos += 1;
                    }
                    self.column += self.pos - begin;
                    self.push(
                        out,
                        Kind::Word(source[begin..self.pos].into()),
                        line,
                        column,
                    );
                }
                c if c.is_ascii_digit()
                    || (c == b'.' && bytes.get(self.pos + 1).is_some_and(u8::is_ascii_digit)) =>
                {
                    let begin = self.pos;
                    let mut pos = self.pos;
                    let digits = |pos: &mut usize| {
                        while bytes
                            .get(*pos)
                            .is_some_and(|c| c.is_ascii_digit() || *c == b'_')
                        {
                            *pos += 1;
                        }
                    };
                    digits(&mut pos);
                    if bytes.get(pos) == Some(&b'.') {
                        pos += 1;
                        digits(&mut pos);
                    }
                    if matches!(bytes.get(pos), Some(b'e' | b'E')) {
                        pos += 1;
                        if matches!(bytes.get(pos), Some(b'+' | b'-')) {
                            pos += 1;
                        }
                        digits(&mut pos);
                    }
                    // Keep a suffix (including malformed suffixes) in the same token.
                    while bytes
                        .get(pos)
                        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
                    {
                        pos += 1;
                    }
                    self.pos = pos;
                    self.column += pos - begin;
                    self.push(out, Kind::Number(source[begin..pos].into()), line, column);
                }
                b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'.' | b':' | b',' | b'+' | b'-'
                | b'*' | b'%' | b'/' | b'=' | b'!' | b'<' | b'>' | b'&' | b'|' | b'^' | b'~'
                | b'?' => {
                    let begin = self.pos;
                    if let Some(next) = bytes.get(self.pos + 1)
                        && matches!(
                            (c, *next),
                            (b'-', b'>')
                                | (b'=', b'=')
                                | (b'!', b'=')
                                | (b'<', b'=')
                                | (b'>', b'=')
                                | (b'/', b'/')
                                | (b'<', b'<')
                                | (b'>', b'>')
                        )
                    {
                        self.pos += 1;
                        self.column += 1;
                    }
                    if matches!(
                        &source[begin..=self.pos],
                        "+" | "-" | "*" | "/" | "//" | "%"
                    ) && bytes.get(self.pos + 1) == Some(&b'=')
                    {
                        self.pos += 1;
                        self.column += 1;
                    }
                    if matches!(c, b'(' | b'[' | b'{') {
                        self.delimiters.push((c, start.clone()));
                    }
                    if matches!(c, b')' | b']' | b'}') {
                        let expected = match c {
                            b')' => b'(',
                            b']' => b'[',
                            _ => b'{',
                        };
                        if self.delimiters.pop().map(|(open, _)| open) != Some(expected) {
                            return Err(
                                start.error(format!("unmatched or mismatched `{}`", c as char))
                            );
                        }
                    }
                    let text = &source[begin..=self.pos];
                    let symbol = SYMBOLS
                        .iter()
                        .find(|symbol| **symbol == text)
                        .expect("the lexer accepts only listed symbols");
                    self.pos += 1;
                    self.column += 1;
                    self.push(out, Kind::Symbol(symbol), line, column);
                }
                _ => {
                    let c = source[self.pos..].chars().next().unwrap();
                    return Err(start.error(format!("unexpected character `{c}`")));
                }
            }
        }
        let (line, column) = (self.line, self.column);
        if let Some((open, at)) = self.delimiters.last() {
            return Err(at.error(format!("unclosed `{}`", *open as char)));
        }
        if self.open {
            self.push(out, Kind::Newline, line, column);
        }
        for _ in 1..self.indents.len() {
            self.push(out, Kind::Dedent, line, column);
        }
        self.push(out, Kind::Eof, line, column);
        self.done = true;
        Ok(())
    }
}

#[derive(Clone)]
struct Function {
    once: bool,
    captures: Vec<Capture>,
    foreign: Option<crate::foreign::Declaration>,
    export: Option<String>,
    name: String,
    type_params: Vec<(String, Option<TypeRef>)>,
    at: Token,
    inputs: Vec<(String, TypeRef)>,
    /// Aligned with inputs; synthetic declarations may leave this empty.
    defaults: Vec<Option<Expr>>,
    output: TypeRef,
    doc: String,
    /// Where the docstring was written, for diagnostics about a body that is only a docstring.
    doc_at: Option<Token>,
    body: Vec<Stmt>,
}

#[derive(Clone)]
struct Capture {
    name: String,
    mutable: bool,
    borrowed: bool,
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
        if matches!(name.as_str(), "&" | "&mut") {
            // `&()` is the type of a borrowed unit payload; spelling it lets
            // generic `&T` signatures instantiate with `T = ()`.
            let inner = self.args[0].resolve(aliases)?.unwrap_or(Ty::Unit);
            if matches!(inner, Ty::Ref(..)) {
                return Err(self.at.error("references to references are not supported"));
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
            if args.iter().any(Ty::contains_reference) {
                return Err(self
                    .at
                    .error("references cannot be stored in enum payloads"));
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
struct Elif {
    at: Token,
    condition: Expr,
    body: Vec<Stmt>,
}

#[derive(Clone)]
struct Expr {
    at: Token,
    kind: Expression,
}
#[derive(Clone)]
enum Expression {
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
    Method(Box<Expr>, String, Vec<Expr>, Pos),
    // The name is boxed to keep `Expr` within its size bound with the position.
    GenericMethod(Box<Expr>, Box<str>, Vec<TypeRef>, Vec<Expr>, Pos),
    /// `base[...]` whose brackets parse both as type arguments and as an index.
    /// `generic` is the Generic* reading, with any call arguments; `index` is
    /// `Index(base, ...)` without them. The parser cannot tell a local from a
    /// declaration, so module name resolution keeps one reading and no later
    /// pass sees this variant.
    Ambiguous {
        generic: Box<Expr>,
        index: Box<Expr>,
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
}

/// Where a member name was written. Expressions carry only the position of
/// their first token, so `a.b.c` would otherwise not say where `c` is. A
/// synthetic member has the default position, which no source token has.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Pos {
    line: u32,
    column: u32,
}

impl Pos {
    fn of(at: &Token) -> Self {
        Pos {
            line: at.line as u32,
            column: at.column as u32,
        }
    }
}

#[derive(Clone)]
enum Clause {
    For(Vec<String>, Expr),
    If(Expr),
}

/// Indented blocks and expressions nest to this many levels in total. Every
/// pass over the tree, and the tree's own clone and drop, recurses once per
/// level, so an unbounded depth overflows the compiler's stack (issue #70).
const MAX_NESTING: usize = 64;
const NESTING: &str = "block and expression nesting exceeds the implementation limit of 64";

struct Parser {
    /// The item being parsed and the items buffered after it, behind the
    /// last token of the item before.
    tokens: VecDeque<Token>,
    /// Whether each token lies inside `()`, `[]`, or `{}`. The lexer drops
    /// layout tokens there, so the parser cannot recover this from `tokens`.
    in_brackets: VecDeque<bool>,
    pos: usize,
    /// Tokens released ahead of `tokens`.
    released: usize,
    /// The token counts, from the start of the module, at which the buffered
    /// items end.
    items: VecDeque<usize>,
    /// Whether `tokens` reaches the end of the module.
    complete: bool,
    /// Set when the parser asks for a token past the buffered items. What it
    /// then parsed is not what the whole module holds.
    starved: Cell<bool>,
    /// Stands in for a token past the buffered items.
    end: Token,
    type_depth: usize,
    /// Blocks and expressions open around the one being parsed.
    depth: usize,
    /// Level of the deepest node in the expression being parsed. An operator
    /// parsed after its left operand moves that operand down a level, so the
    /// depth of the calls into `expr` alone does not bound the tree.
    deepest: usize,
    /// Whether the module has an f-string, which needs `std.fmt`.
    formats: bool,
}
impl Parser {
    /// A parser for a module whose tokens name `origin` as their source file.
    fn new(origin: Option<Arc<str>>) -> Self {
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
                source: origin,
            },
            type_depth: 0,
            depth: 0,
            deepest: 0,
            formats: false,
        }
    }
    /// Releases the tokens of the items already parsed and buffers the next
    /// item with `lookahead` items after it. Call between top-level items.
    fn fill(&mut self, lexer: &mut Lexer, lookahead: usize) -> Result<()> {
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
                token.source = self.end.source.clone();
                if [")", "]", "}"].iter().any(|close| token.is(close)) {
                    depth = depth.saturating_sub(1);
                }
                self.in_brackets.push_back(depth > 0);
                if ["(", "[", "{"].iter().any(|open| token.is(open)) {
                    depth += 1;
                }
            }
            self.items.push_back(self.released + self.tokens.len());
        }
        self.complete = lexer.done;
        Ok(())
    }
    /// Token `index` of the buffer, or None past the end of the module.
    fn token(&self, index: usize) -> Option<&Token> {
        let token = self.tokens.get(index);
        if token.is_none() && !self.complete {
            self.starved.set(true);
        }
        token
    }
    /// Parse a block or an expression one level below the current one.
    fn nested<T>(&mut self, parse: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
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
    fn clause(&mut self, keyword: usize, outer: usize, element: &mut usize) -> Result<Expr> {
        *element += 1;
        if *element > MAX_NESTING {
            return Err(self.tokens[keyword].error(NESTING));
        }
        self.depth += outer;
        let expression = self.expr(1);
        self.depth -= outer;
        expression
    }
    fn peek(&self) -> &Token {
        self.token(self.pos).unwrap_or(&self.end)
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
        if let Kind::Word(ref s) = t.kind
            && !reserved(s)
            && !s.starts_with("__plenty_")
        {
            return Ok(s.to_string());
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
            at,
            name: Some("|".into()),
            args,
        })
    }
    fn ty_inner(&mut self) -> Result<TypeRef> {
        let at = self.peek().clone();
        if self.eat("&") {
            let name = if self.eat("mut") { "&mut" } else { "&" };
            if self.type_depth >= 64 {
                return Err(at.error("type nesting exceeds the implementation limit of 64"));
            }
            self.type_depth += 1;
            let inner = self.ty_inner();
            self.type_depth -= 1;
            return Ok(TypeRef {
                concrete: None,
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
            let mut name = name.to_string();
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
                    at,
                    name: Some(name),
                    args,
                });
            }
            return Ok(TypeRef {
                concrete: None,
                at,
                name: Some(name),
                args: self.type_arguments()?,
            });
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
        args.shrink_to_fit();
        Ok(args)
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
    fn type_parameters(&mut self) -> Result<Vec<(String, Option<TypeRef>)>> {
        let mut params = Vec::new();
        if self.eat("[") {
            loop {
                let at = self.peek().clone();
                let name = self.name()?;
                if builtin(&name) || params.iter().any(|(n, _)| n == &name) {
                    return Err(at.error("duplicate or builtin type parameter"));
                }
                let bound = if self.eat(":") {
                    Some(self.ty()?)
                } else {
                    None
                };
                params.push((name, bound));
                if self.eat("]") {
                    break;
                }
                self.expect(",")?;
            }
        }
        Ok(params)
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
        self.function_header(class, false, false, false)
    }
    fn function_header(
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
        let mut type_params = Vec::new();
        if !anonymous && self.eat("[") {
            if class.is_some() && matches!(name.as_str(), "__init__" | "__del__") {
                return Err(at.error("lifecycle methods cannot declare their own type parameters"));
            }
            loop {
                let param_at = self.peek().clone();
                let param = self.name()?;
                if builtin(&param) || type_params.iter().any(|(n, _)| n == &param) {
                    return Err(param_at.error("duplicate or builtin type parameter"));
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
        let mut defaults = Vec::new();
        let mut adapters = Vec::new();
        while !self.eat(")") {
            let param_at = self.peek().clone();
            let param = self.name()?;
            if captures.iter().any(|c| c.name == param) {
                return Err(param_at.error("a parameter cannot shadow a closure capture"));
            }
            if inputs.iter().any(|(n, _)| n == &param) {
                return Err(param_at.error("duplicate parameter"));
            }
            let ty = if let Some(class) =
                class.filter(|_| param == "self" && inputs.is_empty() && !self.peek().is(":"))
            {
                TypeRef {
                    concrete: None,
                    // Preserve the shorthand's provenance for receiver diagnostics.
                    at: param_at.clone(),
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
            let adapter = if foreign && self.eat("as") {
                let token = self.take();
                if token.is("utf8") {
                    crate::foreign::Argument::Utf8
                } else if token.is("c_string") {
                    crate::foreign::Argument::CString
                } else {
                    return Err(token.error("expected utf8 or c_string C argument adapter"));
                }
            } else {
                crate::foreign::Argument::Direct
            };
            if foreign {
                adapters.push(adapter);
            }
            let default =
                if self.eat("=") {
                    if anonymous {
                        return Err(
                            param_at.error("anonymous functions cannot declare parameter defaults")
                        );
                    }
                    if foreign || exported {
                        return Err(param_at
                            .error("C imports and exports cannot declare parameter defaults"));
                    }
                    if param == "self" && class.is_some() {
                        return Err(param_at.error("self cannot have a default value"));
                    }
                    Some(self.expr(0)?)
                } else {
                    if defaults.iter().any(Option::is_some) {
                        return Err(param_at
                            .error("required parameters must precede parameters with defaults"));
                    }
                    None
                };
            inputs.push((param, ty));
            defaults.push(default);
            if self.eat(")") {
                break;
            }
            self.expect(",")?;
        }
        if inputs.len() > 256 {
            return Err(at.error("at most 256 parameter/local slots are supported"));
        }
        defaults::validate_dependencies(&inputs, &defaults)?;
        if defaults.iter().all(Option::is_none) {
            defaults = Vec::new();
        }
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
    /// The body of an anonymous function written inside brackets: one
    /// expression, parsed as the tail expression of an indented body so both
    /// forms share every later rule. Brackets suspend layout, so statements
    /// cannot be delimited here.
    fn inline_body(&mut self) -> Result<Vec<Stmt>> {
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
    fn suite(&mut self) -> Result<Vec<Stmt>> {
        self.nested(Self::suite_inner)
    }
    fn suite_inner(&mut self) -> Result<Vec<Stmt>> {
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
        // A growing vector reserves at least four statements, which is over
        // 2 KB for a one-line body. The whole tree is live when lowering
        // starts, so the unused capacity counts against peak memory (issue #54).
        body.shrink_to_fit();
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
                    Statement::SetIndex {
                        target,
                        value: self.statement_value()?,
                    }
                } else if matches!(&self.peek().kind, Kind::Symbol(s)
                    if matches!(&**s, "+=" | "-=" | "*=" | "/=" | "//=" | "%="))
                {
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
    /// expression, or a `match` block whose selected case supplies it.
    fn statement_value(&mut self) -> Result<Expr> {
        if !self.peek().is("match") {
            return self.expr(0);
        }
        let (at, value, cases) = self.match_block()?;
        Ok(Expr {
            at,
            kind: Expression::Match(Box::new(value), cases),
        })
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
    fn parenthesized(&mut self) -> Result<Expression> {
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
    fn expr(&mut self, min: u8) -> Result<Expr> {
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
            Kind::Number(n) => Expression::Number(n.to_string()),
            Kind::Text(s) => Expression::Text(s.to_string()),
            Kind::Format(parts) => self.format_literal(parts)?,
            Kind::Bytes(b) => Expression::Bytes(b.clone()),
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
                Expression::Unary(op.into(), Box::new(self.expr(10)?))
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
                } else if matches!(&**s, "list" | "dict" | "set" | "range" | "channel")
                    && self.peek().is("[")
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
                    Expression::Call(s.to_string(), self.arguments()?)
                } else {
                    Expression::Name(s.to_string())
                }
            }
            _ if at.is("match") => return Err(at.error(MATCH_VALUE_POSITION)),
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
                let index = self.expr(0)?;
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
            self.deepen()?;
            // `a ** b` parses as `a * (*b)`, which is valid when `b` is a
            // reference. Mark the dereference so that its failure reports
            // the missing operator rather than a dereference.
            let power = op == "*"
                && self.peek().is("*")
                && self.peek().line == token.line
                && self.peek().column == token.column + 1;
            let mut right = self.expr(prec + 1)?;
            if power && matches!(&right.kind, Expression::Unary(op, _) if op == "*") {
                right.at = Token {
                    kind: Kind::Symbol("**"),
                    ..token.clone()
                };
            }
            let at = left.at.clone();
            left = Expr {
                at,
                kind: Expression::Binary(op, Box::new(left), Box::new(right)),
            };
        }
        Ok(left)
    }
}

/// A string prefix. Each has exactly one spelling, matched in
/// `Lexer::literal_prefix`.
#[derive(Clone, Copy, Debug)]
enum Prefix {
    Bytes,
    Format,
}

/// Decodes the escape after a backslash in byte text: a shared escape or
/// `\xHH`. Returns the byte and the source bytes consumed after the backslash.
fn byte_escape(rest: &str) -> std::result::Result<(u8, usize), String> {
    let escaped = rest.chars().next().expect("caller checked for input");
    if escaped == 'x' {
        let digits = rest
            .get(1..3)
            .filter(|digits| digits.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or("`\\x` needs exactly two hexadecimal digits")?;
        return Ok((u8::from_str_radix(digits, 16).expect("hex digits"), 3));
    }
    match escape(escaped) {
        Some(byte) => Ok((byte as u8, 1)),
        None => Err(format!("unsupported escape \\{escaped} in a byte literal")),
    }
}

/// The escapes string and byte literals share; byte literals add `\xHH`.
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
        "ChannelError" => crate::sum::channel_error(),
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

pub(crate) fn builtin(name: &str) -> bool {
    named_type(name).is_some()
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
                | "__test_fault"
                | "__report_write"
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
                | "next"
                | "copy"
                | "drop"
                | "Box"
                | "Shared"
        )
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
pub(crate) fn reserved(name: &str) -> bool {
    matches!(
        name,
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

const MATCH_VALUE_POSITION: &str = "a `match` block is a value only as the whole right side of a binding, a single-target assignment, or `return`; bind its value to a name first";

/// What becomes of the value of a suite's final expression.
#[derive(Clone)]
enum Tail {
    /// It is destroyed, and a `Result` cannot be discarded this way.
    Discard,
    /// It is the function's result, and the function's cleanup follows.
    Function,
    /// It is the value of a `match` block, with the type the block's context
    /// expects. The suite's own locals are cleaned up beneath it.
    Value(Type),
    /// It is returned from the function, as the value of a case of
    /// `return match`. A suite that ends without one returns unit.
    Return,
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
    pipelines: Vec<pipelines::Pipeline>,
    pipeline_generators: usize,
    function_name: String,
    bare_self_receiver: bool,
    source_tail: Vec<crate::ownership::Site>,
    tail_candidates: Vec<tail_calls::Candidate>,
    closure_loans: HashMap<String, Vec<usize>>,
    captures: HashSet<String>,
    heap: &'a mut Heap,
    sigs: &'a mut HashMap<String, Arc<FnSig>>,
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
    yield_type: Type,
    loans: Vec<crate::ownership::Loan>,
    reference_locals: HashMap<u8, usize>,
    expression_temps: Vec<u8>,
    contexts: Vec<contexts::Context>,
    return_origin: Option<u8>,
    returned_fields: &'a mut HashMap<String, Vec<usize>>,
    /// Source name of each slot, kept after the binding leaves scope so an
    /// ownership diagnostic can still name it.
    slot_names: Vec<Option<Arc<str>>>,
    /// Position of the last source marker, restored after scope cleanup.
    site: std::cell::Cell<Option<crate::ownership::Site>>,
    /// The next identity for `Forward` markers.
    forwarded: std::cell::Cell<u32>,
}

fn site(at: &Token, scope_end: bool) -> crate::ownership::Site {
    crate::ownership::Site {
        line: at.line as u32,
        column: at.column as u32,
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
        self.same(Some(a), Some(ty.clone()), &left.at)?;
        let (b, other_loans) = self.observe_expected(right, Some(ty.clone()), ops)?;
        if b.is_numeric() && b != ty && self.fixes_numeric_type(left, &ty) {
            return Err(mixed_numeric(left, &ty, right, &b));
        }
        if op == "*" && matches!(b, Ty::List(_)) {
            return Err(right.at.error(LIST_REPETITION));
        }
        self.same(Some(b), Some(ty.clone()), &right.at)?;
        loans.extend(other_loans);
        self.mark_arithmetic(op, &ty, at, ops);
        let output = Self::numeric_operation(op, ty, at, ops)?;
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
    fn numeric_operation(op: &str, ty: Ty, at: &Token, ops: &mut Vec<Op>) -> Result<Ty> {
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
        // A collection literal's error is always AllocError, so its context keeps
        // that error and the mismatch is reported below rather than as a missing `?`.
        let literal = matches!(&ungroup(value).kind, Expression::Collection { kind, .. } if kind != "generator");
        let operand_context = expected.map(|ty| {
            if literal {
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
        if operand.as_ref().is_some_and(Ty::key_view) {
            return Err(value.at.error("keys() returns a borrowed KeyView without AllocError; remove `?`, or use keys_snapshot(), into_keys(), or to_set() for an owned result"));
        }
        let Some(Ty::Enum(source)) = operand else {
            return Err(e.at.error("`?` requires a Result or Option operand"));
        };
        if !source.propagatable() || source.is_option() != target.is_option() {
            let note = if literal {
                "; a collection literal returns Result[collection, AllocError]"
            } else {
                ""
            };
            return Err(e.at.error(format!(
                "`?` operand and function return must use the same Result or Option family{note}"
            )));
        }
        let target = self.infer_propagated_producers(target, &source);
        if !source.is_option() && !target.accepts_result_error(&source) {
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
                return Err(e.at.error(format!("`?` operand error {source_error} cannot propagate as {target_error}; missing alternatives: {missing}{hint}")));
            }
            if literal {
                let error = &target.get().variants[1].fields[0];
                return Err(e.at.error(format!("`?` requires identical Result error types; a collection literal fails with AllocError, which cannot propagate as {error}; declare a sum that includes it, such as Result[_, AllocError | {error}], or match the literal and convert the error explicitly")));
            }
            let hint = producer_conflict_hint(
                &target.get().variants[1].fields[0],
                &source.get().variants[1].fields[0],
            );
            return Err(e.at.error(format!(
                "`?` requires identical Result error types; convert the error explicitly{hint}"
            )));
        }
        let success = usize::from(source.is_option());
        let payload = source.get().variants[success].fields[0].clone();
        let mut cleanup = Vec::new();
        if !self.contexts.is_empty() {
            self.cleanup(0, &mut cleanup);
        }
        ops.push(Op::Try {
            source: Box::new(source),
            target: Box::new(target),
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
        if self.is_pipeline(e) {
            return Err(e.at.error("iterator pipelines cannot escape their creating scope; bind locally or consume with a loop or terminal"));
        }
        let ty = match &e.kind {
            Expression::NamedArgument(..) => {
                return Err(e.at.error("named arguments require a statically resolved function, method, or class constructor"));
            }
            Expression::Anonymous(function) => {
                if function.once || !function.captures.is_empty() {
                    return self.closure_value(function, &e.at, ops).map(Some);
                }
                let mut function = (**function).clone();
                function.name = format!("__plenty_anonymous_{}", self.generics.functions.len());
                let name = function.name.clone();
                register_signature(&function, self.aliases, self.sigs, self.returned_fields)?;
                self.generics
                    .inherit_requirement(&self.function_name, &name);
                self.generics.declare(function);
                Some(self.function_value(&name, &e.at, ops)?)
            }
            Expression::GenericValue(name, types) => {
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
            Expression::Type(_) => {
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
                        let (ty, loans) = self.field(e, ops)?;
                        if ty.affine() {
                            let copy = if ty.can_copy() && !ty.recursive_data() {
                                "copy(value.field).unwrap(), "
                            } else {
                                ""
                            };
                            let borrow = if matches!(ty, Ty::Enum(_)) && ty.recursive_data() {
                                "match &value.field (or &mut value.field)"
                            } else {
                                "&value.field (or &mut value.field)"
                            };
                            return Err(e.at.error(format!("cannot move out of a field; use {copy}{borrow}, or replace(value.field, replacement) on a mutable owner")));
                        }
                        Self::end_reads(loans, ops);
                        Some(ty)
                    }
                }
            }
            Expression::Tuple(values) => Some(self.tuple(values, None, &e.at, ops)?),
            Expression::Format(format) => self.format(format, &e.at, ops)?,
            Expression::Collection { .. } => Some(self.fallible_display(e, None, ops)?),
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
                if !self.names.contains_key(name) && self.sigs.contains_key(name) {
                    return self.function_value(name, &e.at, ops).map(Some);
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
                        // The parser marks a `*` written directly after a
                        // multiplication `*`; see the binary operator loop.
                        _ if e.at.is("**") => return Err(e.at.error(
                            "exponentiation with `**` is not supported; use `base.pow(exponent)`",
                        )),
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
                        return Err(e
                            .at
                            .error("cannot move out of a reference; use copy or borrow"));
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
                        ops.push(Op::Sub);
                    }
                }
                Some(ty.clone())
            }
            Expression::Binary(op, left, right) if op == "in" || op == "not in" => {
                let (a, mut loans) = self.observe(left, ops)?;
                let (b, other_loans) = self.observe(right, ops)?;
                loans.extend(other_loans);
                if b.restricted_storage() && !matches!(b, Ty::KeyView(_)) {
                    return Err(e
                        .at
                        .error("membership does not consume generators; use a loop"));
                }
                let element = b
                    .element()
                    .ok_or_else(|| e.at.error("membership requires an iterable"))?;
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
                    return self.boolean_reduction(name, args, &e.at, ops).map(Some);
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
                    "select_recv" | "select_recv_nowait" | "select_recv_timeout"
                ) {
                    return self.channel_select(name, args, &e.at, ops);
                }
                if name == "ThreadPoolExecutor" {
                    return self.executor_new(args, &e.at, ops);
                }
                if name == "spawn" {
                    return Err(e.at.error("spawn is a scoped operation; use with spawn(worker, arguments...)? as task:"));
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
                        if ty.key_view() {
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
                        CollectionOp::TryPrint(types.remove(0))
                    } else {
                        CollectionOp::TryPrintMany(types)
                    };
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
                if let Some(operation) = crate::stream::StreamOp::named(name) {
                    let operation = match operation.literal() {
                        Some(literal) if self.literal_source(operation.source(), args) => literal,
                        _ => operation,
                    };
                    let exclusive = |index| operation.exclusive(index);
                    let call = CollectionOp::Stream(operation);
                    return self.exclusive_call(
                        call,
                        &exclusive,
                        operation.source(),
                        args,
                        &e.at,
                        ops,
                    );
                }
                if let Some(operation) = crate::crypto::CryptoOp::named(name) {
                    let exclusive = |index| operation.exclusive(index);
                    let call = CollectionOp::Crypto(operation);
                    return self.exclusive_call(call, &exclusive, None, args, &e.at, ops);
                }
                if let Some(operation) = crate::compress::CompressOp::named(name) {
                    let operation = match operation.literal() {
                        Some(literal) if self.literal_source(operation.source(), args) => literal,
                        _ => operation,
                    };
                    let exclusive = |index| operation.exclusive(index);
                    let call = CollectionOp::Compress(operation);
                    return self.exclusive_call(
                        call,
                        &exclusive,
                        operation.source(),
                        args,
                        &e.at,
                        ops,
                    );
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
                    let operation = match operation.literal() {
                        Some(literal) if self.literal_source(operation.source(), args) => literal,
                        _ => operation,
                    };
                    let exclusive = |index| operation.exclusive(index);
                    let call = CollectionOp::Net(operation);
                    return self.exclusive_call(
                        call,
                        &exclusive,
                        operation.source(),
                        args,
                        &e.at,
                        ops,
                    );
                }
                if let Some(operation) = crate::sqlite::SqliteOp::named(name) {
                    return self.system_call(CollectionOp::Sqlite(operation), args, &e.at, ops);
                }
                if let Some(operation) = crate::tls::TlsOp::named(name) {
                    let operation = match operation.literal() {
                        Some(literal) if self.literal_source(operation.source(), args) => literal,
                        _ => operation,
                    };
                    let exclusive = |index| operation.exclusive(index);
                    let call = CollectionOp::Tls(operation);
                    return self.exclusive_call(
                        call,
                        &exclusive,
                        operation.source(),
                        args,
                        &e.at,
                        ops,
                    );
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
                    "__report_write" => Some(CollectionOp::ReportWrite),
                    _ => None,
                } {
                    // Trusted std hooks are not user-visible API.
                    let standard =
                        e.at.source
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
                        if let Some(target) = target.as_ref().filter(|t| t.is_collection()) {
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
        self.push_call(Op::Call(name.to_owned()), &sig.inputs, &argument_loans, ops);
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
            if let Ty::Ref(inner, mutable) = expected {
                let (ty, loan) = self.call_borrow(arg, *mutable, Some((**inner).clone()), ops)?;
                // A borrowed box also lends its content.
                let ty = if ty == *expected {
                    ty
                } else {
                    self.unbox_reference(ty, ops)
                };
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
    fn push_call(&self, call: Op, inputs: &[(String, Ty)], loans: &[usize], ops: &mut Vec<Op>) {
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
        ops.push(call);
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
        let returned_loan = if self.return_origin.is_some() {
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
            finish_return(&mut returned);
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
            _ => false,
        })
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
    /// A suite of `return match` that ends without returning a value returns
    /// unit, so no case continues past the statement.
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
                    if matches!(tail, Tail::Value(_))
                        && ty.as_ref().is_some_and(Ty::contains_reference)
                    {
                        // The case's bindings and locals end before the value
                        // is used, and no loan follows it out of the suite.
                        return Err(e.at.error(
                            "a `match` block used as a value cannot produce a reference or a closure with borrowed captures; produce an owned value, or assign in each case",
                        ));
                    }
                    if last && self.return_origin.is_some() && matches!(ty, Some(Ty::Ref(..))) {
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
                    if is_continue {
                        self.cleanup(scope, ops);
                        ops.extend(step.iter().cloned());
                    } else {
                        self.leaving(scope, ops);
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
                    if let Expression::Tuple(targets) = &target.kind {
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
                    mutable,
                    value,
                } => {
                    let start = ops.len();
                    self.unpack_tuple(names, *mutable, value, ops)?;
                    self.building(&value.at, start, ops, block_value(value));
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
                    let (ty, origin) = self.binding_value(value, context, ops)?;
                    if let Some(expected) = annotation {
                        let expected = expected.resolve(self.aliases)?.ok_or_else(|| {
                            expected.at.error("unit bindings are not supported yet")
                        })?;
                        self.same(Some(ty.clone()), Some(expected), &stmt.at)?;
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
                        self.locals.push(ty.clone());
                        ide::local_type(&stmt.at, &ty);
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
/// `e` is a `match` block whose selected case's final expression is its value.
fn block_value(e: &Expr) -> bool {
    matches!(ungroup(e).kind, Expression::Match(..))
}
/// `ops` contains a `match` case value's `Building::Hold`.
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
    lower(modules::single(source)?, heap)
}

#[cfg_attr(feature = "profile", hotpath::measure)]
pub(crate) fn compile_file(
    path: &std::path::Path,
    root: Option<&std::path::Path>,
    require_main: bool,
    heap: &mut Heap,
) -> Result<Program> {
    lower(modules::load(path, root, require_main)?, heap)
}

#[cfg_attr(feature = "profile", hotpath::measure)]
pub(crate) fn compile_graph(
    path: &std::path::Path,
    graph: &crate::project::SourceGraph,
    require_main: bool,
    heap: &mut Heap,
) -> Result<Program> {
    lower(modules::load_graph(path, graph, require_main)?, heap)
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
        })
    }

    pub(crate) fn compile(&self, heap: &mut Heap) -> Result<Program> {
        match self {
            Sources::Text(source) => compile(source, heap),
            Sources::Graph {
                entry,
                graph,
                appendix,
            } => lower(
                modules::load_graph_with(entry, graph, true, appendix.as_deref())?,
                heap,
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
            } => {
                identity.extend_from_slice(b"graph\n");
                identity.extend_from_slice(modules::identity(entry, graph)?.as_bytes());
                identity.extend_from_slice(format!("\n{appendix:?}").as_bytes());
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
    let program = lower(modules::generated_interface(source)?, &mut heap)?;
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

const NO_ORIGIN: &str =
    "returned references borrow from the first reference parameter; add a reference parameter";
const IMMUTABLE_ORIGIN: &str =
    "a mutable returned reference borrows from the first reference parameter, which must be `&mut`";
const UNRESOLVED_SUM_MEMBER: &str = "a closure signature in an anonymous sum names no concrete environment; only a function's declared return type can infer it, from the closure the body returns or propagates; use a generic parameter bounded by OnceCallable or Callable";

fn register_signature(
    f: &Function,
    aliases: &TypeAliases,
    sigs: &mut HashMap<String, Arc<FnSig>>,
    returned_fields: &mut HashMap<String, Vec<usize>>,
) -> Result<Arc<FnSig>> {
    register_resolved(f, None, aliases, sigs, returned_fields)
}

/// [`register_signature`] for a function whose parameter and return types
/// `generics::prepare` already resolved, or that resolves them when
/// `resolved` is `None`. Resolving a type again would find the same type, so
/// only the cost differs.
#[cfg_attr(feature = "profile", hotpath::measure)]
fn register_resolved(
    f: &Function,
    resolved: Option<generics::Signature>,
    aliases: &TypeAliases,
    sigs: &mut HashMap<String, Arc<FnSig>>,
    returned_fields: &mut HashMap<String, Vec<usize>>,
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
    let Entry::Vacant(entry) = sigs.entry(f.name.clone()) else {
        return Err(f.at.error(format!(
            "function `{}` is already defined; redefinition is not supported",
            f.name
        )));
    };
    let (types, mut output) = match resolved {
        Some(resolved) => resolved,
        None => protocols::signature(f, aliases)?,
    };
    if types.iter().any(Ty::key_view) || output.as_ref().is_some_and(Ty::key_view) {
        return Err(f.at.error("dictionary key views are local-only; user-function parameters, returns, and captures require an owned keys_snapshot(), into_keys(), or to_set()"));
    }
    let pipeline = |ty: &Ty| {
        matches!(ty, Ty::Iter(..)) || matches!(ty, Ty::Ref(t, _) if matches!(**t, Ty::Iter(..)))
    };
    if types.iter().any(pipeline) || output.as_ref().is_some_and(pipeline) {
        return Err(f.at.error(
            "iterator pipelines are local-only; parameters, returns, and lending are not supported",
        ));
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
    if let Some(Ty::Ref(_, mutable)) = &output {
        // The first reference parameter is the result's origin; later ones
        // are ordinary inputs whose loans end with the call.
        let Some((_, origin)) = inputs.iter().find(|(_, ty)| matches!(ty, Ty::Ref(..))) else {
            return Err(f.at.error(NO_ORIGIN));
        };
        if *mutable && !matches!(origin, Ty::Ref(_, true)) {
            return Err(f.at.error(IMMUTABLE_ORIGIN));
        }
    }
    if generators::yields(&f.body)
        && let Some(Ty::Generator(t)) = &output
    {
        output = Some(crate::generator::ty(
            t.element.clone(),
            Some(f.name.clone()),
        ));
    }
    let outputs = output.into_iter().collect();
    let sig = Arc::new(FnSig { inputs, outputs });
    if let Some(fields) = references::returned_fields(f, &sig) {
        returned_fields.insert(f.name.clone(), fields);
    }
    entry.insert(Arc::clone(&sig));
    Ok(sig)
}

/// Lowers one function. An error in the body of a generic instance is reported
/// at the use that first required the instance, with the body position in a
/// note: see `ownership::Instantiation`.
#[cfg_attr(feature = "profile", hotpath::measure)]
fn lower_function(
    f: Arc<Function>,
    heap: &mut Heap,
    sigs: &mut HashMap<String, Arc<FnSig>>,
    generics: &mut generics::Engine,
    aliases: &TypeAliases,
    access: &modules::AccessMap,
    returned_fields: &mut HashMap<String, Vec<usize>>,
) -> Result<()> {
    if generics.lowered(&f.name) {
        return Ok(());
    }
    let required = generics.required.get(&f.name).cloned();
    let outer = aliases
        .data
        .attribute(required.clone().map(data::Requiring::Instance));
    let lowered = lower_body(f, heap, sigs, generics, aliases, access, returned_fields);
    aliases.data.attribute(outer);
    lowered.map_err(|error| {
        if error.is::<Attributed>() {
            return error;
        }
        let text = match (&required, error.downcast_ref::<Located>()) {
            (Some(required), Some(error)) => {
                required.report(&format!("{}: ", error.location), &error.message)
            }
            (Some(required), None) => required.report("", &error.to_string()),
            (None, _) => error.to_string(),
        };
        Box::new(Attributed(text)) as Box<dyn Error>
    })
}

fn lower_body(
    f: Arc<Function>,
    heap: &mut Heap,
    sigs: &mut HashMap<String, Arc<FnSig>>,
    generics: &mut generics::Engine,
    aliases: &TypeAliases,
    access: &modules::AccessMap,
    returned_fields: &mut HashMap<String, Vec<usize>>,
) -> Result<()> {
    let (mut sig, mut yield_type, mut lower, mut body) = crate::profile_block!(
        "frontend::setup_function",
        {
            if !generics.active.insert(f.name.clone()) {
                return Err(f.at.error("recursive generator factory requires a concrete return type; recursive inline frames are not supported"));
            }
            let sig = Arc::clone(&sigs[&f.name]);
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
                generics.active.remove(&f.name);
                generics.complete(&f.name, &sig);
                generics.compiled.push(Op::DefineFn(
                    f.name.clone(),
                    Box::new(CompiledFn {
                        location: None,
                        origins: Arc::default(),
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
                bare_self_receiver: f.inputs.first().is_some_and(|(name, ty)| {
                    name == "self" && ty.name.as_deref() == Some("&") && ty.at.is("self")
                }),
                source_tail: Vec::new(),
                tail_candidates: Vec::new(),
                closure_loans: HashMap::default(),
                captures: f
                    .captures
                    .iter()
                    .filter(|capture| !f.once || capture.borrowed)
                    .map(|c| c.name.clone())
                    .collect(),
                returned_fields,
                heap,
                sigs,
                generics,
                aliases,
                access,
                names: HashMap::default(),
                pipelines: Vec::new(),
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
                yield_type: yield_type.clone(),
                loans: Vec::new(),
                reference_locals: HashMap::default(),
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
                slot_names: Vec::new(),
                site: Default::default(),
                forwarded: Default::default(),
            };
            for ((_, ty), (_, written)) in sig.inputs.iter().zip(&f.inputs) {
                ide::local_type(&written.at, ty);
            }
            for (i, (name, ty)) in sig.inputs.iter().enumerate() {
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
                if let Ty::Ref(_, mutable) = ty {
                    if yield_type.is_some() {
                        return Err(f.at.error("generators cannot capture references"));
                    }
                    let loan = lower.new_loan(i as u8, *mutable, None, &mut body);
                    lower.reference_locals.insert(i as u8, loan);
                }
            }
            (sig, yield_type, lower, body)
        }
    );
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
                *lower
                    .sigs
                    .get_mut(&f.name)
                    .expect("a lowered function has a registered signature") = sig.clone();
            }
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
        let facts =
            f.at.source
                .as_ref()
                .map(|source| lower.generics.source_facts(source));
        let compiled = CompiledFn {
            location: f
                .at
                .source
                .as_ref()
                .map(|source| format!("{source}:{}:{}", f.at.line, f.at.column).into()),
            origins: Arc::new(crate::ownership::Origins {
                standard_library: facts.as_ref().is_some_and(|(_, standard)| *standard),
                source: f.at.source.clone(),
                trap_file: facts.map(|(relative, _)| relative),
                names: lower.slot_names,
                required: lower.generics.required.get(&f.name).cloned(),
            }),
            generator: yield_type,
            sig,
            doc: f.doc.as_str().into(),
            body: body.into(),
            locals: lower.locals.into(),
        };
        generics.active.remove(&f.name);
        generics.complete(&f.name, &compiled.sig);
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
fn lower(resolved: modules::Resolved, heap: &mut Heap) -> Result<Program> {
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
    functions.extend(enums::expand(
        enums
            .into_iter()
            .filter(|e| e.type_params.is_empty())
            .collect(),
        &aliases,
    )?);
    functions.extend(classes::expand(
        classes
            .into_iter()
            .filter(|c| c.type_params.is_empty())
            .collect(),
        &aliases,
    )?);
    let (mut generics, prepared) = generics::prepare(functions, &aliases, protocols)?;
    generics.source_root = source_root;
    generics.standard_sources = standard_sources;
    while !generics.expand_data(&aliases)?.is_empty() {}
    // One entry per function: sized up front, the table is not rehashed key by
    // key at every doubling while the program is lowered (issue #57).
    let declared = generics.pending.len();
    let mut sigs = HashMap::with_capacity_and_hasher(declared, Default::default());
    generics.compiled.reserve(declared);
    let mut returned_fields = HashMap::default();
    // Every function is visited once here. A pass over the whole program
    // costs cache misses per function once the program outgrows the caches,
    // so the checks below visit only the functions they concern (issue #57).
    let mut boundary = Vec::new();
    let mut generator_parameters = false;
    // `prepare` resolved the types of the functions it queued, in queue
    // order. Those `expand_data` queued after them are resolved here.
    let mut prepared = prepared.into_iter();
    for f in &generics.pending {
        let sig = register_resolved(
            f,
            prepared.next(),
            &aliases,
            &mut sigs,
            &mut returned_fields,
        )?;
        generator_parameters |= sig.inputs.iter().any(|(_, t)| t.unresolved_generator());
        if f.foreign.is_some() || f.export.is_some() {
            boundary.push(f.as_ref());
        }
    }
    // The emptied list is released before lowering, which sets peak memory.
    drop(prepared);
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
        let (yields, entry) = generics
            .functions
            .get("main")
            .and_then(|declared| Some((declared.yields, declared.function.as_ref()?)))
            .ok_or_else(|| {
                at.error(format!(
                    "binary application requires a module-level main taking no parameters and returning {MAIN_RETURN_TYPES}"
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
    // Its canonical types imply that the trusted std.report module is loaded.
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
                    let (symbol, function) = generics.instantiate(
                        "std.report.entry_report",
                        vec![error.clone()],
                        &protocol.at,
                        &aliases,
                        &access,
                        "main",
                    )?;
                    if let Some(function) = function {
                        register_signature(&function, &aliases, &mut sigs, &mut returned_fields)?;
                        generics.declare(function);
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
        generics.pending.retain(|f| {
            !sigs[&f.name]
                .inputs
                .iter()
                .any(|(_, t)| t.unresolved_generator())
        });
    }
    loop {
        // Resolving a specialized return type can introduce data instances even
        // when its body only returns Err. Drain those methods before finishing.
        loop {
            let functions = generics.expand_data(&aliases)?;
            if functions.is_empty() {
                break;
            }
            for f in functions {
                register_signature(&f, &aliases, &mut sigs, &mut returned_fields)?;
            }
        }
        let Some(f) = generics.pending.pop_front() else {
            break;
        };
        if ide::active() && ide::skipped(&f.name) {
            continue;
        }
        let lowered = lower_function(
            f,
            heap,
            &mut sigs,
            &mut generics,
            &aliases,
            &access,
            &mut returned_fields,
        );
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
    tail_calls::validate(&ops, &generics)?;
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
