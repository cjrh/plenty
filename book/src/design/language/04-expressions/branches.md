# Suites, conditionals, and returns

A suite's final expression supplies its value. Other expressions evaluate and
are discarded, except that implicitly discarding a `Result` is a compile error.
Handle it with `?`, matching, or a function; `drop(result)` explicitly discards
it. This applies in non-final conditionals, matches, loops, resource scopes,
and generator bodies. It does not prohibit unused bindings, `Option`, or
aggregates containing Results.

Continuing branches of value-producing `if`/`elif`/`else` suites must agree on
type. An `if` without `else` can only produce unit. The inline conditional uses
`value_if_true if condition else value_if_false`; only its selected value is
evaluated. Without an expected type from its context, the first value's type is
the expected type of the second: `Some(1) if ready else Nothing` works, while
`Nothing if ready else Some(1)` needs an annotation. Its values may be
references into different owners, joined as an `if` block's arms are.

An `if` takes any number of `elif` arms. The arms are alternatives at one level,
tested in order; they do not count toward the
[nesting limit](index.md#nesting). An `if` written inside an `else` suite does.

```plenty fragment
def clamp_low(value: i64, minimum: i64) -> i64:
    if value < minimum:
        return minimum
    value
```

`return value` exits the enclosing function from any suite. Bare `return`
returns unit. `return if condition:` returns its selected suite's value.
`return match value:` returns the final expression of the
[selected case](matching.md#a-match-as-a-statements-value). Every explicit return must match the declared result, including
returns in non-final or statically unchosen branches.

A returning branch contributes no value to paths that continue. Every continuing
function path must still produce the declared result; an `if` without `else`
cannot prove that all paths return. This analysis follows structure without
constant-condition folding. Statements after an explicit return or an exhaustive
conditional whose branches all return are unreachable errors.

`break` and `continue` likewise contribute no branch value when they exit a suite
inside a loop. Ordinary exits perform [cleanup](../05-ownership/index.md).

## An if as a statement's value

A block `if` can be the whole right-hand side of a binding or single-place
assignment, or follow `return`. Its selected suite's final expression supplies
the value:

```plenty
def classify(flag: bool) -> Result[i64, OverflowError[i64]]:
    result = if flag:
        base = 10
        base + 1
    else:
        2
    Ok(result * 2)

def main() -> Result[(), IoError | OverflowError[i64]]:
    print(classify(True)?, classify(False)?)?
    Ok(())
```
```output
22 4
```

`mut` and annotations work as for ordinary bindings. An annotation or the
assigned destination's type supplies the expected type to every continuing arm,
including every `elif` and `else`; this allows contextual `Nothing` and empty
collection literals. Without one, the first continuing arm is inferred alone
and supplies the expected type of the arms after it, as the first value of an
inline conditional does for the second, so only a first continuing arm of
`Nothing` or `[]?` needs an annotation. Continuing arms must agree. Arms ending
in `return`, `break`, or `continue` contribute no value. If every arm exits,
write an ordinary `if` statement instead of binding its nonexistent value. An absent `else`
produces unit on its unselected path; unit cannot be stored in a binding.

Conditions are evaluated in order; only the selected suite runs. Its locals
are cleaned up before the binding or assignment, including before evaluating
[destination indices](assignment.md). `?`, `return`, `break`, and `continue`
keep their enclosing function or loop. The block may produce a reference, plain
or inside an `Option` or `Result`; while it is used, every arm's owner stays
borrowed, as for [`unwrap_or`](../05-ownership/05-optional-references.md#choosing-with-if-or-match).
A binding or assignment cannot receive a view or a closure with borrowed
captures from the block; produce an owned value instead. These are the same
lifetime and evaluation rules as a
[match used as a value](matching.md#a-match-as-a-statements-value).

`return if condition:` returns each selected suite's final expression as though
it ended with `return expression`. Returned references follow ordinary
[returned-reference rules](../05-ownership/04-returned-references.md), and calls
in these final positions remain [source-tail calls](../06-functions/06-tail-calls.md).
A suite without a value returns unit. Every path leaves the enclosing suite,
so a statement directly after the whole block is unreachable. An arm that
executes `break` or `continue` still exits to its enclosing loop rather than
returning from the function; code after that loop can run.

The block ends the statement. Block `if` and `match` remain unsupported inside
call arguments, operands, augmented assignments, or assignments to several
targets. Bind the selected value first, or use an inline conditional for two
single expressions.
