# Named arguments and fresh defaults

Status: proposed, not implemented. This is the preferred narrow revision of
[B36](../backlog.md), separating named arguments/defaults from the older
[unpacking proposal](keyword-arguments-and-unpacking.md). It does not approve
splat unpacking, variadics, `/`, or `*` signature markers. Current calls remain
positional and defaults remain unsupported.

## Construction should communicate structure

At baseline `bef41b1`, `stdlib/std/json.plenty`'s `decoder` (line 701) builds
`Decoder` with 28 positional fields. Many have identical types. Names would
help review, but 28 independently supplied values remain a poor abstraction.

Prefer an explicit `Decoder.__init__` that accepts meaningful inputs, initializes
internal state by field name, and establishes invariants. An instance cannot
be created uninitialized and then escape while a caller fills its fields:
[current initialization rules](../design/language/03-types/classes.md) require
all fields on every successful initializer exit. This is desirable.

Use a configuration class for coherent caller choices; `std.json.Limits`
already does this. Group cursor/location, token-decoding, and container state
only where the grouping has a real invariant. Passing a new class containing
the same 28 unexplained arguments merely relocates the problem.

A builder is useful when construction is incremental, validation spans several
settings, or a sequence of required choices matters. Its `build` returns a valid
object or a typed error. It is not necessary for every record. Current explicit
initializers allow only unit or allocation failure, so factories remain useful
for domain validation errors. [Typestate](typestate-and-owned-transitions.md)
can strengthen a builder when required stages justify extra types.

Other review sites: `json.encoder`, `http.server.config`, `http.client.config`,
and `http.body.multipart_limits`. Names help callers distinguish timeouts,
capacities, and limits; structure and constructors keep internal state private.

## Named call arguments without splats

Hypothetical syntax:

```text
def connect(host: &str, port: u16, timeout_ns: u64 = 1000000u64) -> Result[Connection, NetError]:
    ...

connect("example", timeout_ns=5000000u64, port=443u16)
```

- Positional arguments must precede every named argument. Names may then appear
  in any order. No positional argument may follow a named argument.
- Bind names statically; diagnose unknown, repeated, missing, and multiply
  supplied parameters, including a positional value plus the same name.
- Evaluate the callee/receiver once, then supplied argument expressions once
  in written order, regardless of parameter order. Only placement into parameter
  slots is reordered. Type-check each expression against its resolved parameter.
- Preserve move/borrow checking and early-propagation cleanup of evaluated
  arguments. A method receiver reservation must span the same evaluation steps;
  reordering must not relax alias restrictions.
- Defaults follow required parameters in a declaration. Naming a required
  parameter does not make it optional. Explicitly passing `Nothing` is a value,
  not an instruction to evaluate a default.
- Cover named functions, statically resolved methods (excluding `self` as a
  supplied argument), and generated/explicit constructors. Constructor visibility
  and private-field restrictions remain unchanged.

Parameter names become source API commitments. A rename can break callers.
Callable values currently have positional structural signatures without names;
keep their calls positional. Protocol parameter names currently need not match
implementations. Initially keep calls through protocol constraints positional
too; do not infer a keyword contract from whichever concrete class specializes
the body. Adding protocol-level names later would be an explicit compatibility
decision. Builtins require a deliberate inventory of stable parameter names.

This needs argument-binding and lowering work, diagnostics, imported signature
metadata, and ownership tests. No runtime argument dictionary is required, but
calling it a trivial parser change would underestimate the work.

## Defaults: two stages with one freshness rule

Start with the older proposal's allocation-free literal defaults. Then consider
fresh expressions, using the same omitted-argument semantics. Defaults are
evaluated once per omitted argument per invocation, never stored as a shared
mutable object. Python's documented definition-time evaluation is explicitly
not the intended model. [Python default arguments](https://docs.python.org/3/tutorial/controlflow.html#default-argument-values)

For fresh expressions, evaluate supplied arguments first in written order,
then omitted defaults in parameter declaration order, before entering the body.
Resolve default names in the function's defining module. Initially prohibit
references to parameters or `self`, avoiding reordered/dependent initialization.
Reject dependency cycles rather than inventing a dependency scheduler. Callable
values still require full positional arity; omission is not part of their type.

A default expression must produce the parameter's value using ordinary typing.
Construct a fresh owner for an omitted owned parameter. For an omitted borrowed
parameter, a proposed extension materializes a fresh temporary owner and lends
it to that parameter for the call. Use Plenty's existing annotation placement:

```text
# Proposed, not current Plenty.
def work(data: &mut list[i64] = []?,
         lookups: &mut dict[str, f64] = {}?) -> Result[Option[bool], AllocError]:
    ...
```

The caller supplying either argument must still supply a legal exclusive loan.
An omitted argument gets separate storage for every call. Mutation of an
omitted scratch buffer is discarded at call completion; callers needing that
output must pass storage or receive an owned result. Empty-container expected
types come from the parameter. References into these temporary owners must not
escape in returned references or borrowed closures. Reject such calls when the
return provenance could include an omitted temporary; never extend its lifetime
implicitly or store it in an aggregate.

## Allocation failure cannot disappear into an Option

In Plenty, `[]` and `{}` produce `Result` because allocation is fallible. A bare
default `=[]` therefore does not currently type-check as a list, and an
`Option[bool]` return has no place for `AllocError`. Freshness alone does not
solve this mismatch.

Recommended rule: explicit `?` inside a default propagates through the declared
callee result contract, not the lexical caller's return type. A call with an
omitted fallible default still has exactly the declared return type. The example
therefore returns `Result[Option[bool], AllocError]`. On default failure the body
does not run, evaluated owned arguments/defaults are cleaned up, and that call
produces `Err`. Supplying all arguments does not change its static return type.
Error conversions must be explicit as elsewhere (or follow a separately accepted
error-set rule); no hidden `Failure` conversion or trap is introduced.

Lower this as a compiler-generated default wrapper or equivalent call-site
control flow with callee-local propagation. Simply pasting a `?` expression
into the caller would give incorrect semantics. Diagnostics should identify
the declaration/default and the call that omitted it. Keep this stage separate
from allocation-free defaults because its cleanup and effect semantics are
substantially more involved.

An alternative is to accept an optional owned buffer and construct it inside a
fallible function today. That is more ceremony but uses existing rules. Do not
make omission alone change a function's return type or silently abort on OOM.

## Evaluation criteria

Use named arguments on HTTP limits and timeout configuration, and compare a
structured JSON initializer against the current 28-value constructor. Test
out-of-order keywords with observable side effects; positional/name collisions;
imports, generics, methods, constructors and visibility; cleanup after argument
or default failure; repeated mutable-default calls; explicit arguments skipping
defaults; nonescaping default loans; and callable/protocol restrictions.

Field defaults are a separate construction feature, not implied by parameter
defaults. Avoid combining this work with splats or a general builder framework.
Update the reference and runnable tutorial only when each stage is implemented.
