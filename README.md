# plenty
Stack-based Programming language

## Tutorial

Plenty is a stack language: a program is a stream of whitespace-separated
words, and each word either pushes a value onto the stack or operates on the
values already there. Start the REPL with `cargo run`, or run a file with
`cargo run -- path/to/script.plenty`.

Each example below shows a program followed by the stack it leaves behind —
which is what the `.` word prints.

<!-- BEGIN TUTORIAL: generated from tests/tutorial.rs - do not edit by hand, run `UPDATE_README=1 cargo test` -->

### The stack and numbers

A program is a stream of whitespace-separated words. A number is a word that pushes itself onto the stack.

```forth
1 2 3
```

```
[1i64 2i64 3i64]
```

### Arithmetic

`+`, `-`, `*` and `/` each pop the top two values and push the result. They read in stack order, so `10 2 -` means `10 - 2`.

```forth
10 2 -
```

```
[8i64]
```

### Operators consume only what they need

An operator touches just the top two values; everything below it on the stack is left alone.

```forth
1 2 3 4 +
```

```
[1i64 2i64 7i64]
```

### Clearing the stack

`:clear` discards every value on the stack.

```forth
1 2 3 :clear
```

```
[]
```

### Text

A bare word that is not a number or an operator is text. `+` joins two pieces of text instead of adding them.

```forth
hello world +
```

```
["helloworld"]
```

### Quoted strings

Wrap text in double quotes to push it as a single string. Spaces, operators, and other special characters inside the quotes are taken verbatim.

```forth
"hello world" " and goodbye" +
```

```
["hello world and goodbye"]
```

### Functions

Define a function with `: name { signature } "docstring" body... ;`. The signature lists inputs as `name Type` pairs, then `->`, then output types; `{ x i64 -> i64 }` reads as "takes one `i64` named `x`, leaves one `i64`". Inside the body, those input names refer to the values passed in — so the body can mention `x` instead of juggling the stack. The docstring describes what the function does. Both the signature and the docstring are mandatory — together they form the function's interface. Call the function by prefixing its name with a colon.

```forth
: double { x i64 -> i64 } "Double an integer." x 2 * ;
5 :double
```

```
[10i64]
```

### Functions calling functions

A function body may call other functions. Defining a function never disturbs the stack.

```forth
: double { x i64 -> i64 } "Double an integer." x 2 * ;
: quad { x i64 -> i64 } "Multiply by four." x :double :double ;
3 :quad
```

```
[12i64]
```

### Named inputs replace stack juggling

Each input named in the signature is in scope for the whole body — write the name to load it. A function with several inputs can refer to each by name, in any order, as many times as it likes, without `dup`, `swap`, or `rot`.

```forth
: hypot-sq { a i64 b i64 -> i64 } "Square the hypotenuse: a*a + b*b." a a * b b * + ;
3 4 :hypot-sq
```

```
[25i64]
```

### Booleans and comparisons

`true` and `false` are the `Bool` literals. The comparison operators `=`, `<`, and `>` pop two values and push a `Bool`; `not` negates one. `=` accepts any two values of the same type (`i64`, `Str`, or `Bool`); `<` and `>` are integers only. A `Bool` is *not* an integer: there is no "zero is false" convention. The only way to get a `Bool` is to produce one.

```forth
1 2 <  3 3 =  true not
```

```
[true true false]
```

### Branching with `match`

`match` is the only branching primitive. It pops the top-of-stack value and runs the bracketed body of the first arm whose pattern matches; `end` closes the construct. Every match must be exhaustive — for a `Bool`, that means both `true` and `false` arms (or a wildcard). There is no `if` and no `else`: `match` covers both jobs without privileging `Bool` over any other finite type.

```forth
: describe { flag Bool -> Str } "Render a Bool as text."
  flag match
    true  [ "yes" ]
    false [ "no"  ]
  end ;
true :describe  false :describe
```

```
["yes" "no"]
```

### Wildcards for the open cases

`i64` and `Str` have unbounded value spaces, so a match on either must include a wildcard arm — `_` — that catches everything not named above. Patterns are tested in order, so specific arms first and `_` last. The arm body sees the surrounding stack and the surrounding function's locals; the brackets are syntactic structure, not a separate sub-stack.

```forth
: name-it { n i64 -> Str }
  "Name a small integer; anything else is 'many'."
  n match
    0 [ "zero" ]
    1 [ "one"  ]
    2 [ "two"  ]
    _ [ "many" ]
  end ;
1 :name-it  7 :name-it
```

```
["one" "many"]
```

### Iteration is recursion

Plenty has no `for` or `while`. A function that needs to repeat calls itself, and the compiler detects when that recursive call sits in *tail* position — the last thing the function would do before returning — and reuses the current call's frame instead of stacking a new one. A million tail calls cost the same call-stack space as one. The pattern is always the same: thread the running total through an accumulator argument so the recursive call ends the body.

```forth
: sum-to { n i64 acc i64 -> i64 }
  "Tail-recursive accumulator: 1 + 2 + ... + n + acc."
  n 0 = match
    true  [ acc ]
    false [ n 1 - acc n + :sum-to ]
  end ;
100 0 :sum-to
```

```
[5050i64]
```

### Picking a width

Numbers in source default to `i64`. To use a smaller — or unsigned — width, write the explicit cast: `:as-i8`, `:as-u8`, `:as-i32`, and so on. Casts pop one integer and push it at the target width, with Rust's `as` semantics (sign-extend, zero-extend, truncate, reinterpret). Arithmetic is same-width — `i32 + i64` is a type error, by design — so the cast is where the width change is made visible. The width travels with the value: `[200u8]` is a `u8` whatever else is on the stack.

```forth
200 :as-u8 50 :as-u8 +
```

```
[250u8]
```

<!-- END TUTORIAL -->

### Other words

`:listdir` prints the entries of the current directory.

## Keeping the tutorial honest

The tutorial section above is generated from `tests/tutorial.rs`, where every
example is also a test. `cargo test` runs each example, checks the stack it
produces, and fails if this README is out of date. `UPDATE_README=1 cargo test`
regenerates the section. The examples therefore cannot drift from the
interpreter.
