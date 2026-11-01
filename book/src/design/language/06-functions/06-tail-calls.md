# Recursion and tail calls

A source-tail call supplies the function's result without further work. This
includes final expressions, explicit returns, selected conditional-expression
branches, tail `if`/`match` arms, the cases of `return match`, and result
branches of `and`/`or`. A `match` block bound or assigned to a name is not a
tail position. Arithmetic,
wrapping in `Ok` or `Some`, conversion, or `?` after a call makes it non-tail.
Evaluating arguments before the call does not make it non-tail.

For a direct source-tail call within its caller's direct recursion cycle,
compilation requires an approved native tail transfer. Otherwise it reports
the callee and the lifetime, resource-scope exit, or representation restriction
at that call. Cycles are computed after concrete generic specialization and
include both tail and non-tail direct edges. Different specializations are
different nodes. A cycle that also has non-tail edges has no constant-stack
guarantee.

A transfer evaluates arguments left to right, then drops pending temporaries
and the caller's remaining owned parameters and locals in ordinary exit order,
then enters the callee. Moved arguments belong to the callee. Destructors on
unmoved locals run before the callee body and do not by themselves prevent a
transfer. An argument's `?` takes an ordinary error exit instead.

These restrictions retain the caller for an ordinary call:

- Any reference argument into the caller's locals, owned parameters, or
  temporaries, including a receiver in that storage.
- A pending `with` exit action after the call.
- An owned inline argument, such as a range, record, closure, or generator frame.
- A result requiring conversion, wrapping, propagation, or incompatible result
  storage.

Nonrecursive ordinary fallbacks are legal. Reference arguments originating in
the caller's reference parameters may be forwarded, including reborrows,
projections, borrowed-match payloads, borrowed iteration items, and forwarded
reference results. Their owners belong to an older caller. Exact-compatible
inline results can also transfer, including ranges, records, nested sums, and
concrete generator frames. No hidden heap allocation extends argument lifetime.

Indirect `Callable` calls follow the same transfer restrictions but are outside
the direct recursion guarantee. Concrete closure invocation, worker starts,
generator construction, and generator bodies are outside that graph. Ordinary
recursive calls during generator resumption may consume stack. Explicit
direct-call edges in generated adapters participate; an indirect call into an
adapter does not create a direct edge. See the
[tail-call ABI](../../toolchain/02-architecture/tail-calls.md) for implementation.
