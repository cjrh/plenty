//! The Plenty virtual machine: a stack of [`Value`]s, the [`Heap`] behind it, a
//! dictionary of user-defined functions, and the loop that runs [`Op`]s.

use std::collections::HashMap;
use std::error::Error;
use std::rc::Rc;

use log::debug;

use crate::lexer;
use crate::op::{self, Op};
use crate::value::{Heap, Value};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// A running Plenty interpreter.
///
/// One call — [`Vm::run`] — lexes, compiles, and executes a chunk of source.
/// Everything else is either inspection ([`Vm::stack_repr`],
/// [`Vm::function_names`]) or a single explicit reset ([`Vm::clear`]).
#[derive(Default)]
pub struct Vm {
    stack: Vec<Value>,
    heap: Heap,
    /// Compiled function bodies, shared (`Rc`) so a call need not copy the body
    /// and so a function can safely call itself.
    functions: HashMap<String, Rc<[Op]>>,
}

impl Vm {
    pub fn new() -> Vm {
        Vm::default()
    }

    /// Lex, compile, and execute `source`.
    ///
    /// Output-producing words (`.`, `:listdir`) write to stdout as a side
    /// effect. On error, the ops before the failing one have already run — the
    /// stack is left as they left it.
    pub fn run(&mut self, source: &str) -> Result<()> {
        debug!("run: {source:?}");
        let toks = lexer::lex(source);
        let ops = op::compile(&toks, &mut self.heap)?;
        for op in &ops {
            self.exec(op)?;
        }
        Ok(())
    }

    /// Render the stack the way Plenty itself would print it — `[1 2 "three"]`.
    ///
    /// This is a *language-level* view, deliberately independent of how the VM
    /// stores things, so callers (the `.` word, tests) never depend on internal
    /// representation.
    pub fn stack_repr(&self) -> String {
        let rendered: Vec<String> = self.stack.iter().map(|&v| self.render(v)).collect();
        format!("[{}]", rendered.join(" "))
    }

    /// The names of every currently-defined function, sorted.
    pub fn function_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.functions.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    /// Discard every value on the stack. Defined functions are kept.
    pub fn clear(&mut self) {
        self.stack.clear();
    }

    // --- execution -------------------------------------------------------

    /// Execute one instruction against the stack.
    fn exec(&mut self, op: &Op) -> Result<()> {
        match op {
            Op::PushInt(n) => self.stack.push(Value::Int(*n)),
            Op::PushStr(id) => self.stack.push(Value::Str(*id)),
            Op::Add => self.add()?,
            Op::Sub => self.int_binop(i64::checked_sub, "integer overflow")?,
            Op::Mul => self.int_binop(i64::checked_mul, "integer overflow")?,
            Op::Div => self.int_binop(
                |a, b| if b == 0 { None } else { a.checked_div(b) },
                "division by zero",
            )?,
            Op::Display => println!("{}", self.stack_repr()),
            Op::Clear => self.clear(),
            Op::ListDir => list_dir()?,
            Op::DefineFn(name, body) => {
                self.functions.insert(name.clone(), Rc::clone(body));
            }
            Op::Call(name) => self.call(name)?,
        }
        Ok(())
    }

    /// `+`: integer addition, or text concatenation, depending on the operands.
    fn add(&mut self) -> Result<()> {
        let b = self.pop()?;
        let a = self.pop()?;
        let result = match (a, b) {
            (Value::Int(a), Value::Int(b)) => {
                Value::Int(a.checked_add(b).ok_or("integer overflow")?)
            }
            (Value::Str(a), Value::Str(b)) => {
                let joined = format!("{}{}", self.heap.str(a), self.heap.str(b));
                Value::Str(self.heap.add_str(joined))
            }
            (a, b) => {
                return Err(
                    format!("cannot add {} and {}", self.render(a), self.render(b)).into(),
                )
            }
        };
        self.stack.push(result);
        Ok(())
    }

    /// Pop two integers `a b`, push `op(a, b)`; fail with `err` when `op`
    /// returns `None` — overflow, or division by zero.
    fn int_binop(&mut self, op: fn(i64, i64) -> Option<i64>, err: &'static str) -> Result<()> {
        let b = self.pop_int()?;
        let a = self.pop_int()?;
        self.stack.push(Value::Int(op(a, b).ok_or(err)?));
        Ok(())
    }

    /// Run the body of a previously-defined function.
    fn call(&mut self, name: &str) -> Result<()> {
        let body = self
            .functions
            .get(name)
            .cloned()
            .ok_or_else(|| format!("undefined function: {name}"))?;
        for op in body.iter() {
            self.exec(op)?;
        }
        Ok(())
    }

    // --- stack helpers ---------------------------------------------------

    /// Pop one value, or fail with a stack-underflow error.
    fn pop(&mut self) -> Result<Value> {
        self.stack.pop().ok_or_else(|| "stack underflow".into())
    }

    /// Pop one value, requiring it to be an integer.
    fn pop_int(&mut self) -> Result<i64> {
        match self.pop()? {
            Value::Int(n) => Ok(n),
            other => Err(format!("expected an integer, found {}", self.render(other)).into()),
        }
    }

    /// Render a single value as Plenty would print it.
    fn render(&self, value: Value) -> String {
        match value {
            Value::Int(n) => n.to_string(),
            // `{:?}` quotes and escapes the string, so text reads as text.
            Value::Str(id) => format!("{:?}", self.heap.str(id)),
        }
    }
}

/// Print the entries of the current directory, one per line.
fn list_dir() -> Result<()> {
    for entry in std::fs::read_dir(".")? {
        println!("{}", entry?.path().display());
    }
    Ok(())
}
