# Checked defaults: implementation and validation

> Companion to [checked arithmetic by default](../checked-by-default.md).
> **Compiler core implemented; adoption incomplete.** The acceptance contract
> below is not yet satisfied. Remaining work stays in **B14** in the
> [backlog](../../backlog.md); this page contains no performance evidence.

## Current implementation

Function bodies collect arithmetic failures against their declared public
`Result` type, without changing successful return rules. `unchecked:` selects
trapping operators and leaves suite values unwrapped. Nested functions no
longer inherit a private callback effect. Generator arithmetic receives a
targeted diagnostic; ordinary native tail returns remain supported.

`tests/test_checked_defaults.rs` exercises the new contracts, all eight
integer widths, nesting, mutable and stored callbacks, worker error transport,
fallible key sorting, cleanup, skipped compound writes, generators and tail
recursion. The private callback error channel and its lifetime restrictions
are removed; its tests are replaced by public-contract coverage. The retained
local arithmetic suite passes.

The initial library migration covers `std.math`, `std.bytes`, `std.time`,
`std.encoding`, `std.io`, `std.io.typed`, `std.fmt`, `std.fs`, `std.env`,
`std.secrets`, core `std.http`, `std.net`, `std.ready`, `std.text`, `std.random`,
`std.test`, `std.url`, `std.process`, `std.http.decoder`, `std.http.typed`
and `std.http.client`, plus `std.json`, `std.unicode`, `std.complex`, `std.stat`,
`std.linalg`, `std.simd`, `std.log`, `std.crypto`, `std.tls`, `std.uuid`,
`std.jwt`, `std.http.server`, `std.toml`, `std.regex`, `std.cli`, CLI progress,
`std.dataframe`, `std.plot`, HTTP body/cookie/proxy helpers, `std.xml`,
`std.csv`, `std.tempfile`, `std.system`, `std.fs.watch`, SQLite/KV wrappers,
`std.compress`, and actor core/TCP/TLS/HTTP-server adapters.
Integer math magnitude/reductions return `Result[T, OverflowError[T]]`;
separate `*_float` helpers retain IEEE float values. Serial pipeline `sum()`
returns a public result for integer items. Sorting accepts `Result[K, E]`
key callbacks and returns `Result[(), E]`. Bounds-proven byte/time operations
use small documented opt-outs; fallible size and time calculations retain
their existing error contracts. Encoding expansion sizes are checked before
allocation, stream position/total limits return their declared errors, and
filesystem walk reservation no longer counts pending frames twice (issue #702).
Pure capacity helpers are tested at representation limits without allocating
huge buffers. The HTTP decimal accumulator subtracts ASCII bias before checked
multiply/add (issue #703), accepting the complete i64 range while mapping larger
lengths to protocol errors. `FakeStream.fail_after` returns a public
`Result[(), OverflowError[i64]]`; failures preserve its pending schedule.
Counter exhaustion in the stream protocol maps to OutOfRange before mutation.
URL expansion and process extents check representation limits before allocating
or consuming another chunk. Process argument encoding reserves additional bytes
without counting held bytes twice (issue #704). Incremental decoder counters are
bounded by its per-feed, line, head and body limits. Typed chunk writers
preflight the complete wire extent before delivery, preserving progress and
transport causes. Client callbacks retain their own contracts; deadline
saturation and bounded pool/retry/body arithmetic are explicit. No blanket `unchecked:`
conversion was applied.

JSON retains its input/output budgets and maps one-based position exhaustion
to `JsonFault.InputTooLarge` before advancing the failing cursor. Partial output
compaction preserves delivered progress without allocating. Unicode normalization
uses explicit scalar-ceiling, successful-append and positive-index bounds. The
JSON/Unicode component passed 35 isolated-target runtime-check tests, including
debug/optimized representation boundaries and the Unicode normalization corpus,
and received an independent source review. A separately reviewed TOML/regex
checkpoint preserves parser faults, edit atomicity, regex work/memory limits
and replacement validation order. Public mutable regex matches expose length
subtraction as `Result[i64, OverflowError[i64]]`; unavailable TOML coordinates
report InputTooLarge at the valid offset with zero line/column, following the
user's explicit choices. Its 40 isolated-target tests pass, with six added
boundary tests in both optimization modes. A separately reviewed XML/CSV
checkpoint preserves parser faults and stream ownership. CSV position exhaustion
returns the existing stream OutOfRange error; preflight keeps offsets exact,
record MAX valid and explicit flushing of already-finished bytes available.

The reviewed numerical checkpoint preserves complex powers at `i64.MIN`, matrix
shape/index invariants and statistical capacity causes. Ordinary integer SIMD
arithmetic returns lane-typed overflow results; explicit wrapping and IEEE float
APIs remain unchanged. The SIMD tutorial and generated declarations match.
Observation-only logger, server-report and actor statistics saturate at their
existing maximum, following the user's explicit policy; semantic progress stays
exact and fallible. Server output and unread extents preflight before delivery
or buffer growth. TLS/crypto/UUID/JWT retain their existing public errors with
source-visible buffer, DER, timestamp and time/leeway bounds.

Actor slots and timer/request records retire rather than wrap exhausted identity
generations. Mailbox counters rebase all linked target-owned exit fences while
preserving message-prefix ordering. An affine arithmetic error reaches its
parent with both actor heaps reclaimed. Complete actor TLS, crypto, JWT, HTTPS
and server/client tests pass after dependency integration. Combined targeted
validation has 733 passes, two failures in unmigrated HTTP body/cookie, and one
pre-existing ignored test. The two native-engine linalg bodies were still unrun at that checkpoint;
the follow-up native-engine results are recorded below.
At that checkpoint the registry still reported 29 failing module analyses; broad tutorial validation
remains incomplete. These results are not all-features or performance acceptance.

A later reviewed checkpoint adds bounded CLI parsing/progress, checked dataframe
extents, plot arithmetic and body/cookie byte budgets. Actor HTTP keeps exact
response accounting and equivalent bounded lingering-threshold state; complete
TCP/TLS/control/error tests pass. Initial optional-engine self-skips were followed by actual local-engine runs:
all 17 dataframe and nine linalg tests pass, including relocation and allocation
cleanup after fixture-only arithmetic fixes. This does not replace the native
arithmetic audit. Reviewed proxy/helper fixtures restore descriptor, cancellation,
RFC and bulk-scan checks; actor HTTP interop retains real curl and 600-mutation
corpus behavior. Temporary/system/watch/SQLite wrappers and compression preserve
existing errors and resource/progress contracts. Compression's live gzip/Brotli
interop runs; SQLite's optional vector extension and Docker cases remain unrun.
The registry now has 13 failing module analyses: archive, diff, remaining HTTP
consumers, image, markup/template/MIME and sketches. These remain in progress.

Adoption remains incomplete. Other std modules, parallel reductions and many
tests/examples still fail to type-check. Full-suite validation, remaining
native arithmetic audits and performance measurements are still required.

## Reuse and limits

Source inspection at local `main` revision `96f6fc42` found useful existing
machinery in `src/frontend/checked.rs`, `src/codegen/checked.rs`, and
`src/numeric.rs`: a stack of checked scopes, per-operation error collection,
cleanup paths, and cold exits producing a typed error. Scope records already
distinguish function forwarding from arithmetic checking. The parser already
recognizes contextual `checked:` suites in value positions.

Adding an explicit trapping mode and retaining nested `checked:` should be a
contained extension of that machinery. Represent the nearest arithmetic mode
explicitly; do not merely search past an unchecked scope for an outer checked
target. Function failures use the declared public `Result` ABI. Remove the
private wrapper and forwarding machinery for lexical checked callbacks under
the uniform function rule below. Success returns, `?`, early exits, cleanup and
tail calls need auditing.

The whole change is **not** a small default toggle. Most arithmetic-bearing
functions currently declare plain returns, and generic/protocol bodies,
callbacks, generators, runtime helpers and std signatures depend on that.
The core implementation reuses these paths; no timing experiment has been performed.

## Functions and callbacks

Every named function and anonymous function has its own default error boundary,
regardless of the mode surrounding its definition or call. Its declared return
type must cover its escaping arithmetic errors. A `checked:` or `unchecked:`
suite affects arithmetic in that function body, not a nested function's body.
Inlining must preserve this distinction.

Remove the special hidden error channel for closures defined inside explicit
`checked:` blocks. That behavior was implemented for issue #614 under the old
opt-in model; retaining it would give closures two different error mechanisms
depending on their definition site. With checked defaults, an ordinary declared
`Result` provides the same recoverability and a visible contract. Existing
scope-bound callback behavior is not a compatibility requirement.

```text
def make_incrementer() -> Callable[[i64], Result[i64, OverflowError[i64]]]:
    def(value: i64) -> Result[i64, OverflowError[i64]]:
        Ok(value + 1)
```

This closure needs no surrounding checked block or private error channel. Its
arithmetic contract does not tie its lifetime to its definition scope; ordinary
capture ownership and borrow rules still apply. A closure defined inside
`unchecked:` likewise defaults to recoverable arithmetic. To make its arithmetic
trap, put `unchecked:` inside the closure body, where the opt-out is visible.

Calls returning `Result` still require explicit handling, even within `checked:`.
Using `?` there exits the function as usual; it does not turn the call's error
into the enclosing block's result. Local recovery from a callee uses `match` or
ordinary Result operations. The cost of the uniform rule is that a checked
block cannot transparently collect arithmetic errors from an arbitrary callback
invocation; the callback and its consumer expose and handle them normally.

Update callback consumers to support the appropriate fallible return contracts.
Review pipelines, sorting, generic helpers, worker jobs and boxed callables;
change their APIs where that produces the clearest design. Do not preserve
infallible callback signatures by blanket-inserting `unchecked:`. Worker error
transport and resource ownership still need explicit contracts, but no lexical
checked-block lifetime or private callback effect crosses those boundaries.

## Special boundaries and library code

- **Generators:** today's `Generator[T]` cannot return a function-level `Err`
  from its suspended body. Reject unhandled recoverable arithmetic there with a
  targeted diagnostic; locally handle a `checked:` result or use a deliberate
  `unchecked:`. A fallible generator protocol is not implied by this proposal.
- **Destructors and protocol returns:** revise protocols where fallible results
  fit their purpose. Where execution semantics require infallibility, such as
  cleanup with no error recipient, recover locally or explicitly opt out. Do not
  preserve a fixed return merely because it is the current API.
- **FFI and exports:** choose explicit ABI contracts for fallible operations.
  Wrappers for foreign APIs must respect those APIs; Plenty-owned exports can
  change. Checked defaults do not invent a cross-language `Result` ABI. Native implementations need their own documented arithmetic
  audit because they are not Plenty source suites.
- **Generic functions and recursion:** validate against declared returns and
  bounds, with specialization diagnostics where needed. Do not discover public
  signatures by a call-graph fixed point or change effects with inlining.
- **Std and builtins:** apply the same default to Plenty implementations. Audit
  builtin integer reductions such as `sum`, which currently trap regardless of
  surrounding `checked:`. Prefer a public fallible contract where arithmetic can
  fail; retain a trap only as an explicit, documented choice. Fallible user
  operator hooks remain ordinary calls returning `Result`, not implicit effects.
- **Internal compiler/runtime arithmetic:** compiler implementation language
  operations do not acquire Plenty source semantics. Generated operations that
  implement a public primitive must nevertheless honor that primitive's contract.

These are implementation and execution-contract questions. Changing
user-function lowering alone does not make all library arithmetic recoverable.
Any public std signature changes need a std-conventions review and regenerated
signature index as part of implementation.

## Coordinated repository changes and teaching

Plenty has no external users or dependent systems. Breaking syntax, APIs and
behavior is acceptable: select the clearest resulting design and update the
repository together. There is no compatibility release gate, edition dependency,
legacy mode, deprecation period or external migration plan.

Implementation still needs to update arithmetic-bearing functions, callback
consumers, std implementations, examples and tests. This is ordinary work to
make the repository obey the new contract, not a reason to preserve an inferior
API. Diagnostics should explain missing arithmetic errors and the choices of
propagation, local recovery or deliberate termination. Do not insert unchecked
blocks throughout the repository merely to make the build pass.

Explicit checked expressions remain useful for local recovery. Recovery
calculations can themselves fail and need the same scrutiny. Use a contextual
`unchecked:` suite consistently with the block grammar; identifier compatibility
is not a requirement if a different grammar proves clearer.

Teach simple arithmetic in a fallible entry point, with a short explanation of
`Result` and `Ok(())`, then introduce local recovery and deliberate opt-outs.
Update lessons and application examples to recommend the new behavior directly;
no old-to-new migration lesson is needed. Keep tutorial/reference examples on
current behavior until implementation lands; proposal examples use `text` fences.

## Validation contract

Acceptance needs semantic tests for function propagation, precise unions,
missing-error diagnostics, locally handled errors, all nesting directions,
value/statement suites, and ordinary calls returning `Result` inside unchecked
code. Exercise every integer width, division proofs, `MIN // -1`, `MIN % -1`,
negative powers, constant operands, compound assignments and forwarded newtypes.

Ownership tests must cover resources and temporaries on failure, partial
mutation, skipped writes, `?`, explicit returns, loop exits and tail recursion.
Callback tests must verify that named and anonymous functions keep their own
declared error contracts when defined or called in either block mode. Cover
stored and returned callbacks, generic borrowers, generators, worker boundaries
and ordinary borrow-based escape restrictions; remove tests for the retired
private checked channel. Inlining and optimization
must preserve the lexical contract. Trapping tests need child processes and must
verify termination in both debug and optimized builds.

Compare today's unadorned trapping arithmetic, today's explicit checked block,
proposed implicit checked function, and proposed unchecked block on equivalent
workloads. Inspect emitted IR/object code as well as timings: count guards,
result construction, cleanup and return ABI differences. Measure tight loops,
call-heavy kernels and realistic std consumers on success and failure paths.
Verify the no-allocation arithmetic contract and report compiler/build-time and
code-size costs. No speedup claim is justified before these measurements.

The [original checked-arithmetic design](../completed/checked-arithmetic.md)
records why local PR #678, which returned a `Result` from every operator, was
abandoned. Preserve its lesson: use plain values between operators and a shared
failure exit, rather than reviving per-operator wrapping and unwrapping.

Adoption also requires updated executable tutorial/reference examples, updated
arithmetic and trap documentation, the std signature index, the full relevant
compiler/runtime tests and the complete docs CI checks. The proposal itself
changes none of those implemented contracts.
