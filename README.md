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

Define a function with `: name { signature } ["docstring"] body... ;`. The signature lists inputs as `name Type` pairs, then `->`, then output types; `{ x i64 -> i64 }` reads as "takes one `i64` named `x`, leaves one `i64`". Inside the body, those input names refer to the values passed in — so the body can mention `x` instead of juggling the stack. A docstring is optional but, when present, must immediately follow the header. If a function starts by pushing text, write an empty docstring first: `"" "text"`. Call a function by prefixing its name with a colon.

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

### Comments, compact delimiters, and small helpers

`#` starts a comment through the next newline. `{`, `}`, `[`, `]`, and `;` do not need surrounding spaces. A function may omit its docstring. Inside a function, unknown bare words are errors, so write text as `"..."`.

```forth
# A compact definition with no docstring.
: id{x i64 -> i64}x;
42 :id
```

```
[42i64]
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

Numbers in source default to `i64`. Add a suffix for a direct width: `200u8`, `-1i8`, or `42i32`. A suffixed literal must fit its type. Use an explicit cast — `:as-i8`, `:as-u8`, and so on — when you intentionally truncate or reinterpret a value. Arithmetic is same-width, so `i32 + i64` is a type error.

```forth
200u8 50u8 +
```

```
[250u8]
```

### Small stack operations

`drop` discards the top value, `dup` copies it, and `swap` exchanges the top two values. They work on every type. Use them for small local adjustments; named function inputs stay clearer for larger work.

```forth
1 true swap dup drop
```

```
[true 1i64]
```

### More comparisons and Boolean values

`!=`, `<=`, and `>=` complete the comparison set. `and` and `or` combine already-evaluated `Bool` values; use `match` when one path must not run.

```forth
1 2 !=  2 2 <=  true false or
```

```
[true true true]
```

<!-- END TUTORIAL -->

### Output words

- `.` prints the whole stack and leaves it unchanged. Use it to inspect state.
- `:print` pops one value and renders it without a newline. For example,
  `42 :print` writes `42i64`.
- `:println` pops one `Str` and writes its raw bytes with a newline.

## Example: an AOT-compiled stdin filter

The `:readline`, `:contains`, and `:println` words are the small I/O surface
Plenty exposes. Together with `drop` and tail recursion (DESIGN.md §11.8), a
complete stream-filter program fits in a handful of definitions. The program
below reads newline-delimited strings from stdin and writes back only the lines
containing the letter `m`.

```forth
: handle-line { line Str -> }
    "Print `line` to stdout if it contains the substring \"m\",
     otherwise drop it. The decision happens here so the recursion
     in :filter does not need to dup the line value."
    line "m" :contains match
        true  [ line :println ]
        false [ ]
    end ;

: filter { -> }
    "Read newline-delimited strings from stdin until EOF, printing
     only those that contain the letter m. Iteration is recursion plus
     mandatory TCO (DESIGN.md §11.8); the :readline match dispatches
     on the got-a-line? bool and the recursive call sits at the tail
     of the true arm."
    :readline match
        true  [ :handle-line :filter ]
        false [ drop ]
    end ;

:filter
```

Compile it to a native executable:

```sh
cargo run -- --compile examples/filter_m.plenty -o filter_m
```

Run it against a stream of lines:

```sh
printf 'apple\nbanana\nmango\ncherry\nmelon\nplum\norange\n' | ./filter_m
```

Output:

```
mango
melon
plum
```

## Keeping the tutorial honest

The tutorial section above is generated from `tests/tutorial.rs`, where every
example is also a test. `cargo test` runs each example, checks the stack it
produces, and fails if this README is out of date. `UPDATE_README=1 cargo test`
regenerates the section. The examples therefore cannot drift from the
interpreter.
