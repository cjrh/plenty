# `std.cli`

For terminal progress bars and spinners, see [`std.cli.progress`](cli-progress.md).

`std.cli` parses an explicit schema into typed owned values. Its policy is checked
Plenty; it uses the existing byte arguments, strict UTF-8 decoder, integer parser,
and lossless [`Path`](path.md). No reflection, native parser, package download, or
additional link dependency is involved. Supported target: x86_64 Linux GNU.

## Schema and results

Construct `Spec(name: str, kind: Kind, positional: bool, required: bool,
repeatable: bool, help: str)`. All fields are public. `Kind` is `Flag`, `Integer`,
`Text`, or `Path`; there are no implicit defaults or conversions.

Option names include their leading `--`; positional names are display labels.
Names contain ASCII letters, digits, `_`, and interior `-`. Empty/duplicate names,
the reserved `--help`, positional flags, required positionals after optional ones,
and positionals after a repeatable positional are invalid. Only the final
positional can repeat. Options can appear between positionals in the schema.

`validate(schema: &list[Spec]) -> Result[(), CliError]` checks these rules. Both
parsing and help generation validate first, including when input requests help.

`parse(schema: &list[Spec], arguments: &list[list[u8]], start: i64)
-> Result[Parsed, CliError]` borrows both inputs. Pass the unmodified `args_bytes()`
snapshot with `start=1` to skip the invocation name, or fixture tokens with
`start=0`. A start outside `0..=len(arguments)` is `UnexpectedArgument(start)`.
Indices in errors are absolute indices in these inputs, never relative to start.

`Parsed` has public `help_requested: bool` and `entries: list[Entry]` fields.
Each `Entry` has `spec: i64` (the schema index) and `value: Value`. Entries preserve
argument encounter order, including interleaved repeated options. Match `Value`:

| Variant | Meaning |
|---|---|
| `Flag` | The option was present |
| `Integer(i64)` | Checked decimal integer; invalid or overflowing input fails |
| `Text(str)` | Owned, strictly decoded UTF-8, including an empty string |
| `Path(Path)` | Owned exact native bytes, including invalid UTF-8 |

Absent optional specs have no entry; applications supply their own defaults.
Neither inputs nor returned data borrow each other. Raw access through `args()`
and `args_bytes()` remains available.

## Token policy

- Only exact long options are supported: `--count 12`, not `-c`, abbreviations,
  combined short flags, or `--count=12`.
- A non-flag option consumes exactly its next token, even `--`, `--help`, or
  another option's name. End of input is `MissingValue`; a noninteger value for
  an integer option is `InvalidInteger`. This makes negative values unambiguous.
- An unconsumed `--` ends option recognition. Every later token is positional,
  including `--help`. A lone `-`, an empty argument, and tokens beginning with
  a minus followed by a digit are also positional before termination. Other
  dash-prefixed tokens are options; use `--` before a path like `-file`.
- `--help` returns immediately with `help_requested=True` and any preceding
  entries. It skips requiredness checks and later tokens, but does not override
  earlier errors. As an option value or after `--`, it is ordinary data.
- Nonrepeatable duplicate options fail. Required specs need at least one entry.
  Positionals fill schema order; a repeatable final positional takes the rest.

## Help, errors, and exit codes

`help(schema: &list[Spec], program: &str) -> Result[str, CliError]` generates a
usage heading and schema-ordered descriptions, with types and required/repeatable
markers plus `--help`. It borrows its inputs and returns owned text.

`CliError` distinguishes `Schema(schema_index)`, `UnknownOption(argument_index)`,
`UnexpectedArgument(argument_index)`, `MissingValue(schema_index)`,
`Duplicate(schema_index)`, `Missing(schema_index)`, `InvalidInteger(argument_index)`,
`InvalidUtf8(argument_index)`, and `Allocation(AllocError)`.
Paths never go through text decoding. Integer/text values and option names do;
invalid UTF-8 is rejected without replacement decoding.

`describe(error: &CliError) -> Result[str, AllocError]` provides a readable
diagnostic with the relevant index. A UI may match the typed error to also show
`schema[index].name` or an escaped path from `arguments[index]`.
`describe_for(error: &CliError, schema: &list[Spec]) -> Result[str, AllocError]`
uses schema names for schema-index errors, such as `missing required argument
FILE`; it safely falls back to the numeric index when it is outside the schema.
`exit_status(error: &CliError) -> i32` returns 2 for usage errors and 1 for schema
or allocation failures. Neither function writes or exits. Applications return 0
for help/success, report errors to stderr, and return the selected status. See
[`std.error`](error.md) for nonallocating fallback reporting.

## Costs and limits

Schema validation uses quadratic name comparisons and allocates temporary name
bytes. For S schema entries and A arguments, parsing takes O(S² + A·S) comparisons
plus decoding/copying input bytes. It allocates S counters, returned entries, and
owned text/path payloads. Integer/flag payloads need no additional owned storage.
Partial output is released on failure; all these allocation failures are typed.
Help repeatedly concatenates, with worst-case quadratic copying in output size.
`describe` may allocate; the allocation-error message itself is static.

There are no globals or OS handles, and input is only observed. Values follow
ordinary Plenty ownership and worker-send rules; imports receive the same checks
as application code. There are no subcommands, short aliases, float/bool-string
conversion, automatic environment defaults, or process-exit side effects.

The [typed arguments lesson](../../tutorial/08-resources/typed-arguments.md)
shows a reusable data-tool schema and explicit error handling.
The repository's `examples/countbytes.plenty` is a complete byte-counting tool:
compile it with `plenty --compile examples/countbytes.plenty -o countbytes`, then
run `./countbytes --total FILE...`. It reads each file fully and handles parsing,
help, path context, and exit statuses explicitly.

## Acknowledgements

- Schema design draws on Python's [`argparse`](https://docs.python.org/3/library/argparse.html) and the Rust [`clap`](https://crates.io/crates/clap) crate.
