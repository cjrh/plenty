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
evaluated.

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
def classify(flag: bool) -> i64:
    result = if flag:
        base = 10
        base + 1
    else:
        2
    result * 2

def main() -> Result[(), IoError]:
    print(classify(True), classify(False))?
    Ok(())
```
```output
22 4
```

`mut` and annotations work as for ordinary bindings. An annotation or the
assigned destination's type supplies the expected type to every continuing arm,
including every `elif` and `else`; this allows contextual `Nothing` and empty
collection literals. Continuing arms must agree. Arms ending in `return`,
`break`, or `continue` contribute no value. If every arm exits, write an ordinary
`if` statement instead of binding its nonexistent value. An absent `else`
produces unit on its unselected path; unit cannot be stored in a binding.

Evaluate conditions in order and only evaluate the selected suite. Its locals
are cleaned up before the binding or assignment, including before evaluating
[destination indices](assignment.md). `?`, `return`, `break`, and `continue`
keep their enclosing function or loop. A binding or assignment cannot receive a
reference or a closure with borrowed captures from the block; produce an owned
value instead. These are the same lifetime and evaluation rules as a
[match used as a value](matching.md#a-match-as-a-statements-value).

`return if condition:` returns each selected suite's final expression as though
it ended with `return expression`. Returned references follow ordinary
[returned-reference rules](../05-ownership/04-returned-references.md), and calls
in these final positions remain [source-tail calls](../06-functions/06-tail-calls.md).
A suite without a value returns unit. All paths exit the function, so a
statement after the whole block is unreachable.

The block ends the statement. Block `if` and `match` remain unsupported inside
call arguments, operands, augmented assignments, or assignments to several
targets. Bind the selected value first, or use an inline conditional for two
single expressions.
