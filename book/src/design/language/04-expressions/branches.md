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

```python
def clamp_low(value: i64, minimum: i64) -> i64:
    if value < minimum:
        return minimum
    value
```

`return value` exits the enclosing function from any suite. Bare `return`
returns unit. `return match value:` returns the final expression of the
[selected case](matching.md#a-match-as-a-statements-value). Every explicit return must match the declared result, including
returns in non-final or statically unchosen branches.

A returning branch contributes no value to paths that continue. Every continuing
function path must still produce the declared result; an `if` without `else`
cannot prove that all paths return. This analysis follows structure without
constant-condition folding. Statements after an explicit return or an exhaustive
conditional whose branches all return are unreachable errors.

`break` and `continue` likewise contribute no branch value when they exit a suite
inside a loop. Ordinary exits perform [cleanup](../05-ownership/index.md).
