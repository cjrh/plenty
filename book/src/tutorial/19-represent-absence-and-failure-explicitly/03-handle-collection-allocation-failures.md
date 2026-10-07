# Handle collection allocation failures

Start with `list[T].new()` when you need to handle failure while creating an
empty list. It returns `Result[list[T], AllocError]`. Sets and dictionaries work
the same way: `set[T].new()` and `dict[K, V].new()`.

If you know how many entries you need, use `with_capacity(n)`. The collection
starts empty and has space for at least `n` entries, including any hash table.
Here construction and insertion both propagate errors to the caller:

```plenty
def answers() -> Result[list[i64], AllocError]:
    mut values = list[i64].with_capacity(2)?
    values.append(21)?
    values.append(42)?
    Ok(values)

def main() -> Result[(), IoError]:
    match answers():
        case Ok(values):
            print(values)?
        case Err(error):
            match error:
                case AllocError.OutOfMemory:
                    print("not enough memory")?
                case AllocError.CapacityOverflow:
                    print("requested capacity is too large")?
    Ok(())
```

```output
[21, 42]
```

The `?` unwraps the newly owned collection on success. On failure, construction
reclaims any memory it already obtained and returns the error. The error itself
needs no allocation. Type aliases work too: after `type Numbers = list[i64]`,
you can write `Numbers.new()`.

Use `append` for a list, `add` for a set, and `insert` for a dictionary
when you need to handle allocation failure. Each returns `Result[(), AllocError]`.
`reserve(n)` reserves space for at least `n` additional entries; it is available
on all three collection types and returns the same result type.

```plenty
def add_answers(values: &mut list[i64]) -> Result[(), AllocError]:
    values.reserve(2)?
    values.append(21)?
    values.append(42)?
    Ok(())

def main() -> Result[(), Failure]:
    mut values: list[i64] = []?
    match add_answers(&mut values):
        case Ok(done):
            print(values)?
        case Err(error):
            match error:
                case AllocError.OutOfMemory:
                    print("not enough memory")?
                case AllocError.CapacityOverflow:
                    print("requested capacity is too large")?
    Ok(())
```

```output
[21, 42]
```

`AllocError` is always available. Its two variants need no payload or allocation.
`OutOfMemory` means the allocator rejected a request. `CapacityOverflow` means
the requested size cannot be represented; a negative reservation is also a
capacity error. You can handle a failure without terminating the program:

```plenty
def main() -> Result[(), Failure]:
    mut values = [1, 2]?
    match values.reserve(-1):
        case Ok(done):
            print("reserved")?
        case Err(error):
            print(error)?
    print(values)?
    Ok(())
```

```output
AllocError.CapacityOverflow
[1, 2]
```

On failure the collection keeps its existing contents. An insertion consumes its
arguments even when it fails, so an owned argument is cleaned up rather than
returned to you. Reserve first if you want to keep an item until storage is ready.
Successful reservation covers collection storage, including a dictionary or
set's hash table; constructing the elements themselves may still allocate.

These methods require a mutable receiver, just like `append` and `add`. Dictionary
`insert(key, value)` replaces an existing value or adds a new entry. Adding an
existing set element or replacing a dictionary entry needs no storage growth.

Allocating literals, comprehensions, constructors, string operations, and `copy`
all return results. Handle each allocating argument too: `copy([1, 2]?)?`
checks both construction and copying. A plain `[]` produces a result; it never
silently aborts on allocation failure. Explicit `.unwrap()` chooses to terminate
if that result is an error. User destructors must handle their own errors.
