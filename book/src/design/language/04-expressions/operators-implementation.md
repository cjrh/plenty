# Operator lowering and performance

Operator hooks use the same typed call operations, loan checks, argument ABI,
caller attribution, and reachability/cache machinery as named calls. Reflected
operations evaluate arguments in source order and reorder their already computed
values for the call. They never rerun operand expressions. Dispatch uses the
specialized nominal types; it does not add vtables or runtime type tests.

Keep four costs distinct:

- **Dispatch:** an ordinary direct call, eligible for the same bounded inlining
  as an explicitly named call.
- **Borrowing:** shared parameters lend existing storage without copying matrix
  payloads. The compiler may need an addressable temporary for a computed owner.
- **Results:** inline class result storage uses the normal return ABI. Returning
  a large matrix still requires whatever allocation its implementation specifies.
- **Expression chains:** `(a + b) + c` invokes two operations and may build two
  results. This implementation does not fuse loops, build expression templates,
  or silently change evaluation order or floating-point association. Named
  destination-writing methods remain useful for large numerical workloads.

## Bounded optimization and evidence

The release leaf inliner accepts shared class receivers that only project scalar
fields, alongside its existing scalar inputs and arithmetic. It still requires
a scalar output, no locals, no calls or writes, and at most 24 non-marker typed
operations. Tail calls to eligible leaves also substitute their body when the
caller has no managed locals to clean up before the call. This
benefits ordinary named methods as well as operator hooks. Managed aggregates,
matrix allocation, and result-producing class constructors remain ordinary calls.

The regression probe compares `a + b`, `a.__add__(b)`, and `a.x + b.x` for a
class containing one `f64`. Operator and named calls have identical native
instructions in both build modes. In release mode all three produce identical
12-instruction x86-64 bodies, with no call or jump to the hook. Before the
inliner extension, both call spellings added a five-instruction tail-call wrapper.
This is code-generation evidence, not a claim about matrix-kernel throughput.
Separate tests disable heap allocation across owned class addition and in-place
updates, and check destructor and failure paths.

## Design sources

Python supplies the familiar hook names and the separation between ordinary,
reflected, and in-place operations. Plenty deliberately uses static resolution,
shared value-producing receivers, and unit-returning mutable in-place methods
instead of Python's dynamic `NotImplemented` and replacement-result behavior.
See the [Python data model](https://docs.python.org/3/reference/datamodel.html#emulating-numeric-types).

[D's operator specification](https://dlang.org/spec/operatoroverloading.html)
rewrites operators into named member calls and has separate right-hand and
assignment hooks. Its operator-string templates reduce declaration repetition;
they do not themselves eliminate allocation or intermediate results. Plenty
uses ordinary named methods without adding string-template machinery.

The [C++ operator rules](https://eel.is/c++draft/over.match.oper) likewise transform
operator notation to function calls while preserving the operator's sequencing
rules. That separation motivates keeping source evaluation order independent of
which operand owns the selected hook. Inlining remains an optimization: even
[Clang's always-inline attribute](https://clang.llvm.org/docs/AttributeReference.html#always-inline-force-inline)
does not guarantee substitution. Plenty reuses its bounded leaf substitution
rather than adding an operator-only optimizer or promising that every call vanishes.

[Rust's `Add` trait](https://doc.rust-lang.org/std/ops/trait.Add.html) separates
the right-hand operand and output types. Plenty's signatures likewise need not
return the receiver type; this permits scalar results and explicit `Result`
values without introducing associated types for this feature.
