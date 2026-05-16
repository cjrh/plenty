//! The Plenty virtual machine: a stack of [`Value`]s, the [`Heap`] behind it, a
//! dictionary of user-defined functions, and the loop that runs [`Op`]s.
//!
//! Execution is a flat loop over an explicit `frames` stack (§11.8). One frame
//! carries one body (`Rc<[Op]>`) and one program counter; popping a frame that
//! owns a locals slot also truncates `self.locals`. Calls push a new Call
//! frame; match arms push a Block frame that *borrows* the enclosing call's
//! locals; tail calls pop the enclosing Call frame and push a replacement,
//! which is what makes recursive iteration bounded.

use std::collections::HashMap;
use std::error::Error;
use std::rc::Rc;

use log::debug;

use crate::lexer;
use crate::op::{self, CompiledFn, FnSig, MatchArm, Op, Pattern, Ty};
use crate::value::{Heap, Value};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// One execution context on the VM's `frames` stack.
///
/// A `Frame` is either a *call* frame (owns the locals slot starting at
/// `locals_start`) or a *block* frame (a match arm body, or the top-level
/// run; `owns_locals = false`, `locals_start` inherited from the nearest
/// enclosing call). Inheriting `locals_start` lets `LoadLocal` resolve
/// against `self.frames.last()` without walking the stack.
struct Frame {
    body: Rc<[Op]>,
    pc: usize,
    locals_start: usize,
    owns_locals: bool,
}

/// A running Plenty interpreter.
///
/// One call — [`Vm::run`] — lexes, compiles, type-checks, and executes a
/// chunk of source. Everything else is either inspection ([`Vm::stack_repr`],
/// [`Vm::function_names`]) or a single explicit reset ([`Vm::clear`]).
#[derive(Default)]
pub struct Vm {
    stack: Vec<Value>,
    heap: Heap,
    /// Compiled function bodies and their docstrings, shared (`Rc` inside
    /// `CompiledFn`) so a call need not copy either and so a function can
    /// safely call itself.
    functions: HashMap<String, CompiledFn>,
    /// Per-call locals, all calls' frames packed end-to-end into one `Vec`.
    /// The active call's `i`-th input lives at `locals[frame.locals_start + i]`.
    /// One backing allocation amortises across nested and recursive calls.
    locals: Vec<Value>,
    /// The execution-context stack. Empty between `run` calls (the top-level
    /// frame pushed at the start of `run` is popped when its ops are
    /// exhausted, or torn down on error).
    frames: Vec<Frame>,
}

impl Vm {
    pub fn new() -> Vm {
        Vm::default()
    }

    /// Lex, compile, type-check, and execute `source`.
    ///
    /// The flow is **lex → compile → check → exec** (§7, §9, §11.6, §11.8).
    /// All three pre-execution stages are atomic: if any of them fails, *no*
    /// op in this `run` executes and the VM's state — stack, heap, function
    /// dictionary — is unchanged by this call (the heap may carry interned
    /// literals from the abandoned compile, but the heap is append-only
    /// and those bytes are unreachable from the dictionary).
    ///
    /// Output-producing words (`.`, `:listdir`) write to stdout as a side
    /// effect. On an *execution* error, the ops before the failing one have
    /// already run — the stack is left as they left it. Active call frames
    /// and their locals are always torn down before `run` returns, whether
    /// by success or by error: subsequent `run` calls always start with an
    /// empty `frames` stack.
    pub fn run(&mut self, source: &str) -> Result<()> {
        debug!("run: {source:?}");
        let toks = lexer::lex(source)?;
        let ops = op::compile(&toks, &mut self.heap)?;
        // The checker sees the union of (already-defined sigs ∪ sigs in
        // this source). Cloning the `Rc<FnSig>`s is one refcount bump per
        // entry — cheap, and it lets `op::check` own its working table.
        let prior_sigs: HashMap<String, Rc<FnSig>> = self
            .functions
            .iter()
            .map(|(n, f)| (n.clone(), Rc::clone(&f.sig)))
            .collect();
        // Seed the abstract stack from the live runtime stack so a REPL
        // line containing only `+` sees the values left by the previous
        // line (§11.6). `Value -> Ty` is total: every value's runtime tag
        // maps to exactly one checker type.
        let initial_stack: Vec<Ty> = self.stack.iter().map(|&v| Ty::from(v)).collect();
        op::check(&ops, initial_stack, &prior_sigs)?;

        // Push the top-level frame and run the interpreter loop. The
        // top-level frame is a "borrowing" frame (no locals of its own,
        // `locals_start = 0`); the compiler never emits `LoadLocal` here,
        // so the borrowed `locals_start` is never consulted.
        self.frames.push(Frame {
            body: Rc::from(ops.into_boxed_slice()),
            pc: 0,
            locals_start: 0,
            owns_locals: false,
        });
        let result = self.run_loop();

        // Tear down whatever frames remain — empty on success, non-empty on
        // error. Calling code is entitled to assume a clean frames stack
        // before the next `run`.
        while let Some(frame) = self.frames.pop() {
            if frame.owns_locals {
                self.locals.truncate(frame.locals_start);
            }
        }
        result
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

    /// The docstring of a defined function, or `None` if no such function
    /// exists. The docstring is captured at compile time (§11.7) and is the
    /// single thing tools — LSP hover, generated docs, REPL `help` — display
    /// for a function alongside its signature.
    pub fn function_doc(&self, name: &str) -> Option<&str> {
        self.functions.get(name).map(|f| f.doc.as_ref())
    }

    /// The stack-effect signature of a defined function, or `None` if no
    /// such function exists. Together with [`Vm::function_doc`], this gives
    /// tools everything they need to render a function's interface.
    pub fn function_sig(&self, name: &str) -> Option<&FnSig> {
        self.functions.get(name).map(|f| f.sig.as_ref())
    }

    /// Discard every value on the stack. Defined functions are kept.
    pub fn clear(&mut self) {
        self.stack.clear();
    }

    // --- execution -------------------------------------------------------

    /// The main interpreter loop. Reads ops from the innermost frame, pops
    /// finished frames, returns when the frames stack is empty.
    fn run_loop(&mut self) -> Result<()> {
        loop {
            // Fetch the next op, unwinding exhausted frames as needed.
            let op = loop {
                let Some(frame) = self.frames.last_mut() else {
                    // No frames left → top-level done.
                    return Ok(());
                };
                if frame.pc < frame.body.len() {
                    let op = frame.body[frame.pc].clone();
                    frame.pc += 1;
                    break op;
                }
                // Frame is at end-of-body. Pop it, cleaning up its locals
                // slot if it owns one.
                let finished = self.frames.pop().expect("checked just above");
                if finished.owns_locals {
                    self.locals.truncate(finished.locals_start);
                }
            };

            self.exec_op(op)?;
        }
    }

    /// Execute one op against the current frame.
    fn exec_op(&mut self, op: Op) -> Result<()> {
        match op {
            Op::PushInt(n) => self.stack.push(Value::Int(n)),
            Op::PushStr(id) => self.stack.push(Value::Str(id)),
            Op::PushBool(b) => self.stack.push(Value::Bool(b)),
            Op::Add => self.add()?,
            Op::Sub => self.int_binop(i64::checked_sub, "integer overflow")?,
            Op::Mul => self.int_binop(i64::checked_mul, "integer overflow")?,
            Op::Div => self.int_binop(
                |a, b| if b == 0 { None } else { a.checked_div(b) },
                "division by zero",
            )?,
            Op::Eq => self.eq()?,
            Op::Lt => self.cmp_int(|a, b| a < b)?,
            Op::Gt => self.cmp_int(|a, b| a > b)?,
            Op::Not => self.not()?,
            Op::Display => println!("{}", self.stack_repr()),
            Op::Clear => self.clear(),
            Op::ListDir => list_dir()?,
            Op::DefineFn(name, func) => {
                self.functions.insert(name, func);
            }
            Op::Call(name) => self.do_call(&name)?,
            Op::TailCall(name) => self.do_tail_call(&name)?,
            Op::LoadLocal(i) => self.load_local(i)?,
            Op::Match(arms) => self.do_match(arms)?,
        }
        Ok(())
    }

    /// Push the `i`-th local of the active call's frame.
    ///
    /// The compiler only emits `LoadLocal` inside a function body, and a
    /// match-arm block frame inherits its enclosing call's `locals_start`,
    /// so `self.frames.last()` always points at a frame whose
    /// `locals_start` is the right one to index from.
    fn load_local(&mut self, i: u8) -> Result<()> {
        let frame = self
            .frames
            .last()
            .ok_or("LoadLocal executed outside any frame")?;
        let v = *self
            .locals
            .get(frame.locals_start + i as usize)
            .ok_or("LoadLocal index out of range")?;
        self.stack.push(v);
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

    /// `=`: polymorphic equality over Int/Bool/Str. Mixed-type pairs are
    /// rejected by the type checker; the defensive arm below protects
    /// against direct VM construction outside the public `run` path.
    fn eq(&mut self) -> Result<()> {
        let b = self.pop()?;
        let a = self.pop()?;
        let result = match (a, b) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => self.heap.str(a) == self.heap.str(b),
            (a, b) => {
                return Err(format!(
                    "cannot compare {} and {} with `=`",
                    self.render(a),
                    self.render(b)
                )
                .into())
            }
        };
        self.stack.push(Value::Bool(result));
        Ok(())
    }

    /// `<` / `>`: integer-only comparison.
    fn cmp_int(&mut self, op: fn(i64, i64) -> bool) -> Result<()> {
        let b = self.pop_int()?;
        let a = self.pop_int()?;
        self.stack.push(Value::Bool(op(a, b)));
        Ok(())
    }

    /// `not`: pop a Bool, push its negation. Type checker enforces Bool.
    fn not(&mut self) -> Result<()> {
        let top = self.pop()?;
        match top {
            Value::Bool(b) => {
                self.stack.push(Value::Bool(!b));
                Ok(())
            }
            other => Err(format!("`not` requires Bool, got {}", self.render(other)).into()),
        }
    }

    /// Begin a function call: drain inputs into a fresh locals frame and
    /// push a Call frame. Control returns automatically when that frame's
    /// `pc` reaches its body's end (see `run_loop`).
    fn do_call(&mut self, name: &str) -> Result<()> {
        let (sig, body) = self.lookup_fn(name)?;
        let n = sig.inputs.len();
        if self.stack.len() < n {
            return Err(format!("stack underflow calling `{name}`").into());
        }
        let locals_start = self.locals.len();
        // Drain preserves order: `inputs[0]` is the deepest popped value and
        // ends up at `locals[locals_start]`, which is what the compiler
        // assumes when it emits `LoadLocal(0)` for that name.
        let drained_from = self.stack.len() - n;
        self.locals.extend(self.stack.drain(drained_from..));
        self.frames.push(Frame {
            body,
            pc: 0,
            locals_start,
            owns_locals: true,
        });
        Ok(())
    }

    /// Tail call (§11.8). Drain the new args, then pop the enclosing call
    /// frame (along with any match-arm block frames stacked above it) and
    /// push the replacement Call frame *in place* of the old one. The
    /// recursion depth does not grow.
    fn do_tail_call(&mut self, name: &str) -> Result<()> {
        let (sig, body) = self.lookup_fn(name)?;
        let n = sig.inputs.len();
        if self.stack.len() < n {
            return Err(format!("stack underflow calling `{name}`").into());
        }
        // Capture args before we touch the frame stack — they were
        // computed against the old locals and must survive the teardown.
        let drained_from = self.stack.len() - n;
        let new_args: Vec<Value> = self.stack.drain(drained_from..).collect();

        // Pop block frames until we pop the enclosing call frame too.
        loop {
            let frame = self
                .frames
                .pop()
                .ok_or("TailCall executed outside any call")?;
            if frame.owns_locals {
                // Tear down the old call's locals, then install the new
                // call's locals into the slot they just vacated.
                self.locals.truncate(frame.locals_start);
                let locals_start = self.locals.len();
                self.locals.extend(new_args);
                self.frames.push(Frame {
                    body,
                    pc: 0,
                    locals_start,
                    owns_locals: true,
                });
                return Ok(());
            }
            // It was a block frame — no locals to tear down. Keep going.
        }
    }

    /// Pop the matched value, walk arms, push a block frame for the first
    /// matching arm. Exhaustiveness is the checker's job (§11.8); the
    /// runtime `no arm matched` error is defensive only.
    fn do_match(&mut self, arms: Rc<[MatchArm]>) -> Result<()> {
        let value = self.pop()?;
        for arm in arms.iter() {
            if self.pattern_matches(arm.pattern, value) {
                // Inherit the enclosing call's locals from the current
                // frame (which is the one running this `Match` op).
                let locals_start = self
                    .frames
                    .last()
                    .map(|f| f.locals_start)
                    .unwrap_or(0);
                self.frames.push(Frame {
                    body: Rc::clone(&arm.body),
                    pc: 0,
                    locals_start,
                    owns_locals: false,
                });
                return Ok(());
            }
        }
        Err("no `match` arm matched (the checker should have caught this)".into())
    }

    /// Match one pattern against one value. Pure: never modifies VM state.
    fn pattern_matches(&self, pat: Pattern, val: Value) -> bool {
        match (pat, val) {
            (Pattern::Wildcard, _) => true,
            (Pattern::Int(a), Value::Int(b)) => a == b,
            (Pattern::Bool(a), Value::Bool(b)) => a == b,
            (Pattern::Str(a), Value::Str(b)) => self.heap.str(a) == self.heap.str(b),
            _ => false,
        }
    }

    /// Look up a function by name, cloning the `Rc<FnSig>` and `Rc<[Op]>`
    /// out of the dictionary so the dispatcher doesn't hold a borrow on
    /// `self` for the rest of the call setup. Cheap (two refcount bumps).
    fn lookup_fn(&self, name: &str) -> Result<(Rc<FnSig>, Rc<[Op]>)> {
        self.functions
            .get(name)
            .map(|f| (Rc::clone(&f.sig), Rc::clone(&f.body)))
            .ok_or_else(|| format!("undefined function: {name}").into())
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
            Value::Bool(b) => if b { "true" } else { "false" }.to_string(),
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
