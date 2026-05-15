# plenty
Stack-based Programming language

## Tutorial

Plenty is a stack language: a program is a stream of whitespace-separated
words, and each word either pushes a value onto the stack or operates on the
values already there. Start the REPL with `cargo run`.

Each example below shows a program followed by the stack it leaves behind —
which is what the `.` word prints.

<!-- BEGIN TUTORIAL: generated from tests/tutorial.rs - do not edit by hand, run `UPDATE_README=1 cargo test` -->

### The stack and numbers

A program is a stream of whitespace-separated words. A number is a word that pushes itself onto the stack.

```forth
1 2 3
```

```
[1 2 3]
```

### Arithmetic

`+`, `-`, `*` and `/` each pop the top two values and push the result. They read in stack order, so `10 2 -` means `10 - 2`.

```forth
10 2 -
```

```
[8]
```

### Operators consume only what they need

An operator touches just the top two values; everything below it on the stack is left alone.

```forth
1 2 3 4 +
```

```
[1 2 7]
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

Define a function with `: name body... ;`, and call it by prefixing its name with a colon. The body is compiled once, when the function is defined.

```forth
: double 2 * ;
5 :double
```

```
[10]
```

### Functions calling functions

A function body may call other functions. Defining a function never disturbs the stack.

```forth
: double 2 * ;
: quad :double :double ;
3 :quad
```

```
[12]
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
