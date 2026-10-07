# Learning Plenty

Plenty combines Python-shaped syntax with explicit types and native compilation.
This guide teaches the language that works today. It grows alongside the
compiler; the runnable examples and expected errors below are tested with
`cargo test --test test_tutorial`.

You do not need to know the old stack-based Plenty. This is a new language.
If you know Python, the indentation and function syntax will look familiar,
but values have fixed types and function interfaces always declare their types.

## 1. Run your first program

Save this in `hello.plenty`:

```plenty
def main() -> Result[(), IoError]:
    print("Hello, Plenty!")?
    print(6 * 7)?
    Ok(())
```

Output:

```output
Hello, Plenty!
42
```

From this repository, run it with:

```sh
cargo run -- hello.plenty
```

To check the program without running it, use `cargo run -- --check hello.plenty`.
To compile a native executable:

```sh
cargo run -- --compile hello.plenty -o /tmp/hello-plenty
/tmp/hello-plenty
```

Plenty always compiles ahead of time with Cranelift. Running a file compiles a
temporary native executable, runs it, and removes it afterward. Both running
and compiling require the system linker driver `cc` on PATH. Checking does not.
The Rust runtime is already packaged with Plenty; you do not need `cargo` or
`rustc` to compile Plenty programs.

`print` returns `Result[(), IoError]`: either successful completion (`Ok(())`) or
an output error. The trailing `?` continues on success, or returns the error
from `main` after cleaning up its local values. The final `Ok(())` explicitly
reports success. Lesson 19 explains results and recovery in detail.

Examples that combine different error types use `Result[(), Failure]`.
`Failure` means that the caller only needs to know whether the work succeeded:
each `?` drops the original error details and propagates a small, allocation-free
marker. Use a specific type such as `IoError` when the caller needs those details.

Execution starts by calling `main` once. Every binary application must declare
`main` with no parameters. It can return `()`, `i32`, `Result[(), E]`, or
`Result[i32, E]`. An `Err` produces exit status one after dropping its payload;
`Ok(())` produces zero and `Ok(status)` uses that status. Errors are not printed
automatically. Returning a Result lets `main` use `?` too. The `()` form
finishes successfully with exit status zero. The `i32` form returns a process
exit status: zero means success, and nonzero means failure. Use an `i32` literal
such as `0i32` or `1i32`; return annotations also guide unsuffixed literals.

Keep executable statements inside functions. Module scope contains `def`,
`class`, `enum`, and `type` declarations, plus imports. Bindings inside `main`
are local to it, so other functions receive values through typed parameters.
The modules chapter below explains imports and `pub` visibility.

Like other functions, `main` may return early, use a final expression, or call
functions declared later in the file. Its owned locals are dropped before the
program exits, including when it returns a nonzero status. Operating systems
limit the range of observable exit statuses; use small nonnegative codes for
portable command-line programs.

You can also choose the successful process status explicitly:

```plenty
def main() -> Result[i32, IoError]:
    print("ready")?
    Ok(0i32)
```

```output
ready
```

Calling a function at module scope is an error, even if `main` is also declared:

```plenty-error
def main() -> ():
    print("ready").unwrap()

main()
```

```error
executable statements are not allowed at module scope
```

Use `--compile` when you want to keep the executable and run it repeatedly
without compiling again. The executable does not need Plenty installed.

Every Plenty example in this guide is a separate, complete program. You can
copy any example into a file without first running the earlier examples. In the
modules chapter, also save the companion files labelled with their filenames.

The tutorial test extracts every `plenty` and `plenty-error` block and its
preceding `plenty-file` companion modules directly from this document. It checks
successful output through both compile-and-run and an
explicitly compiled executable, and checks rejected examples for the expected
diagnostic without executing their effects. Standalone lesson sources and
generated Markdown are a proposed improvement to the authoring workflow.

## 2. Name values

Use `=` to create a binding. Plenty infers the type of a local value, so you
usually do not need an annotation:

```plenty
def main() -> Result[(), IoError]:
    # Comments begin with a hash.
    price = 20
    quantity = 3
    total = price * quantity
    print(total)?
    Ok(())
```

```output
60
```

Bindings are immutable by default. Assignment is not a way to silently change
an existing value or its type:

```plenty-error
def main() -> ():
    score = 10
    score = 11
```

The diagnostic includes:

```error
`score` is immutable; declare it with `mut`
```

Use `mut` when a value needs to change. Write it once, at the declaration:

```plenty
def main() -> Result[(), IoError]:
    mut score: i64 = 10
    score = score + 5
    print(score)?
    Ok(())
```

```output
15
```

A mutable binding still keeps its original type. It cannot start as an integer
and later become a string. `score: i64` explicitly names the type; leaving out
the annotation would infer the same type here.

## 3. Choose the width of an integer

The built-in integer names specify their sizes:

| Types | Meaning |
| --- | --- |
| `i8`, `i16`, `i32`, `i64` | Signed integers of 8, 16, 32, or 64 bits |
| `u8`, `u16`, `u32`, `u64` | Unsigned integers of those widths |

For example, `i8` can hold -128 through 127, and `u8` can hold 0 through 255.
Floating-point numbers use `f32` or `f64`; other primitive value types include
`bool` and `str`.

A whole-number literal without a suffix, such as `42`, defaults to `i64` when
there is no annotation, parameter, return type, or typed arithmetic context.
Append a built-in integer type to choose another width:

```plenty
def main() -> Result[(), IoError]:
    small: u8 = 200u8
    offset: i32 = -12i32
    large: u64 = 1_000_000u64
    print(small)?
    print(offset)?
    print(large)?
    Ok(())
```

```output
200
-12
1000000
```

An annotation guides an unsuffixed literal, so `small: u8 = 200` works.
It does not convert an already typed initializer: use an explicit cast for that.

Arithmetic requires matching types:

```plenty-error
def main() -> ():
    small = 7u8
    print(small + 1i64).unwrap()
```

```error
expected u8, got i64
```

Choose a matching literal or explicitly convert a value:

```plenty
def main() -> Result[(), IoError]:
    small = 7u8
    print(small + 1u8)?
    print(i64(small) + 1)?
    Ok(())
```

```output
8
8
```

Arithmetic overflow is a runtime error, rather than wrapping silently. Explicit
integer casts have different semantics: narrowing discards high bits. For
example, `u8(257)` produces `1`. Widening preserves signedness appropriately;
a same-width cast between signed and unsigned types reinterprets the bits.
Use casts deliberately; they are not range-validation functions.

`+`, `-`, and `*` have their usual arithmetic precedence. `//` divides integers
and rounds down, including for negative values:

```plenty
def main() -> Result[(), IoError]:
    print(1 + 2 * 3)?
    print((1 + 2) * 3)?
    print(-7 // 3)?
    Ok(())
```

```output
7
9
-3
```

Integer division by zero is a runtime error. Use `/` for floating-point division.

### Work with floating-point numbers

A decimal or exponent literal defaults to `f64`. Use an `f32` suffix when you
want 32-bit arithmetic. Both operands must have the same type; casts are explicit:

```plenty
def main() -> Result[(), IoError]:
    distance: f64 = 7.5
    time: f64 = 2.0
    print(distance / time)?
    print(1.25f32 + 0.5f32)?
    print(2.5e-2)?
    print(f64(3) / 2.0)?
    print(i32(-2.75))?
    Ok(())
```

```output
3.75
1.75
0.025
1.5
-2
```

Float-to-integer casts discard the fractional part toward zero and clamp results
outside the integer's range; NaN converts to zero. Integer-to-float casts and
`f64` to `f32` casts may round. There is no implicit widening or narrowing:

```plenty-error
def main() -> ():
    small: f32 = 1.5f64
```

```error
expected f32, got f64
```

Write `1.5f32` or `f32(1.5)` instead. Floats follow IEEE arithmetic: division by
zero can produce infinity or NaN, and arithmetic overflow can produce infinity.
NaN is unequal to everything, including itself; ordered comparisons with it are
false. The same comparison rules apply inside collections and enum payloads.

```plenty
def main() -> Result[(), IoError]:
    zero = 0.0
    print(1.0 / zero)?
    unknown = zero / zero
    print(unknown == unknown)?
    print(unknown != unknown)?
    Ok(())
```

```output
inf
False
True
```

Floats support `+`, `-`, `*`, `/`, unary signs, and comparisons.
`//` and `%` currently require integers. Floats can be list elements, dictionary
values, class fields, and enum payloads, but cannot be dictionary keys or set
elements. Float literals that overflow their declared width are compile errors.

## 4. Define functions with clear interfaces

Every parameter and return type must be declared:

```plenty
def double(value: i64) -> i64:
    """Return twice the supplied value."""
    value * 2

def main() -> Result[(), IoError]:
    print(double(21))?
    Ok(())
```

```output
42
```

`value: i64` declares the parameter; `-> i64` declares the result. The final
expression in the function supplies that result. You can also write
`return value * 2` explicitly. Parameters are immutable.

Use spaces to indent a body. Four spaces are conventional; use the same
indentation for statements in the same block. Tabs are rejected.

A string at the beginning of a function is its documentation. Triple quotes
allow a docstring to span lines. When a function should immediately return a
string, write `return "text"` so it cannot be mistaken for documentation.

Functions are known throughout the file, so one function may call a function
declared later. Calls currently use positional arguments only.

## 5. Make decisions that produce values

A condition must have type `bool`. Comparisons such as `==`, `!=`, `<`, `<=`,
`>`, and `>=` produce booleans. There is no implicit conversion of zero, an
empty string, or another value to `False`.

An `if` can supply a function's result:

```plenty
def maximum(first: i64, second: i64) -> i64:
    if first > second:
        first
    else:
        second

def main() -> Result[(), IoError]:
    print(maximum(7, 12))?
    Ok(())
```

```output
12
```

Both continuing branches need the same type. Use `elif` for additional cases.
For a short choice, Python's conditional expression is also supported:

```plenty
def main() -> Result[(), IoError]:
    age = 20
    category = "adult" if age >= 18 else "child"
    print(category)?
    Ok(())
```

```output
adult
```

`and` and `or` short-circuit: the right side is evaluated only when needed.
`not` negates a boolean. All three require boolean operands:

```plenty
def main() -> Result[(), IoError]:
    divisor = 0
    safe = divisor != 0 and 10 // divisor > 1
    print(safe)?
    print(not safe)?
    Ok(())
```

```output
False
True
```

The division is skipped here. Chained comparisons such as `0 < x < 10` are
not supported yet; write `0 < x and x < 10`.

## 6. Return early and continue on the other path

A guard clause handles a special case before the main computation:

```plenty
def clamp_low(value: i64, minimum: i64) -> i64:
    if value < minimum:
        return minimum
    value

def main() -> Result[(), IoError]:
    print(clamp_low(3, 10))?
    print(clamp_low(12, 10))?
    Ok(())
```

```output
10
12
```

`return` exits the entire function, even when it appears in nested branches.
Every return must match the function's declared result. Code after a guaranteed
exit is rejected. Every path that reaches the end must also supply the declared
result; handling just one branch is not enough:

```plenty-error
def incomplete(flag: bool) -> i64:
    if flag:
        return 42

def main() -> ():
    pass
```

```error
expected i64, got ()
```

## 7. Do work without returning data

The unit type, written `()`, describes completion without producing data.
It is not a missing-value marker. A fallible function with no success data
returns `Result[(), E]`:

```plenty
def greet(name: str) -> Result[(), Failure]:
    print(("Hello, " + name)?)?
    Ok(())

def greet_if(enabled: bool, name: str) -> Result[(), Failure]:
    if not enabled:
        return Ok(())
    greet(name)

def main() -> Result[(), Failure]:
    greet_if(False, "Ada")?
    greet_if(True, "Ada")?
    Ok(())
```

```output
Hello, Ada
```

A bare `return` returns unit in a function declared `-> ()`. `print` returns
unit inside a Result, so `print(value)?` has a unit success value. Use `pass`
for an intentionally empty block. Unit can be a return type, but unit parameters
and stored unit bindings are not supported yet.

There is no `None` value and no implicit nullable type. Lesson 19 introduces
`Option` for absence and `Result` for recoverable errors.

## 8. Give types names of your own

There is no built-in `int`. If you want that name, choose its meaning explicitly:

```plenty
type int = i32

def increment(value: int) -> int:
    value + int(1)

def main() -> Result[(), IoError]:
    answer: int = increment(int(41))
    print(answer)?
    Ok(())
```

```output
42
```

`type int = i32` declares a type alias. `int(41)` is exactly the integer cast
`i32(41)`. Another program can choose `type int = i64` instead. Aliases do not
change unconstrained literal defaults, but `answer: int = 41` uses the annotation
to choose the literal's width. `41i32` and `int(41)` are also explicit choices. Aliases are not literal
suffixes, so `41int` is not valid syntax.

Names that describe your data can make interfaces easier to read:

```plenty
type Count = u32
type ItemCount = Count

def add_one(count: ItemCount) -> ItemCount:
    count + 1u32

def main() -> Result[(), IoError]:
    print(add_one(41u32))?
    Ok(())
```

```output
42
```

An alias is another name for the same type. `Count`, `ItemCount`, and `u32`
are interchangeable here; an alias does not create a distinct type, enforce
units of measurement, or add runtime overhead.

Aliases can name integers, booleans, strings, collections, unit, or other aliases.
Numeric aliases support casts; collection aliases support collection constructors. Declare aliases at module scope; they are
visible throughout that file, including before their declaration. Alias chains
must eventually reach a concrete type; cycles and unknown targets are errors.
An alias cannot redefine a built-in name, another alias, or a function name.

## 9. Keep branch-local names local

Assignments to an existing mutable binding survive a branch. New names declared
inside a branch belong to that branch:

```plenty
def main() -> Result[(), IoError]:
    mut price = 100
    discounted = True
    if discounted:
        discount = 20
        price = price - discount
    print(price)?
    Ok(())
```

```output
80
```

Here, `price` is still available after the branch; `discount` is not. Declare a
mutable binding before the branch when subsequent code needs to read it.
Alternatively, put the choice in a small function or a conditional expression.

## 10. Work with text

Both single and double quotes make strings. Triple-quoted strings can contain
physical newlines. Common escapes include `\n`, `\r`, `\t`, escaped quotes,
and `\\`. Strings support UTF-8 and embedded NUL using `\0`.

```plenty
def main() -> Result[(), Failure]:
    message = ('Hello, ' + "Plenty!")?
    print(message)?
    print(contains(message, "Plenty"))?
    Ok(())
```

```output
Hello, Plenty!
True
```

`print` accepts one value and adds a newline. It prints text without quotes and
integers without width suffixes. `contains(text, part)` tests for a substring.
There is just one string type, `str`. Strings are immutable values; `mut` permits
replacing a binding rather than editing its bytes. `len` counts Unicode scalar
values; indexing returns `Result[str, AllocError]` containing one scalar.
Iteration yields the same result type for each character. Storage uses explicit lengths,
so an embedded NUL does not end a string. Interpolation and general conversion
to strings are not available yet.

```plenty
def main() -> Result[(), Failure]:
    text = "é\0😀"
    print(len(text))?
    print(text[-1]?)?
    print("\0" in text)?
    print([text]?)?
    Ok(())
```

```output
3
😀
True
["é\0😀"]
```

## 11. Repeat work with tail recursion

A tail-recursive function can repeat work without growing the call stack.
For collection traversal, the next lessons introduce `for` loops.

```plenty
def sum_to(n: i64, total: i64) -> i64:
    if n <= 0:
        return total
    sum_to(n - 1, total + n)

def main() -> Result[(), IoError]:
    print(sum_to(100, 0))?
    Ok(())
```

```output
5050
```

The recursive call is the last operation on that path. Plenty reuses its call
frame in native code. An explicit
`return sum_to(n - 1, total + n)` works too. In contrast, `1 + recurse(...)`
still has addition to do after the call and is not a tail call.

## 12. Explore with small programs

Save an example in a file, edit it, and run it again with `plenty example.plenty`
(or `cargo run -- example.plenty` from this repository). Each run compiles the
complete file and starts a fresh process. Use `--check` for feedback without
running your program.

Use `print(value)` inside `main` to see a value. A final expression is the
function's return value and must match its declared type; `main() -> ()` cannot
end with a bare integer. Parse/type errors run no code. Runtime errors
can occur after earlier effects, such as printing, have already happened.
The run command preserves the program's input, output, and working directory,
and reports a failing exit status when the program fails.

There is no interactive REPL or interpreter, and JIT compilation is out of scope.

## 13. Lists, moves, and explicit copies

A list contains values of one type. Use `list[T]` in signatures and annotations.
An empty list needs an annotation or a typed constructor such as `list[i64]()`.

```plenty
def main() -> Result[(), Failure]:
    mut original: list[i64] = [10, 20]?
    mut changed = copy(original)?
    changed.append(30)?
    changed[0] = 99
    print(original)?
    print(changed)?
    print(changed[-1])?
    print(len(changed))?
    Ok(())
```

```output
[10, 20]
[99, 20, 30]
30
3
```

The explicit `copy(original)` creates independent contents. Without `copy`,
assignment transfers ownership and the old binding cannot be used. Updates happen
in place and require a `mut` owner or an exclusive reference (lesson 22).
Owned function parameters are immutable bindings; reference parameters can grant
permission to change the caller's value.
Negative indices count from the end. Invalid indices stop the program with a
runtime error.

`copy(original)` returns a `Result`, as do all allocating operations. The
examples here propagate failures with `?`; lesson 19 shows how to recover instead.

Collections can nest. To update an inner list while keeping the outer collection,
use `mut child = copy(rows[0])?`, update `child`, then assign it back with
`rows[0] = child`. This last assignment transfers the child into the collection. Direct nested
assignment such as `rows[0][0] = 1` is not implemented.

## 14. Dictionaries and sets

Dictionaries map a single key type to a single value type. Keys and set elements
may be integers, booleans, or strings. Dictionary values can include collections.

```plenty
def main() -> Result[(), Failure]:
    mut scores: dict[str, i64] = {"Ada": 10, "Grace": 20}?
    scores["Ada"] = 12
    scores.insert("Lin", 30)?
    print(scores["Ada"])?
    print("Grace" in scores)?
    print(scores.keys()?)?
    print(scores.values()?)?

    mut names: set[str] = set()?
    names.add("Ada")?
    names.add("Ada")?
    print(len(names))?
    print("Ada" in names)?
    Ok(())
```

```output
12
True
["Ada", "Grace", "Lin"]
[12, 20, 30]
1
True
```

A repeated dictionary key replaces its value and keeps its insertion position.
`keys()` and `values()` return new lists in insertion order. When dictionary
values contain mutable collections, use `copy(scores).values()` to request
independent payloads explicitly. Sets remove
duplicates and have no promised iteration order. `{}` is an empty dictionary;
use an annotation with `set()` or write `set[str]()` for an empty set.

Missing keys in `scores[key]` are runtime errors. Use `scores.get(key)` when
absence is expected. It returns an `Option`: `Some(value)` if found, or `Nothing`
if absent. Match the two possibilities explicitly:

```plenty
def main() -> Result[(), Failure]:
    scores = {"Ada": 12, "Grace": 0}?
    match scores.get("Ada"):
        case Some(score):
            print(score)?
        case Nothing:
            print("unknown player")?
    print(scores.get("Grace"))?
    print(scores.get("Lin"))?
    print(scores["Ada"])?
    Ok(())
```

```output
12
Option[i64].Some(0)
Option[i64].Nothing
12
```

A stored zero, `False`, empty string, or `Nothing` still counts as a present
value. For example, looking up a stored `Nothing` returns `Some(Nothing)`.
There is no optional default argument; use a match to choose a fallback.
The later lessons explain `Option` and its `?` propagation in more detail.

`get` observes the dictionary and key, leaving both available. The lookup itself
does not allocate, so it needs no allocation-error result. String
and immutable enum results retain their existing storage and remain valid even
if the dictionary is subsequently updated or dropped.

`get` supports values such as numbers, booleans, strings, and immutable enums.
It rejects mutable collections, classes, and enums containing them. Use `pop`
to remove those values and take ownership, or use `&dictionary[key]` to borrow
an existing value. Optional borrowed lookup is deferred. No mutable value is
silently copied by `get`.

### Remove an entry and take its value

`pop(key)` removes a dictionary entry and returns `Some(value)`, or `Nothing`
when the key is absent. It requires a mutable dictionary and transfers ownership
of the stored value, so it works with lists and classes too:

```plenty
def main() -> Result[(), Failure]:
    mut groups = {"ready": [1, 2]?, "waiting": [3]?}?
    match groups.pop("ready"):
        case Some(items):
            mut work = items
            work.append(4)?
            print(work)?
        case Nothing:
            print("no work")?
    print(groups)?
    print(groups.pop("missing"))?
    Ok(())
```

```output
[1, 2, 4]
{"waiting": [3]}
Option[list[i64]].Nothing
```

There is no hidden copy. The returned value remains valid if the dictionary is
dropped, and its new owner cleans it up normally. Discarding the result of `pop`
also cleans up the removed value, including any custom `__del__` method.

The operation preserves the order of remaining entries and reuses existing
storage without allocating. Reinserting a removed key puts it at the end.
Removal currently shifts entries and rebuilds the hash index, so its cost grows
with the dictionary's size and reserved capacity. It accepts exactly one key;
there is no default argument. Sets use `discard`, described below.

### Read a list element that might be missing

`items.get(index)` returns `Some(value)` for an existing element or `Nothing`
for an out-of-range index. It takes one `i64` index, including negative indices:

```plenty
def main() -> Result[(), Failure]:
    names = ["Ada", "Bea"]?
    print(names.get(-1))?
    match names.get(2):
        case Some(name):
            print(name)?
        case Nothing:
            print("no name at that position")?
    print(names)?
    Ok(())
```

```output
Option[str].Some("Bea")
no name at that position
["Ada", "Bea"]
```

This is a constant-time read that leaves the list unchanged and allocates nothing.
Numbers and booleans are copied; strings and immutable enum values share their
existing storage. A returned string remains valid after the original list is
updated or dropped. There is no default argument; choose a fallback with `match`.

Like dictionary `get`, this supports scalar and immutable elements. For lists
containing mutable collections or classes, use `pop` to transfer ownership, or
explicitly copy through ordinary indexing. Individual element borrowing is not
implemented yet. Ordinary `items[index]` still traps for an out-of-range index.

### Remove list elements

Lists also have `pop`. With no argument it removes the last element; an `i64`
index selects another position. Negative indices count from the end:

```plenty
def main() -> Result[(), Failure]:
    mut tasks = [[1]?, [2]?, [3]?]?
    match tasks.pop(1):
        case Some(task):
            mut work = task
            work.append(4)?
            print(work)?
        case Nothing:
            print("no task")?
    print(tasks)?
    print(tasks.pop())?
    print(tasks.pop(-1))?
    print(tasks.pop())?
    Ok(())
```

```output
[2, 4]
[[1], [3]]
Option[list[i64]].Some([3])
Option[list[i64]].Some([1])
Option[list[i64]].Nothing
```

As with dictionaries, `pop` transfers ownership and needs a mutable list or an
exclusive reference. Empty lists and out-of-range indices return `Nothing`.
Remaining elements stay in order, capacity is retained, and removal does not
allocate. Removing the last element is constant time; removing an earlier element
shifts the elements after it. The returned owner handles cleanup, so discarding
a successful result also drops its element.

### Remove set members

Use `values.discard(value)` to remove a member from a mutable set. It returns
`True` if the member was present and removed, or `False` if it was absent:

```plenty
def main() -> Result[(), Failure]:
    mut names = {"Ada", "Bea"}?
    name = "Ada"
    print(names.discard(name))?
    print(names.discard(name))?
    print(name)?
    print("Bea" in names)?
    print(len(names))?
    names.add(name)?
    print(len(names))?
    Ok(())
```

```output
True
False
Ada
True
1
2
```

The argument is observed, so `name` remains usable. Missing values are harmless,
and discarding a stored zero, `False`, or empty string still returns `True`.
Removal allocates nothing and preserves capacity for reuse. Like dictionary
removal, it currently rebuilds the hash index after a hit; its cost grows with
the set's size and reserved capacity. Sets still promise no iteration order.
Use an exclusive reference when removing members through a function parameter.

Collection equality compares contents; dictionary and set order do not matter.

## 15. Iterate over values

`for` visits list elements, dictionary keys, set elements, range integers, or
characters of a string. A character is a one-character `str`.

```plenty
def main() -> Result[(), Failure]:
    mut total = 0
    for n in range(1, 5)?:
        total = total + n
    print(total)?

    scores = {"Ada": 10, "Grace": 20}?
    for name in &scores:
        print(name)?
        print(scores[name])?

    print(list(range(5, 0, -2)?)?)?
    print([character? for character in "hé"]?)?
    Ok(())
```

```output
10
Ada
10
Grace
20
[5, 3, 1]
["h", "é"]
```

`range(stop)` starts at zero. `range(start, stop, step)` permits a negative
step, but never zero. The stop value is excluded. A range stores its bounds
without building a list, but currently allocates its own small owner. Both
range construction and list materialization return results:
`list(range(5)?)?` propagates both failures. String iteration similarly returns
one checked character at a time: `[character? for character in text]?`.

Ranges default to `i64`; `range[u8](8)` explicitly produces u8 values. Start
and stop must fit the chosen type. Step remains signed i64, so unsigned ranges
can descend too. A typed bound can also select the element type.

The iterable is evaluated once. Iterating an owned collection transfers it into
the loop; use `for item in &values` to preserve the owner. Borrowed iteration
prevents conflicting mutation; owned list elements become shared references.
Use `&mut values` for mutable element references. To iterate a snapshot while
mutating the original, request it explicitly with `for item in copy(values)`.
Generators are consumed by iteration. Loop variables and new body bindings do not
escape the loop; changes to enclosing `mut` bindings persist. A loop has unit
result. `return` can exit a containing function from a loop. Lesson 17 covers
`break` and `continue`. Tuple unpacking and dictionary `items()` are taught below.

## 16. Build collections with comprehensions

A comprehension produces a new collection from an iterable and optional filters.
The element expression runs only after the filters pass.

```plenty
def main() -> Result[(), Failure]:
    squares = [n * n for n in range(8)? if n % 2 == 0]?
    print(squares)?
    print({n: n * n for n in [2, 3]?}?)?
    print(len({n // 2 for n in range(8)?}?))?
    print([x * 10 + y for x in range(3)? for y in range(x)?]?)?
    Ok(())
```

```output
[0, 4, 16, 36]
{2: 4, 3: 9}
4
[10, 20, 21]
```

Multiple clauses nest from left to right. Later iterables can use earlier
variables, and you can use multiple `if` filters. Variables remain local to
the comprehension. Parenthesize a conditional expression used as an iterable
or filter. Dictionary comprehensions evaluate each key before its value.

`%` is modulo: the result follows the divisor's sign, so `-7 % 3` is `2`.
Both operands must have the same integer type.

Every element must have the same type; there is no implicit numeric widening:

```plenty-error
def main() -> ():
    values = [1, True].unwrap()
```

```error
expected list[i64]
```

Both comprehensions and repeated `append` calls use growing storage in place.
Assignment does not copy collections, and mutation does not secretly copy their
contents. Use `copy` when duplication is intended. Storage is reclaimed as owners
are replaced or leave scope; no `free` calls or reference-count management are needed.

## 17. Repeat until a condition changes

Use `while` when a condition determines how long to repeat. The condition must
be `bool`, and it is checked before every iteration, including the first:

```plenty
def main() -> Result[(), IoError]:
    mut remaining = 3
    while remaining > 0:
        print(remaining)?
        remaining = remaining - 1
    print("go")?
    Ok(())
```

```output
3
2
1
go
```

`continue` skips the rest of the current iteration. `break` exits the loop:

```plenty
def main() -> Result[(), IoError]:
    mut n = 0
    while True:
        n = n + 1
        if n % 2 == 0:
            continue
        if n > 5:
            break
        print(n)?
    Ok(())
```

```output
1
3
5
```

Update the condition's inputs before a `continue` when needed; otherwise a
`while` loop can repeat forever. In a `for` loop, `continue` automatically
advances to the next element:

```plenty
def main() -> Result[(), Failure]:
    for n in range(6)?:
        if n == 1:
            continue
        if n == 4:
            break
        print(n)?
    Ok(())
```

```output
0
2
3
```

Both statements affect only the innermost loop. Use `return` to leave a
function from inside any depth of loops. New bindings in a loop body stay
inside that body, while updates to enclosing `mut` bindings persist. Loops
have unit result; `break` cannot carry a value. Python's loop `else` clauses
are not supported.

The compiler conservatively assumes every loop can finish, even `while True`.
A function returning a value therefore still needs a result after the loop.
Statements directly after an unconditional exit are rejected:

```plenty-error
def main() -> ():
    while True:
        break
        print("unreachable").unwrap()
```

```error
unreachable statement after a control-flow exit
```

## 18. Describe alternatives with enums

An enum says which alternatives a value can have. Each variant may carry typed
data. Match every possibility to extract that data:

```plenty
enum Reading:
    Missing
    Value(i64)
    Invalid(str)

def describe(reading: Reading) -> str:
    match reading:
        case Reading.Missing:
            "no reading"
        case Reading.Value(number):
            "positive" if number > 0 else "nonpositive"
        case Reading.Invalid(reason):
            reason

def main() -> Result[(), Failure]:
    print(describe((Reading.Missing)?))?
    print(describe(Reading.Value(42)?))?
    print(describe(Reading.Invalid("sensor offline")?))?
    Ok(())
```

```output
no reading
positive
sensor offline
```

Variants without data omit parentheses. Payloads may have several positions,
such as `Pair(i64, str)`. Bindings in a case are immutable and stay inside
that case. Use `_` for an unused payload position, or a final whole-value
`case _:` for the remaining variants. Duplicate or missing cases are errors:

```plenty-error
enum Switch:
    On
    Off

def main() -> ():
    match (Switch.On).unwrap():
        case Switch.On:
            print("on").unwrap()
```

```error
non-exhaustive match; missing Switch.Off
```

A final match produces a function's result, like a final `if`. Cases may also
return early or break/continue an enclosing loop. Enum declarations can refer to
other nonrecursive types and aliases. Enums with identical variants but different
names remain different types. Payloads can contain strings, collections, and
other enums. Matching consumes enums containing mutable payloads and transfers
the bound payloads; use `match copy(value)` to preserve such an owner.

## 19. Represent absence and failure explicitly

`Option[T]` has two variants: `Some(T)` and `Nothing`.
`Some`, `Nothing`, `Ok`, and `Err` are built in and need no type prefix.
`Nothing` belongs to one concrete option type; it is not a universal null.

```plenty
def first_positive(values: list[i64]) -> Option[i64]:
    for value in values:
        if value > 0:
            return Some(value)
    Nothing

def main() -> Result[(), Failure]:
    for values in [[-1, 0]?, [-1, 42]?]?:
        match first_positive(values):
            case Some(value):
                print(value)?
            case Nothing:
                print("not found")?
    Ok(())
```

```output
not found
42
```

Use `Result[T, E]` when the absent result has an explanation. Its variants
are `Ok(T)` and `Err(E)`. The return signature supplies both types:

```plenty
def divide(left: i64, right: i64) -> Result[i64, str]:
    if right == 0:
        return Err("division by zero")
    Ok(left // right)

def main() -> Result[(), Failure]:
    for result in [divide(8, 2), divide(8, 0)]?:
        match result:
            case Ok(value):
                print(value)?
            case Err(message):
                print(message)?
    Ok(())
```

```output
4
division by zero
```

There is no implicit unwrapping or exception. Constructors get missing type
information from a binding annotation, function parameter, or return signature.
`Some(42)` already contains enough information to infer `Option[i64]`.
`Nothing` has no payload to infer from; `Ok` and `Err` each need the other
variant's type from context:

```plenty
def main() -> Result[(), IoError]:
    found = Some(42)
    missing: Option[i64] = Nothing
    success: Result[i64, str] = Ok(42)
    failure: Result[i64, str] = Err("not ready")
    print(found == Some(42))?
    print(missing == Option[i64].Nothing)?
    match failure:
        case Ok(value):
            print(value)?
        case Err(message):
            print(message)?
    Ok(())
```

```output
True
True
not ready
```

Qualified forms remain available when you want to state all types at the
construction site. A type alias works as well. Without sufficient context, the
compiler asks for a type instead of guessing:

```plenty-error
def main() -> ():
    answer = Ok(42)
```

```error
cannot infer `Ok`
```

For an operation that can fail but has no success data, return `Result[(), E]`
and construct success with `Ok(())`:

```plenty
def validate(name: str) -> Result[(), str]:
    if len(name) == 0:
        return Err("name is empty")
    Ok(())

def main() -> Result[(), Failure]:
    for result in [validate("Plenty"), validate("")]?:
        match result:
            case Ok(_):
                print("valid")?
            case Err(message):
                print(message)?
    Ok(())
```

```output
valid
name is empty
```

Unit is a real success payload, distinct from the absence represented by
`Nothing`. `Option[()]` also works. Standalone unit bindings and unit parameters
remain unsupported. User-defined enum variants still use their enum's prefix,
even if a variant happens to be named `Ok` or `Some`.

### Pass a failure back with `?`

Put `?` after a `Result` expression to extract its `Ok` payload or return its
`Err` immediately. This keeps a sequence of fallible operations easy to read:

```plenty
def validate(name: str) -> Result[(), str]:
    if len(name) == 0:
        return Err("name is empty")
    Ok(())

def checked_name(name: str) -> Result[str, str]:
    validate(name)?
    Ok(name)

def main() -> Result[(), IoError]:
    print(checked_name("Plenty"))?
    print(checked_name(""))?
    Ok(())
```

```output
Result[str, str].Ok("Plenty")
Result[str, str].Err("name is empty")
```

Here `validate(name)?` has a unit success value, so it can stand alone. On
failure, the final `Ok(name)` never runs. Live local values and previously
evaluated expression temporaries are cleaned up automatically, just as for
an explicit `return`.

`?` works with `Option` too: it extracts `Some` or immediately returns `Nothing`.

```plenty
def positive(n: i64) -> Option[i64]:
    Some(n) if n > 0 else Nothing

def doubled(n: i64) -> Option[i64]:
    Some(positive(n)? * 2)

def main() -> Result[(), IoError]:
    print(doubled(21))?
    print(doubled(-1))?
    Ok(())
```

```output
Option[i64].Some(42)
Option[i64].Nothing
```

The enclosing function must return the same family: `Result` for a `Result`
operand or `Option` for an `Option` operand. Success payload types can differ,
but `Result` error types must match exactly unless the function explicitly
chooses `Failure`, described below. Use `match` when you need to convert errors
while preserving details. `?` is not supported inside generators.

```plenty-error
def read_number() -> Result[i64, str]:
    Err("not a number")

def checked() -> Result[i64, i64]:
    Ok(read_number()?)

def main() -> ():
    print(checked()).unwrap()
```

```error
`?` requires identical Result error types
```

`Option` and `Result` wrappers do not allocate on the heap, including when
nested. Their payloads keep their usual behavior: `Some([1, 2]?)` allocates the
list, but adds no wrapper allocation. Returning or propagating an existing sum
does not allocate a wrapper either. Printing and operations on the payload can
still allocate, and report failures through their own results.

### Discard error details deliberately

When callers only need success or failure, declare `Result[T, Failure]`.
Each `?` can then propagate a different error type:

```plenty
def work(text: str) -> Result[list[u8], Failure]:
    n = u8.parse(text)?               # ParseError
    values = [n, n + 1u8]?           # AllocError
    print("built values")?          # IoError
    Ok(values)

def main() -> Result[(), IoError]:
    print(work("41"))?
    print(work("invalid"))?
    Ok(())
```

```output
built values
Result[list[u8], Failure].Ok([41, 42])
Result[list[u8], Failure].Err(Failure.Unspecified)
```

`Failure.Unspecified` is the type's only value. On an error path, `?` drops the
original error and propagates this marker, with normal scope cleanup. The
conversion and marker do not allocate; custom cleanup can still perform its
own operations. This works in helpers as well as `main`. A `main` that returns
`Err` exits with status one and does not automatically print the error.

This choice discards details, so use a concrete error type when a caller needs
to inspect or report the cause. It only changes `?`: returning an existing
`Result[T, IoError]` directly from a `Result[T, Failure]` function is still a type
error. To report failure yourself, use `Err(Failure.Unspecified)`. End successful
paths explicitly with `Ok(value)` or `Ok(())`.

`Failure` does not catch runtime traps and does not turn `Option.Nothing` into an
error. `.unwrap()` is a separate, explicit choice to terminate immediately on
`Err` or `Nothing`, without normal scope cleanup. For example:

```plenty
def main() -> Result[(), IoError]:
    found = Some(42)
    print(found.unwrap())?
    Ok(())
```

```output
42
```

Use `?` for propagation and `match` for recovery; reserve `.unwrap()` for places
where termination is the intended policy, including the destructor and generator
examples below whose signatures cannot propagate errors.

### Handle collection allocation failures

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

### Copy without losing the source on failure

`copy(value)` returns
`Result[T, AllocError]`. It observes the source, so your original value remains
usable whether copying succeeds or fails. Use `?` to keep the success path short:

```plenty
def extended(source: &list[i64]) -> Result[list[i64], AllocError]:
    mut result = copy(source)?
    result.append(30)?
    Ok(result)

def main() -> Result[(), Failure]:
    original = [10, 20]?
    print(extended(&original))?
    print(original)?
    Ok(())
```

```output
Result[list[i64], AllocError].Ok([10, 20, 30])
[10, 20]
```

This also works for nested collections, ordinary classes, and enum payloads.
If copying a later field or element fails, the partial copy is cleaned up and
the original remains unchanged. Scalars and immutable values, including strings,
need no allocation to copy; their existing immutable storage can be shared.

Generators and classes with
custom cleanup cannot be copied, including when nested in another value.
The operation covers duplication itself. In `copy([1, 2]?)?`, the first `?`
handles construction and the second handles copying.

### Take dictionary snapshots with recoverable allocation

`keys()` and `values()` build new lists in dictionary insertion order,
returning `Result[list[T], AllocError]`. They take no arguments. Use `?` to
propagate allocation failure:

```plenty
def names(scores: &dict[str, i64]) -> Result[list[str], AllocError]:
    result = scores.keys()?
    Ok(result)

def main() -> Result[(), Failure]:
    mut scores = {"Ada": 10, "Bea": 20}?
    saved = scores.values()
    scores["Ada"] = 30
    print(names(&scores))?
    print(saved)?
    print(scores)?
    Ok(())
```

```output
Result[list[str], AllocError].Ok(["Ada", "Bea"])
Result[list[i64], AllocError].Ok([10, 20])
{"Ada": 30, "Bea": 20}
```

For numbers, strings, and other immutable values, the dictionary remains usable
after either outcome. Strings and immutable enum storage are shared; the new
list does not copy their contents. The snapshot remains valid after the source
changes or leaves scope. An empty dictionary produces `Ok([])` if its new list
header can be allocated.

For owned values such as lists or classes, `values()` requires an owned
temporary, just like `values()`. Request duplication explicitly to keep the
original:

```plenty
def rows(data: &dict[str, list[i64]]) -> Result[list[list[i64]], AllocError]:
    copy(data)?.values()

def main() -> Result[(), Failure]:
    data = {"first": [1, 2]?, "second": [3]?}?
    print(rows(&data))?
    print(data)?
    Ok(())
```

```output
Result[list[list[i64]], AllocError].Ok([[1, 2], [3]])
{"first": [1, 2], "second": [3]}
```

Calling `make_dictionary().values()` instead transfers owned values from
that temporary into the list. If allocation fails, the temporary and its values
are cleaned up. `keys()` never takes the dictionary's values, so it works on
borrowed dictionaries regardless of their value type.

Both operations reserve all list storage before retaining or transferring any
elements. They report `OutOfMemory` or `CapacityOverflow` through `AllocError`.
There are no separate aborting snapshot methods.

### Reverse a list in place

`reverse()` mutates a list through a `mut` binding or `&mut` reference and returns
`()`. It allocates nothing and moves no elements out of the list:

```plenty
def flip(items: &mut list[i64]) -> ():
    items.reverse()

def main() -> Result[(), Failure]:
    mut items = [10, 20, 30]?
    flip(&mut items)
    print(items)?
    items.reverse()
    print(items)?
    Ok(())
```

```output
[30, 20, 10]
[10, 20, 30]
```

Owned elements such as classes and nested lists work too. Reversing does not
copy or destroy them; eventual list cleanup follows their new order. To keep
the original order separately, explicitly copy the list first.

### Empty a collection and reuse its storage

Lists, dictionaries, and sets have `clear() -> ()`. The method requires mutable
access, drops the contents, and retains capacity for later insertions:

```plenty
def main() -> Result[(), Failure]:
    mut items = [10, 20]?
    items.clear()
    print(items)?
    print(items.append(30))?
    print(items)?
    mut scores = {"Ada": 10}?
    scores.clear()
    print(scores)?
    Ok(())
```

```output
[]
Result[(), AllocError].Ok(())
[30]
{}
```

Clearing itself allocates no storage. Owned elements are destroyed before the
method returns, in list order or dictionary insertion order; custom destructors
may have their own effects and allocations. Clearing an empty collection is fine.

### Extend a list by transferring another list

`items.extend(other)` returns `Result[(), AllocError]`. It moves all elements
from another list of the same type after reserving space:

```plenty
def combined() -> Result[list[i64], AllocError]:
    mut items = list[i64].new()?
    more = [10, 20, 30]?
    items.extend(more)?
    Ok(items)

def main() -> Result[(), IoError]:
    print(combined())?
    Ok(())
```

```output
Result[list[i64], AllocError].Ok([10, 20, 30])
```

The source is consumed even when reservation fails; its elements are then cleaned
up and the destination stays unchanged. To retain a source, explicitly write
`items.extend(copy(source)?)`. References and general iterables are not
accepted as sources yet. The destination needs mutable access. Existing capacity
is reused, and moving elements does not clone or allocate their contents.
The literal in this example retains its usual terminal allocation policy; use
fallible constructors and insertions if source creation must also be recoverable.

### Update a dictionary by transferring another dictionary

`destination.update(source)` returns `Result[(), AllocError]`, replacing
existing values and adding new entries. Existing keys keep their positions;
new keys appear in source insertion order:

```plenty
def merge(destination: &mut dict[str, i64], source: dict[str, i64]) -> Result[(), AllocError]:
    destination.update(source)

def main() -> Result[(), Failure]:
    mut scores = {"Ada": 10, "Bea": 20}?
    changes = {"Bea": 25, "Cam": 30, "Ada": 15}?
    print(merge(&mut scores, changes))?
    print(scores)?
    Ok(())
```

```output
Result[(), AllocError].Ok(())
{"Ada": 15, "Bea": 25, "Cam": 30}
```

The source is consumed on both outcomes. On allocation failure, the destination
stays unchanged and source owners are dropped. Use
`destination.update(copy(source)?)` to keep the source. Storage for every
new entry is reserved before any value is replaced. Old destination values are
dropped during successful replacement; their destructors keep their own effects.
Updating only existing keys needs no new runtime storage. Only same-typed
dictionaries are accepted; pair iterables and keyword arguments are deferred.

### Update a set by transferring another set

Sets also support `update(source) -> Result[(), AllocError]`. Members already
present are kept, and missing members transfer from the source:

```plenty
def main() -> Result[(), Failure]:
    mut names = {"Ada", "Bea"}?
    more = {"Bea", "Cam"}?
    print(names.update(more))?
    print(len(names))?
    print("Ada" in names and "Bea" in names and "Cam" in names)?
    Ok(())
```

```output
Result[(), AllocError].Ok(())
3
True
```

The source must be a set of the same type and is consumed on both outcomes.
Use `names.update(copy(source)?)` to preserve it. Failed reservation leaves
the destination unchanged and cleans up the consumed source. Duplicate-only and
empty sources need no new runtime storage; existing capacity can cover new
members too. Set iteration order remains unspecified.

### Compare sets

Set relationships borrow both sets and allocate nothing:

```plenty
def main() -> Result[(), Failure]:
    needed = {"read"}?
    available = {"read", "write"}?
    print(needed.issubset(available))?
    print(available.issuperset(needed))?
    print(needed.isdisjoint({"write"}?))?
    print(len(needed))?
    Ok(())
```

```output
True
True
True
1
```

Both operands must have the same set type. Empty sets are subsets of every set
and disjoint from every set; neither operand is consumed.

### Combine sets without consuming them

`union` returns an independent set inside a `Result`. Both inputs are borrowed:

```plenty
def merged(a: &set[i64], b: &set[i64]) -> Result[set[i64], AllocError]:
    a.union(b)

def main() -> Result[(), Failure]:
    a = {1, 2}?
    b = {2, 3}?
    match merged(&a, &b):
        case Ok(values):
            print(len(values))?
            print(1 in values and 3 in values)?
        case Err(error):
            print(error)?
    print(len(a))?
    Ok(())
```

```output
3
True
2
```

An allocation failure preserves the inputs. Set iteration order is unspecified.

Find common members with `intersection`:

```plenty
def main() -> Result[(), Failure]:
    match {1, 2}?.intersection({2, 3}?):
        case Ok(common):
            print(len(common))?
            print(2 in common)?
        case Err(error):
            print(error)?
    Ok(())
```

```output
1
True
```

Use `difference` to remove another set's members from a new result:

```plenty
def main() -> Result[(), Failure]:
    wanted = {1, 2, 3}?
    completed = {2, 3, 4}?
    match wanted.difference(completed):
        case Ok(remaining):
            print(len(remaining))?
            print(1 in remaining)?
        case Err(error):
            print(error)?
    print(len(wanted))?
    Ok(())
```

```output
1
True
3
```

The receiver determines which members can appear in the result. Both inputs remain usable.

`symmetric_difference` keeps members found in exactly one of the two inputs:

```plenty
def main() -> Result[(), Failure]:
    match {1, 2}?.symmetric_difference({2, 3}?):
        case Ok(changed):
            print(len(changed))?
            print(1 in changed and 3 in changed)?
            print(2 in changed)?
        case Err(error):
            print(error)?
    Ok(())
```

```output
2
True
False
```

### Filter a set in place

`intersection_update` retains common members in a mutable set. It borrows the
other set, returns unit, and reuses existing storage without allocating:

```plenty
def main() -> Result[(), Failure]:
    mut selected = {1, 2, 3}?
    allowed = {2, 3, 4}?
    selected.intersection_update(allowed)
    print(len(selected))?
    print(1 in selected)?
    print(2 in selected and 3 in selected)?
    print(len(allowed))?
    Ok(())
```

```output
2
False
True
3
```

The source must be a different set: it stays borrowed while the destination is
mutated. Use `intersection` when you want an independent result instead.

Use `difference_update` to remove the other set's members in place:

```plenty
def main() -> Result[(), Failure]:
    mut pending = {1, 2, 3}?
    done = {2, 3, 4}?
    pending.difference_update(done)
    print(len(pending))?
    print(1 in pending)?
    print(len(done))?
    Ok(())
```

```output
1
True
3
```

This also allocates nothing and keeps the source usable. To remove every member
without needing another set, use `clear()`.

### Classify text without allocating

`isascii()` accepts only ASCII characters, including controls and NUL.
`isspace()` checks for nonempty text containing only Unicode whitespace:

```plenty
def main() -> Result[(), IoError]:
    print("hello".isascii())?
    print("é".isascii())?
    print("".isascii())?
    print(" \t\n".isspace())?
    print(" x ".isspace())?
    print("".isspace())?
    Ok(())
```

```output
True
False
True
True
False
False
```

Both queries borrow their receiver and allocate nothing. Whitespace means the
same Unicode White_Space characters removed by `strip`; NUL and zero-width
space do not count.

### Remove an exact prefix or suffix

These methods remove one complete match at the chosen end. They return an
independent string inside a `Result`, leaving the original usable:

```plenty
def title(name: &str) -> Result[str, AllocError]:
    name.removeprefix("draft-")?.removesuffix(".txt")

def main() -> Result[(), IoError]:
    name = "draft-notes.txt"
    print(title(&name))?
    print("abab".removeprefix("ab"))?
    print("notes.txt".removesuffix(".csv"))?
    Ok(())
```

```output
Result[str, AllocError].Ok("notes")
Result[str, AllocError].Ok("ab")
Result[str, AllocError].Ok("notes.txt")
```

Empty patterns do nothing. Each method can fail to allocate even when the
contents stay unchanged. Use `strip` for surrounding whitespace instead.

### Search and count list elements

For lists of integers, floats, booleans, or strings, these queries borrow their
inputs and allocate nothing:

```plenty
def main() -> Result[(), Failure]:
    names = ["Ada", "Lin", "Ada"]?
    print(names.count("Ada"))?
    print(names.find("Ada"))?
    print(names.rfind("Ada"))?
    print(names.find("missing"))?
    Ok(())
```

```output
2
Option[i64].Some(0)
Option[i64].Some(2)
Option[i64].Nothing
```

Searches return optional zero-based positions. The query must match the element
type exactly. Float comparisons follow IEEE rules: NaN never matches, and positive
and negative zero match each other. Searching lists of aggregates is not yet supported.

### Take a list slice

`items.slice(start, stop)` returns a new list inside a `Result`. Start is
included and stop is excluded. Negative bounds count from the end, and bounds
outside the list clamp to its ends:

```plenty
def middle(items: &list[i64]) -> Result[list[i64], AllocError]:
    items.slice(1, -1)

def main() -> Result[(), Failure]:
    items = [10, 20, 30, 40]?
    print(middle(&items))?
    print(items.slice(-100, 100))?
    print(items.slice(3, 1))?
    print(items)?
    Ok(())
```

```output
Result[list[i64], AllocError].Ok([20, 30])
Result[list[i64], AllocError].Ok([10, 20, 30, 40])
Result[list[i64], AllocError].Ok([])
[10, 20, 30, 40]
```

Both bounds must be `i64`. Slice syntax such as `items[1:3]`, omitted bounds,
and steps are not implemented yet. Even an empty result can fail to allocate
its list header; use `?` or `match` to handle `AllocError`.

Strings and immutable enums in the slice share their immutable storage and
outlive the original list. For owned elements such as nested lists or classes,
the receiver must be an owned temporary. Use
`copy(items)?.slice(start, stop)` to preserve the original, or call the
method on a list returned by a function to transfer selected elements. Unselected
elements of that temporary are dropped after the call. If allocation fails,
the temporary is cleaned up in full; a borrowed source remains unchanged.

### Check a text prefix or suffix

`startswith` and `endswith` borrow strings and return `bool` without allocating.
Matches are literal and case-sensitive; an empty prefix or suffix always matches.

```plenty
def main() -> Result[(), IoError]:
    name = "report.plenty"
    print(name.startswith("report"))?
    print(name.endswith(".plenty"))?
    print(name.endswith(".PLENTY"))?
    print("".startswith(""))?
    Ok(())
```

```output
True
True
False
True
```

Each method takes one string, including a reference. Bounds and lists or tuples
of alternative patterns are not supported yet.

### Find a substring

`find` and `rfind` return the first or last match as `Option[i64]`. Unlike Python's
`-1` sentinel, a missing match is `Nothing`. Positions count Unicode scalars:

```plenty
def main() -> Result[(), IoError]:
    text = "é🙂é🙂"
    print(text.find("🙂"))?
    print(text.rfind("🙂"))?
    print(text.find("missing"))?
    print(text.rfind(""))?
    Ok(())
```

```output
Option[i64].Some(1)
Option[i64].Some(3)
Option[i64].Nothing
Option[i64].Some(4)
```

Both operations borrow their inputs and allocate nothing. Use `match` or `?` in
an `Option`-returning function. An empty needle matches at the beginning for
`find` and at the end for `rfind`. Each takes one literal pattern; optional
bounds and regexes are not supported.

### Count occurrences

`text.count(needle)` returns an `i64` without allocating. Matches are literal
and do not overlap. An empty needle counts Unicode scalar boundaries:

```plenty
def main() -> Result[(), IoError]:
    print("banana".count("ana"))?
    print("aaaaa".count("aa"))?
    print("é🙂".count(""))?
    print("".count(""))?
    print("text".count("missing"))?
    Ok(())
```

```output
1
2
3
1
0
```

The method borrows both strings and takes exactly one pattern argument.

### Trim surrounding whitespace

`strip()`, `lstrip()`, and `rstrip()` remove Unicode whitespace
from both ends, the left end, or the right end. Each returns `Result[str, AllocError]`:

```plenty
def main() -> Result[(), IoError]:
    text = "  é🙂  "
    print(text.strip())?
    print(text.lstrip())?
    print(text.rstrip())?
    print(" \t\n".strip())?
    Ok(())
```

```output
Result[str, AllocError].Ok("é🙂")
Result[str, AllocError].Ok("é🙂  ")
Result[str, AllocError].Ok("  é🙂")
Result[str, AllocError].Ok("")
```

They borrow the source and allocate only the final independent string. Empty
and unchanged results still need that allocation. Interior whitespace is kept;
NUL and zero-width space are not trimmed. These methods use Unicode White_Space
and take no arguments; custom character sets are not supported yet.

### Repeat text

`text.repeat(count)` accepts an `i64` and returns `Result[str, AllocError]`.
Zero and negative counts produce an empty string:

```plenty
def main() -> Result[(), IoError]:
    print("é🙂".repeat(3))?
    print("x".repeat(0))?
    print("x".repeat(-2))?
    print("x".repeat(9223372036854775807))?
    Ok(())
```

```output
Result[str, AllocError].Ok("é🙂é🙂é🙂")
Result[str, AllocError].Ok("")
Result[str, AllocError].Ok("")
Result[str, AllocError].Err(AllocError.CapacityOverflow)
```

The source stays usable. The runtime checks the complete size before allocating
one independent output string; even empty and single-copy outputs can fail to
allocate. Empty input with a large count is handled directly. String multiplication
syntax is not supported yet.

### Slice text by Unicode scalar position

Strings also have `slice(start, stop) -> Result[str, AllocError]`, using the
same required `i64` bounds and clamping rules as lists. Positions count Unicode
scalar values, just like `len` and indexing:

```plenty
def main() -> Result[(), IoError]:
    text = "Aé🙂Z"
    print(text.slice(1, -1))?
    print(text.slice(-100, 100))?
    print(text.slice(3, 1))?
    print(text)?
    Ok(())
```

```output
Result[str, AllocError].Ok("é🙂")
Result[str, AllocError].Ok("Aé🙂Z")
Result[str, AllocError].Ok("")
Aé🙂Z
```

The result owns its UTF-8 bytes and outlives the source. The operation creates
only the final string, including for an empty or whole-string slice, so even
these cases can return `Err(AllocError.OutOfMemory)`. Source strings are never
modified. Finding byte boundaries scans the text without a temporary character
list. Combining marks count separately; positions are not grapheme clusters or
byte offsets.

### Replace literal text with recoverable allocation

`text.replace(old, new)` replaces every non-overlapping match, scanning left
to right, and returns `Result[str, AllocError]`. It borrows all three strings:

```plenty
def rename(text: &str) -> Result[str, AllocError]:
    updated = text.replace("Plenty", "plenty")?
    Ok(updated)

def main() -> Result[(), IoError]:
    original = "Plenty is Plenty"
    print(rename(&original))?
    print(original)?
    print("aaaaa".replace("aa", "X"))?
    print("Aé".replace("", "-"))?
    print("banana".replace("na", ""))?
    Ok(())
```

```output
Result[str, AllocError].Ok("plenty is plenty")
Plenty is Plenty
Result[str, AllocError].Ok("XXa")
Result[str, AllocError].Ok("-A-é-")
Result[str, AllocError].Ok("ba")
```

An empty search string inserts the replacement before, between, and after Unicode
scalars. An empty replacement removes matches. Matches are literal and inserted
text is not searched again; regexes and a replacement-count limit are not supported.
The result owns its bytes and survives destruction of every input.

The runtime checks the complete output size and allocates only the final string.
Even a call with no matches, or an empty result, makes that allocation and can
return `OutOfMemory`; unrepresentable sizes return `CapacityOverflow`. Failure
leaves all inputs unchanged. Constructing the arguments follows their own
allocation policies.

### Build text with recoverable allocation

Use `text.concat(other)` to combine two strings. To combine a list of strings,
use `separator.join(parts)`. Both return `Result[str, AllocError]`:

```plenty
def greeting(names: &list[str]) -> Result[str, AllocError]:
    joined = ", ".join(names)?
    "Hello, ".concat(joined)?.concat("!")

def main() -> Result[(), Failure]:
    names = ["Ada", "Bea"]?
    print(greeting(&names))?
    print(names)?
    print("-".join([]?))?
    Ok(())
```

```output
Result[str, AllocError].Ok("Hello, Ada, Bea!")
["Ada", "Bea"]
Result[str, AllocError].Ok("")
```

The methods observe their inputs, so `names` is still available afterward.
Joining first calculates the final size and then allocates one output buffer;
it does not build a succession of intermediate strings. An empty list gives an
empty string, and a one-element list adds no separator. Empty pieces still count:
joining `["a", "", "b"]` with `"-"` gives `"a--b"`.

UTF-8 characters and embedded `\0` are preserved, just as with ordinary strings.
These methods currently require strings and a `list[str]`; joining a generator
or another kind of iterable is not supported yet. Input construction, string `+`,
string indexing, and printing also return results when they can fail.

### Split text into fields

Use `text.split(separator)` to split on an explicit, nonempty string. It
returns `Result[list[str], AllocError]`, so `?` can propagate allocation failure:

```plenty
def fields(line: &str) -> Result[list[str], AllocError]:
    line.split("::")

def main() -> Result[(), IoError]:
    line = "Ada::Bea::::"
    match fields(&line):
        case Ok(parts):
            print(parts)?
            print(" / ".join(parts))?
        case Err(error):
            print(error)?
    print(line)?
    print("".split(","))?
    Ok(())
```

```output
["Ada", "Bea", "", ""]
Result[str, AllocError].Ok("Ada / Bea /  / ")
Ada::Bea::::
Result[list[str], AllocError].Ok([""])
```

Adjacent and trailing separators preserve empty fields. Matches do not overlap:
`"aaaaa".split("aa")` succeeds with `["", "", "a"]`. A separator that does not
occur produces a single piece containing the whole input. Unicode and embedded
`\0` work in both the input and the separator.

The operation observes both strings. Each output piece has independent storage;
the result remains valid after the input is dropped. If allocation fails partway
through, the partial result is cleaned up and both inputs remain unchanged.

An empty separator is invalid and terminates the program with a runtime error;
it does not return `AllocError`. Check `len(separator) > 0` when the separator
comes from user input. There is currently no omitted-separator whitespace mode
or maximum-split argument.

### Look up a character without trapping

Use `text.get(index)` when the index might be out of range or character
allocation might fail. It returns `Result[Option[str], AllocError]`: `Result`
reports allocation failure, while `Option` tells you whether the index exists.

```plenty
def show_character(text: &str, index: i64) -> Result[(), Failure]:
    character = text.get(index)?
    match character:
        case Some(value):
            print(value)?
        case Nothing:
            print("missing")?
    Ok(())

def main() -> Result[(), IoError]:
    text = "café🙂"
    print(show_character(&text, -1))?
    print(show_character(&text, 3))?
    print(show_character(&text, 5))?
    print(text)?
    Ok(())
```

```output
🙂
Result[(), Failure].Ok(())
é
Result[(), Failure].Ok(())
missing
Result[(), Failure].Ok(())
café🙂
```

Indices are `i64` and count Unicode scalars, as with `text[index]`. Negative
indices count backward from the end; `-1` selects the last scalar. Empty strings
and indices outside either end return `Ok(Nothing)` without allocating. A valid
index allocates one independent string for the character. Neither wrapper
allocates, and the original string remains available on every outcome.

In the example, `?` handles the allocation-error path and leaves an `Option[str]`
for the match. `Nothing` is a successful lookup with no character, so it does not
propagate an error. Ordinary `text[index]` still terminates the program for a
missing index; allocation failure is returned in its Result. Both forms scan UTF-8 to reach the requested
scalar; repeated indexing is not a constant-time way to traverse text.

## 20. Produce values lazily with generators

A generator function declares `Generator[T]` and uses `yield` statements:

```plenty
def countdown(start: i64) -> Generator[i64]:
    print("starting").unwrap()
    mut remaining = start
    while remaining > 0:
        yield remaining
        remaining = remaining - 1

def main() -> Result[(), Failure]:
    numbers = countdown(3)?
    print("created")?
    for number in numbers:
        print(number)?
    Ok(())
```

```output
created
starting
3
2
1
```

Calling the function evaluates its arguments immediately, but its body starts
only when iteration requests the first value. Each yield pauses the body and
keeps its locals for the next request. A bare return or the end of the body
finishes the generator. It cannot return a value.

Comprehensions and collection constructors also consume generators:

```plenty
def numbers(limit: i64) -> Generator[i64]:
    for n in range(limit).unwrap():
        yield n

def main() -> Result[(), Failure]:
    print([n * n for n in numbers(6)? if n % 2 == 1]?)?
    print(list(numbers(3)?)?)?
    Ok(())
```

```output
[1, 9, 25]
[0, 1, 2]
```

For one value at a time, name a mutable generator and call `next`.
It returns `Some(value)` or `Nothing`; exhaustion stays exhausted:

```plenty
def once() -> Generator[str]:
    yield "hello"

def main() -> Result[(), Failure]:
    mut messages = once()?
    print(next(messages))?
    print(next(messages))?
    print(next(messages))?
    Ok(())
```

```output
Option[str].Some("hello")
Option[str].Nothing
Option[str].Nothing
```

Breaking out of consuming iteration drops the suspended generator without
executing the statements after its last yield. There are no generator expressions,
`yield from`, `send`, or async operations. Yielded owned values transfer ownership; use `yield copy(value)` to retain an
independent mutable value in the generator. A generator may yield strings,
collections, classes, and enums, but not another generator.

## 21. Understand which values copy and which move

Integers and booleans copy cheaply. Strings share immutable storage without
copying their bytes. Enums containing only these immutable values can share storage
too. Collections, classes, generators, and enums containing owned values move on
assignment, owned argument passing, and return. Use `copy(value)` to duplicate
collections or their enclosing enums. Nested mutable contents are copied too;
immutable strings and enums can share storage. Generators and values containing
custom class cleanup cannot be copied.

A generator owns a position in an advancing computation. Assignment transfers
that owner instead of copying the position:

```plenty
def once() -> Generator[i64]:
    yield 42

def main() -> Result[(), Failure]:
    first = once()?
    second = first
    print(list(second)?)?
    Ok(())
```

```output
[42]
```

After a move, the old binding cannot be used:

```plenty-error
def once() -> Generator[i64]:
    yield 42

def main() -> ():
    first = once().unwrap()
    second = first
    list(first).unwrap()
    pass
```

```error
use of moved or possibly moved binding
```

Arguments, returns, and `for` iteration also move generators. `next` is the
exception: it temporarily uses a named mutable generator without consuming the
owner. A mutable binding can be reinitialized after moving its previous value.

The compiler checks all possible continuing paths. A move on just one branch
makes later reuse unsafe. Within a loop, an outer generator must be reinitialized
before any path repeats the loop. These rules are intentionally conservative.
Generators cannot be stored in collections or enum payloads yet.

Loans for local references and function arguments are checked before compilation.
The next lesson explains how to use them.

## 22. Borrow instead of transferring ownership

A shared reference, `&T`, permits observation. An exclusive reference, `&mut T`,
permits mutation. The owner remains responsible for cleanup. Function signatures
state which access is needed:

```plenty
def total(values: &list[i64]) -> i64:
    mut result = 0
    for value in values:
        result = result + value
    result

def add(values: &mut list[i64], value: i64) -> Result[(), AllocError]:
    values.append(value)?
    Ok(())

def main() -> Result[(), Failure]:
    mut numbers = [1, 2]?
    print(total(&numbers))?
    add(&mut numbers, 3)?
    print(numbers)?
    Ok(())
```

```output
3
[1, 2, 3]
```

References can also be local bindings. They are immutable bindings themselves;
`&mut` grants access to the target. Use `*reference` to read a scalar or replace
the target. Collection operations such as `append`, indexing, and `len` work
through references directly.

```plenty
def main() -> Result[(), IoError]:
    mut score = 10
    reference = &mut score
    *reference = *reference + 5
    print(*reference)?
    score = 20
    print(score)?
    Ok(())
```

```output
15
20
```

The exclusive borrow ends after the last use of `reference`, so assigning to
`score` afterward is allowed. A reference that will be used later keeps its loan
live, including across branches and loop iterations:

```plenty-error
def main() -> ():
    mut numbers = [1, 2].unwrap()
    view = &numbers
    numbers.append(3).unwrap()
    print(view).unwrap()
```

```error
conflicting borrow
```

A reference can be reborrowed temporarily. An exclusive reference may lend shared
or exclusive access, but its conflicting access is suspended while that child
borrow is live. Reference arguments automatically reborrow an existing reference;
they do not transfer the referenced owner.

This subset borrows named bindings and their class fields. Reference bindings must
be initialized with `&name`, `&mut name` (including field paths), or a call
returning a reference, and cannot be reassigned. References cannot
be stored in collections, captured by generators, or
remain live across `yield`. Element references such as `&items[0]` are not yet
supported. Use `next` through an exclusive generator reference when borrowing
a generator; generator iteration still consumes its owner.

Functions can return references when exactly one parameter is a reference. The
returned value must borrow that parameter, never a local owner. The caller keeps
the original value alive and borrowed until the result's last use.

```plenty
class Pair:
    left: i64
    right: i64
    def left_ref(self: &mut Pair) -> &mut i64:
        &mut self.left

def main() -> Result[(), Failure]:
    mut pair = Pair(1, 2)?
    left = pair.left_ref()
    *left = 8
    print(pair.left)?
    Ok(())
```
```output
8
```

This simple getter preserves the returned field's identity, so unrelated fields
remain available after the call. More complex getters conservatively protect the
whole argument. A mutable returned reference requires a mutable reference parameter.
Use a named receiver, class field, or element of named storage; temporary receivers
cannot supply a reference that outlives their owner.

## 23. Cleanup and early drop

Owned values clean up automatically when their scope exits, including through
`return`, `break`, and `continue`. Moving a value transfers that responsibility.
A borrow never becomes responsible for destroying the original value.

Use `drop(value)` to release an owner earlier. The consumed binding becomes
unavailable; a mutable binding may then receive another value:

```plenty
def pending() -> Generator[i64]:
    print("started").unwrap()
    yield 1

def main() -> Result[(), Failure]:
    mut numbers = [1, 2, 3]?
    drop(numbers)
    numbers = [4]?
    print(numbers)?

    task = pending()?
    drop(task)
    print("done")?
    Ok(())
```

```output
[4]
done
```

Dropping a generator cleans its captured values without resuming its body.
Borrow checking prevents dropping an owner while a reference still needs it.
A borrow can end at its last use; that does not implicitly destroy its owner.

Strings may share immutable storage. Dropping one string owner does not invalidate
another. There is no tracing garbage collector. Fatal runtime traps terminate
without unwinding scopes. Classes can provide custom cleanup with `__del__`, as
the next lesson shows.

## 24. Group fields and methods in a class

A Plenty class is a record with a fixed set of typed fields. Use the familiar
Python layout, without a decorator. If you omit `__init__`, the compiler generates
a constructor taking the fields in their declaration order:

```plenty
class Point:
    x: i64
    y: i64

    def squared_length(self) -> i64:
        self.x * self.x + self.y * self.y

    def shift(self: &mut Point, amount: i64) -> ():
        self.x = self.x + amount
        self.y = self.y + amount

def main() -> Result[(), Failure]:
    mut point = Point(3, 4)?
    print(point.squared_length())?
    point.shift(1)
    mut changed = copy(point)?
    changed.x = 20
    print(point)?
    print(changed)?
    Ok(())
```

```output
25
Point(x=4, y=5)
Point(x=20, y=5)
```

Ordinary methods borrow `self` read-only by default: `self` is shorthand for
`self: &Point` here. A method that changes fields declares `self: &mut Point`.
Calling it requires a `mut` binding or an exclusive reference. Every other
parameter and every return has an explicit type.

Class instances move even when their fields are all integers. `other = point`
transfers ownership; `copy(point)` requests independent fields. Reading an
integer or string field is fine, but an owned field such as a list or another
class must be borrowed, observed, or explicitly copied. Classes have structural
equality and a generated printed representation.

There is no inheritance, dynamic attribute creation, or class-variable syntax.
Fields currently have no default values, and calls use positional arguments.

## 25. Control construction with __init__

Define `__init__` when construction needs calculations or checks. It replaces the
generated field constructor. Its bare `self` is an exclusive reference, and it
must return `()`:

```plenty
class Span:
    start: i64
    end: i64

    def __init__(self, start: i64, length: i64) -> ():
        self.start = start
        self.end = self.start + length

    def length(self) -> i64:
        self.end - self.start

def main() -> Result[(), Failure]:
    span = Span(10, 5)?
    print(span)?
    print(span.length())?
    Ok(())
```

```output
Span(start=10, end=15)
5
```

Every field must be initialized on every path out of the constructor. You may
read a field already initialized, but cannot pass the whole unfinished `self` to
another function or method. A loop might never execute, so initializing a field
only inside a loop is insufficient.

```plenty-error
class Pair:
    first: i64
    second: i64

    def __init__(self, first: i64) -> ():
        self.first = first

def main() -> ():
    pass
```

```error
fields not initialized: second
```

## 26. Borrow fields and clean up resources

Fields can be borrowed directly, including fields inside nested classes:

```plenty
class Basket:
    count: i64
    items: list[str]

def main() -> Result[(), Failure]:
    mut basket = Basket(0, []?)?
    count = &mut basket.count
    *count = 2
    basket.items.append("apple")?
    basket.items.append("pear")?
    print(basket)?
    Ok(())
```

```output
Basket(count=2, items=["apple", "pear"])
```

The borrow ends after `count`'s last use. Distinct fields can be borrowed or
changed independently. Borrowing a whole record still overlaps all its fields.
Collection element references such as `&basket.items[0]` protect the collection
against changes that could invalidate the element's address.
You can replace a class-valued field by assignment, but cannot replace a whole
class through `*reference = new_instance` yet.

```plenty
class Position:
    x: i64
    y: i64

def main() -> Result[(), Failure]:
    mut position = Position(1, 2)?
    x = &mut position.x
    y = &mut position.y
    *x = 10
    *y = 20
    print(*x)?
    print(*y)?
    Ok(())
```
```output
10
20
```

Use `__del__` for cleanup that belongs to an owned instance. It runs automatically
when the instance leaves scope, is replaced, or is explicitly dropped. Its bare
`self` is an exclusive reference, like in `__init__`:

```plenty
class Resource:
    name: str

    def __del__(self) -> ():
        print(("release " + self.name).unwrap()).unwrap()

class Pair:
    first: Resource
    second: Resource

    def __del__(self) -> ():
        print("release pair").unwrap()

def work() -> Result[(), Failure]:
    pair = Pair(Resource("first")?, Resource("second")?)?
    spare = Resource("spare")?
    print("working")?
    Ok(())

def main() -> Result[(), Failure]:
    work()?
    print("done")?
    Ok(())
```

```output
working
release spare
release pair
release first
release second
done
```

Locals clean up in reverse declaration order. A class's `__del__` runs before
automatic field cleanup; fields then clean up in declaration order. You do not
need to destroy the fields yourself. Moving an instance transfers its cleanup
responsibility and does not run the destructor.

Lifecycle methods are not called directly. A class with custom cleanup, or one
containing a value with custom cleanup, cannot be copied yet. That avoids
duplicating ownership of a resource accidentally. A destructor cannot yield or
return a recoverable error; an explicit closing method could return `Result`
when reporting failure matters. Fatal traps do not run cleanup.

Functions holding values with observable cleanup currently use ordinary calls
in return position so that cleanup still happens after the called function.
Numeric tail-recursive functions from the earlier lesson keep their tail calls.

## 27. Split a program into modules

An import names a source file. It does not execute that file. A library module
contains declarations and imports; only the application's `main` starts the
program. Names are private to their defining module unless marked `pub`.

Save this companion file as `geometry.plenty`:

```plenty-file geometry.plenty
pub class Point:
    pub x: i64
    pub y: i64

    pub def squared_length(self) -> i64:
        self.x * self.x + self.y * self.y

pub def make_point() -> Result[Point, AllocError]:
    Point(3, 4)
```

Save the application beside it, for example as `main.plenty`:

```plenty
import geometry
from geometry import Point as Position

def main() -> Result[(), Failure]:
    point: Position = geometry.make_point()?
    print(point.squared_length())?
    print(point.x)?
    Ok(())
```

```output
25
3
```

`import geometry` makes declarations reachable through `geometry.Name`.
`from geometry import Point as Position` binds just that type under a local name.
You can also write `import geometry as geo`, then use `geo.Point`. These aliases
refer to the same declarations, not copies or new types. Two separate modules
may each define `Point`; those are distinct types.

Imports are absolute. `import tools.geometry` reads `tools/geometry.plenty`
under the source root. Directories provide namespaces and need no `__init__`
file. The root defaults to the entry file's directory. If the entry file is
deeper in the tree, select the root explicitly:

```sh
plenty --module-root src src/app/main.plenty
plenty --module-root src --compile src/app/main.plenty -o app
plenty --module-root src --check-module src/tools/geometry.plenty
```

`--check` checks a complete application and requires `main`. `--check-module`
checks a library and its imports without that requirement. An imported function
named `main` is an ordinary function; importing it does not call it.

Public classes do not automatically expose their fields or methods. Mark each
part of the public API explicitly. Without `__init__`, the generated field
constructor is public only when the class and every field are public. To keep
fields private while allowing construction, declare `pub def __init__`:

Save this example's companion file as `counter.plenty`:

```plenty-file counter.plenty
pub class Counter:
    value: i64

    pub def __init__(self, start: i64) -> ():
        self.value = start

    pub def increment(self: &mut Counter) -> ():
        self.value = self.value + 1

    pub def read(self) -> i64:
        self.value

    def __del__(self) -> ():
        print("counter closed").unwrap()
```

Application:

```plenty
from counter import Counter

def main() -> Result[(), Failure]:
    mut count = Counter(41)?
    count.increment()
    print(count.read())?
    Ok(())
```

```output
42
counter closed
```

The private destructor still runs automatically. Private fields and methods are
accessible throughout their defining module, including from helper functions,
but not from another module. Direct reads, writes, and borrows all enforce this.
Here is a rejected example with its own companion file, `secret.plenty`:

```plenty-file secret.plenty
pub class Secret:
    value: i64

    pub def __init__(self, value: i64) -> ():
        self.value = value
```

```plenty-error
from secret import Secret

def main() -> ():
    item = Secret(42).unwrap()
    print(item.value).unwrap()
```

```error
secret.Secret.value` is private
```

`pub` also applies to functions, enums, and type aliases. A public enum exposes
all its variants. Public signatures cannot mention private classes or enums,
even through aliases or containers. Ordinary imports are private bindings;
importing a name does not re-export it. Relative imports, wildcards, circular
imports, and `pub import` are not supported yet.

Privacy controls direct access, not secrecy: automatic printing and equality
still inspect a class's complete structural value, including private fields.
`pub` does not export a C symbol or change the native calling convention.

## Reading integers from text

Use a sized type's `parse` method for external text. It borrows the string and
returns an allocation-free `Result`. Decimal ASCII digits and an optional sign
are accepted, with surrounding Unicode whitespace ignored. Numeric prefixes,
underscores, and non-ASCII digits are not accepted.

```plenty
def main() -> Result[(), IoError]:
    print(u8.parse(" 255 "))?
    print(u8.parse("256"))?
    print(i64.parse("hello"))?
    Ok(())
```
```output
Result[u8, ParseError].Ok(255)
Result[u8, ParseError].Err(ParseError.OutOfRange)
Result[i64, ParseError].Err(ParseError.Invalid)
```

Functions returning the same error type can propagate parsing failures with `?`.

## Reading floating-point text

Floating-point parsing accepts decimal/exponent forms and surrounding whitespace.
Overflow returns `OutOfRange`; underflow can round to zero. Explicit `inf`,
`infinity`, and `nan` are accepted case-insensitively, with optional signs.

```plenty
def main() -> Result[(), IoError]:
    print(f32.parse("1.25e2"))?
    print(f32.parse("1e100"))?
    print(f64.parse("-0.0"))?
    Ok(())
```
```output
Result[f32, ParseError].Ok(125.0)
Result[f32, ParseError].Err(ParseError.OutOfRange)
Result[f64, ParseError].Ok(-0.0)
```

## Converting scalars to text

`str.from` converts numbers or booleans with one recoverable string
allocation. It returns `Result[str, AllocError]`, so it combines with `?` and
the existing fallible string operations. Floats use shortest round-trip text.

```plenty
def main() -> Result[(), IoError]:
    print(str.from(42))?
    print(str.from(-0.0f32))?
    print(str.from(True))?
    Ok(())
```
```output
Result[str, AllocError].Ok("42")
Result[str, AllocError].Ok("-0.0")
Result[str, AllocError].Ok("True")
```

## Writing text with recoverable errors

`write_stdout(text)` borrows a string and writes exactly its contents, without a
newline. Success returns the number of Unicode characters. An I/O error can
follow a partial write, so retrying the whole string can duplicate output.

```plenty
def main() -> Result[(), IoError]:
    result = write_stdout("Hello!\n")
    print(result)?
    Ok(())
```
```output
Hello!
Result[i64, IoError].Ok(7)
```

`IoError.System(code)` carries a native OS error code (or zero when unavailable).
`IoError.Data` carries `DataError.InvalidUtf8` or
`DataError.Allocation(AllocError)`. All these error values allocate nothing.

## Flushing output and reporting diagnostics

Use `write_stderr(text)` for diagnostics. It returns the same character-count
result as `write_stdout`. `flush_stdout()` and `flush_stderr()` return
`Result[(), IoError]`. Flush when the caller needs to observe buffered-write
errors or when displaying a prompt; a flush does not promise disk durability.

```plenty
def report() -> Result[(), IoError]:
    write_stdout("ready")?
    flush_stdout()?
    Ok(())

def main() -> Result[(), IoError]:
    print(report())?
    Ok(())
```
```output
readyResult[(), IoError].Ok(())
```

## Reading a line

`input()` returns `Result[Option[str], IoError]`. It removes a trailing LF or
CRLF. `Some("")` means an empty line; `Nothing` means end-of-file. UTF-8 errors
and allocation failures return `Err`. A failure can consume input, so retrying
does not promise to repeat the same line. Input currently requires a Unix host.
For a prompt, call `write_stdout` and `flush_stdout` before `input()`.

This reusable function is ready for a caller to connect to stdin; the example
does not invoke it, so running the tutorial never waits for keyboard input.

```plenty
def echo_line() -> Result[bool, IoError]:
    match input()?:
        case Some(line):
            write_stdout(line)?
            write_stdout("\n")?
            Ok(True)
        case Nothing:
            Ok(False)

def main() -> Result[(), IoError]:
    print("echo_line is ready")?
    Ok(())
```
```output
echo_line is ready
```

## Command-line arguments

`args()` returns `Result[list[str], IoError]`. Index zero is the executable's
invocation name, followed by the supplied arguments. Spaces within one argument
remain intact. Each call produces an independent list; allocation failure and
invalid UTF-8 are recoverable errors. Pass arguments when launching a compiled
binary, for example `./program first "two words"`.

```plenty
def count_arguments() -> Result[bool, IoError]:
    values = args()?
    Ok(len(values) >= 1)

def main() -> Result[(), IoError]:
    print(count_arguments())?
    Ok(())
```
```output
Result[bool, IoError].Ok(True)
```

## Reading a UTF-8 file

`read_text(path)` opens, reads, and closes a file, returning
`Result[str, IoError]`. Like Python text reading, CRLF and bare CR become LF.
Encoding is always strict UTF-8. The initial file backend requires Linux.
Paths are relative to the process working directory; the helper owns and closes
its temporary handle even when reading or allocating fails.

This function can read your own configuration file. The tutorial checks its
types without depending on a file on your machine.

```plenty
def configuration(path: &str) -> Result[str, IoError]:
    read_text(path)

def main() -> Result[(), IoError]:
    print("configuration reader is ready")?
    Ok(())
```
```output
configuration reader is ready
```

## Writing a UTF-8 file

`write_text(path, text)` creates or replaces a file and returns the number of
Unicode characters written. It writes exact UTF-8 bytes without translating
newlines. Its temporary file handle is closed before it returns. Allocation of
the path happens before truncating an existing file, but OS failures can leave
truncated or partially written contents. This is not an atomic-save operation.

Run this in a scratch directory: it replaces `plenty-example.txt`.

```plenty
def save_and_read() -> Result[str, IoError]:
    write_text("plenty-example.txt", "Hello, é!\n")?
    read_text("plenty-example.txt")

def main() -> Result[(), IoError]:
    print(save_and_read())?
    Ok(())
```
```output
Result[str, IoError].Ok("Hello, é!\n")
```

The tutorial tests execute file examples in temporary directories, independently
for compile-and-run and compiled-binary execution.

## Appending text

`append_text(path, text)` adds exact UTF-8 bytes to the end of a file, creating
it if necessary. It returns the number of characters appended. Like writing,
it can leave partial output after an OS error; one call is not guaranteed to be
an atomic record when several processes write concurrently.

Run this in a scratch directory: it replaces `plenty-log.txt` before appending.

```plenty
def log_example() -> Result[str, IoError]:
    write_text("plenty-log.txt", "started\n")?
    append_text("plenty-log.txt", "finished\n")?
    read_text("plenty-log.txt")

def main() -> Result[(), IoError]:
    print(log_example())?
    Ok(())
```
```output
Result[str, IoError].Ok("started\nfinished\n")
```

These helpers cover whole files. Automatic cleanup closes their private handles
on both success and failure.

## Owned file handles

`open(path)` opens an existing file for reading. `open(path, "w")` creates or
truncates a file, and `open(path, "a")` creates or appends. All return
`Result[File, IoError]`. Files move on assignment and cannot be copied. They
close automatically when dropped; call `close()` to observe a close error.

Run this in a scratch directory: it replaces `plenty-handle.txt`.

```plenty
def inspect_file() -> Result[(), IoError]:
    mut file = open("plenty-handle.txt", "w")?
    print(file.closed)?
    file.close()?
    print(file.closed)?
    file.close()?
    Ok(())

def main() -> Result[(), IoError]:
    print(inspect_file())?
    Ok(())
```
```output
False
True
Result[(), IoError].Ok(())
```

Closing twice succeeds. A closed file remains a valid value; it no longer owns
a handle. Automatic close cannot report errors or promise durable writes.
The initial implementation supports Linux and reports unsupported I/O elsewhere.
Use `with` to close a file at the end of a block. `read()` returns independent
text from the current position through EOF. It normalizes CRLF and bare CR to
LF and validates UTF-8. Another read at EOF returns an empty string.

Run this in a scratch directory: it replaces `plenty-reading.txt`.

```plenty
def read_example() -> Result[str, IoError]:
    write_text("plenty-reading.txt", "hello\r\nworld")?
    with open("plenty-reading.txt")? as file:
        return Ok(file.read()?)

def main() -> Result[(), IoError]:
    print(read_example())?
    Ok(())
```
```output
Result[str, IoError].Ok("hello\nworld")
```

Returning the owned text closes the file first. `with &mut file as stream:`
also closes on exit, while retaining the original owner in its closed state.
Read errors may consume input before failing. Automatic context exit cannot
report close errors; call `file.close()?` explicitly when those matter.

## Recoverable construction

### Borrowing collection elements

Borrow an element when you want to inspect or change it without moving its owner:

```plenty
def main() -> Result[(), Failure]:
    mut counts = {"visits": 1}?
    value = &mut counts["visits"]
    *value = 2
    print(counts)?
    Ok(())
```
```output
{"visits": 2}
```

The loan ends after its last use. While an element reference is live, the
collection cannot grow, remove entries, or move. Different indices are treated
as potentially overlapping. Missing keys and out-of-range indices still trap.

### Formatting and output

`str.repr` formats a borrowed value; `print` writes a value and newline,
reporting formatting allocation failures and output errors:

```plenty
def main() -> Result[(), Failure]:
    values = [1, 2]?
    print(str.repr(values))?
    print(print(values))?
    Ok(())
```
```output
Result[str, AllocError].Ok("[1, 2]")
[1, 2]
Result[(), IoError].Ok(())
```

Formatting failure writes nothing. An output error can leave a partial write.
The source remains usable; neither call copies a mutable collection.

### Collection literals

Collection literals directly return `Result[collection, AllocError]`:

```plenty
def build() -> Result[list[list[i64]], AllocError]:
    [[1, 2]?, [3]?]

def main() -> Result[(), IoError]:
    print({"answer": 42})?
    print(build())?
    Ok(())
```
```output
Result[dict[str, i64], AllocError].Ok({"answer": 42})
Result[list[list[i64]], AllocError].Ok([[1, 2], [3]])
```

Construction stops at its first allocation failure, drops the partial collection,
and skips later entries. The nested `?` operations extract each inner list and
propagate its failure to `build`. There is no prefix `try` keyword.
Empty displays need context: `values: Result[list[i64], AllocError] = []`, or
`values: list[i64] = []?` inside a function returning `Result[..., AllocError]`.

### Collecting iterators

Comprehensions support the same explicit allocation boundary:

```plenty
def squares() -> Result[list[i64], AllocError]:
    [n * n for n in range(6)? if n % 2 == 0]

def main() -> Result[(), IoError]:
    print(squares())?
    Ok(())
```
```output
Result[list[i64], AllocError].Ok([0, 4, 16])
```

Output allocation failure stops iteration and drops the partial result. It does
not undo earlier effects or catch failures in ordinary calls inside the expression.

Collect an owned iterator with recoverable list growth:

```plenty
def numbers() -> Generator[i64]:
    yield 3
    yield 6

def collect() -> Result[list[i64], AllocError]:
    source = numbers.new()?
    list[i64].from(source)

def main() -> Result[(), IoError]:
    print(collect())?
    Ok(())
```
```output
Result[list[i64], AllocError].Ok([3, 6])
```

`from` consumes the source. If output allocation fails, it drops the partial
list and remaining iterator. Earlier iterator side effects are not undone, and
allocations inside the generator body still follow that body's chosen APIs.

Sets provide the same constructor, removing duplicates in first-seen order:

```plenty
def main() -> Result[(), Failure]:
    print(set[i64].from([3, 1, 3, 2]?))?
    Ok(())
```
```output
Result[set[i64], AllocError].Ok({3, 1, 2})
```

The example propagates input construction failure with `[3, 1, 3, 2]?`.
Dictionary sources iterate
over keys; borrowed sources and string iteration are not supported by `from`.

### Creating generators

Generators can be returned inside `Result` and extracted with `?`:

```plenty
def numbers() -> Generator[i64]:
    yield 10
    yield 20

def source() -> Result[Generator[i64], AllocError]:
    numbers.new()

def total() -> Result[i64, AllocError]:
    mut values = source()?
    mut result = 0
    for n in values:
        result = result + n
    Ok(result)

def main() -> Result[(), IoError]:
    print(total())?
    Ok(())
```
```output
Result[i64, AllocError].Ok(30)
```

`numbers()` and its `numbers.new()` alias return `Result[Generator[T], AllocError]`.
Argument expressions retain
their own failure behavior, and moved arguments are dropped on allocation failure.
Dropping a wrapped generator releases its captures without executing its body.

### Creating enum values

Enum variants also offer explicit fallible construction:

```plenty
enum Message:
    Empty
    Text(str)

def main() -> Result[(), IoError]:
    print(Message.Text.new("hello"))?
    print(Message.Empty.new())?
    Ok(())
```
```output
Result[Message, AllocError].Ok(Message.Text("hello"))
Result[Message, AllocError].Ok(Message.Empty)
```

Payload arguments move into the constructor and are dropped if its allocation
fails. Nullary variants take no arguments to `new`.

### Creating classes

Use `Class.new(...)` to handle failure to allocate instance storage. It takes
the same arguments as the ordinary constructor and works with `?`:

```plenty
class Point:
    x: i64
    y: i64

def point() -> Result[Point, AllocError]:
    Ok(Point.new(3, 4)?)

def main() -> Result[(), IoError]:
    print(point())?
    Ok(())
```
```output
Result[Point, AllocError].Ok(Point(x=3, y=4))
```

Arguments move into the call even when allocation fails. Failure drops those
arguments; it does not run the new instance's initializer or destructor.
Allocations inside argument expressions and an ordinary `__init__` retain their
own failure behavior.

An initializer can also propagate allocation failures itself:

```plenty
class Buffer:
    values: list[i64]

    def __init__(self: &mut Buffer, size: i64) -> Result[(), AllocError]:
        self.values = list[i64].with_capacity(size)?
        Ok(())

def main() -> Result[(), IoError]:
    print(Buffer.new(8))?
    Ok(())
```
```output
Result[Buffer, AllocError].Ok(Buffer(values=[]))
```

Both `Buffer(...)` and `Buffer.new(...)` return the checked construction result.
If initialization fails, the fields already
initialized are dropped, but the incomplete instance's `__del__` is skipped.
Successful initialization must fill every field and return `Ok(())`.

## Reading lines

`readline()` keeps the terminating newline, translating LF, CRLF, and bare CR
to `"\n"`. An empty line is `"\n"`; EOF is `""`. The last line need not have a
terminating newline. Unlike `input()`, line reading retains that terminator.

Run this in a scratch directory: it replaces `plenty-lines.txt`.

```plenty
def count_lines() -> Result[i64, IoError]:
    write_text("plenty-lines.txt", "first\r\n\rfinal")?
    mut count = 0
    with open("plenty-lines.txt")? as file:
        while True:
            line = file.readline()?
            if line == "":
                break
            count = count + 1
    Ok(count)

def main() -> Result[(), IoError]:
    print(count_lines())?
    Ok(())
```
```output
Result[i64, IoError].Ok(3)
```

Each call reports decoding, I/O, or allocation failure through its Result. You
can mix `readline()` and `read()` on the same file; both share newline state.
Failures may consume input, so retrying is not a rollback. Direct `for line in
file` iteration is not yet implemented. Size-limited reads and saved positions are
introduced below.

## Writing through a file

`write(text)` returns a character count and preserves exact bytes, including
newlines. `flush()` flushes runtime buffers; files are currently unbuffered.
Use `sync()` when you need to request that the OS synchronize file contents and
metadata. Closing or flushing alone does not make that request.

Run this in a scratch directory: it replaces `plenty-stream.txt`.

```plenty
def write_example() -> Result[(), IoError]:
    with open("plenty-stream.txt", "w")? as file:
        print(file.write("hello\n")?)?
        file.flush()?
        file.sync()?
        file.close()?
    with open("plenty-stream.txt", "a")? as file:
        file.write("goodbye\n")?
    Ok(())

def main() -> Result[(), IoError]:
    print(write_example())?
    Ok(())
```
```output
6
Result[(), IoError].Ok(())
```

Explicit close lets you propagate its error. Context exit then closes the already
closed owner harmlessly. Writing can leave a prefix after an OS error, and append
calls are not guaranteed to be atomic records across processes. Synchronization
still follows your filesystem's durability rules.

## Context managers

Use `with` when a class needs an action at the end of a block. Its `__enter__`
method supplies the `as` value; `__exit__` takes only a mutable receiver and
returns unit. No inheritance or protocol declaration is needed.

```plenty
class Message:
    text: str
    def __enter__(self: &mut Message) -> str:
        print("enter").unwrap()
        self.text
    def __exit__(self: &mut Message) -> ():
        print("exit").unwrap()

def main() -> Result[(), Failure]:
    with Message("hello")? as text:
        print(text)?
    print("after")?
    Ok(())
```
```output
enter
hello
exit
after
```

The manager moves into the block. The entry value and body locals are cleaned up
before `__exit__`, and the manager is dropped afterward. This also happens on
`return`, `?`, `break`, and `continue`; nested managers exit in reverse order.
An owned entry value can be moved out, for example by returning it. The `as`
binding itself is only visible inside the body. Omit `as` for unit entry methods.
Acquire fallible resources before entry: `with acquire()?:` propagates acquisition
failure without entering that context.

To keep a manager after the block, borrow it explicitly. The original binding
is exclusively borrowed until exit completes.

```plenty
class Counter:
    count: i64
    def __enter__(self: &mut Counter) -> i64:
        self.count = self.count + 1
        self.count
    def __exit__(self: &mut Counter) -> ():
        self.count = self.count + 10

def main() -> Result[(), Failure]:
    mut counter = Counter(0)?
    with &mut counter as n:
        print(n)?
    print(counter.count)?
    Ok(())
```
```output
1
11
```

Several managers can share one `with` statement. They enter from left to right
and exit from right to left, just like nested blocks. Later manager expressions
can use earlier `as` bindings.

```plenty
class Label:
    text: str
    def __enter__(self: &mut Label) -> str:
        print(self.text).unwrap()
        self.text
    def __exit__(self: &mut Label) -> ():
        print(self.text).unwrap()

def main() -> Result[(), Failure]:
    with Label("first")? as first, Label("second")? as second:
        print("body")?
    Ok(())
```
```output
first
second
body
second
first
```

Exit is infallible and cannot suppress errors. Check fallible writes or flushes
explicitly. Fatal traps do not run exits, and `yield` inside `with` is not yet
supported.

Most resource managers will return a reference to themselves. This gives the
body access to their methods and fields while `with` retains ownership and
arranges cleanup. The reference cannot escape the block.

```plenty
class Counter:
    count: i64
    def __enter__(self: &mut Counter) -> &mut Counter:
        &mut self
    def __exit__(self: &mut Counter) -> ():
        print(self.count).unwrap()

def main() -> Result[(), Failure]:
    with Counter(1)? as counter:
        counter.count = 7
    Ok(())
```
```output
7
```

## Bounded stream reads

Read a bounded number of characters with `read(count)`. The count is Unicode
characters, not UTF-8 bytes. Zero consumes nothing; a negative count reads to EOF.

```plenty
def demo() -> Result[(), IoError]:
    write_text("bounded.txt", "é🦀hello")?
    with open("bounded.txt")? as file:
        print(file.read(2)?)?
        print(file.read()?)?
    Ok(())
def main() -> Result[(), IoError]:
    print(demo())?
    Ok(())
```
```output
é🦀
hello
Result[(), IoError].Ok(())
```


`readline(count)` limits a line read in the same way. A long line can arrive in
several pieces; a negative count reads the rest of the line.

```plenty
def demo() -> Result[(), IoError]:
    write_text("lines.txt", "abcd\nnext")?
    with open("lines.txt")? as file:
        print(file.readline(2)?)?
        print(file.readline(2)?)?
        print(file.readline(2))?
    Ok(())
def main() -> Result[(), IoError]:
    print(demo())?
    Ok(())
```
```output
ab
cd
Result[str, IoError].Ok("\n")
Result[(), IoError].Ok(())
```

## Checking stream capabilities

Use `readable()` and `writable()` to inspect an open file through a shared reference.
Closed files return an error; `closed` itself remains an infallible property.

```plenty
def demo() -> Result[(), IoError]:
    with open("capabilities.txt", "w")? as file:
        print(file.readable()?)?
        print(file.writable()?)?
    Ok(())
def main() -> Result[(), IoError]:
    print(demo())?
    Ok(())
```
```output
False
True
Result[(), IoError].Ok(())
```

## Creating a file without overwriting

Use mode `"x"` when an existing path should be an error. This check happens
atomically during creation, so there is no separate existence-check race.

```plenty
def demo() -> Result[(), IoError]:
    with open("new.txt", "x")? as file:
        file.write("first version")?
    print(read_text("new.txt")?)?
    Ok(())
def main() -> Result[(), IoError]:
    print(demo())?
    Ok(())
```
```output
first version
Result[(), IoError].Ok(())
```

Add `+` to an open mode to enable both reading and writing. `r+` preserves an
existing file, `w+` truncates or creates, `x+` creates exclusively, and `a+`
starts at EOF and always appends writes. Writes overwrite bytes at the current
position; they do not insert characters. Use care with multibyte text.

```plenty
def demo() -> Result[(), IoError]:
    write_text("update.txt", "hello")?
    with open("update.txt", "r+")? as file:
        file.read(1)?
        file.write("a")?
    print(read_text("update.txt")?)?
    Ok(())
def main() -> Result[(), IoError]:
    print(demo())?
    Ok(())
```
```output
hallo
Result[(), IoError].Ok(())
```

## Saving and restoring text positions

`tell()` returns an opaque `u64` position. Pass it back to `seek()` on the same
file to resume reading there, or use `seek(0u64)` to rewind. Do not calculate with
these values: they include newline state and are not byte or character counts.
Saved positions are only meaningful while the file contents remain unchanged.

```plenty
def demo() -> Result[(), IoError]:
    write_text("positions.txt", "one\r\ntwo")?
    with open("positions.txt")? as file:
        file.readline()?
        saved = file.tell()?
        print(file.read()?)?
        file.seek(saved)?
        print(file.read()?)?
    Ok(())
def main() -> Result[(), IoError]:
    print(demo())?
    Ok(())
```
```output
two
two
Result[(), IoError].Ok(())
```

`truncate(size)` resizes a writable file in **bytes** and returns its new length.
With no size, it truncates at the current physical position. Neither form moves
the cursor. A text-position cookie is not a byte size. Cutting through a multibyte
character makes the file invalid UTF-8; a later read reports that error.

```plenty
def demo() -> Result[(), IoError]:
    write_text("short.txt", "keep rest")?
    with open("short.txt", "r+")? as file:
        file.read(4)?
        print(file.truncate()?)?
    print(read_text("short.txt")?)?
    Ok(())
def main() -> Result[(), IoError]:
    print(demo())?
    Ok(())
```
```output
4
keep
Result[(), IoError].Ok(())
```

## Reading a collection of lines

`readlines()` collects the remaining lines into an independent `list[str]`,
keeping their translated newlines. At EOF it returns an empty list. Failures
release the partial list, but may have consumed input. For large or untrusted
files, bounded `readline(count)` lets you control memory use instead.

```plenty
def demo() -> Result[list[str], IoError]:
    write_text("collection.txt", "first\r\n\nlast")?
    with open("collection.txt")? as file:
        return file.readlines()
def main() -> Result[(), IoError]:
    print(demo())?
    Ok(())
```
```output
Result[list[str], IoError].Ok(["first\n", "\n", "last"])
```

`writelines(lines)` writes a `list[str]` without consuming it. It inserts no
newlines: include them in the strings when wanted. The operation returns
`Result[(), IoError]`; an error may leave a partially written file.

```plenty
def demo() -> Result[(), Failure]:
    lines = ["one\n", "two\n"]?
    with open("written.txt", "w")? as file:
        file.writelines(lines)?
    print(len(lines))?
    with open("written.txt")? as file:
        print(file.readlines()?)?
    Ok(())
def main() -> Result[(), IoError]:
    print(demo())?
    Ok(())
```
```output
2
["one\n", "two\n"]
Result[(), Failure].Ok(())
```

## Splitting text that is already in memory

`splitlines()` returns an independent list without line endings. Pass
`True` to keep the original endings. A final newline does not add an extra
empty line, and empty text produces an empty list. Unlike file reads, this method
also recognizes Unicode line/paragraph separators and the other Python-style
text line boundaries.

```plenty
def main() -> Result[(), IoError]:
    print("first\r\n\nlast\n".splitlines())?
    print("first\r\nlast".splitlines(True))?
    print("".splitlines())?
    Ok(())
```
```output
Result[list[str], AllocError].Ok(["first", "", "last"])
Result[list[str], AllocError].Ok(["first\r\n", "last"])
Result[list[str], AllocError].Ok([])
```

## Borrowing elements in a loop

Use `&items` to inspect owned list elements without moving or copying them.
Each loop variable is a shared reference. Use `&mut items` to get mutable
references, including for scalar elements. The list itself cannot grow, shrink,
or be moved during either loop. Shared loops over copyable elements still yield
values, as they did before.

```plenty
class Score:
    value: i64

def main() -> Result[(), Failure]:
    mut scores = [Score(2)?, Score(5)?]?
    for score in &mut scores:
        score.value = score.value + 1
    print([score.value for score in &scores]?)?
    mut numbers = [10, 20]?
    for number in &mut numbers:
        *number = *number + 2
    print(numbers)?
    Ok(())
```
```output
[3, 6]
[12, 22]
```

## Returning and unpacking tuples

A tuple groups values with different types. Write `(value,)` for one component;
parentheses without a comma simply group an expression. Function signatures may
spell a tuple as `(i64, str)` or `tuple[i64, str]`. Unpacking transfers owned
components, and `_` discards a component. Indices must be integer literals.

```plenty
def measurement() -> Result[(i64, str), AllocError]:
    (42, "cm")

def main() -> Result[(), Failure]:
    value, unit = measurement()?
    print(value)?
    print(unit)?
    for number, word in [(1, "one")?, (2, "two")?]?:
        print((number, word)?)?
    print((3, "three"))?
    Ok(())
```
```output
42
cm
(1, "one")
(2, "two")
Result[tuple[i64, str], AllocError].Ok((3, "three"))
```

Tuple storage currently allocates. `(a, b)` returns `Result[tuple[A, B], AllocError]`;
it consumes its evaluated components even on failure. As with fallible collection
displays, use checked operations separately inside component expressions.

## Dictionary key/value loops

`items()` borrows a dictionary in insertion order. It works directly in loops
and comprehensions, without allocating item tuples or a snapshot. Owned values
are shared references; scalar and string values remain values. Use an exclusive
borrow to update values. Keys stay immutable.

```plenty
def main() -> Result[(), Failure]:
    mut counts = {"apple": 2, "pear": 3}?
    for fruit, count in (&mut counts).items():
        *count = *count + 1
    print([(fruit, count)? for fruit, count in counts.items()]?)?
    print([a + b for a, b in [(1, 2)?, (3, 4)?]?]?)?
    Ok(())
```
```output
[("apple", 3), ("pear", 4)]
[3, 7]
```

The first comprehension explicitly builds a snapshot of tuples. For dictionaries
containing owned values, use `copy(value)?` when a snapshot
needs independent owned values. The dictionary cannot grow or shrink while an
item loop is using it. An `items()` view cannot yet be stored in a variable.

## Choosing numeric types in expressions

Type arguments go before the call: `range[u8](8)`. A collection annotation or
function return type can also guide a directly written range comprehension.

```plenty
def squares() -> Result[list[u8], AllocError]:
    [n * n for n in range(8)? if n % 2 == 0]

def main() -> Result[(), Failure]:
    print(squares()?)?
    print([n * n for n in range[u8](8)? if n % 2 == 0]?)?
    small: list[u16] = [n + 1 for n in range(3)?]?
    print(small)?
    print(list(range[u8](5, 0, -2)?)?)?
    fraction: f32 = 3.5
    print(fraction * 2)?
    Ok(())
```
```output
[0, 4, 16, 36]
[0, 4, 16, 36]
[1, 2, 3]
[5, 3, 1]
7.0
```

Context guides unsuffixed literals, never changes an existing value's type.
`x: u8 = 256` fails, as does assigning an i64 binding to a u8 binding. Explicit
suffixes remain authoritative. A range's bounds must fit its element type;
`range[u8](256)` therefore fails even though its stop is exclusive.

Inference stays within straightforward expressions. It does not infer a range
type backward through filters, arbitrary calls, or a previously stored range.
Use explicit `range[T]` in those cases. There is no silent numeric widening or
narrowing.

## Writing generic functions

Declare type parameters after the function name and supply them before the call.
Ownership still follows the concrete type: `identity[list[i64]]` transfers the
list, while a function taking `&T` borrows its argument.

```plenty
def identity[T](value: T) -> T:
    value

def sum_to[T: IntType](stop: T) -> Result[T, AllocError]:
    mut total: T = 0
    for n in range[T](stop)?:
        total = total + n
    Ok(total)

def first[T](values: &list[T]) -> &T:
    &values[0]

def main() -> Result[(), Failure]:
    print(sum_to[u16](5)?)?
    values = identity[list[i64]]([3, 4]?)
    print(first[i64](&values))?
    print(values)?
    Ok(())
```
```output
10
3
[3, 4]
```

`IntType` restricts a parameter to integer types. Unconstrained `T` is useful
when the body only moves, borrows, or uses operations supported by the chosen
concrete type. Each specialization is checked and compiled once. Type arguments
are currently mandatory; generic classes and methods are not implemented yet.

## Requiring methods with a protocol

A protocol describes an interface a generic function needs. A class satisfies
it by having methods with matching signatures; no inheritance or registration
is required. Importing a protocol does not add methods to a class.

```plenty
protocol Readable:
    def read(self) -> str:
        pass

class Message:
    text: str
    def read(self) -> str:
        self.text

def read_message[T: Readable](source: &T) -> str:
    source.read()

def main() -> Result[(), Failure]:
    message = Message("hello")?
    print(read_message[Message](&message))?
    Ok(())
```
```output
hello
```

`self` means a shared receiver. Write `self: &mut ProtocolName` when a required
method mutates the receiver. Method parameters, return types, and receiver
borrowing must match exactly. All required methods are checked at specialization,
including methods the generic function does not happen to use. Across modules,
the class methods must be visible to the generic function's defining module.

Protocols currently constrain class type arguments; they cannot be stored as
values. Use `source: &T` with `T: Readable`, not `source: Readable`. Protocol
fields, inheritance, and default method implementations are deferred.

## Borrowing a field through a getter

A simple getter that directly returns a field preserves the field's identity
for the borrow checker. After the call, unrelated fields remain available:

```plenty
class Pair:
    x: i64
    y: i64
    def x_ref(self: &mut Pair) -> &mut i64:
        &mut self.x

def main() -> Result[(), Failure]:
    mut pair = Pair(1, 2)?
    x = pair.x_ref()
    pair.y = 7
    *x = 9
    print(pair)?
    Ok(())
```
```output
Pair(x=9, y=7)
```

More complex getters, including ones that choose between fields, conservatively
borrow the whole argument. Indexed references also retain the collection's whole
borrow. The getter call itself still needs the access its receiver type requests.

## Where the language goes next

This guide deliberately uses implemented features. Broader file stream operations,
recursive types, stored references, closures, and C interoperability remain future work; async/await is out of
scope. See [DESIGN.md](DESIGN.md) for the language contract and roadmap.

When a lesson feels awkward, that is useful feedback for the language design.
The tutorial and its executable examples should change alongside new features,
so teaching the language remains part of building it.
