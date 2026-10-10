# Functions, expressions, and control flow

Named functions are declared at module scope or as class methods, with complete
signatures. Capture-free multiline anonymous function expressions can appear in
function bodies; see [function values](../25-function-values.md). There are no
nested named functions, captured environments, default/keyword arguments,
overloads, or redefinitions.
Signatures are collected before any body is checked, allowing forward calls
and mutual recursion. All declarations and statements are checked before any
native code is emitted or executed.

A suite's last expression is its value. Continuing branches of a value-producing
`if`/`elif`/`else` must agree. A non-final expression is evaluated and discarded,
except that implicitly discarding a `Result` is a compile error. Handle it with
`?`, matching, or another function; use `drop(result)` for deliberate discard.
This also applies inside non-final conditionals, matches, loops, context blocks,
and generator bodies. A final function expression or explicit return can still
return a `Result`. The check uses the resolved type, including aliases; it does
not reject unused bindings, `Option`, or aggregates containing Results.
An `if` without an `else` can only have unit result. The inline conditional uses Python order:
`value_if_true if condition else value_if_false`.

Conditions and Boolean operators accept only `bool`; there is no truthiness.
`and` and `or` short-circuit. Arguments and ordinary binary operands evaluate
left to right. Each expression is evaluated once.

`return value` exits the enclosing function from any suite; bare `return`
returns unit. Every explicit return is checked against the declared return
type, even inside a non-final or statically unchosen branch. A returning branch
does not participate in the type join of paths that continue. A function with
any continuing path must still produce its declared result on that path; an
`if` without `else` cannot prove that all paths return. This analysis is
structural, without constant-condition folding. Statements after an explicit
return or an exhaustive conditional whose branches all return are rejected
as unreachable at their source position.

```python
def clamp_low(value: i64, minimum: i64) -> i64:
    if value < minimum:
        return minimum
    value
```

`for` and `while` loops, including `break` and `continue`, are implemented.
Tail calls in final expressions, final branches, and explicit return
expressions (including early guard clauses) become tail-call operations.
Cranelift emits `return_call` or `return_call_indirect` with the Tail calling
convention. The arguments are evaluated first; the caller's remaining owned
parameters and locals, including those with destructors, are then dropped in
ordinary exit order before the transfer. A call passing a reference into the
caller's own locals, owned parameters, or temporaries, or a `return` inside a
`with` block, is an ordinary call followed by cleanup. References that originate
in the caller's reference parameters are forwarded by a tail call; see
[deterministic destruction](../14-deterministic-destruction.md).

A source-tail direct call within its caller's direct recursion cycle must have
an approved native tail-call plan. Otherwise compilation fails at that call,
naming the callee and the lifetime, context-exit, or ABI restriction. This
includes final expressions, explicit returns, conditional expressions, tail
`if`/`match` arms, and the result branches of short-circuit `and`/`or`.
Argument evaluation occurs before the transfer; `?`,
result conversion, and arithmetic after the call are subsequent computation,
so those expressions are not source-tail calls. An explicit return within
`with` is a source-tail candidate but is rejected when it is recursive and the
context needs a post-call exit action. Ordinary nonrecursive fallbacks remain legal.

The compiler computes cycles after concrete generic specialization and includes
both tail and non-tail direct edges. Different specializations are different
nodes. This rule does not promise constant stack for a cycle that also has
non-tail edges. Calls through `Callable` values and concrete closure invocation
are outside this graph, as are worker starts and closure construction. Generated
adapter bodies contribute any explicit direct-call edges they contain, but an
indirect call into an adapter does not create a direct edge. Generator frame
construction and generator bodies are excluded; ordinary recursive calls while
resuming a generator can still consume native stack.
