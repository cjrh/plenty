# Closure environments

`def [captures](parameters) -> T:` creates a reusable concrete environment.
Captures are explicit; no implicit deep copy occurs. The body takes either
[anonymous function form](02-function-values.md): an indented block outside
brackets, one expression inside them.

| Capture | Access inside the body |
| --- | --- |
| `value` | Own the value; read or borrow it |
| `mut value` | Own private mutable state |
| `&value` | Share a surrounding binding |
| `&mut value` | Exclusively borrow a mutable surrounding binding |

Scalars copy into captures, immutable strings keep their sharing semantics,
and affine owners move. An owned capture's source need not be mutable; a
borrowed mutable source must be. Captures cannot be redeclared or shadowed by
loop, comprehension, unpacking, or match bindings inside the body.

```plenty
def counter(start: i64) -> Closure[[], i64]:
    def [mut start]() -> i64:
        start = start + 1
        start

def main() -> Result[(), IoError]:
    mut count = counter(2)
    print(count())?
    print(count())?
    Ok(())
```
```output
3
4
```

Calls borrow the environment and may repeat. Mutable captures require an
exclusive environment borrow and a mutable closure binding or owner. Captured
owners can be borrowed and mutated as permitted but cannot be moved out; use
a consuming closure when ownership must leave the body. Assigning a closure
moves it; `copy` and equality are unavailable. Display is `<closure>`.

`Closure[[parameter types], result]` describes a concrete environment's
signature. Its producer identity is retained: different closure expressions
do not unify merely because their signatures match. Factories and generic
consumers preserve that concrete layout. Prefer `F: Callable[...]` with `&F`
or `&mut F` for reusable callback APIs accepting both named functions and
closures; the generic constraint is separate from a code-only `Callable` value.
Diagnostics identify a concrete closure by its definition's source file, line,
and column (line and column for source supplied without a filename). This
location describes its origin; different generic instantiations of the same
expression can still have distinct environment types.

Creating, moving, passing, returning, calling, and dropping inline environments
adds no heap allocation. Captured owners drop in reverse capture order.
Owned environments can capture other owned environments, ranges, and standard
sums, and can travel through `Option`/`Result` or suspended generator frames.
Reusable closures cannot capture generator frames. Body operations and capture
expressions retain their own allocation and error contracts.

Capture loans follow moves and remain live through environment borrowing and
later argument evaluation. Shared captures prohibit owner mutation or destruction;
exclusive captures also prohibit competing reads. Borrowing environments can
be passed by reference, including through generic callbacks, but cannot cross
a function boundary by value, escape their creating function, own-nest, enter
sum/container storage, or suspend in generators. Borrowing another environment
retains its transitive loans.

Calls can target temporaries, such as `make(3)(4)` or `wrapped.unwrap()(4)`;
the callee survives the full expression and cleans up if an argument propagates.
Capturing and one-shot closures cannot return references or yield.
Environment nesting is limited to 64 levels and must fit the native stack
layout limit. Closure calls do not perform native tail transfers.
