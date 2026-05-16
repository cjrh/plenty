# Plenty — Interpreter Design

A precise reference for the implementation of the Plenty interpreter. This is a
living document: when the interpreter changes, this file changes with it.

## 1. Purpose and design north star

Plenty is a stack-based programming language. The current implementation is a
tree-walking interpreter written in Rust; the language is being designed so
that the same source can later be compiled ahead of time to a native binary.

Three convictions shape every design decision:

1. **Low memory consumption is the headline property.** Stack languages can
   run in very little memory; Plenty leans into that. Stack slots stay small
   (16-byte `Value`), variable-sized data lives behind handles in a single
   `Heap`, compiled function bodies are shared by reference (`Rc<[Op]>`). Any
   proposed change must state its memory cost.

2. **Encapsulation is how we manage cognitive load.** Stack languages
   (Factor, Forth) are notorious for becoming illegible the moment a program
   grows. Plenty's answer is not richer syntax — it is to make the unit of
   thought a short, well-named, well-documented function and to make those
   functions easy to define, document, type, and compose. A user should
   spend most of their time writing small functions and almost no time
   inspecting their bodies.

3. **Complexity is the enemy.** A feature whose user benefit is marginal but
   whose implementation cost is high is rejected. When in doubt we go the
   *other* way: accept lost flexibility in exchange for a much simpler
   implementation. Dynamic behaviours that fight ahead-of-time compilation
   are first in line to be dropped.

The implementation strategy is **clean layering**: a small set of deep modules
with simple interfaces (see "A Philosophy of Software Design" — deep modules,
information hiding, different layer/different abstraction).

## 2. Goals and non-goals

The goals and non-goals below are deliberately stronger than the current
implementation. They describe what Plenty is being built towards, and exist so
that future changes can be measured against an explicit stance rather than
re-litigated each time.

### 2.1 Goals

- **Run in very little memory.** Small stack slots, handle-based heap, shared
  compiled bodies, no per-call allocation on the hot path.
- **Three deployment modes from one front end.** The same lex/compile path
  feeds all three:
  - **Embed** — Plenty as a Rust library inside a host program.
  - **REPL** — interactive use, statements typed one at a time.
  - **AOT native binary** — a `.plenty` script (or a collection of scripts)
    compiled to a standalone executable, likely via an LLVM backend.
- **Static types, present at the surface.** Type annotations are part of the
  language, not an optional analysis. They serve documentation, AOT codegen,
  and tooling equally.
- **Encapsulation by composition.** Short functions with clear names, type
  signatures and inline documentation are the canonical way to build larger
  programs.
- **Honest documentation.** Tutorial examples are tests; design changes
  update this document; the implementation never silently disagrees with
  what we say it does.

### 2.2 Non-goals

- **Dynamic features that complicate AOT.** Anything that requires the full
  compiler to be present at runtime (`eval`-style mechanisms, redefining
  functions inside a hot loop, untyped late binding across modules) is a
  non-goal. The REPL may relax some of these restrictions; the AOT path
  will not.
- **Rich/clever surface syntax.** Plenty stays close to Forth's "stream of
  words" model. Operator precedence, fluent chains, infix DSLs and the like
  are non-goals — they trade implementation simplicity for marginal user
  benefit.
- **Implementation cleverness for its own sake.** Complex internal
  representations (interned everything, hand-rolled allocators, JIT) are
  only on the table once a simpler design has been proven inadequate.
- **A general scripting language.** Plenty does not aim to replace Python or
  Lua. The target user is someone who wants a small, predictable, low-memory
  language they can embed or ship as a tiny native binary.

## 3. Architecture

Source flows through three layers, each its own module, plus a data layer that
sits underneath all three:

```
 text ──lexer::lex──▶ Tok ──op::compile──▶ Op ──Vm::exec──▶ effects
                                  │                 │
                                  └──── value ──────┘
                              (Value, Heap, StrId)
```

| Layer       | Module      | Responsibility                                          |
|-------------|-------------|---------------------------------------------------------|
| Syntax      | `lexer.rs`  | Raw text → a flat stream of words, resolving quoting.   |
| Operations  | `op.rs`     | Words → fully-resolved instructions (`Op`).             |
| Data        | `value.rs`  | The values on the stack and the heap that backs them.   |
| Machine     | `vm.rs`     | Holds the stack/heap/dictionary; executes `Op`s.        |
| Wiring      | `lib.rs`    | Declares modules; re-exports the public API.            |
| REPL        | `main.rs`   | Read-eval-print loop over the public API.               |

### Module dependency graph (acyclic)

```
value   lexer
  │  ╲   │
  │   ╲  │
  │    op
  │   ╱
  vm
  │
 lib ── main
```

- `value` and `lexer` depend only on `std`.
- `op` depends on `lexer` (`Tok`) and `value` (`Heap`, `StrId`).
- `vm` depends on `lexer`, `op`, and `value`.
- `main` depends only on the crate's public API (`Vm`).

The AOT compilation path (§11.1) is planned as a second consumer of the same
`Op` stream — a sibling to `vm.rs`, not a replacement for it. The intent is
that everything up to and including `Op` is shared between the interpreter
and the AOT backend.

## 4. Data layer — `value.rs`

### `StrId`

```rust
pub struct StrId(u32);   // field private to the module
```

An opaque 4-byte handle to a string held in a `Heap`. Derives
`Clone, Copy, Debug, PartialEq, Eq, Hash`. A `StrId` is only meaningful to the
`Heap` that issued it.

### `Value`

```rust
pub enum Value {
    I8(i8),  I16(i16), I32(i32), I64(i64),
    U8(u8),  U16(u16), U32(u32), U64(u64),
    Str(StrId),
    Bool(bool),
}
```

A single value on the Plenty stack. Derives `Clone, Copy, Debug, PartialEq`.

- **Size: 16 bytes**, and the test `a_stack_slot_stays_small` enforces
  `size_of::<Value>() <= 16`. This is the central memory invariant.
- `Copy`, so the stack never clones values; `Value` carries no owned heap data.
- Variable-sized data (text now; arrays later) lives in the `Heap` and is
  referenced here by handle, never stored inline. There is deliberate room for
  a future `Arr(ArrId)` variant without growing the slot.

### `Heap`

```rust
pub struct Heap { strings: Vec<String> }      // derives Default

impl Heap {
    pub fn add_str(&mut self, s: String) -> StrId;   // store, return handle
    pub fn str(&self, id: StrId) -> &str;            // borrow by handle
}
```

Backing store for values too large for a 16-byte stack slot.

- **Append-only.** Strings produced at runtime are added and never removed.
  There is no deduplication and no reclamation. This is a known limitation
  (§12).
- `str` indexes `strings` directly; it panics only on a handle the `Heap` never
  issued, which is a VM bug, never a user-program error.

## 5. Syntax layer — `lexer.rs`

### `Tok`

```rust
pub enum Tok<'a> {
    Text(&'a str),   // raw inner content of a "..." literal, escapes undecoded
    Word(&'a str),   // an unquoted word — meaning resolved later
}
```

A lexical unit. Derives `Clone, Copy, Debug, PartialEq`. Each token **borrows a
slice of the source** — the lexer allocates nothing. `Tok::Text` carries the
raw inner slice of a `"..."` literal; escape sequences inside it (`\"`, `\\`)
are decoded later, in the compiler, when the text is interned into the [`Heap`].
This keeps `Tok` `Copy` and the lexer allocation-free.

### `lex`

```rust
pub fn lex(source: &str) -> Result<Vec<Tok<'_>>>;
```

Whitespace separates words. A `"` opens a string literal that runs to the next
unescaped `"`, capturing everything between verbatim — newlines, spaces, and
operator characters all included. Inside the literal, `\X` consumes both
characters without interpreting them, so `\"` does not close the string.

The only lex error is an **unterminated string literal**: a `"` with no
matching close quote before end of input. Every other source string is
lexically valid.

Behaviour by source form:

| Source                              | Emits                              |
|--------------------------------------|------------------------------------|
| `"..."`                              | `Tok::Text(inner_slice)`           |
| any other whitespace-bounded run     | `Tok::Word(run)`                   |
| `"` with no matching close quote     | error: unterminated string literal |

There is no separate quoting mechanism beyond `"..."`; characters like `` ` ``
and `~` are ordinary and become parts of words like any other character.

## 6. Operation layer — `op.rs`

### `Op`

```rust
pub enum Ty {                     // base type vocabulary (§11.2)
    I8, I16, I32, I64,
    U8, U16, U32, U64,
    Str, Bool,
}

pub struct FnSig {
    pub inputs:  Vec<(String, Ty)>,   // name+type pairs (names matter; §11.5)
    pub outputs: Vec<Ty>,             // bare types (output names are doc-only)
}

pub enum Op {
    PushInt(i64),
    PushStr(StrId),                // literal already interned into the heap
    PushBool(bool),                // `true` / `false` literal
    Add, Sub, Mul, Div,
    Eq, Lt, Gt,                    // comparisons (Bool result)
    Not,                           // boolean negation
    Display,                       // the `.` word
    Clear,                         // the `:clear` word
    ListDir,                       // the `:listdir` word
    DefineFn(String, CompiledFn),  // bind name -> compiled function
    Call(String),                  // invoke a function by name (late-bound)
    TailCall(String),              // tail-position call; reuses the frame (§11.8)
    LoadLocal(u8),                 // push the i-th input of the active call
    Match(Rc<[MatchArm]>),         // structured branch (§11.8)
    Cast(Ty),                      // integer width conversion (§11.2)
}

pub struct MatchArm {
    pub pattern: Pattern,
    pub body:    Rc<[Op]>,         // arm body — runs against the current stack
}

pub enum Pattern {
    Int(i64),
    Str(StrId),
    Bool(bool),
    Wildcard,
}

pub struct CompiledFn {
    pub sig:  Rc<FnSig>,           // stack-effect signature (§11.2)
    pub doc:  Rc<str>,             // docstring (§11.7)
    pub body: Rc<[Op]>,            // compiled instructions
}
```

A fully-resolved instruction. Derives `Clone, Debug, PartialEq`.

- An `Op` is *resolved*: numbers are parsed, string literals are already in the
  heap, function bodies are already compiled.
- `Op` is **not** size-optimised (it contains `String` and `Rc<[Op]>`; roughly
  40 bytes). This is intentional — the compiled instruction stream is not the
  hot data structure. Only the runtime stack (`Value`) is size-critical.
- Function bodies are `Rc<[Op]>`: compiled once, shared by reference, never
  copied when a function is called or stored.

**`CompiledFn` is one type, used at both layers.** `Op::DefineFn` carries
one *and* `Vm.functions` stores one. Splitting into parallel
compile-time / runtime types would have duplicated the shape with no
abstraction gain, since every field is just shared by reference.

**Three separate `Rc` fields, not `Rc<Inner>`.** `sig`, `doc`, and `body`
each have their own refcount so that, for example, `Vm::function_doc`
returns a `&str` borrowed from the `Rc<str>` directly without forcing
the caller to hold a refcount on the body or signature. Field-level
sharing is the point; `Rc<Inner>` would couple their lifetimes for no
benefit.

### `compile`

```rust
pub fn compile(toks: &[Tok], heap: &mut Heap) -> Result<Vec<Op>>;
```

The **only** path from `Tok` to `Op`. Used for both top-level source and,
recursively, function bodies — hence it depends only on the `Heap` (for
interning), never on the `Vm`. Internally it constructs a `Compiler` and calls
`compile_seq(Stop::EndOfInput)`.

### `Compiler` (private)

```rust
struct Compiler<'t, 'src> {
    toks: &'t [Tok<'src>],
    pos: usize,
    heap: &'t mut Heap,
}
```

A recursive-descent cursor over the token slice. The three pieces of state
travel together through the recursion that handles nested `: ... ;`
definitions, so they are bundled into one struct.

```rust
enum Stop { EndOfInput, Semicolon }
```

- `compile_seq(&mut self, stop: Stop) -> Result<Vec<Op>>` — compiles tokens from
  `pos` until `stop` is reached. `pos` is advanced past every consumed token,
  including `:` and `;`.
  - `;` with `stop == Semicolon` → return the body (the `;` is consumed).
  - `;` with `stop == EndOfInput` → error: `';' has no matching ':'`.
  - `:` → call `compile_definition` (nesting is handled by recursion).
  - any other `Word` → `compile_word`.
  - `Text` → intern into the heap, emit `PushStr`.
  - end of input with `stop == Semicolon` → error: `':' has no matching ';'`.
- `compile_definition(&mut self) -> Result<Op>` — called with `pos` on the name
  token (just past `:`). The four parts, in order: **name**, **type header**,
  **docstring**, **body**. Each is mandatory; missing or malformed parts are
  compile errors with a name-bearing message.
  - *Name.* A `Word` other than `:` or `;`. A `Text` literal or a missing
    token is an error.
  - *Type header.* `compile_sig` consumes `{ name Type ... -> Type ... }`
    (§11.2). The `->` is required; both sides may be empty. Output names
    are accepted but discarded — outputs are stored as bare `Ty`s.
  - *Docstring.* A `Tok::Text` immediately after the header. Escapes are
    decoded via `unescape` and the result is stored as an `Rc<str>`.
  - *Body.* `compile_seq(Stop::Semicolon)`.
  Returns `Op::DefineFn(name, CompiledFn { sig, doc, body })`.

- `compile_sig(&mut self, fn_name: &str) -> Result<FnSig>` — parses one
  header. Inputs are `Word`-then-`Type` pairs until `->`; using a known
  type word (`i8`/.../`i64`, `u8`/.../`u64`, `Str`, `Bool`) in the
  input-name slot is a dedicated "input requires a name before the type"
  error. Outputs are either bare type words or `Word`-then-`Type` pairs
  (the names are discarded). Unknown type words are rejected with a "not
  a known type" error.

### Word resolution

A bare `Tok::Word` inside `compile_seq` is resolved in two steps:

1. **Local name check.** When the compiler is inside a function body, it
   keeps the enclosing function's input names on a small stack
   (`local_scopes: Vec<Vec<String>>`). If the word matches one of the
   *topmost* scope's names, the compiler emits `Op::LoadLocal(index)` and
   stops. Only the topmost scope is consulted: nested function definitions
   do not inherit their enclosing function's locals (§11.5).
2. **`compile_word` fallback.** Otherwise the word is handed to
   `compile_word`, whose resolution order is:

   | Word form                       | Result                                    |
   |---------------------------------|-------------------------------------------|
   | parses as `i64`                 | `Op::PushInt(n)`                          |
   | `+` `-` `*` `/`                 | `Op::Add` / `Sub` / `Mul` / `Div`         |
   | `.`                             | `Op::Display`                             |
   | `:clear`                        | `Op::Clear`                               |
   | `:listdir`                      | `Op::ListDir`                             |
   | `:as-i8` ... `:as-u64`          | `Op::Cast(Ty::...)` — integer width cast  |
   | `:name` (any other `:`-prefix)  | `Op::Call(name)`                          |
   | anything else                   | `Op::PushStr(intern(word))` — bare text   |

### `compile_word` (private)

```rust
fn compile_word(word: &str, heap: &mut Heap) -> Result<Op>;
```

Resolves a single ordinary word against the table above. It never receives
`:` or `;` — `compile_seq` intercepts those — and it is never called for a
word that resolved to a local. The local check sits in `compile_seq` itself,
which is the only caller of `compile_word`.

### `check` (§11.6)

```rust
pub fn check(
    ops: &[Op],
    initial_stack: Vec<Ty>,
    prior_sigs: &HashMap<String, Rc<FnSig>>,
) -> Result<()>;
```

A pass — not a transformation. Forward abstract interpretation of `ops`
over a tiny type lattice (`Ty`). For each op, the checker pops its
declared inputs from a `Vec<Ty>` shadowing the runtime stack, errors on
underflow or mismatch, and pushes its outputs. `initial_stack` is the
abstract stack the checker begins with — the REPL passes the live
runtime stack's types here so a line containing only `+` sees the
values left by the previous line; file-execution mode passes an empty
stack.

- **Builtin effects are hardcoded.** `PushInt` `() -> (i64)`,
  `PushStr` `() -> (Str)`, `Add` is `(T T -> T)` for any integer width
  `T` or `(Str Str -> Str)` (mixed widths or mixed types are rejected),
  `Sub`/`Mul`/`Div` are `(T T -> T)` for any integer width `T`,
  `Lt`/`Gt` are `(T T -> Bool)` for any integer width `T`, `Cast(target)`
  is `(T -> target)` for any integer source `T` and target,
  `Display`/`ListDir` are no-ops on the type stack, `Clear` empties it,
  `LoadLocal(i)` pushes the type at index `i` of the enclosing
  function's input list, `Call(name)` looks up the sig and applies its
  full stack effect, `DefineFn` recursively checks the body (no change
  to the outer stack).
- **`prior_sigs` is the VM's dictionary.** The checker copies it, then
  walks `ops` (top-level and nested) collecting every `DefineFn`'s sig
  into the same table. This makes **forward references within a single
  source resolve** without an additional pass: a `Call` op anywhere in
  `ops` may name a function defined later in the same `ops`.
- **Function bodies are checked recursively.** A body's abstract stack
  starts **empty** — `Op::Call` drains inputs into the locals frame at
  runtime, so they are reached via `LoadLocal`, not from the data stack
  — and its locals context is the input types in declaration order. At
  end of body the abstract stack must equal the declared outputs
  exactly; anything else is a type error.
- **Top-level programs have no declared sig.** They are checked op-by-op
  with `locals: &[]` and no end-of-stream invariant: leaving values on
  the stack is the REPL case, not an error. The abstract stack is seeded
  from the live runtime stack (§11.6 "REPL stack continuity"), so a line
  containing only `+` sees the values left by the previous line.
  Individual op-level errors (underflow, mismatch, undefined call) are
  still caught.
- **Branch joins are out of scope.** When control flow lands (§11.6),
  both arms of a branch must agree pointwise at the join; the mechanism
  is deferred with the surface that needs it.

`check` does not produce a typed IR — the `Op` stream stays free of
type information. Keeping types out of `Op` is what lets the AOT
backend (§11.1) take or leave the checker as it sees fit.

## 7. Machine layer — `vm.rs`

### `Vm`

```rust
pub struct Vm {
    stack: Vec<Value>,
    heap: Heap,
    functions: HashMap<String, CompiledFn>,
    locals: Vec<Value>,     // every active call's locals, packed end-to-end
    frames: Vec<Frame>,     // the execution-context stack — see below
}                                                       // derives Default

struct Frame {
    body: Rc<[Op]>,          // the op stream this frame is iterating
    pc: usize,               // index of the next op to run
    locals_start: usize,     // the enclosing call's locals frame start
    owns_locals: bool,       // true → popping this frame tears down the locals
                             //         starting at `locals_start`
                             // false → frame is borrowing an outer call's locals
                             //         (a match-arm block frame, or top level)
}
```

The running interpreter. All fields are private.

`locals` and `frames` together implement per-call named locals (§11.5)
and the loop-based execution model (§11.8). The active call's `i`-th
input lives at `locals[frame.locals_start + i]` where `frame` is the
innermost frame whose `owns_locals` is true. One backing allocation for
`locals` amortises across nested and recursive calls; popping a frame
that owns its locals is `frames.pop()` plus
`locals.truncate(frame.locals_start)`. A match-arm block pushes a frame
that *borrows* the enclosing call's locals — `owns_locals = false` — so
its pop is free. The top-level frame is also a borrowing frame.

### Public API

```rust
pub fn new() -> Vm;
pub fn run(&mut self, source: &str) -> Result<()>;
pub fn stack_repr(&self) -> String;
pub fn function_names(&self) -> Vec<&str>;     // sorted
pub fn function_doc(&self, name: &str) -> Option<&str>;   // captured docstring
pub fn function_sig(&self, name: &str) -> Option<&FnSig>; // captured signature
pub fn clear(&mut self);                       // clears the stack, not functions
```

- `run` — lex, then `op::compile`, then `op::check` against the union of
  the VM's existing sigs and the sigs in the just-compiled `ops`, then
  `exec` each op in turn. Every pre-execution stage is atomic (see §9):
  if any of them fails, no op runs and the VM's dictionary, stack, and
  frames are unchanged.
- `stack_repr` — a stable, *language-level* rendering of the stack
  (e.g. `[1 2 "three"]`), deliberately independent of internal representation.
  It is what the `.` word prints and what tests assert against.

### Execution — the interpreter loop (private)

The interpreter is a loop over the `frames` stack. Each iteration reads
the next op from the innermost frame; when a frame's `pc` reaches the
end of its body, the frame is popped (truncating `locals` if it owns
them) and the loop continues with the parent. The loop returns when
`frames` is empty.

One op-dispatch:

| `Op`               | Action                                                                      |
|--------------------|-----------------------------------------------------------------------------|
| `PushInt`/`PushStr`/`PushBool` | push the value                                                  |
| `Add`              | `add` — polymorphic over `(T, T)` and `(Str, Str)`                      |
| `Sub`/`Mul`/`Div`  | `int_binop` with `checked_*` arithmetic                                     |
| `Eq`/`Lt`/`Gt`     | pop two, compare, push a `Bool` (`Eq` polymorphic; `Lt`/`Gt` integer-only)      |
| `Not`              | pop a `Bool`, push its negation                                             |
| `Display`          | `println!` the `stack_repr`                                                 |
| `Clear`            | clear the data stack                                                        |
| `ListDir`          | print directory entries                                                     |
| `DefineFn(n,f)`    | `functions.insert(n.clone(), f.clone())` — **stack untouched**              |
| `Call(n)`          | drain the callee's inputs into a new locals frame, push a Call frame        |
| `TailCall(n)`      | pop block frames + the enclosing Call frame, then push the replacement      |
| `LoadLocal(i)`     | push `locals[frame.locals_start + i]` onto the data stack                   |
| `Match(arms)`      | pop the matched value, pick the first matching arm, push a Block frame      |

Helpers and conventions:

- `add` — pops two values; `(T, T)` → `checked_add`; `(Str, Str)`
  → concatenate into the heap; otherwise an error. Operands are
  concatenated in natural order (`a` then `b`, where `b` was on top).
- `int_binop(op: fn(i64,i64) -> Option<i64>, err)` — pops two integers
  `a`, `b` (with `b` on top), pushes `op(a, b)`, errors with `err` when
  `op` returns `None`. `Sub`/`Mul` use `i64::checked_*`; `Div` uses a
  closure that maps divide-by-zero and overflow to `None`.
- `Call(n)` — looks the function up, clones the `Rc<FnSig>` and
  `Rc<[Op]>`, drains `sig.inputs.len()` values off the data stack into
  a fresh locals frame, and **pushes a `Call`-kind frame** onto
  `frames`. Control returns to the parent frame automatically when
  this body's `pc` reaches its end; the locals are torn down at that
  point. Cloning the `Rc`s decouples the function dictionary from the
  active body, which is what makes self-recursion borrow-safe.
- `TailCall(n)` — same drain into a temporary, then pop block frames
  off `frames` until the enclosing Call frame is reached, tear down
  *its* locals, pop it, and push the replacement Call frame with the
  drained args. Net effect: the recursion depth does not grow, and the
  data-stack arguments slot into the same range of `locals` the old
  call vacated.
- `Match(arms)` — pops the matched value, walks `arms` in order,
  pushes a Block-kind frame for the first arm whose pattern matches.
  The checker has already verified exhaustiveness, so the search
  cannot fall off the end on a compiled program — but the runtime
  raises an error if it ever does, as a defence against direct VM
  construction outside the public `run` path.
- `load_local(i)` — pushes `locals[frame.locals_start + i]` onto the
  data stack. Only reachable inside a function body (the compiler
  never emits `LoadLocal` at the top level), so `frame.locals_start`
  always refers to a real frame.
- `pop` / `pop_int` / `pop_bool` — pop one value; the `_int` /
  `_bool` variants additionally error on the wrong type.
- `render(Value) -> String` — `i64` → decimal; `Str` → `{:?}`
  (quoted/escaped); `Bool` → `true` / `false`.

## 8. Language semantics

### Evaluation model

- A program is a whitespace-separated stream of words. Each word either pushes a
  value or operates on values already on the stack.
- Within one `run` call: lex → compile-all → execute-all.
- **State persists across `run` calls.** In the REPL each input line is a
  separate `run`; the stack, heap, and function dictionary all carry over.

### Operand order

Binary operators take the top two stack values. For non-commutative operators
the deeper value is the left operand: `a b -` computes `a - b`, `a b /`
computes `a / b`.

### `+` is overloaded

`integer + integer` is integer addition; `Str + Str` is concatenation; any other
combination is an error. `-`, `*`, `/` are integer-only.

### Numbers

The integer type is `i64`. Overflow is an **error**, not a panic or a wrap
(all arithmetic uses `checked_*`). Division by zero is an error.

### Text

A bare word that is not a number, an operator, or a `:`-prefixed word is text.
A double-quoted string `"..."` is also text, and is the only way to write text
containing whitespace or characters that would otherwise be read as operators.
Inside `"..."`, `\"` and `\\` are the only escape sequences recognised; any
other `\X` is a compile error. `+` concatenates text.

### Functions

- Defined with `: name { sig } "docstring" body... ;`. All four parts are
  mandatory (§11.2 for the header, §11.7 for the docstring). The compiler
  rejects any definition that omits or malforms one of them with a
  name-bearing error. Definition is a **compile-time** construct: the body
  is carved out of the token stream and compiled to a nested `Rc<[Op]>`;
  the signature is parsed into an `FnSig` and stored as an `Rc<FnSig>`;
  the docstring is decoded once and stored as an `Rc<str>`. All three live
  inside the `CompiledFn` that `Op::DefineFn` carries.
  **Defining a function never touches the runtime stack.**
- Definitions nest (`: outer { ... } "..." ... : inner { ... } "..." ... ; ... ;`),
  handled by the recursion in `compile_seq`/`compile_definition`.
- Called with `:name`. Calls are **late-bound at runtime**: `Op::Call`
  stores the name and resolves it against the dictionary at execution
  time, so a function may refer to one defined later (as long as the
  definition runs before the call does).
- The signature **is** type-checked, before execution (§6 `check`,
  §11.6). A body whose ops do not consume and produce exactly what the
  declared sig promises is rejected pre-execution; so is a call whose
  argument types do not match the callee's inputs, and a call to a
  function not defined anywhere in this source or in the existing
  dictionary.

### Locals

The input names in a function's header are in scope for the whole body
(§11.5). Inside the body, writing one of those names pushes the
corresponding value onto the data stack.

- A call drains its declared inputs off the data stack into a fresh locals
  frame, leftmost (deepest) first. So `: f { a i64 b i64 -> ... }` invoked
  as `1 2 :f` enters the body with no `a`/`b` on the data stack and with
  `locals = [..., 1, 2]`; `a` and `b` are how the body reaches them.
- A bare word inside a body that *matches* an input name compiles to
  `Op::LoadLocal(index)`. A bare word that does not match any input falls
  through to the existing word resolution (§6) — numbers parse, operators
  dispatch, `:name` calls, and unknown bare words still push as text.
- Scope is the *innermost* function body only. A nested `: ... ;` inside
  another definition has its own locals; the inner body cannot see the
  outer's. The compiler enforces this by only consulting the topmost entry
  of its `local_scopes` stack on a name lookup.
- Calls tear their frame down on every exit, including error returns. A
  recoverable error inside a call therefore cannot leave the VM's frame
  state inconsistent — the next `run` call starts with the same empty
  frame stack the failing one started with.
- Each call gets a brand-new frame, so self-recursion (once control flow
  lands and termination is possible) sees its own `n`/`a`/etc., not the
  enclosing call's.

A function with more than `u8::MAX` (255) inputs is rejected at
compile time, since `Op::LoadLocal` indexes its frame with a `u8`. The
limit is a degenerate-input guard, not a tuning knob.

### Control flow

Plenty has one branching primitive, `match`, and no looping primitive
(§11.8). The full surface is:

```forth
value match
  PATTERN [ BODY ]
  PATTERN [ BODY ]
  _       [ BODY ]
end
```

- `match` pops one value off the data stack and dispatches on it.
- Each arm is a *pattern* (a typed literal — `0`, `true`, `"hello"`
  — or `_`) followed by a *bracketed block*. The first arm whose
  pattern matches runs; subsequent arms do not.
- An arm body runs against the same data stack and the same locals
  frame as the surrounding code. The brackets are syntactic structure,
  not a value.
- `end` closes the match.

The checker enforces two properties at compile time: every arm leaves
the stack in the same shape (the *branch join*), and every match is
exhaustive (both `true` and `false` for `Bool`, a `_` arm for `i64` or
`Str`). A non-exhaustive match is a compile error, not a runtime one.

### Iteration is recursion

A function that needs to repeat calls itself. The compiler detects when
a call sits in tail position — last op of the body, or last op of a
match arm whose enclosing match is itself in tail position — and emits
`Op::TailCall` in place of `Op::Call`. The interpreter reuses the
current call's locals frame for a `TailCall`, so tail-recursive loops
do not grow the call stack. Non-tail calls grow an explicit frame
stack on the heap, not the host's Rust call stack, so even deep
non-tail recursion is bounded by available memory rather than by a
host ulimit. There are no `for`, `while`, or `do` words.

### Built-in words summary

| Word           | Effect                                                                 |
|----------------|------------------------------------------------------------------------|
| `+ - * /`      | binary arithmetic (`+` also concatenates text)                         |
| `= < >`        | comparisons; push a `Bool` (`=` is polymorphic; `< >` are integer-only)    |
| `not`          | pop a `Bool`, push its negation                                        |
| `true` `false` | push the `Bool` literal                                                |
| `match … end`  | dispatch on the top-of-stack value (§11.8)                             |
| `[ … ]`        | compile-time block — only valid as a match-arm body (§11.8)            |
| `_`            | wildcard pattern (in match-arm position only)                          |
| `.`            | print the whole stack (does **not** pop)                               |
| `:clear`       | discard every value on the stack                                       |
| `:listdir`     | print the entries of the current directory                             |
| `: name { sig } "doc" body ;` | define a function with mandatory header and docstring   |
| `:name`        | call the function `name`                                               |

## 9. Error handling

- Error type: `Box<dyn Error>`, aliased privately as
  `type Result<T> = std::result::Result<T, Box<dyn Error>>` in `lexer.rs`,
  `op.rs`, and `vm.rs`. Errors are currently stringly-typed (see §12).
- **Pre-execution errors are atomic**: if `lexer::lex`, `op::compile`,
  or `op::check` fails, no `Op` executes and the VM's stack, heap, and
  function dictionary are unchanged. (The heap may carry interned
  literals from the abandoned `compile`, but the heap is append-only
  and those bytes are unreachable from the dictionary, so they are
  benign — see §12.1 on heap reclamation.)
- **Execution errors are not atomic**: if `exec` fails partway through a
  program, the ops before the failing one have already run and their effects on
  the stack/heap/dictionary stand. Locals frames *are* unwound on every
  call exit, so an execution error inside a body cannot leave a stranded
  frame behind (§8 Locals).
- Error categories:
  - **Lexing**: unterminated string literal (`"` with no matching close
    quote).
  - **Compilation**: malformed definitions (unmatched `:`/`;`, missing
    or non-word function name, missing header, missing docstring),
    invalid escape sequences inside `"..."`, function with more than
    `u8::MAX` inputs.
  - **Type checking** (§11.6): stack underflow in a body, type mismatch
    on an op's inputs, mismatch between a body's actual end-of-body
    stack and its declared outputs, call to a function neither in the
    VM's dictionary nor defined in the same source, mismatched argument
    type at a call site.
  - **Execution**: arithmetic overflow, division by zero. Stack
    underflow and unknown-function errors at runtime are now degenerate
    (the checker rules them out for compiled sources), but the runtime
    still raises them defensively — they protect against direct
    construction of `Vm` state outside the public API.
- The REPL (`main.rs`) reports an error with `eprintln!` and continues; it does
  not abort the session.
- **Errors involving a function are name-bearing.** Header-parsing
  errors begin `"function \`<name>\` type header: ..."`; body-level
  type errors are wrapped `"in \`<name>\`: ..."`. Stack-language
  errors are hard to localise — anchoring them to a definition is the
  cheapest way to give the reader a starting point.

## 10. Testing and documentation infrastructure

### `tests/test_basic.rs`

Representation-independent regression tests: arithmetic, `"..."` string
literals, function definition with mandatory header and docstring,
function-scoped named locals, type-error rejection (bodies that don't
match their declared sigs, mismatched calls, mixed-type `+`), and the
`size_of::<Value>() <= 16` invariant. Assertions are made against
`stack_repr` / `function_names`, never against internal Debug output.

### `tests/tutorial.rs` — tutorial-as-tests

The tutorial is defined here as data and is the single source of truth:

```rust
struct Example { title, prose, program, stack }   // all &'static str
const EXAMPLES: &[Example];
```

- `verify_and_render()` makes one pass over `EXAMPLES`: it runs each `program`,
  asserts `stack_repr()` equals the recorded `stack`, and renders the Markdown
  for that example. The check and the render share a pass, so rendered output
  is always verified output.
- `splice_tutorial()` replaces the text between the
  `<!-- BEGIN TUTORIAL -->` / `<!-- END TUTORIAL -->` markers in `README.md`,
  leaving the markers and all hand-written prose intact.
- The single test `readme_tutorial_stays_in_sync`:
  - normally — asserts `README.md` matches the rendered output (fails on drift);
  - with `UPDATE_README=1` in the environment — rewrites the section instead.
- The splice is idempotent: re-running the update produces a byte-identical file.

CI runs `cargo build` + `cargo test`, so example drift cannot merge.

## 11. Design intent for future iterations

This section captures *committed direction* — not yet implemented, but settled
enough that incoming changes should be measured against it. New work that
contradicts these intents needs a design-doc update first, not a quiet
deviation.

### 11.1 Compilation targets

Plenty supports three deployment modes from one front end. The lex/compile
pipeline (`lexer` → `op::compile` → `Op`) is shared; only the consumer of
`Op` differs.

```
text ──lex──▶ Tok ──compile──▶ Op ──┬── Vm::exec ──▶ effects                       (REPL / embed)
                                    │
                                    └── resolve ──▶ Program ──▶ codegen ──▶ binary  (AOT)
```

- **Embed.** Plenty is a Rust library. The crate already re-exports `Vm`;
  hosts construct one and call `run` / `stack_repr`. The embedding API is
  expected to grow (typed push/pop, function registration from the host)
  but should stay a small surface.
- **REPL.** Already implemented in `main.rs`. The REPL is allowed
  flexibilities the AOT path does not have — most importantly, definitions
  and calls interleaving across input lines.
- **AOT native compilation.** A `.plenty` script (or a small set of scripts)
  compiles to a standalone native binary. The current intent is to lower
  `Op` to LLVM IR and let LLVM produce the executable; this is provisional
  until a simpler backend has been ruled out. The AOT path will reject
  programs that rely on truly dynamic behaviour (e.g. defining a function
  whose body is not known at compile time). Those restrictions are part of
  the language contract for the AOT mode, not bugs.

Architectural consequence: nothing below the `op` layer may depend on the
`vm` layer. The `Op` stream must remain fully self-contained — that is what
makes a second backend possible.

#### Op-stream readiness for AOT

§11.1's claim — that one `Op` stream feeds both the interpreter and an AOT
backend — only holds if the existing `Op` variants can be lowered to native
code without a redesign. This subsection inventories the current set
against that claim. It is a design check, not an implementation plan.

##### Op-by-op survey and the resolution pass

For each current `Op`, what an AOT backend would do with it:

| `Op`               | AOT treatment                                             |
|--------------------|-----------------------------------------------------------|
| `PushInt(n)`       | push the constant onto the runtime stack                  |
| `PushStr(id)`      | push a `StrId` constant; literals baked as static data    |
| `Add`              | `checked_add` for `integer`, runtime call for `Str Str`   |
| `Sub`/`Mul`/`Div`  | `checked_*` with an error branch on `None`                |
| `Display`          | runtime call                                              |
| `Clear`            | runtime call (reset stack length)                         |
| `ListDir`          | runtime call                                              |
| `LoadLocal(i)`     | load from the current locals frame                        |
| `Call(name)`       | **does not survive** — needs resolution to a direct call  |
| `DefineFn(n, f)`   | **does not survive** — must be extracted before codegen   |

Seven of nine variants lower trivially. The two outliers are
interpreter-flavoured: `Op::Call(String)` is late-bound by name, and
`Op::DefineFn` installs a runtime dictionary entry. AOT consumes a closed
source set, so both collapse to compile-time work in one pass over the
top-level stream:

1. Walk the stream collecting every `DefineFn` into a function table
   keyed by name; bodies are walked recursively for nested definitions.
2. Assign each function a `FnId` (`u32` suffices).
3. Walk again, in every body and in the residual top-level stream,
   replacing each `Call(name)` with the resolved id. An unresolved
   `Call` is an AOT-mode compile error.

The pass produces `Program { functions: Vec<CompiledFn>, main: Vec<Op> }`,
ready for codegen. **`Op` itself does not change**: the pass sits between
`op::compile` and the AOT backend, mirroring how `op::check` sits between
`op::compile` and `Vm::exec`. §11.1's architectural claim holds without
modification — the AOT-specific work is one additional pass on the same
shared stream.

The interpreter does not run this pass. Late binding is a feature there,
not a deficiency.

##### AOT-mode language restrictions

The resolution pass forces, in writing, the surface restrictions §11.1
already implied:

- **Closed world.** Every `Call(name)` must resolve to a `DefineFn` in
  the same source set. The dictionary the REPL builds incrementally is
  not available.
- **No redefinition.** Two `DefineFn`s for the same name are a compile
  error in AOT mode. (In REPL mode the second shadows the first.)
- **Top-level effects are `main`.** Whatever ops sit outside any
  definition become the program's entry point, executed in order. There
  is no other notion of "main".

These are part of the AOT contract, not bugs.

##### Runtime library footprint

AOT-compiled programs link a small `libplenty` written in Rust:

- A growable `Value` stack — the interpreter's `Vec<Value>` re-used
  verbatim.
- A growable `Heap` — the interpreter's append-only `Vec<String>` re-used
  verbatim.
- Per-op helpers (`plenty_concat`, `plenty_display`, `plenty_clear`,
  `plenty_listdir`) and a handful more for arithmetic error paths.
- A static data section emitting every compile-time-interned string with
  its stable `StrId`.

Memory cost stays consistent with §1's north star: stack and heap are the
same data structures the interpreter uses, sized identically. The AOT
binary does **not** embed the compiler.

##### Codegen tactics — runtime stack, not SSA

Two ways to lower a stack VM: keep a runtime `Vec<Value>` and emit
`push`/`pop` calls, or lift each slot to an SSA value at codegen time.
The second is faster but requires a shadow stack for `Display` and
`Clear` (both inspect the whole live stack), which gives most of the
advantage back. The runtime-stack approach is materially simpler and is
what AOT mode uses. SSA lifting is reopened only if profiling on real
Plenty programs shows the per-op call overhead dominates.

Branch joins (§11.6) cost almost nothing under this model: when control
flow lands, the join block sees one runtime stack whose length both arms
were already obliged to keep consistent (the type checker enforces the
stack-effect agreement; codegen just emits two basic blocks and a join).
Under SSA lifting the same problem is the classic phi-node case. Another
reason to start with the runtime stack.

##### Backend choice — deferred

LLVM and Cranelift are the two plausible backends. LLVM produces better
optimised code and has the larger ecosystem; Cranelift is pure Rust, has
no multi-hundred-megabyte toolchain dependency, and produces binaries
faster. For a language whose hot path is calls into a small runtime
library, LLVM's optimisation advantage is small. The decision is
deferred until codegen actually starts, but the working assumption is
Cranelift on §11.4 grounds.

##### Sequencing implication

Nothing in the survey requires changing `Op` before adding control flow.
Conditionals and loops can land next, against the existing `Op` stream;
the resolution pass and codegen stay decoupled from that work. The one
nuance is that branch joins (§11.6) need to be designed against both
the type checker and the eventual AOT backend at the same time — but
the runtime-stack codegen makes that joint design cheap, as noted above.

### 11.2 Type system

Plenty will have a **static type system with annotations at the surface**.
Types serve three audiences equally: the user (as documentation), the AOT
backend (as a precondition for efficient codegen), and tools (as a
machine-checkable interface description).

**Surface syntax.** Every function definition carries a single header
declaration between its name and its body:

```forth
: hypot  { a i64 b i64 -> i64 }           a a * b b * + sqrt ;
: divmod { a i64 b i64 -> q i64 r i64 }   a b / a b mod ;
: zero   { -> i64 }                       0 ;
: greet  { name Str -> Str }              hello name + ;
```

The header is a brace-delimited unit with three parts:

- **Inputs**, written as `name Type` pairs. The names are the
  function-scoped locals (§11.5); the types are checked. Names and types
  alternate as ordinary whitespace-separated words.
- An arrow `->`.
- **Outputs**, written either as bare `Type`s or as `name Type` pairs.
  Output names are documentation-only — the type checker treats them as
  comments. They exist so multi-output signatures like `divmod`'s read
  clearly, without forcing ceremony on simpler functions.

The header *is* the binder *is* the signature. Input names and input
types appear in exactly one place, together. The drift between binder
and signature that arrangements like Factor's `:: ( ... )` invite is
structurally impossible here.

**Three rules that hold without exception.** Mandatory rules give the
language a small, learnable shape; partial or contextual rules do not.

1. The `->` arrow is always present. `{ -> i64 }` (no inputs) and
   `{ x i64 -> }` (no outputs) are both legal; `{ x i64 }` (no arrow) is
   not.
2. Every function definition carries a header. An unsignatured definition
   is a compile error. Inference inside the body is welcome; inference of
   the public signature is not — it would defeat the documentation
   purpose the type system exists for.
3. The header sits between the function name and the body, as one
   brace-delimited unit. No other position is legal.

**Type vocabulary.** The base types are:

- `i8`, `i16`, `i32`, `i64` — signed integers of the named bit width.
- `u8`, `u16`, `u32`, `u64` — unsigned integers of the named bit width.
- `Str` — heap-backed string (held by `StrId`).
- `Bool` — `true` or `false`. Produced by the literals `true`/`false` and
  by the comparison operators `=`, `<`, `>` (and `not` for negation);
  consumed by `match` (§11.8).

Sized integers are a hard rule, picked over a polymorphic `i64` for two
reasons: it aligns the surface with the low-memory north star (the user
can place a hot inner loop's `n` in a `u8` if that's enough) and it maps
one-to-one onto the integer types the AOT backend (§11.1) will lower to.
Arrays, sum types, and floating-point types are deferred (§12.7, §12.14,
§12).

**No implicit conversions.** A value of one type is never silently
accepted where a value of another type is expected. The rule applies in
two places:

- **Between integers and Bool.** An integer in a position that expects a
  `Bool` is a type error — Plenty does not have the "`0` is false,
  anything else is true" convention. A `Bool` is a `Bool`, and the only
  way to get one is to produce one (a literal, or a comparison).
- **Between integer widths.** `i32 + i64` is a type error. To combine
  values of different widths, the user writes the cast they want — see
  "Explicit casts" below. This forecloses the silent-truncation /
  silent-promotion class of bugs and matches what the AOT backend will
  emit at the IR level anyway.

**Integer literals are `i64`.** Numbers written in source — `42`, `-7`,
`0` — push as `i64`. Smaller widths are reached *only* via an explicit
cast word: `255 :as-u8`, `-1 :as-i8`. The hard rule beats per-literal
type suffixes (`42i8`, `0u32`) on the metric §11.2 cares about — one
form to learn, no special syntax in the lexer.

**Explicit casts.** Eight cast words convert between integer widths:
`:as-i8`, `:as-i16`, `:as-i32`, `:as-i64`, `:as-u8`, `:as-u16`,
`:as-u32`, `:as-u64`. Each pops one integer of any width and pushes its
representation at the target width. The conversion follows Rust's `as`
semantics: widening sign-extends signed sources and zero-extends
unsigned ones, narrowing truncates, equal-width signedness change
reinterprets the bit pattern. Casts that silently change a value's
mathematical meaning (e.g. `-1 :as-u8 → 255u8`) are still allowed —
that is precisely the point of an explicit cast, as opposed to an
implicit conversion.

**Arithmetic, comparison, equality.**

- `+`, `-`, `*`, `/` require **same-width integers** (or `Str Str` for
  `+`, which concatenates). The output has the same width as the
  operands.
- `<`, `>` require same-width integers, output `Bool`.
- `=` accepts any pair of the same type (integer-of-any-width, `Str`,
  `Bool`), output `Bool`.

**Rendering.** Integer values print with their width suffix
(`42i64`, `255u8`, `-1i8`); `Bool` prints as `true`/`false`; `Str` is
quoted. The width is part of how a stack slot reads at a glance — a
`u8` is not interchangeable with an `i64`, so the rendered form makes
that clear.

**Other committed properties.**

- **Checked at compile time.** Type errors are caught during `op::compile`
  (or a dedicated pass immediately after it), never at runtime. A
  type-checked program does not need runtime type tags on the operations
  it uses; runtime tags on `Value` remain only because the REPL and `.`
  need to introspect.
- **Monomorphic in user code.** Parametric polymorphism, row types,
  effects, and other rich features are non-goals for the first pass.
  Polymorphic *builtins* (`dup`, `swap`, `drop`, `over`, `rot`) get their
  stack effects from the compiler's builtin table — a side channel — so
  the surface language stays free of type variables. If user-defined
  polymorphism turns out to be necessary later, that decision reopens; it
  does not need to be made now.
- **Inference where it pays for itself.** Inference is local to a body,
  not whole-program. The body need not be annotated; the header must be.

The data layer (`Value`, `Heap`) does not need to change to support typing —
types live in the `op` layer and above. `PushInt` and `PushStr` already
carry their type implicitly.

### 11.3 Encapsulation as the primary tool

Encapsulation is how Plenty programs stay legible at scale. The language must
make the encapsulating unit — a short, named, documented, typed function —
the cheapest thing to write.

Committed direction:

- **Documentation is a language feature, not a comment convention.** Each
  function may carry a docstring that the implementation captures, and that
  tools (REPL `help`, generated docs) can display. A bare `#`-style comment
  is *not* the documentation mechanism; the docstring belongs to the
  function it documents.
- **Type signatures double as interfaces.** Once §11.2 lands, a function's
  signature plus its docstring should be enough to use it without reading
  its body.
- **Short bodies are cheap.** Defining many small functions must remain
  syntactically light. The current `: name body... ;` form is consistent
  with this; any future syntax that adds ceremony per function will be
  rejected on those grounds.
- **Calls read as one word.** `:name` is one token; a sequence of calls reads
  as a pipeline. Anything that makes a call site multi-line or noisy works
  against this goal.

Modules / namespaces are deferred. They are the obvious next step once
single-file programs grow past a few dozen functions, but adding them now
would cost more than it returns.

### 11.4 Complexity is the enemy — decision rule

Every proposed feature must answer, in writing:

1. **User benefit** — what becomes possible, or significantly easier.
2. **Implementation cost** — modules touched, lines added, new invariants.
3. **Carry cost** — what ongoing maintenance / cognitive burden this
   imposes on every future contributor.

Marginal user benefit at high implementation or carry cost is a rejection.
When a choice is between a flexible-but-complex design and a
constrained-but-simple one, the constrained design wins unless the missing
flexibility is provably essential. This rule applies to surface syntax,
internal representation, and external dependencies equally.

A short list of standing rejections that follow from this rule:

- Operator precedence and infix notation.
- `eval`-style runtime compilation.
- A JIT in the same crate as the interpreter.
- Whole-program type inference.
- Macro / metaprogramming systems.

These can be reopened, but only by demonstrating concrete user benefit that
the simpler design cannot achieve.

### 11.5 Stack juggling and locals

Stack languages collapse into illegibility the moment a function needs more
than a few values in flight at once: the body fills with `dup swap rot over`,
the reader has to simulate the stack mentally, and the function stops being a
unit of thought. This is *the* cognitive-load failure mode that §11.3's
encapsulation story has to defend against. A function whose body is dominated
by stack-shuffling words has already lost.

Committed direction: **function-scoped named locals**, in the spirit of
Factor's `:: name ( a b -- c ) ... ;`. A binder near the function head names
each input value; inside the body those names refer to those values and read
like ordinary words.

- **Scope is the function body, full stop.** No globals, no cross-function
  sharing, no nesting beyond what the body's own structure provides. Locals
  exist to remove `dup swap rot` from one function — nothing more.
- **Names align with the stack-effect signature by construction.** The
  binder *is* the input portion of the type header from §11.2: there is
  one declaration containing both, so the binder names and the signature
  names are literally the same tokens. Drift is impossible.
- **Implementation lives in the `op` layer plus a tiny VM addition.** The
  binder is parsed by `op::compile`; references to a local compile to an
  indexed load against a small per-frame locals array on the `Vm`. No new
  module, no new top-level concept — locals are an implementation detail
  of `Op::Call`'s execution, not a parallel storage system.
- **Surface syntax is settled.** The function header
  `{ name Type ... -> Type ... }` (§11.2) is the binder; the body refers
  to those names as ordinary words. The brace pair that names the inputs
  also types them, so reaching for locals is one declaration, not two.

Rejected alternative: **anonymous registers / a Forth-style `>r` / `r>` aux
stack.** Cheaper to implement (one or two new `Op` variants and a small
slot array), but it is the shallow fix per §11.4 — values now live in two
unnamed places, and the cognitive burden moves rather than disappearing.
Forth's experience with `>r` / `r>` is the evidence: they are widely
considered a footgun. Plenty does not get a return stack or numbered
registers; if a future need demands a between-function scratchpad it will
be a real value type (a tuple, a small array), not a second storage layer.

### 11.6 Type checking — stack effects, not Hindley-Milner

The surface committed in §11.2 is monomorphic, and polymorphic builtins are
typed by a side channel. Together those decisions remove the reason
Hindley-Milner exists: HM is the textbook answer for *inferring* polymorphic
types in a language whose surface lets users write them, and Plenty has no
such surface. The checker Plenty actually needs is **forward abstract
interpretation of the `Op` stream over a tiny type lattice** — the same idea
as the JVM verifier, or as Forth `( -- )` comments if anyone checked them.

Committed direction:

- **The checker is a pass, not a transformation.** It takes `&[Op]` plus a
  side table of declared `FnSig`s and returns `Result<(), TypeError>`. It
  does not produce a typed-IR; the `Op` stream stays free of type
  information. Keeping types out of `Op` is what lets the AOT backend
  (§11.1) take or leave the checker as it sees fit.
- **State is one `Vec<Ty>` shadowing the runtime stack.** For each `Op` in
  the body: pop the types it declares, error on mismatch or underflow,
  then push its output types. At end of body the state must equal the
  function's declared outputs.
- **Builtin effects are hardcoded in the checker.** `dup` peeks the top
  and pushes it again; `swap` swaps; `+` dispatches `T T -> T (any integer width)`
  versus `Str Str -> Str`. No type variables ever enter the checker's
  vocabulary, which is what "polymorphic builtins via side channel"
  literally means. If user-defined polymorphism is ever reopened (§11.2),
  unification can be introduced *then* — not built up-front for a feature
  that is explicitly out of scope.
- **Implementation order is forced.** (1) Parse the header from §11.2.
  (2) Implement function-scoped named locals (§11.5) — they must exist
  before the checker can resolve `a` to a type. (3) Add the checker pass.
  Each piece is small on its own; together they are a few hundred lines.

Rejected alternative: **adopt Hindley-Milner anyway, for its
respectability.** HM would buy correctness Plenty already obtains by
other means, at the cost of a substitution map, an occurs check, and the
cognitive burden of "what *generalised* here?" on every contributor.
That is §11.4's trade exactly in reverse: high implementation and carry
cost for no user-visible benefit. The simpler design wins on the merits.

Landed: **branch joins.** Per §11.8, control flow is `match`, and every
arm must leave the stack in the same shape. The checker snapshots the
abstract stack at `match`, type-checks each arm body against a copy of
the snapshot, and requires the resulting stacks to agree pointwise; the
agreed shape becomes the match's overall stack effect. No new
machinery beyond a per-arm snapshot was needed.

Landed: **REPL stack continuity.** Each `run` call's abstract stack is
seeded from the live runtime stack (`Vec<Value>` → `Vec<Ty>` via
`From<Value> for Ty`), not started empty. Without this, a REPL line
containing only `+` would fail the check even when the previous line left
two compatible values on the stack — the runtime would accept it but the
checker, blind to prior state, would not. Seeding closes that gap: state
persistence (§8) applies to the checker's view as well as the VM's.
There is no new persistent state — `self.stack` remains the single source
of truth, and the abstract stack is derived from it once per `run`.

### 11.7 Documentation and string literals

A function's interface is its signature *and* its docstring. Tools — LSP
hover, generated docs, REPL `help` — depend on both being present; §11.3's
"type signatures double as interfaces" assumes the docstring half exists.

**Surface syntax.** A new lexical form `"..."` is Plenty's string literal.
Between an unescaped `"` and the next unescaped `"`, every character is
taken verbatim — newlines included. Two escape sequences are recognised:

| Sequence | Meaning         |
|----------|-----------------|
| `\"`     | a literal `"`   |
| `\\`     | a literal `\`   |

Any other character following a `\` is a compile error. An unterminated
string (end of input reached before the closing `"`) is a compile error.
The lexer emits one token per literal, carrying the de-escaped inner
content.

`"..."` is used uniformly — for docstrings, for stack-pushed text values,
anywhere a string appears in source. There is no separate "docstring
string" form versus "value string" form.

**Three rules that hold without exception.**

1. **Every function definition carries a docstring.** A function without
   one is a compile error. The docstring is part of the interface; the
   interface is mandatory.
2. **The docstring's position is fixed.** It is the third part of every
   definition, between the type header and the body:

   ```forth
   : hypot { a i64 b i64 -> ... }
       "Euclidean distance from origin."
       a a * b b * + sqrt ;
   ```

   No other position is legal.
3. **The docstring is captured into function metadata at compile time.**
   It is consumed by the compiler as part of the function definition; it
   does not appear on the runtime stack and does not execute.

**Retiring the backtick literal forms.** Both the literal run
`` ` ... ~ `` and the single-word prefix `` `word `` are superseded by
`"..."`. Plenty has exactly one text-literal syntax going forward;
two forms for the same concept is the kind of fluidity §11 explicitly
wants to avoid. The `~` word goes back to being an ordinary word, the
same as any other.

**Designed for the LSP, generated docs, and `help`.** All three want one
thing: given a function name, return its signature and docstring. With
this design, the docstring is one lexer token at one source position,
captured by the compiler as one owned `String` per function. Tools that
link the Plenty crate get it trivially; tools that re-implement parsing
have only a few simple rules to mirror.

### 11.8 Control flow — one branching primitive, recursion for iteration

Plenty has **one** branching primitive — `match` — and **no looping
primitive**. Iteration is recursion plus mandatory tail-call
optimisation. The user-visible control-flow vocabulary is small enough
to state in one line: `match`, `end`, `[`, `]`, `_`. There is no `if`,
no `else`, no `for`, no `while`, no `do`. `Bool` is just a two-variant
value handled the same way as any other finite type.

**`match` is how you branch.**

```forth
: classify { x i64 -> Str } "name a small number"
  x match
    0 [ "zero" ]
    1 [ "one"  ]
    _ [ "many" ]
  end ;
```

Surface:

- `match` consumes the top-of-stack value and dispatches on it.
- Each arm is `PATTERN [ BODY ]`. Patterns are typed literals (`0`,
  `true`, `"foo"`) or `_` (the wildcard).
- `end` closes the match.

Two **mandatory rules** that hold without exception:

1. *Every arm has the shape `PATTERN [ BODY ]`.* No `->` or `=>` between
   pattern and block. No separator between arms. Arm order is
   significant: the first matching arm wins.
2. *Every match is exhaustive.* For `Bool`, both `true` and `false` arms
   must be present (a wildcard arm also satisfies exhaustiveness). For
   `i64` and `Str` (whose value spaces are unbounded), a `_` arm is
   required. The checker rejects non-exhaustive matches at compile time.

**Brackets are compile-time blocks, not quotation values.**

A `[ ... ]` is **syntactic structure**, the same way `: ... ;` and
`{ ... }` already are. The ops between the brackets are compiled into a
separately-stored `Rc<[Op]>` that the match arm holds and that the
runtime executes against the *current* data stack and the *current*
locals frame. There is no `Value::Quot`, no first-class code, no
quotation type in the type system. A bracketed block is reachable only
as a match arm body.

This is the load-bearing decision that lets §11.2's monomorphism remain
intact. First-class quotations would force quotation types — a stack
effect *as* a type — which is the gateway to row polymorphism (Factor's
approach). The block-as-structure reading sidesteps that gateway
entirely.

**`Bool` is not syntactically privileged.** With `match` as the only
conditional, `Bool` is just a two-variant value that you handle the way
you'd handle any other finite type. An `if`-style sugar would be a
second way to say the same thing, which the hard-rules stance refuses
on principle; see also the exhaustiveness rule, which makes "every
conditional has both branches" a property of `match` rather than a
separate rule.

**Branch joins, as promised by §11.6.** The type checker treats each
arm independently — snapshot the abstract stack at `match`, pop the
matched value's type, type-check each arm body against a copy of the
snapshot, require all arms to leave the stack in the same shape. That
shape becomes the stack effect of the match as a whole. A mismatch at
the join is a type error with both shapes named. No new machinery is
needed beyond the per-arm snapshot.

**Iteration is recursion.**

```forth
: countdown { n i64 -> } "print n down to 1, then stop"
  n .
  n 1 > match
    true  [ n 1 - :countdown ]
    false [ ]
  end ;
```

There is no `for`, no `while`, no special looping construct. A function
that needs to repeat calls itself, and the recursive call sits in tail
position — the last op of an arm that is itself the last op of the
function body. The compiler **detects tail position structurally**
during a post-compile pass over each function body: a `Call` is in tail
position if it is the last op of the body, or the last op of an arm
body whose enclosing `Match` is itself in tail position. Detected tail
calls are rewritten in place to `Op::TailCall`.

**Tail-call optimisation is mandatory, not opportunistic.** The
interpreter's main loop recognises `Op::TailCall` and reuses the
current call's locals frame instead of allocating a new one; the call
stack does not grow. Without this guarantee, recursion is not a
legitimate substitute for looping — every iteration would consume host
stack space, which contradicts the low-memory north star (§1) on the
very feature meant to make iteration cheap. The guarantee is part of
the language contract, not an optimisation.

**The interpreter is a loop, not a recursive walker.** To make TCO
work, the prior recursive `exec`-per-op design is replaced by an
explicit interpreter loop with an explicit frame stack (§7). Each
frame carries the body it is executing (`Rc<[Op]>`), a program
counter, and a discriminator that says whether it owns a locals frame
(a call frame) or borrows the enclosing call's (a match-arm block
frame). Executing a `TailCall` pops the current call frame — and any
block frames sitting above it — then pushes the replacement call
frame with the inputs drained from the data stack.

**Pattern binders are deferred with sum types.** Today's patterns are
typed literals and `_`. When sum types land (§12.14), patterns will
additionally introduce *binders* — `Ok x` would bind the payload as a
local `x` scoped to the arm's body, extending §11.5's locals mechanism
per-arm. Nothing in the current design has to change to accommodate
them; the arm-body compilation path already inherits the enclosing
function's locals scope, and pattern binders just push onto it for the
arm's duration.

**Comparison and Boolean vocabulary.** `=`, `<`, `>` are comparison
ops; `not` is boolean negation. `=` is polymorphic over the equality
types (`T T -> Bool (any integer width)`, `Str Str -> Bool`, `Bool Bool -> Bool`); `<`
and `>` are `(T T -> Bool (any integer width))`. Additional comparisons (`!=`, `<=`,
`>=`) and boolean operators (`and`, `or`) are open — not committed
direction, not deliberately omitted, just not on the immediate path.
There is no short-circuit semantics: both operands of `and`/`or` (if
they land) are values already on the stack. Short-circuit dispatch is
what `match` is for.

## 12. Known limitations and open questions

For future iterations. Update this section as items are resolved or added.
Items marked **(direction)** are pinned by §11; items without that tag are
open.

1. **Heap is append-only.** Runtime strings accumulate and are never reclaimed;
   there is no deduplicating interning and no garbage collection. This is the
   most significant open memory issue, despite the low-memory north star.
2. **Type checker — implemented.** **(direction)** §11.2 committed the
   surface syntax and §11.6 the checking approach. Both are now in:
   `op::check` runs forward abstract interpretation of the op stream
   against the union of (VM dictionary sigs ∪ sigs in the current
   source). Function bodies must agree with their declared inputs and
   outputs, calls must agree with their callees' signatures, builtin
   stack effects are hardcoded (`+` is polymorphic over `integer` and
   `Str Str`; everything else is monomorphic). Top-level ops are
   checked op-by-op without an end-of-stream invariant, so the REPL
   case keeps working.

   The runtime is no longer the front line for type errors: `+` is
   still polymorphic at exec time, but the polymorphism cannot meet a
   mismatched pair, because the checker rejects those first. Function
   names are still looked up by string at exec time, but a name that
   would not resolve has been caught by the checker. Both pieces of
   runtime defence remain in place — they protect against direct VM
   construction outside the public `run` path, not against compiled
   source.
3. **No AOT backend yet.** **(direction)** §11.1 commits to one; only the
   tree-walking VM exists. No `Op`-to-IR lowering, no LLVM dependency, no
   `.plenty` file driver.
4. **File-execution mode — implemented.** **(direction)** `plenty FILE`
   reads the whole file, lexes/compiles/checks/runs it on a fresh `Vm`,
   and exits — stdout is the program's, stderr is for diagnostics, and
   the exit status is 0 on success and non-zero on any compile, type, or
   runtime error. The REPL is the no-argument behaviour; `-h`/`--help`
   prints usage. The binary is the only entry point that distinguishes
   the two modes; the `Vm` itself is unchanged. AOT (§12.3) is the
   remaining piece in the file-driven path.
5. **No comment syntax.** **(direction)** §11.7 commits docstrings + the
   unified `"..."` string literal, and the implementation now lands them:
   `"..."` is the canonical string form, backtick literals are retired,
   docstrings are mandatory on every function definition, and the captured
   doc is exposed via `Vm::function_doc`. A separate comment syntax —
   throwaway text the compiler discards, e.g. `#` to end of line — is
   still open. Until that lands, everything in a source file that is not
   a docstring or a string value is significant.
6. **Function-scoped named locals — implemented.** **(direction)** §11.5
   is in: every input declared in the header is in scope for the whole
   body and reached by writing its name. `Op::Call` drains the inputs into
   a per-call locals frame; `Op::LoadLocal(i)` pushes the `i`-th input
   back onto the data stack.
7. **No arrays.** `Value` has design room for `Arr(ArrId)`, but arrays are not
   implemented. They need a real surface syntax and a heap-backed array store.
8. **`.` semantics undecided.** Plenty's `.` prints the entire stack without
   popping; classic Forth `.` pops and prints only the top. Pick one.
9. **Function names are owned `String`s** in `Op::Call` and `Op::DefineFn`, and
   `String` keys in the dictionary. They could be interned (`StrId`) for
   compactness and faster lookup.
10. **Stringly-typed errors.** `Box<dyn Error>` over ad-hoc strings. A typed
    error enum would give callers something to match on.
11. **Tail-call optimisation — implemented.** **(direction)** §11.8
    commits TCO as part of the language contract (recursion is the
    iteration primitive, so it must not grow the call stack). The
    interpreter is now a loop over an explicit frame stack; a
    `Call` in tail position is compiled to `Op::TailCall`, which
    reuses the enclosing call's locals frame instead of pushing a
    new one. Non-tail calls still recurse on the explicit frame
    stack (not the Rust call stack), so deep non-tail recursion is
    bounded by available heap, not by the host's stack ulimit.
12. **Sized integers — implemented.** **(direction)** §11.2 commits to
    `i8`..`i64` and `u8`..`u64` as the integer vocabulary, with no
    polymorphic `Int`. Arithmetic, comparison, and equality require
    same-width operands; explicit cast words (`:as-i8` ... `:as-u64`)
    convert between widths with Rust-`as` semantics. Integer literals
    default to `i64`; other widths are reached only via a cast. The
    deferred piece is **floating point** — `f32`/`f64` and their
    arithmetic, equality (NaN handling), printing, and parsing — kept
    out of this pass to bound the change.
13. **Literal width suffixes.** Integer literals are always `i64`; you
    cannot write `42u8` or `0i32` directly. The hard rule was chosen
    over per-literal suffixes (one form to learn, no special lexer
    syntax), but it makes short snippets verbose
    (`255 :as-u8` for what could be `255u8`). Adding suffixes later is
    a small, additive change if usage shows the cost is real.
13. **Embedding API is implicit.** Hosts get `Vm::new` / `Vm::run` /
    `Vm::stack_repr`, but there is no typed push/pop or way to register a host
    function. §11.1 implies this surface will grow; the shape is open.
14. **No sum types.** **(direction)** Option and Result are the obvious
    shape now that control flow has landed: single-slot values
    (discriminator + payload — fits the 16-byte invariant for
    `i64`/`Str` payloads), dispatched by `match` (§11.8). The surface
    question §12.14 previously held open (anonymous quotations vs
    `match-*` words with named handlers) is settled the third way:
    `match` with compile-time bracketed arms, neither of the two
    routes named here. What's still open is the *declaration* surface
    for user-defined sum types and the corresponding extension of
    `match` patterns with payload binders (`Ok x [ ... ]` binding `x`
    as a local scoped to the arm body) — both flagged by §11.8 as the
    natural next step.
15. **Bare-word-as-text typo safety.** A bare word in body code that
    isn't a builtin, operator, number, `:name` call, or local still
    pushes as text (§6 word resolution). A typo like `dlb` (for `dbl`)
    silently produces the string `"dlb"`; the type checker then
    complains about a `Str` where some other type was expected —
    technically correct, but misleading. The clean fix is to make an
    unresolved name a compile error inside a function body. Not yet
    committed; tracked here so the rough edge isn't forgotten.
16. **Input-name slot accepts numbers and operators.** The header
    parser binds whatever `Tok::Word` it finds in an input-name
    position, so `{ 2 i64 -> i64 }` treats `2` as a name and the body
    that mentions `2` would load that local instead of pushing two.
    `{ + T T -> T (any integer width) }` is the same problem with an operator
    character. The simple rule "input names must not parse as `i64`
    and must not be one of `+ - * /`" would foreclose this with
    almost no implementation cost.
17. **Bool literals and comparison operators — implemented.**
    **(direction)** §11.2 named `Bool` as a base type and §11.8
    committed `match` as its consumer. Both have landed: `true` and
    `false` are literals, `=`/`<`/`>` are the comparison ops, `not`
    negates a `Bool`. `=` is polymorphic over the equality types
    (`T T -> Bool (any integer width)`, `Str Str -> Bool`, `Bool Bool -> Bool`);
    `<`/`>` are `(T T -> Bool (any integer width))`. Further comparisons (`!=`,
    `<=`, `>=`) and boolean operators (`and`, `or`) are open per
    §11.8's last paragraph — uncommitted but unblocked.
18. **Control flow — implemented.** **(direction)** §11.8 is in:
    `match` with bracketed arms is the single branching primitive,
    iteration is recursion plus mandatory TCO (see also §12.11),
    and the type checker enforces exhaustiveness and pointwise
    branch-join agreement.

## 13. Invariants

These must hold; changing one is a deliberate design decision.

- `size_of::<Value>() <= 16` (test-enforced).
- A `StrId` is only ever passed to the `Heap` that issued it.
- `op::compile` and `op::check` both fully succeed before any `Op` is
  executed within a single `run` call. A failure in either leaves the
  stack, frames, and function dictionary unchanged.
- `compile_word` is never called with the words `:` or `;`.
- `value` and `lexer` have no dependencies on other crate modules; the module
  dependency graph stays acyclic.
- **No module below the `op` layer may depend on the `vm` layer.** The `Op`
  stream stays self-contained so a second backend (AOT, §11.1) can consume
  it without dragging the interpreter in.
- **Tail calls do not grow the call stack** (§11.8). A `Call` op in tail
  position is compiled to `Op::TailCall`, which the interpreter implements
  by reusing the enclosing call's locals frame rather than nesting. The
  test `tail_recursion_runs_without_growing_the_call_stack` enforces this
  on a recursion deep enough that the non-TCO interpreter would overflow.
- **Every `match` is exhaustive** (§11.8). The checker requires both arms
  for `Bool` (or a `_`), and a `_` arm for `i64`/`Str`. The runtime
  preserves a defensive "no arm matched" error path but a compiled,
  type-checked program cannot reach it.
- The tutorial in `README.md` between the `TUTORIAL` markers is generated, not
  hand-edited; `tests/tutorial.rs` is its source of truth.
