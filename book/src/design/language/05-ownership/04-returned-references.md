# Returned references

A function or method returning `&T` or `&mut T` must take exactly one reference
parameter. A mutable result requires a mutable parameter. Every return path
must originate in that parameter; local owners and owned parameters cannot
escape. Direct parameter references, reborrows, field and element projections,
borrowed-match payloads, and forwarding calls are supported. Branches may use
explicit returns; conditional reference expressions are unsupported.

```plenty
class Pair:
    first: i64
    second: i64

def first(pair: &Pair) -> &i64:
    &pair.first

def main() -> Result[(), Failure]:
    pair = Pair(3, 4)
    value = first(&pair)
    print(*value)?
    Ok(())
```

The result extends its input loan until its last use. A body that directly
returns its parameter or a fixed field projection permits a precise returned
field loan; the original call still requires access to its whole argument.
Indexed origins, control-dependent returns, and other bodies conservatively
protect the whole argument, even after further projection.

A caller may bind a result from named storage and reborrow it. Methods returning
references require a named receiver or a field/indexed place in named storage;
temporary receivers are rejected. A shared temporary passed to an ordinary
function survives only its full expression: `len(view("text"))` can use a
returned `&str`, but `r = view("text")` is rejected even when `r` is unused.
Returned references cannot enter aggregates, scoped workers with temporary
origins, or generator suspension. No lifetime annotation or callee-body
inference changes the signature's single-origin requirement.
