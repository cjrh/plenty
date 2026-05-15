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
    Int(i64),
    Str(StrId),
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
pub enum Ty { Int, Str, Bool }    // base type vocabulary (§11.2)

pub struct FnSig {
    pub inputs:  Vec<(String, Ty)>,   // name+type pairs (names matter; §11.5)
    pub outputs: Vec<Ty>,             // bare types (output names are doc-only)
}

pub enum Op {
    PushInt(i64),
    PushStr(StrId),                // literal already interned into the heap
    Add, Sub, Mul, Div,
    Display,                       // the `.` word
    Clear,                         // the `:clear` word
    ListDir,                       // the `:listdir` word
    DefineFn(String, CompiledFn),  // bind name -> compiled function
    Call(String),                  // invoke a function by name (late-bound)
    LoadLocal(u8),                 // push the i-th input of the active call
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
  header. Inputs are `Word`-then-`Type` pairs until `->`; using a known type
  word (`Int`/`Str`/`Bool`) in the input-name slot is a dedicated "input
  requires a name before the type" error. Outputs are either bare type
  words or `Word`-then-`Type` pairs (the names are discarded). Unknown
  type words are rejected with a "not a known type" error.

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
pub fn check(ops: &[Op], prior_sigs: &HashMap<String, Rc<FnSig>>) -> Result<()>;
```

A pass — not a transformation. Forward abstract interpretation of `ops`
over a tiny type lattice (`Ty`). For each op, the checker pops its
declared inputs from a `Vec<Ty>` shadowing the runtime stack, errors on
underflow or mismatch, and pushes its outputs.

- **Builtin effects are hardcoded.** `PushInt` `() -> (Int)`,
  `PushStr` `() -> (Str)`, `Add` is `(Int Int -> Int)` or
  `(Str Str -> Str)` (mixed types are rejected), `Sub`/`Mul`/`Div` are
  `(Int Int -> Int)`, `Display`/`ListDir` are no-ops on the type stack,
  `Clear` empties it, `LoadLocal(i)` pushes the type at index `i` of the
  enclosing function's input list, `Call(name)` looks up the sig and
  applies its full stack effect, `DefineFn` recursively checks the body
  (no change to the outer stack).
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
  the stack is the REPL case, not an error. Individual op-level errors
  (underflow, mismatch, undefined call) are still caught.
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
    locals: Vec<Value>,    // every active call's locals, packed end-to-end
    frames: Vec<usize>,    // start index of each call's locals frame
}                                              // derives Default
```

The running interpreter. All fields are private.

`locals` and `frames` together implement per-call named locals (§11.5).
The active call's `i`-th input lives at `locals[frames.last().unwrap() + i]`.
One backing allocation amortises across nested and recursive calls; popping
a frame is just `frames.pop()` plus `locals.truncate(frame_start)`.

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

### Execution — `exec` (private)

```rust
fn exec(&mut self, op: &Op) -> Result<()>;
```

Dispatches one `Op`:

| `Op`            | Action                                                          |
|-----------------|-----------------------------------------------------------------|
| `PushInt`/`PushStr` | push the value                                              |
| `Add`           | `add` — polymorphic (see below)                                 |
| `Sub`/`Mul`/`Div` | `int_binop` with `checked_*` arithmetic                       |
| `Display`       | `println!` the `stack_repr`                                     |
| `Clear`         | `clear` the stack                                               |
| `ListDir`       | print directory entries (`list_dir`, a free fn)                 |
| `DefineFn(n,f)` | `functions.insert(n.clone(), f.clone())` — **stack untouched** |
| `Call(n)`       | `call` — set up a locals frame, run the body, tear it down      |
| `LoadLocal(i)`  | push `locals[frames.last() + i]` onto the data stack            |

Helpers:

- `add` — pops two values; `(Int, Int)` → `checked_add`; `(Str, Str)` →
  concatenate into the heap; otherwise an error. Operands are concatenated in
  natural order (`a` then `b`, where `b` was on top).
- `int_binop(op: fn(i64,i64) -> Option<i64>, err)` — pops two integers `a`, `b`
  (with `b` on top), pushes `op(a, b)`, errors with `err` when `op` returns
  `None`. `Sub`/`Mul` use `i64::checked_*`; `Div` uses a closure that maps
  divide-by-zero and overflow to `None`.
- `call(name)` — looks the function up, clones the `Rc<FnSig>` and
  `Rc<[Op]>`, *releases the borrow on `self`*, then drains
  `sig.inputs.len()` values off the data stack into a fresh locals frame,
  executes the body, and tears the frame down — on both success and error,
  so a recoverable failure cannot leave a frame stranded. Self-recursion
  gets a brand-new frame on every entry, which is the whole point of the
  per-call frame design. Cloning the `Rc`s makes recursion and
  self-reference borrow-safe.
- `load_local(i)` — pushes `locals[frames.last() + i]` onto the data stack.
  Only reachable from inside a `call`, since the compiler only emits
  `LoadLocal` inside a function body.
- `pop` / `pop_int` — `pop` errors on underflow; `pop_int` additionally errors
  on a non-integer.
- `render(Value) -> String` — `Int` → decimal; `Str` → `{:?}` (quoted/escaped).

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

`Int + Int` is integer addition; `Str + Str` is concatenation; any other
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
  frame, leftmost (deepest) first. So `: f { a Int b Int -> ... }` invoked
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

### Built-in words summary

| Word       | Effect                                                       |
|------------|--------------------------------------------------------------|
| `+ - * /`  | binary arithmetic (`+` also concatenates text)               |
| `.`        | print the whole stack (does **not** pop)                     |
| `:clear`   | discard every value on the stack                             |
| `:listdir` | print the entries of the current directory                   |
| `: name { sig } "doc" body ;` | define a function with mandatory header and docstring |
| `:name`    | call the function `name`                                     |

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
| `Add`              | `checked_add` for `Int Int`, runtime call for `Str Str`   |
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
: hypot  { a Int b Int -> Int }           a a * b b * + sqrt ;
: divmod { a Int b Int -> q Int r Int }   a b / a b mod ;
: zero   { -> Int }                       0 ;
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

1. The `->` arrow is always present. `{ -> Int }` (no inputs) and
   `{ x Int -> }` (no outputs) are both legal; `{ x Int }` (no arrow) is
   not.
2. Every function definition carries a header. An unsignatured definition
   is a compile error. Inference inside the body is welcome; inference of
   the public signature is not — it would defeat the documentation
   purpose the type system exists for.
3. The header sits between the function name and the body, as one
   brace-delimited unit. No other position is legal.

**Type vocabulary.** The base types are:

- `Int` — 64-bit signed integer.
- `Str` — heap-backed string (held by `StrId`).
- `Bool` — `true` or `false`. Produced by literal `true` / `false` and by
  comparison operators; consumed by conditionals (when control flow lands,
  §11.6).

Arrays and sum types are deferred (§12.7, §12.14). No further base types
are planned for the first pass.

**No implicit conversions.** A value of one type is never silently accepted
where a value of another type is expected. In particular, **an `Int` in a
position that expects a `Bool` is a type error** — Plenty does not have
the "`0` is false, anything else is true" convention. The hard-rule
alternative wins: a `Bool` is a `Bool`, and the only way to get one is to
produce one (a literal, or a comparison). This forecloses an entire class
of "what does truthiness mean here?" questions before control flow even
arrives.

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
  and pushes it again; `swap` swaps; `+` dispatches `Int Int -> Int`
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

Deferred: **branch joins.** When control flow lands (an `if`-equivalent,
loops, or a `match` on a sum type), both arms of a branch must produce
the same stack effect, and the checker has to enforce that. The
mechanism — typically requiring both arms to agree pointwise at the join
— is not designed here because control flow itself is not designed yet.
It is flagged so the cost is on the books rather than discovered later.

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
   : hypot { a Int b Int -> Int }
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
   stack effects are hardcoded (`+` is polymorphic over `Int Int` and
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
4. **No file-execution mode.** The binary is REPL-only; there is no
   `plenty path/to/file.plenty`. This is a prerequisite for AOT and likely
   the next concrete step.
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
11. **Unbounded recursion overflows the host stack.** `call`/`exec` recurse on
    the Rust call stack; there is no depth limit and no tail-call handling.
12. **`i64` only.** No floating point, no other numeric widths.
13. **Embedding API is implicit.** Hosts get `Vm::new` / `Vm::run` /
    `Vm::stack_repr`, but there is no typed push/pop or way to register a host
    function. §11.1 implies this surface will grow; the shape is open.
14. **No sum types.** **(direction)** Option and Result are the obvious shape
    once control flow lands: single-slot values (discriminator + payload —
    fits the 16-byte invariant for `Int`/`Str` payloads), and their dispatch
    is the same branch-join problem §11.6 already defers. Design them
    alongside `if` / control flow, not before either is on paper. The open
    question is surface: anonymous quotations (Factor-style `[ ... ]` arms)
    versus dedicated `match-*` words that take named handler functions.
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
    position, so `{ 2 Int -> Int }` treats `2` as a name and the body
    that mentions `2` would load that local instead of pushing two.
    `{ + Int Int -> Int }` is the same problem with an operator
    character. The simple rule "input names must not parse as `i64`
    and must not be one of `+ - * /`" would foreclose this with
    almost no implementation cost.
17. **No Bool literals or comparison operators.** §11.2 names `Bool`
    as a base type produced by `true` / `false` and by comparisons —
    neither is implemented. Now that the type checker is in, the
    absence is load-bearing: a function declared to return `Bool`
    can only do so by forwarding an input. The next step is the
    literals and comparison ops, but they want to land alongside
    control flow (§11.6 branch joins) since that is the reason
    `Bool` exists in the first place.

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
- The tutorial in `README.md` between the `TUTORIAL` markers is generated, not
  hand-edited; `tests/tutorial.rs` is its source of truth.
