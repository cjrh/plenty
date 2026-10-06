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
def main() -> ():
    print("Hello, Plenty!")
    print(6 * 7)
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

Execution starts by calling `main` once. Every binary application must declare
`def main() -> ()` or `def main() -> i32`, with no parameters. The `()` form
finishes successfully with exit status zero. The `i32` form returns a process
exit status: zero means success, and nonzero means failure. Use an `i32` literal
such as `0i32` or `1i32`; an unsuffixed integer is an `i64`.

Keep executable statements inside functions. Module scope contains `def`,
`class`, `enum`, and `type` declarations, plus imports. Bindings inside `main`
are local to it, so other functions receive values through typed parameters.
The modules chapter below explains imports and `pub` visibility.

Like other functions, `main` may return early, use a final expression, or call
functions declared later in the file. Its owned locals are dropped before the
program exits, including when it returns a nonzero status. Operating systems
limit the range of observable exit statuses; use small nonnegative codes for
portable command-line programs.

Here is a complete program that explicitly returns success:

```plenty
def main() -> i32:
    print("ready")
    0i32
```

```output
ready
```

Calling a function at module scope is an error, even if `main` is also declared:

```plenty-error
def main() -> ():
    print("ready")

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
def main() -> ():
    # Comments begin with a hash.
    price = 20
    quantity = 3
    total = price * quantity
    print(total)
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
def main() -> ():
    mut score: i64 = 10
    score = score + 5
    print(score)
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

A whole-number literal without a suffix, such as `42`, has type `i64`.
Append a built-in integer type to choose another width:

```plenty
def main() -> ():
    small: u8 = 200u8
    offset: i32 = -12i32
    large: u64 = 1_000_000u64
    print(small)
    print(offset)
    print(large)
```

```output
200
-12
1000000
```

An annotation checks a type; it does not convert the initializer. For example,
`small: u8 = 200` fails because `200` is an `i64`. Use `200u8` or `u8(200)`.

Arithmetic requires matching types:

```plenty-error
def main() -> ():
    small = 7u8
    print(small + 1)
```

```error
expected u8, got i64
```

Choose a matching literal or explicitly convert a value:

```plenty
def main() -> ():
    small = 7u8
    print(small + 1u8)
    print(i64(small) + 1)
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
def main() -> ():
    print(1 + 2 * 3)
    print((1 + 2) * 3)
    print(-7 // 3)
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
def main() -> ():
    distance: f64 = 7.5
    time: f64 = 2.0
    print(distance / time)
    print(1.25f32 + 0.5f32)
    print(2.5e-2)
    print(f64(3) / 2.0)
    print(i32(-2.75))
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
    small: f32 = 1.5
```

```error
expected f32, got f64
```

Write `1.5f32` or `f32(1.5)` instead. Floats follow IEEE arithmetic: division by
zero can produce infinity or NaN, and arithmetic overflow can produce infinity.
NaN is unequal to everything, including itself; ordered comparisons with it are
false. The same comparison rules apply inside collections and enum payloads.

```plenty
def main() -> ():
    zero = 0.0
    print(1.0 / zero)
    unknown = zero / zero
    print(unknown == unknown)
    print(unknown != unknown)
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

def main() -> ():
    print(double(21))
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

def main() -> ():
    print(maximum(7, 12))
```

```output
12
```

Both continuing branches need the same type. Use `elif` for additional cases.
For a short choice, Python's conditional expression is also supported:

```plenty
def main() -> ():
    age = 20
    category = "adult" if age >= 18 else "child"
    print(category)
```

```output
adult
```

`and` and `or` short-circuit: the right side is evaluated only when needed.
`not` negates a boolean. All three require boolean operands:

```plenty
def main() -> ():
    divisor = 0
    safe = divisor != 0 and 10 // divisor > 1
    print(safe)
    print(not safe)
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

def main() -> ():
    print(clamp_low(3, 10))
    print(clamp_low(12, 10))
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

The unit type, written `()`, describes a function that completes without
producing data. It is not a missing-value marker:

```plenty
def greet(name: str) -> ():
    print("Hello, " + name)

def greet_if(enabled: bool, name: str) -> ():
    if not enabled:
        return
    greet(name)

def main() -> ():
    greet_if(False, "Ada")
    greet_if(True, "Ada")
```

```output
Hello, Ada
```

A bare `return` returns unit. `print` also returns unit. Use `pass` for an
intentionally empty block. Unit can be a return type, but unit parameters and
stored unit bindings are not supported yet.

There is no `None` value and no implicit nullable type. Lesson 19 introduces
`Option` for absence and `Result` for recoverable errors.

## 8. Give types names of your own

There is no built-in `int`. If you want that name, choose its meaning explicitly:

```plenty
type int = i32

def increment(value: int) -> int:
    value + int(1)

def main() -> ():
    answer: int = increment(int(41))
    print(answer)
```

```output
42
```

`type int = i32` declares a type alias. `int(41)` is exactly the integer cast
`i32(41)`. Another program can choose `type int = i64` instead. Aliases do not
change the type of unsuffixed literals: `41` remains an `i64` in either program.
Use `41i32` or `int(41)` when the alias means `i32`. Aliases are not literal
suffixes, so `41int` is not valid syntax.

Names that describe your data can make interfaces easier to read:

```plenty
type Count = u32
type ItemCount = Count

def add_one(count: ItemCount) -> ItemCount:
    count + 1u32

def main() -> ():
    print(add_one(41u32))
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
def main() -> ():
    mut price = 100
    discounted = True
    if discounted:
        discount = 20
        price = price - discount
    print(price)
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
def main() -> ():
    message = 'Hello, ' + "Plenty!"
    print(message)
    print(contains(message, "Plenty"))
```

```output
Hello, Plenty!
True
```

`print` accepts one value and adds a newline. It prints text without quotes and
integers without width suffixes. `contains(text, part)` tests for a substring.
There is just one string type, `str`. Strings are immutable values; `mut` permits
replacing a binding rather than editing its bytes. `len` counts Unicode scalar
values, and indexing returns a one-scalar string. Storage uses explicit lengths,
so an embedded NUL does not end a string. Interpolation and general conversion
to strings are not available yet.

```plenty
def main() -> ():
    text = "é\0😀"
    print(len(text))
    print(text[-1])
    print("\0" in text)
    print([text])
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

def main() -> ():
    print(sum_to(100, 0))
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
def main() -> ():
    mut original: list[i64] = [10, 20]
    mut changed = copy(original)
    changed.append(30)
    changed[0] = 99
    print(original)
    print(changed)
    print(changed[-1])
    print(len(changed))
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

For recoverable allocation failure, `try_copy(original)` returns a `Result`
instead. Lesson 19 shows how to use it with `?`.

Collections can nest. To update an inner list while keeping the outer collection,
use `mut child = copy(rows[0])`, update `child`, then assign it back with
`rows[0] = child`. This last assignment transfers the child into the collection. Direct nested
assignment such as `rows[0][0] = 1` is not implemented.

## 14. Dictionaries and sets

Dictionaries map a single key type to a single value type. Keys and set elements
may be integers, booleans, or strings. Dictionary values can include collections.

```plenty
def main() -> ():
    mut scores: dict[str, i64] = {"Ada": 10, "Grace": 20}
    scores["Ada"] = 12
    scores["Lin"] = 30
    print(scores["Ada"])
    print("Grace" in scores)
    print(scores.keys())
    print(scores.values())

    mut names: set[str] = set()
    names.add("Ada")
    names.add("Ada")
    print(len(names))
    print("Ada" in names)
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
def main() -> ():
    scores = {"Ada": 12, "Grace": 0}
    match scores.get("Ada"):
        case Some(score):
            print(score)
        case Nothing:
            print("unknown player")
    print(scores.get("Grace"))
    print(scores.get("Lin"))
    print(scores["Ada"])
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
does not allocate, so it needs no `try_` prefix or allocation-error result. String
and immutable enum results retain their existing storage and remain valid even
if the dictionary is subsequently updated or dropped.

`get` supports values such as numbers, booleans, strings, and immutable enums.
It rejects mutable collections, classes, and enums containing them. Use `pop`
to remove those values and take ownership; borrowing individual elements remains
future work. No mutable value is silently copied by `get`.

### Remove an entry and take its value

`pop(key)` removes a dictionary entry and returns `Some(value)`, or `Nothing`
when the key is absent. It requires a mutable dictionary and transfers ownership
of the stored value, so it works with lists and classes too:

```plenty
def main() -> ():
    mut groups = {"ready": [1, 2], "waiting": [3]}
    match groups.pop("ready"):
        case Some(items):
            mut work = items
            work.append(4)
            print(work)
        case Nothing:
            print("no work")
    print(groups)
    print(groups.pop("missing"))
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

### Remove list elements

Lists also have `pop`. With no argument it removes the last element; an `i64`
index selects another position. Negative indices count from the end:

```plenty
def main() -> ():
    mut tasks = [[1], [2], [3]]
    match tasks.pop(1):
        case Some(task):
            mut work = task
            work.append(4)
            print(work)
        case Nothing:
            print("no task")
    print(tasks)
    print(tasks.pop())
    print(tasks.pop(-1))
    print(tasks.pop())
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
def main() -> ():
    mut names = {"Ada", "Bea"}
    name = "Ada"
    print(names.discard(name))
    print(names.discard(name))
    print(name)
    print("Bea" in names)
    print(len(names))
    names.add(name)
    print(len(names))
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
def main() -> ():
    mut total = 0
    for n in range(1, 5):
        total = total + n
    print(total)

    scores = {"Ada": 10, "Grace": 20}
    for name in &scores:
        print(name)
        print(scores[name])

    print(list(range(5, 0, -2)))
    print(list("hé"))
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
without building a list; `list(range(...))` materializes its values.

Currently, range arguments and yielded elements are `i64`. A `list[u8]`
annotation does not change a range's element type. Numeric suffixes such as
`2u8` and `3.5f32` work for literals, but `range[u8](8)` is not implemented yet.

The iterable is evaluated once. Iterating an owned collection transfers it into
the loop; use `for item in &values` to preserve the owner. Borrowed iteration
prevents conflicting mutation, and currently supports only copyable elements
(scalars, strings, and enums without mutable payloads). To iterate a snapshot while
mutating the original, request it explicitly with `for item in copy(values)`.
Generators are consumed by iteration. Loop variables and new body bindings do not
escape the loop; changes to enclosing `mut` bindings persist. A loop has unit
result. `return` can exit a containing function from a loop. Lesson 17 covers
`break` and `continue`. Tuple unpacking and `items()` are not implemented yet.

## 16. Build collections with comprehensions

A comprehension produces a new collection from an iterable and optional filters.
The element expression runs only after the filters pass.

```plenty
def main() -> ():
    squares = [n * n for n in range(8) if n % 2 == 0]
    print(squares)
    print({n: n * n for n in [2, 3]})
    print(len({n // 2 for n in range(8)}))
    print([x * 10 + y for x in range(3) for y in range(x)])
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
    values = [1, True]
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
def main() -> ():
    mut remaining = 3
    while remaining > 0:
        print(remaining)
        remaining = remaining - 1
    print("go")
```

```output
3
2
1
go
```

`continue` skips the rest of the current iteration. `break` exits the loop:

```plenty
def main() -> ():
    mut n = 0
    while True:
        n = n + 1
        if n % 2 == 0:
            continue
        if n > 5:
            break
        print(n)
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
def main() -> ():
    for n in range(6):
        if n == 1:
            continue
        if n == 4:
            break
        print(n)
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
        print("unreachable")
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

def main() -> ():
    print(describe(Reading.Missing))
    print(describe(Reading.Value(42)))
    print(describe(Reading.Invalid("sensor offline")))
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
    match Switch.On:
        case Switch.On:
            print("on")
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

def main() -> ():
    for values in [[-1, 0], [-1, 42]]:
        match first_positive(values):
            case Some(value):
                print(value)
            case Nothing:
                print("not found")
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

def main() -> ():
    for result in [divide(8, 2), divide(8, 0)]:
        match result:
            case Ok(value):
                print(value)
            case Err(message):
                print(message)
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
def main() -> ():
    found = Some(42)
    missing: Option[i64] = Nothing
    success: Result[i64, str] = Ok(42)
    failure: Result[i64, str] = Err("not ready")
    print(found == Some(42))
    print(missing == Option[i64].Nothing)
    match failure:
        case Ok(value):
            print(value)
        case Err(message):
            print(message)
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

def main() -> ():
    for result in [validate("Plenty"), validate("")]:
        match result:
            case Ok(_):
                print("valid")
            case Err(message):
                print(message)
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

def greeting(name: str) -> Result[str, str]:
    validate(name)?
    Ok("Hello, " + name)

def main() -> ():
    print(greeting("Plenty"))
    print(greeting(""))
```

```output
Result[str, str].Ok("Hello, Plenty")
Result[str, str].Err("name is empty")
```

Here `validate(name)?` has a unit success value, so it can stand alone. On
failure, the string concatenation never runs. Live local values and previously
evaluated expression temporaries are cleaned up automatically, just as for
an explicit `return`.

`?` works with `Option` too: it extracts `Some` or immediately returns `Nothing`.

```plenty
def positive(n: i64) -> Option[i64]:
    Some(n) if n > 0 else Nothing

def doubled(n: i64) -> Option[i64]:
    Some(positive(n)? * 2)

def main() -> ():
    print(doubled(21))
    print(doubled(-1))
```

```output
Option[i64].Some(42)
Option[i64].Nothing
```

The enclosing function must return the same family: `Result` for a `Result`
operand or `Option` for an `Option` operand. Success payload types can differ,
but `Result` error types must match exactly. Convert errors explicitly with
`match` when needed. `?` is not supported inside generators.

```plenty-error
def read_number() -> Result[i64, str]:
    Err("not a number")

def checked() -> Result[i64, i64]:
    Ok(read_number()?)

def main() -> ():
    print(checked())
```

```error
`?` requires identical Result error types
```

`Option` and `Result` wrappers do not allocate on the heap, including when
nested. Their payloads keep their usual behavior: `Some([1, 2])` allocates the
list, but adds no wrapper allocation. Returning or propagating an existing sum
does not allocate a wrapper either. Printing and operations on the payload can
still allocate. The collection methods below are the first operations that let
you recover from allocation failure.

### Handle collection allocation failures

Start with `list[T].try_new()` when you need to handle failure while creating an
empty list. It returns `Result[list[T], AllocError]`. Sets and dictionaries work
the same way: `set[T].try_new()` and `dict[K, V].try_new()`.

If you know how many entries you need, use `try_with_capacity(n)`. The collection
starts empty and has space for at least `n` entries, including any hash table.
Here construction and insertion both propagate errors to the caller:

```plenty
def answers() -> Result[list[i64], AllocError]:
    mut values = list[i64].try_with_capacity(2)?
    values.try_append(21)?
    values.try_append(42)?
    Ok(values)

def main() -> ():
    match answers():
        case Ok(values):
            print(values)
        case Err(error):
            match error:
                case AllocError.OutOfMemory:
                    print("not enough memory")
                case AllocError.CapacityOverflow:
                    print("requested capacity is too large")
```

```output
[21, 42]
```

The `?` unwraps the newly owned collection on success. On failure, construction
reclaims any memory it already obtained and returns the error. The error itself
needs no allocation. Type aliases work too: after `type Numbers = list[i64]`,
you can write `Numbers.try_new()`.

Use `try_append` for a list, `try_add` for a set, and `try_insert` for a dictionary
when you need to handle allocation failure. Each returns `Result[(), AllocError]`.
`try_reserve(n)` reserves space for at least `n` additional entries; it is available
on all three collection types and returns the same result type.

```plenty
def add_answers(values: &mut list[i64]) -> Result[(), AllocError]:
    values.try_reserve(2)?
    values.try_append(21)?
    values.try_append(42)?
    Ok(())

def main() -> ():
    mut values: list[i64] = []
    match add_answers(&mut values):
        case Ok(done):
            print(values)
        case Err(error):
            match error:
                case AllocError.OutOfMemory:
                    print("not enough memory")
                case AllocError.CapacityOverflow:
                    print("requested capacity is too large")
```

```output
[21, 42]
```

`AllocError` is always available. Its two variants need no payload or allocation.
`OutOfMemory` means the allocator rejected a request. `CapacityOverflow` means
the requested size cannot be represented; a negative reservation is also a
capacity error. You can handle a failure without terminating the program:

```plenty
def main() -> ():
    mut values = [1, 2]
    match values.try_reserve(-1):
        case Ok(done):
            print("reserved")
        case Err(error):
            print(error)
    print(values)
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
`try_insert(key, value)` replaces an existing value or adds a new entry. Adding an
existing set element or replacing a dictionary entry needs no storage growth.

This is the first stage of recoverable allocation. Existing `append`, `add`,
indexed assignment, allocating literals, ordinary constructors, comprehensions,
string `+` and indexing, and `copy` still terminate on allocation failure. For example, `[]` in
the `add_answers` example can fail before the function runs; the `answers`
example uses a fallible constructor to handle that stage too. Printing complex values
and user destructors may also allocate. The new methods provide recovery for
their own storage operations, not yet for every allocation in a program.

### Copy without losing the source on failure

`try_copy(value)` follows the same duplication rules as `copy`, but returns
`Result[T, AllocError]`. It observes the source, so your original value remains
usable whether copying succeeds or fails. Use `?` to keep the success path short:

```plenty
def extended(source: &list[i64]) -> Result[list[i64], AllocError]:
    mut result = try_copy(source)?
    result.try_append(30)?
    Ok(result)

def main() -> ():
    original = [10, 20]
    print(extended(&original))
    print(original)
```

```output
Result[list[i64], AllocError].Ok([10, 20, 30])
[10, 20]
```

This also works for nested collections, ordinary classes, and enum payloads.
If copying a later field or element fails, the partial copy is cleaned up and
the original remains unchanged. Scalars and immutable values, including strings,
need no allocation to copy; their existing immutable storage can be shared.

`try_copy` has the same restrictions as `copy`: generators and classes with
custom cleanup cannot be copied, including when nested in another value.
The recoverable operation covers the duplication itself. For example,
`try_copy([1, 2])` still constructs its list argument using the ordinary literal's
failure policy before the copy begins. Use the fallible constructors when that
initial allocation also needs to be handled.

### Build text with recoverable allocation

Use `text.try_concat(other)` to combine two strings. To combine a list of strings,
use `separator.try_join(parts)`. Both return `Result[str, AllocError]`:

```plenty
def greeting(names: &list[str]) -> Result[str, AllocError]:
    joined = ", ".try_join(names)?
    "Hello, ".try_concat(joined)?.try_concat("!")

def main() -> ():
    names = ["Ada", "Bea"]
    print(greeting(&names))
    print(names)
    print("-".try_join([]))
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
or another kind of iterable is not supported yet. Creating the input list or
strings, ordinary text indexing, and printing keep their existing allocation behavior.
Ordinary string `+` also retains its terminal failure policy.

### Split text into fields

Use `text.try_split(separator)` to split on an explicit, nonempty string. It
returns `Result[list[str], AllocError]`, so `?` can propagate allocation failure:

```plenty
def fields(line: &str) -> Result[list[str], AllocError]:
    line.try_split("::")

def main() -> ():
    line = "Ada::Bea::::"
    match fields(&line):
        case Ok(parts):
            print(parts)
            print(" / ".try_join(parts))
        case Err(error):
            print(error)
    print(line)
    print("".try_split(","))
```

```output
["Ada", "Bea", "", ""]
Result[str, AllocError].Ok("Ada / Bea /  / ")
Ada::Bea::::
Result[list[str], AllocError].Ok([""])
```

Adjacent and trailing separators preserve empty fields. Matches do not overlap:
`"aaaaa".try_split("aa")` succeeds with `["", "", "a"]`. A separator that does not
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

Use `text.try_get(index)` when the index might be out of range or character
allocation might fail. It returns `Result[Option[str], AllocError]`: `Result`
reports allocation failure, while `Option` tells you whether the index exists.

```plenty
def show_character(text: &str, index: i64) -> Result[(), AllocError]:
    character = text.try_get(index)?
    match character:
        case Some(value):
            print(value)
        case Nothing:
            print("missing")
    Ok(())

def main() -> ():
    text = "café🙂"
    print(show_character(&text, -1))
    print(show_character(&text, 3))
    print(show_character(&text, 5))
    print(text)
```

```output
🙂
Result[(), AllocError].Ok(())
é
Result[(), AllocError].Ok(())
missing
Result[(), AllocError].Ok(())
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
missing index or allocation failure. Both forms scan UTF-8 to reach the requested
scalar; repeated indexing is not a constant-time way to traverse text.

## 20. Produce values lazily with generators

A generator function declares `Generator[T]` and uses `yield` statements:

```plenty
def countdown(start: i64) -> Generator[i64]:
    print("starting")
    mut remaining = start
    while remaining > 0:
        yield remaining
        remaining = remaining - 1

def main() -> ():
    numbers = countdown(3)
    print("created")
    for number in numbers:
        print(number)
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
    for n in range(limit):
        yield n

def main() -> ():
    print([n * n for n in numbers(6) if n % 2 == 1])
    print(list(numbers(3)))
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

def main() -> ():
    mut messages = once()
    print(next(messages))
    print(next(messages))
    print(next(messages))
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

def main() -> ():
    first = once()
    second = first
    print(list(second))
```

```output
[42]
```

After a move, the old binding cannot be used:

```plenty-error
def once() -> Generator[i64]:
    yield 42

def main() -> ():
    first = once()
    second = first
    list(first)
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

def add(values: &mut list[i64], value: i64) -> ():
    values.append(value)

def main() -> ():
    mut numbers = [1, 2]
    print(total(&numbers))
    add(&mut numbers, 3)
    print(numbers)
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
def main() -> ():
    mut score = 10
    reference = &mut score
    *reference = *reference + 5
    print(*reference)
    score = 20
    print(score)
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
    mut numbers = [1, 2]
    view = &numbers
    numbers.append(3)
    print(view)
```

```error
conflicting borrow
```

A reference can be reborrowed temporarily. An exclusive reference may lend shared
or exclusive access, but its conflicting access is suspended while that child
borrow is live. Reference arguments automatically reborrow an existing reference;
they do not transfer the referenced owner.

This subset borrows named bindings and their class fields. Reference bindings must
be initialized directly with `&name` or `&mut name` (including field paths) and
cannot be reassigned. References cannot
be stored in collections, returned from functions, captured by generators, or
remain live across `yield`. Element references such as `&items[0]` are not yet
supported. Use `next` through an exclusive generator reference when borrowing
a generator; generator iteration still consumes its owner.

## 23. Cleanup and early drop

Owned values clean up automatically when their scope exits, including through
`return`, `break`, and `continue`. Moving a value transfers that responsibility.
A borrow never becomes responsible for destroying the original value.

Use `drop(value)` to release an owner earlier. The consumed binding becomes
unavailable; a mutable binding may then receive another value:

```plenty
def pending() -> Generator[i64]:
    print("started")
    yield 1

def main() -> ():
    mut numbers = [1, 2, 3]
    drop(numbers)
    numbers = [4]
    print(numbers)

    task = pending()
    drop(task)
    print("done")
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

def main() -> ():
    mut point = Point(3, 4)
    print(point.squared_length())
    point.shift(1)
    mut changed = copy(point)
    changed.x = 20
    print(point)
    print(changed)
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

def main() -> ():
    span = Span(10, 5)
    print(span)
    print(span.length())
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

def main() -> ():
    mut basket = Basket(0, [])
    count = &mut basket.count
    *count = 2
    basket.items.append("apple")
    basket.items.append("pear")
    print(basket)
```

```output
Basket(count=2, items=["apple", "pear"])
```

The borrow ends after `count`'s last use. For now, a field loan protects the whole
record: while `&basket.count` is live, changing `basket.items` also conflicts.
Collection element references such as `&basket.items[0]` are still deferred.
You can replace a class-valued field by assignment, but cannot replace a whole
class through `*reference = new_instance` yet.

Use `__del__` for cleanup that belongs to an owned instance. It runs automatically
when the instance leaves scope, is replaced, or is explicitly dropped. Its bare
`self` is an exclusive reference, like in `__init__`:

```plenty
class Resource:
    name: str

    def __del__(self) -> ():
        print("release " + self.name)

class Pair:
    first: Resource
    second: Resource

    def __del__(self) -> ():
        print("release pair")

def work() -> ():
    pair = Pair(Resource("first"), Resource("second"))
    spare = Resource("spare")
    print("working")

def main() -> ():
    work()
    print("done")
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

pub def make_point() -> Point:
    Point(3, 4)
```

Save the application beside it, for example as `main.plenty`:

```plenty
import geometry
from geometry import Point as Position

def main() -> ():
    point: Position = geometry.make_point()
    print(point.squared_length())
    print(point.x)
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
        print("counter closed")
```

Application:

```plenty
from counter import Counter

def main() -> ():
    mut count = Counter(41)
    count.increment()
    print(count.read())
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
    item = Secret(42)
    print(item.value)
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

## Where the language goes next

This guide deliberately uses implemented features. Input/file APIs,
recursive types, element references, and stored or returned references remain future work. Traits and generics are deferred; async/await is out of
scope. See [DESIGN.md](DESIGN.md) for the language contract and roadmap.

When a lesson feels awkward, that is useful feedback for the language design.
The tutorial and its executable examples should change alongside new features,
so teaching the language remains part of building it.
