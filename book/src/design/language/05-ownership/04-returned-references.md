# Returned references

A function or method returning `&T` or `&mut T` borrows its result from its
first reference parameter, the *origin*. A function without a reference
parameter cannot return a reference, and a mutable result requires an `&mut`
origin. Every return path must originate in the origin; later reference
parameters, local owners, and owned parameters cannot escape. Direct parameter
references, reborrows, field and element projections, borrowed-match payloads,
and forwarding calls are supported. Branches may use explicit returns;
conditional reference expressions are unsupported.

```plenty
class Pair:
    first: i64
    second: i64

def first(pair: &Pair) -> &i64:
    &pair.first

def main() -> Result[(), IoError]:
    pair = Pair(3, 4)
    value = first(&pair)
    print(*value)?
    Ok(())
```

Later reference parameters are ordinary inputs. Their loans end when the call
returns, so a temporary key or a borrow of another owner does not restrict the
result. For a method, the origin is its reference receiver:

```plenty
class Scores:
    names: list[str]
    points: list[i64]

    def points_of(self: &mut Scores, name: &str) -> &mut i64:
        mut i = 0
        while i < len(self.names) - 1:
            if self.names[i] == name:
                return &mut self.points[i]
            i = i + 1
        &mut self.points[len(self.points) - 1]

def main() -> Result[(), AllocError | IoError]:
    mut scores = Scores(["ada", "bob"]?, [3, 5]?)
    mut name = "bob"
    points = scores.points_of(&name)
    name = "eve"
    *points = *points + 10
    print(scores.points)?
    print(name)?
    Ok(())
```

To return a borrow of a different argument, make it the first reference
parameter. A function cannot choose between two arguments; that relationship
needs annotations Plenty does not have. Signatures alone fix the origin, so
`Callable` types and generic functions follow the same rule.

The result extends its origin loan until its last use. A body that directly
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
inference changes the signature's first-reference origin.

A borrow that may be absent or may fail is returned as `Option[&T]` or
`Result[&T, E]` under the same origin rule; see
[optional and fallible references](05-optional-references.md).
