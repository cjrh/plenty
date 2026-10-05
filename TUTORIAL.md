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

Use `--compile` when you want to keep the executable and run it repeatedly
without compiling again. The executable does not need Plenty installed.

Every Plenty example in this guide is a separate, complete program. You can
copy any example into a file without first running the earlier examples.

## 2. Name values

Use `=` to create a binding. Plenty infers the type of a local value, so you
usually do not need an annotation:

```plenty
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
score = 10
score = 11
```

The diagnostic includes:

```error
`score` is immutable; declare it with `mut`
```

Use `mut` when a value needs to change. Write it once, at the declaration:

```plenty
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
The other built-in value types currently available are `bool` and `str`.
Floating-point types such as `f32` and `f64` are not implemented yet.

A whole-number literal without a suffix, such as `42`, has type `i64`.
Append a built-in integer type to choose another width:

```plenty
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
small = 7u8
print(small + 1)
```

```error
expected u8, got i64
```

Choose a matching literal or explicitly convert a value:

```plenty
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
print(1 + 2 * 3)
print((1 + 2) * 3)
print(-7 // 3)
```

```output
7
9
-3
```

Division by zero is a runtime error. `/` is reserved for future floating-point
division and is currently rejected.

## 4. Define functions with clear interfaces

Every parameter and return type must be declared:

```plenty
def double(value: i64) -> i64:
    """Return twice the supplied value."""
    value * 2

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

print(maximum(7, 12))
```

```output
12
```

Both continuing branches need the same type. Use `elif` for additional cases.
For a short choice, Python's conditional expression is also supported:

```plenty
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

print(add_one(41u32))
```

```output
42
```

An alias is another name for the same type. `Count`, `ItemCount`, and `u32`
are interchangeable here; an alias does not create a distinct type, enforce
units of measurement, or add runtime overhead.

Aliases can name integers, booleans, strings, collections, unit, or other aliases.
Integer aliases support casts; collection aliases support collection constructors. Declare aliases at module scope; they are
visible throughout that file, including before their declaration. Alias chains
must eventually reach a concrete type; cycles and unknown targets are errors.
An alias cannot redefine a built-in name, another alias, or a function name.

## 9. Keep branch-local names local

Assignments to an existing mutable binding survive a branch. New names declared
inside a branch belong to that branch:

```plenty
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

A final expression at module scope is evaluated but not displayed. Use
`print(value)` to see its value. Parse/type errors run no code. Runtime errors
can occur after earlier effects, such as printing, have already happened.
The run command preserves the program's input, output, and working directory,
and reports a failing exit status when the program fails.

There is no interactive REPL or interpreter, and JIT compilation is out of scope.

## 13. Lists, moves, and explicit copies

A list contains values of one type. Use `list[T]` in signatures and annotations.
An empty list needs an annotation or a typed constructor such as `list[i64]()`.

```plenty
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

Collections can nest. To update an inner list while keeping the outer collection,
use `mut child = copy(rows[0])`, update `child`, then assign it back with
`rows[0] = child`. This last assignment transfers the child into the collection. Direct nested
assignment such as `rows[0][0] = 1` is not implemented.

## 14. Dictionaries and sets

Dictionaries map a single key type to a single value type. Keys and set elements
may be integers, booleans, or strings. Dictionary values can include collections.

```plenty
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

Missing dictionary keys are runtime errors. Check membership before indexing
when absence is possible; a built-in `Option`-returning lookup is not yet provided.
Collection equality compares contents; dictionary and set order do not matter.

## 15. Iterate over values

`for` visits list elements, dictionary keys, set elements, range integers, or
characters of a string. A character is a one-character `str`.

```plenty
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
`Nothing` belongs to one concrete option type; it is not a universal null.

```plenty
type Found = Option[i64]

def first_positive(values: list[i64]) -> Found:
    for value in values:
        if value > 0:
            return Found.Some(value)
    Found.Nothing

for values in [[-1, 0], [-1, 42]]:
    match first_positive(values):
        case Found.Some(value):
            print(value)
        case Found.Nothing:
            print("not found")
```

```output
not found
42
```

Use `Result[T, E]` when the absent result has an explanation. Its variants
are `Ok(T)` and `Err(E)`. A type alias makes repeated qualification shorter:

```plenty
type Division = Result[i64, str]

def divide(left: i64, right: i64) -> Division:
    if right == 0:
        return Division.Err("division by zero")
    Division.Ok(left // right)

for result in [divide(8, 2), divide(8, 0)]:
    match result:
        case Division.Ok(value):
            print(value)
        case Division.Err(message):
            print(message)
```

```output
4
division by zero
```

Both type arguments are explicit; there is no implicit unwrapping or exception.
Unit payloads such as `Result[(), str]` are not implemented yet. For an operation
without a success value, declare an enum with a nullary success variant and a
payload-bearing failure variant.

## 20. Produce values lazily with generators

A generator function declares `Generator[T]` and uses `yield` statements:

```plenty
def countdown(start: i64) -> Generator[i64]:
    print("starting")
    mut remaining = start
    while remaining > 0:
        yield remaining
        remaining = remaining - 1

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

first = once()
second = first
list(first)
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
mut numbers = [1, 2, 3]
drop(numbers)
numbers = [4]
print(numbers)

def pending() -> Generator[i64]:
    print("started")
    yield 1

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

## Where the language goes next

This guide deliberately uses implemented features. Unit payloads, recursive types,
element references, and stored or returned references remain future work. Traits and generics are deferred; async/await is out of
scope. See [DESIGN.md](DESIGN.md) for the language contract and roadmap.

When a lesson feels awkward, that is useful feedback for the language design.
The tutorial and its executable examples should change alongside new features,
so teaching the language remains part of building it.
