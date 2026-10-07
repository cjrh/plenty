# Direction

Plenty should feel familiar to a Python programmer while being a conventional,
statically typed, ahead-of-time compiled language. It is not Python compatible
and is not an attempt to reproduce Rust. Prefer a small language with explicit
interfaces over dynamic flexibility or elaborate compile-time machinery.

**Fast compilation is a primary goal**, including at the expense of language
features. Native execution, quick edit/check/run cycles, simple semantics, and predictable
memory use matter. Cranelift is the native backend; work with its strengths.

Plenty supports **AOT only**. There is no interpreter or REPL, and JIT support
is out of scope. Language features and runtime layouts target native compilation
without an obligation to support a second execution engine.

Consequences:

- Every function parameter and return type is declared (a method's class supplies
  the type of its bare `self` receiver). Infer types within a
  function; never require whole-program inference to understand an interface.
- Compile each concrete function once. Begin monomorphic. Defer trait solving,
  specialization, implicit coercion searches, and user-defined compile-time
  execution. Avoid features whose analysis creates an unbounded search space.
- Keep parsing, type checking, ownership checking, and code generation separate.
  Cranelift receives already resolved operations with concrete types.
- Favor straight-line lowering and ordinary control-flow graphs. No dependency
  on LLVM, a Python interpreter, tracing, deoptimization, or runtime reflection.
- Optimize compiler simplicity and latency before adding expensive passes.
  Measure parse/check, lowering, native emission, and linking separately. The
  runtime is compiled once when building Plenty and embedded as a native static
  archive. Each AOT build extracts the archive and links it with the generated object.
- No async/await, dynamic attributes, monkey-patching, metaclasses, inheritance,
  implicit nullable references, or exceptions in the initial language.
