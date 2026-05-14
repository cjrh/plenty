//! The operation layer: the instruction set the VM executes, and the step that
//! turns lexed words into instructions.
//!
//! An [`Op`] is fully resolved — numbers parsed, string literals already in the
//! heap, function bodies compiled to nested `Op` sequences. A compiled program
//! is just a `Vec<Op>`, run without ever re-lexing its source.

use std::error::Error;
use std::rc::Rc;

use crate::lexer::Tok;
use crate::value::{Heap, StrId};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

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
    /// Define a function: bind `name` to an already-compiled body.
    ///
    /// The body is carved out of the token stream at compile time, so running
    /// this op never touches the runtime stack — whatever is on it stays put.
    DefineFn(String, Rc<[Op]>),
    /// Invoke a user-defined function by name.
    Call(String),
}

/// Compile lexed words into ops, interning string literals into `heap`.
///
/// This is the only path from `Tok` to `Op`. It is used both for top-level
/// source and, recursively, for function bodies, so it depends on nothing but
/// the `Heap`.
pub fn compile(toks: &[Tok], heap: &mut Heap) -> Result<Vec<Op>> {
    Compiler { toks, pos: 0, heap }.compile_seq(Stop::EndOfInput)
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
/// Bundled into a struct because the three things — the tokens, the position
/// within them, and the heap that literals are interned into — all travel
/// together through the recursion that handles nested `: ... ;` definitions.
struct Compiler<'t, 'src> {
    toks: &'t [Tok<'src>],
    pos: usize,
    heap: &'t mut Heap,
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
                Tok::Word(w) => ops.push(compile_word(w, self.heap)?),
                Tok::Text(s) => ops.push(Op::PushStr(self.heap.add_str(s.to_string()))),
            }
        }
        match stop {
            Stop::Semicolon => Err("':' has no matching ';'".into()),
            Stop::EndOfInput => Ok(ops),
        }
    }

    /// Compile a `: name body... ;` definition. The opening `:` has already
    /// been consumed; the cursor sits on the name. A nested `:` inside the body
    /// is handled by the recursive `compile_seq` call, so definitions nest.
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
        let body = self.compile_seq(Stop::Semicolon)?;
        Ok(Op::DefineFn(name, body.into()))
    }
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
