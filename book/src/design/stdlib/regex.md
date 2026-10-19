# std.regex

`import std.regex` compiles regular expressions and searches `str` text and
[`std.bytes`](bytes.md) buffers with them. A search runs in time linear in
the input for a compiled pattern: there is no backtracking. Every resource a
pattern or search can consume has an explicit limit. Errors are one type,
`RegexError`.

The parser, compiler and matcher (a Pike VM) are Plenty code in the module.
The runtime supplies only primitives: Unicode properties and single-scalar
case mappings from the Rust toolchain's Unicode tables, and reads of borrowed
UTF-8 by byte offset. They do not allocate, except the one that copies a byte
range into a new `str`, which can fail with an allocation error.

`Alphabetic`, `Lowercase`, `Uppercase`, `White_Space`, `Numeric`, `Control`,
and case mappings use the Unicode version in the Rust compiler that builds
Plenty (`char::UNICODE_VERSION`). Their results may change when that toolchain
is upgraded. `Mark` uses the independently pinned Unicode 17.0.0 tables in
[`std.unicode`](unicode.md), as does that module's normalization. The two
data versions need not match; ASCII classes and byte-offset rules are unchanged
by Unicode table upgrades.

## Functions and types

| Item | Result | Contract |
|---|---|---|
| `limits()` | `Limits` | The default limits, listed below |
| `compile(pattern: &str, bounds: &Limits)` | `Result[Regex, RegexError]` | The compiled pattern, or its first error |

`Limits(max_pattern_bytes, max_nesting, max_repeat, max_instructions,
max_memory, max_work)` has public fields in that order:

| Field | Default | Bounds | Exceeded as |
|---|---|---|---|
| `max_pattern_bytes` | 65536 | Pattern length in bytes | `Limit.PatternBytes` |
| `max_nesting` | 128 | Depth of nested groups; at most 1000 | `Limit.Nesting` |
| `max_repeat` | 1000 | Each count in `{n}`, `{n,}`, `{n,m}` | `Limit.Repetition` |
| `max_instructions` | 100000 | Program instructions plus class items, after repetitions are expanded | `Limit.Instructions` |
| `max_memory` | 67108864 | Matcher scratch bytes for one search | `Limit.Memory` |
| `max_work` | 100000000 | Matcher steps in one call | `Limit.Work` |

A negative field, or `max_nesting` above 1000, is `InvalidLimits`. Matcher
memory is `8 × (7n + 2ns + 2s + 8)` bytes for `n` instructions and `s`
capture slots (two per group, counting the whole match); `compile` checks it
for the largest `s` the regex can use, so no later search fails on memory.

`RegexError` is:

| Variant | Meaning |
|---|---|
| `Syntax(SyntaxError)` | Invalid pattern or replacement template; `SyntaxError(offset, message)` gives the byte offset into it |
| `LimitExceeded(Limit)` | A limit was exceeded; also `Limit.Output` and `Limit.Matches` for the per-call limits below |
| `InvalidLimits` | A `Limits` field is out of range |
| `OutOfRange` | A start offset outside the input or inside a UTF-8 sequence, a `Match` outside its haystack, or a `Cursor` from another regex |
| `Allocation(AllocError)` | Allocation failed |

## Regex

All searches are leftmost-first, as in Perl and the Rust `regex` crate: of
the matches that start earliest, the one the pattern prefers wins
(`a|ab` matches `a` in `ab`; greedy repetitions prefer more).

| Method | Result | Contract |
|---|---|---|
| `group_count()` | `i64` | Groups, counting group 0 (the whole match) |
| `group_index(name: &str)` | `Option[i64]` | The number of a named group |
| `instructions()` | `i64` | Program size, for choosing `max_work` |
| `is_match(text: &str)` | `Result[bool, RegexError]` | Whether any match exists; stops at the first |
| `find(text: &str)` | `Result[Option[Match], RegexError]` | The first match |
| `find_at(text: &str, start: i64)` | `Result[Option[Match], RegexError]` | The first match at or after `start` |
| `captures(text: &str)` | `Result[Option[Captures], RegexError]` | The groups of the first match |
| `captures_at(text: &str, start: i64)` | `Result[Option[Captures], RegexError]` | As `captures`, from `start` |
| `cursor()` | `Result[Cursor, RegexError]` | Iteration state for `next_match` |
| `next_match(text: &str, cursor: &mut Cursor)` | `Result[Option[Match], RegexError]` | The next match, or `Nothing` after the last |
| `find_all(text: &str, max_matches: i64)` | `Result[list[Match], RegexError]` | Every match in order |
| `split(text: &str, max_pieces: i64)` | `Result[list[str], RegexError]` | The text between matches |
| `replace(text: &str, template: &str, max_bytes: i64)` | `Result[str, RegexError]` | `text` with the first match replaced |
| `replace_all(text: &str, template: &str, max_bytes: i64)` | `Result[str, RegexError]` | `text` with every match replaced |

`find_at` and `captures_at` start at a scalar boundary; assertions such as
`^` and `\b` still see the text before `start`. Iteration (`next_match`,
`find_all`, `split`, `replace_all`) follows the Rust `regex` crate: after an
empty match, the next search starts one scalar later, and an empty match
where the previous match ended is skipped. So `a*` on `baaa` matches at
0–0 and 1–4, and `split` of `"añ"` by the empty pattern gives `"", "a", "ñ", ""`.

Per-call limits: `find_all` fails with `Limit.Matches` rather than return more
than `max_matches` matches, and `split` rather than return more than
`max_pieces` pieces. `replace` and `replace_all` fail with `Limit.Output` when
the result would exceed `max_bytes` bytes.

A replacement template inserts group `n` for `$n` or `${n}`, a named group
for `${name}`, and `$` for `$$`; a group that did not take part inserts
nothing. Any other `$`, including `$name`, an unknown group, or an unclosed
`${`, is a `Syntax` error at the `$`. The template is checked before any
search.

### Bytes

`is_match_bytes(data)`, `find_bytes(data, start)`,
`captures_bytes(data, start)`, `next_match_bytes(data, cursor)`, and
`find_all_bytes(data, max_matches)` search a `Bytes` buffer. Valid UTF-8
sequences are scalars as in text; each byte of an invalid sequence (including
an encoded surrogate) is one position that nothing matches, not even
`\P{Any}` or `[^a]`. Offsets are byte offsets into `data`, and `start` may be
any offset from 0 to `data.len()`; a start inside a sequence makes its
remaining bytes invalid positions.

### Match, Captures and Cursor

`Match` has public fields `start` and `end` (byte offsets, end exclusive) and
methods `len()`, `is_empty()`, `text(haystack: &str)` and
`bytes(data: &Bytes)`; the last two copy the matched part and are
`OutOfRange` for a haystack it does not fit.

`Captures` has `len()` (equal to `group_count()`), `get(index: i64)` and
`name(name: &str)`, each `Option[Match]`. Group 0 is the whole match; a group
outside the pattern or not taking part is `Nothing`. A group inside a
repetition holds its last iteration.

A `Cursor` holds the matcher scratch and position, so a loop over
`next_match` allocates only once. It works only with the regex that made it.

## Pattern syntax

| Syntax | Meaning |
|---|---|
| `x`, `\x` for punctuation `x` | A literal scalar |
| `\n \t \r \f \v`, `\xHH`, `\x{H...}` | Control characters and a scalar by hexadecimal value |
| `.` | Any scalar except `\n`; any scalar under `s` |
| `[abc]`, `[a-z]`, `[^...]` | A class, range, or negated class; `-` is literal first or last, `]` must be escaped |
| `\d \D` | ASCII digit `[0-9]`, or not |
| `\w \W` | ASCII word scalar `[0-9A-Za-z_]`, or not |
| `\s \S` | ASCII space `[\t\n\v\f\r ]`, or not |
| `\p{Name} \P{Name}` | Has the Unicode property, or not; also inside classes |
| `xy`, `x\|y` | Sequence; alternatives, the first preferred |
| `x* x+ x?` | 0 or more, 1 or more, 0 or 1; greedy |
| `x{n} x{n,} x{n,m}` | Counted repetition |
| `x*? x+? x?? x{n,m}?` | Lazy: prefer fewer |
| `(x)`, `(?P<name>x)`, `(?<name>x)` | Capturing group; names match `[A-Za-z_][A-Za-z0-9_]*` and are unique |
| `(?:x)`, `(?flags:x)` | Non-capturing group, optionally with flags |
| `(?flags)` | Set flags until the end of the enclosing group |
| `^ $` | Start and end of text; of a line under `m` |
| `\A \z` | Start and end of text |
| `\b \B` | ASCII word boundary, or not |

Properties are `Alphabetic`, `Lowercase`, `Uppercase`, `White_Space`,
`Numeric`, `Control`, `Mark` (general category M), `ASCII`, and `Any`. Flags
are `i` (case-insensitive), `m` (multi-line `^ $`), and `s` (`.` matches
`\n`); `(?i-m)` sets `i` and clears `m`.

Case-insensitive matching uses simple, one-scalar case mappings: a scalar
matches another when their lowercase or uppercase mappings agree, so `(?i)k`
matches the Kelvin sign and `(?i)σ` matches `Σ` and `ς`, but `(?i)ss` does
not match `ß`.

These are errors rather than silently different behavior: look-around
(`(?=`, `(?!`, `(?<=`, `(?<!`), backreferences (`\1`), nested classes and
POSIX classes (`[[:alpha:]]`), an empty class (`[]`), a repetition of a
repetition (`a**`; write `(?:a*)*`), a range with a class end (`[\d-z]`), the
`x` flag, and unknown escapes, properties, or flags. A pattern with an
unmatched `)` or `(` is an error too.

## Costs

A search visits each instruction at most once per input position, so its
work is at most `2 × instructions() × (len + 1)` steps. `max_work` caps the
steps of each call: `find`, `captures`, and `is_match` run one search;
`find_all`, `split`, and `replace_all` count every search they make; each
`next_match` call is counted separately. Iteration can rescan text after a
match, so choose `max_work` for the whole call.

A pattern that starts with a literal prefix (outside `i` mode) skips ahead
to each occurrence of the prefix in text. Each search allocates its scratch,
sized by the memory formula above; a `Cursor` reuses one allocation.

The matcher runs as compiled Plenty code: about 1 µs per input byte for a
pattern of a dozen instructions. That is much slower than the Rust `regex`
crate, which has a lazy DFA and literal optimizations this module lacks.

## Choices

The module uses its own matcher rather than the Rust `regex` crate because
that crate allocates infallibly and would abort on allocation failure, and
its Unicode tables would enlarge every program that imports `std.regex`. The
syntax is the subset of the Rust crate's syntax with the same meaning, except
that Perl classes and `\b` are ASCII-only, which the tests check against
the crate.
