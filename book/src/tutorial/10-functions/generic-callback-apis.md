# Accept functions and closures through one API

Constrain a type parameter by its call signature to accept named functions and
captured closures through one parameter. Borrow the callback when its caller
should keep it:

```plenty
def apply[T, F: Callable[[T], T]](f: &F, value: T) -> T:
    f(value)

def double(value: i64) -> i64:
    value * 2

def main() -> Result[(), Failure]:
    function = double
    offset = 2u8
    add = def [offset](value: u8) -> u8:
        offset + value
    print(apply(&function, 3))?
    print(apply(&add, 3u8))?
    Ok(())
```
```output
6
5
```

A short callback can be written in the call. An anonymous function needs
parentheses to be the operand of `&`:

```plenty
def apply[T, F: Callable[[T], T]](f: &F, value: T) -> T:
    f(value)

def main() -> Result[(), Failure]:
    offset = 2
    print(apply(&(def [offset](value: i64) -> i64: offset + value), 3))?
    print(apply(&(def(value: i64) -> i64: value * value), 3))?
    Ok(())
```
```output
5
9
```

`F` preserves the callback's concrete environment type. Both arguments must
agree on `T`; conflicting type evidence is an error. Use `&mut F` for a callback
that changes its captured state. Borrowed captures stay protected throughout
the call.

A concrete parameter such as `f: Callable[[i64], i64]` holds only a function's
code address. The constraint `F: Callable[[i64], i64]` instead checks whether
the actual type can be called with that signature; its captured state remains
inline. Adding `&` to the concrete `Callable` type does not make it accept
closures. `Closure[...]` describes a concrete environment, chiefly when
[returning one from a factory](returning-closures.md).

Passing captured state to a code-only parameter explains the mismatch and
suggests the generic form:

```plenty-error
def apply(f: Callable[[i64], i64], value: i64) -> i64:
    f(value)
def main() -> ():
    offset = 3
    add = def [offset](value: i64) -> i64:
        value + offset
    apply(add, 4)
```
```error
for a generic callback API, use `F: Callable[[i64], i64]` with `callback: &F`
```

## Let the callback determine a separate result type

The first helper requires the argument and result to have the same type `T`.
Give the result its own parameter `U` to allow transformations between types.
The callback's concrete signature determines `U`, including when it does not
appear in another argument. This example uses both forms of callback through
that shared interface:

```plenty
def apply[T, U, F: Callable[[T], U]](callback: &F, value: T) -> U:
    callback(value)

def double(value: i64) -> i64:
    value * 2

def main() -> Result[(), Failure]:
    function = double
    offset = 7
    add = def [offset](value: i64) -> i64:
        offset + value
    print(apply(&function, 10))?
    print(apply(&add, 10))?
    Ok(())
```
```output
20
17
```
