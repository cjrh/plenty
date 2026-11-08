# std.regex

`import std.regex` compiles regular expressions and searches `str` text and
[`std.bytes`](bytes.md) buffers with them. A search runs in time linear in
the input for a compiled pattern: there is no backtracking. Every resource a
pattern or search can consume has an explicit limit. Pattern and search errors
use `RegexError`; arithmetic on freely constructed `Match` offsets has the
separate contract described below.

Patterns, haystacks, replacement templates and group names accept
[`&strview`](../language/04-expressions/slices.md#substring-views), including
borrowed substring ranges. Byte offsets are relative to the supplied view.
Searching inspects its borrowed input directly; `compile` explicitly makes a
fallible owning copy of the pattern it retains, so the compiled regex does not
keep a loan of the caller's string. Match extraction and replacements still
return independent owned results.

The parser, compiler and matching engines are Plenty code in the module:
a Pike VM, bounded ASCII prefix filters, a lazy DFA and a one-pass matcher.
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

```plenty
import std.regex
from std.regex import RegexError

def main() -> Result[(), AllocError | IoError | RegexError]:
    date = std.regex.compile("(?P<year>\\d{4})-(?P<month>\\d{2})-(?P<day>\\d{2})", &std.regex.limits())?
    text = "Released 2024-03-15, patched 2024-04-02."
    print(date.is_match(text)?)?
    match date.captures(text)?:
        case Some(groups):
            match groups.name("year"):
                case Some(year):
                    print("year", year.text(text)?)?
                case Nothing:
                    print("no year")?
        case Nothing:
            print("no date")?
    print(len(date.find_all(text, 10)?))?
    print(date.replace_all(text, "${day}/${month}/${year}", 1000)?)?
    Ok(())
```
```output
True
year 2024
2
Released 15/03/2024, patched 02/04/2024.
```

## Functions and types

| Item | Result | Contract |
|---|---|---|
| `limits()` | `Limits` | The default limits, listed below |
| `compile(pattern: &strview, bounds: &Limits)` | `Result[Regex, RegexError]` | The compiled pattern, or its first error |

`Limits(max_pattern_bytes, max_nesting, max_repeat, max_instructions,
max_memory, max_work)` has public `NonNegative[i64]` fields in that order. A
literal converts where a field is expected; a computed value needs
`NonNegative.try_from(value)?`.

| Field | Default | Bounds | Exceeded as |
|---|---|---|---|
| `max_pattern_bytes` | 65536 | Pattern length in bytes | `Limit.PatternBytes` |
| `max_nesting` | 128 | Depth of nested groups; at most 1000 | `Limit.Nesting` |
| `max_repeat` | 1000 | Each count in `{n}`, `{n,}`, `{n,m}` | `Limit.Repetition` |
| `max_instructions` | 100000 | Program instructions plus class items, after repetitions are expanded | `Limit.Instructions` |
| `max_memory` | 67108864 | Matcher scratch bytes for one search | `Limit.Memory` |
| `max_work` | 100000000 | Matcher steps in one call | `Limit.Work` |

`compile` traps when `max_nesting` is above 1000. Matcher
base scratch memory is `8 × (7n + 2ns + 2s + 8)` bytes for `n` instructions and `s`
capture slots (two per group, counting the whole match); `compile` checks it
for the largest `s` the regex can use, so no later search fails on memory.
An eligible search may also allocate a 66,048-byte DFA cache, only when it
fits in the remaining `max_memory` budget. Smaller budgets select the VM
without rejecting an otherwise valid pattern. The VM scratch remains
available for fallback; all allocations remain fallible.
The estimate is checked before comparison: an unrepresentable word estimate
also exceeds every representable byte budget and reports `Limit.Memory`.
Actual unrepresentable allocation extents report
`Allocation(AllocError.CapacityOverflow)` before any matcher array is allocated.
Work is checked against its remaining budget before increment, including when
`max_work` is `i64.MAX`; exhaustion remains `Limit.Work`, not an arithmetic
error. Repetition-count overflow likewise remains `Limit.Repetition`.


`RegexError` is:

| Variant | Meaning |
|---|---|
| `Syntax(SyntaxError)` | Invalid pattern or replacement template; `SyntaxError(offset, message)` gives the byte offset into it |
| `LimitExceeded(Limit)` | A limit was exceeded; also `Limit.Output` and `Limit.Matches` for the per-call limits below |
| `OutOfRange` | A start offset outside the input or inside a UTF-8 sequence, a `Match` outside its haystack, or a `Cursor` from another regex |
| `Allocation(AllocError)` | Allocation failed |

## Regex

All searches are leftmost-first, as in Perl and the Rust `regex` crate: of
the matches that start earliest, the one the pattern prefers wins
(`a|ab` matches `a` in `ab`; greedy repetitions prefer more).

| Method | Result | Contract |
|---|---|---|
| `group_count()` | `i64` | Groups, counting group 0 (the whole match) |
| `group_index(name: &strview)` | `Option[i64]` | The number of a named group |
| `instructions()` | `i64` | Program size, for choosing `max_work` |
| `is_match(text: &strview)` | `Result[bool, RegexError]` | Whether any match exists; stops at the first |
| `find(text: &strview)` | `Result[Option[Match], RegexError]` | The first match |
| `find_at(text: &strview, start: i64)` | `Result[Option[Match], RegexError]` | The first match at or after `start` |
| `captures(text: &strview)` | `Result[Option[Captures], RegexError]` | The groups of the first match |
| `captures_at(text: &strview, start: i64)` | `Result[Option[Captures], RegexError]` | As `captures`, from `start` |
| `cursor()` | `Result[Cursor, RegexError]` | Iteration state for `next_match` |
| `next_match(text: &strview, cursor: &mut Cursor)` | `Result[Option[Match], RegexError]` | The next match, or `Nothing` after the last |
| `find_all(text: &strview, max_matches: i64)` | `Result[list[Match], RegexError]` | Every match in order |
| `split(text: &strview, max_pieces: i64)` | `Result[list[str], RegexError]` | The text between matches |
| `replace(text: &strview, template: &strview, max_bytes: i64)` | `Result[str, RegexError]` | `text` with the first match replaced |
| `replace_all(text: &strview, template: &strview, max_bytes: i64)` | `Result[str, RegexError]` | `text` with every match replaced |

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
methods `len() -> Result[i64, OverflowError[i64]]`, `is_empty()`,
`text(haystack: &strview)` and `bytes(data: &[u8])`. `len()` checks `end - start`:
public fields may contain arbitrary offsets, so representable negative lengths
are retained and unrepresentable differences return an arithmetic error.
The text and byte methods still copy the matched part and report `OutOfRange`
for a haystack it does not fit. Matches returned by a successful search have
ordered, nonnegative offsets and a representable length.

`Captures` has `len()` (equal to `group_count()`), `get(index: i64)` and
`name(name: &strview)`, each `Option[Match]`. Group 0 is the whole match; a group
outside the pattern or not taking part is `Nothing`. A group inside a
repetition holds its last iteration.

Capture indexing validates the group number before multiplying it by two;
out-of-range indices, including `i64.MAX`, return `Nothing`. Internal opt-outs
are bounded by validated scalar widths, pattern/input spans, configured nesting,
successful instruction emission and preflighted matcher array extents. Unlimited
replacement does not maintain a redundant total-replacements counter.

A `Cursor` holds the matcher scratch and position, so a loop over
`next_match` allocates scratch only when it is created. Use it with the regex
that made it; a different pattern, memory budget or work budget returns
`OutOfRange` before consulting cached transitions.

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

Search is linear in the input for a fixed compiled pattern. The Pike VM
visits each instruction at most once per input position in each of its two
sets. Accelerators can add a bounded scan before transferring to the VM;
there is no unbounded backtracking. `max_work` caps matcher steps in each
call: `find`, `captures`, and `is_match` run one search; `find_all`, `split`,
and `replace_all` count every search they make; each `next_match` call is
counted separately. A native literal batch reserves the sum of its maximum
search spans before starting, charges each needle's actual search span, and
retains that charge across fallback. Even a match near the beginning can charge
an entire window; insufficient budget returns `Limit.Work` before the batch.
Short tails use the scalar filter. Work includes accelerated byte scans and uncached DFA
transitions, and is retained across fallback. Exact step counts depend on
the engine and its cache. Iteration can rescan text after a match, so choose
`max_work` for the whole call.

The compiler selects these accelerators without changing match semantics:

- A case-sensitive literal prefix uses substring search in text. For programs
  of at most 256 instructions, a bit-parallel filter can instead recognize up
  to eight mandatory ASCII positions across alternatives and positive range
  classes. It admits possible matches for the matching engine to confirm; nullable,
  case-insensitive or non-ASCII paths shorten or disable this filter.
  Up to eight paths can occupy separate eight-bit lanes, preventing false
  candidates made by mixing alternatives. Extraction visits at most 2,048
  instructions and keeps at most 17 pending paths; exceeding those bounds
  retains the position-wise union.
- When the extracted paths expand to at most eight ASCII literals of the same
  three-to-eight-byte width, a 64-byte scalar probe returns nearby matches
  promptly. Longer scans then use native substring search in windows
  of at most 1,024 bytes. Windows overlap by `width - 1`, and all literals are
  searched before selecting the earliest candidate. The engine still confirms
  candidates and resolves alternative priority. Literal storage is bounded by
  eight short strings; it is compiled metadata, not per-search scratch.
  Wider classes or excessive path products retain the scalar filter.
- For ASCII-only programs of at most 62 instructions without assertions, a
  lazy DFA answers existence searches. When every match has the same byte
  length, it also supplies match offsets for searches, cursors and replacements.
  At its initial state it uses the same mandatory-prefix filter to skip
  impossible starts. Its 64-state cache is local to the matcher and reused by a cursor. Cache
  exhaustion disables the DFA for that matcher and resumes the VM.
- For small programs without assertions, a one-pass table accelerates searches
  that need only whole-match offsets when each ASCII byte has at most one
  preferred consuming path. It retains greedy/lazy ordering. Non-ASCII input
  transfers to the VM. Failed anchored attempts can consume at most one input
  length before fallback, preventing quadratic rescanning. The table has at
  most 62 × 129 entries and is built only with sufficient memory headroom.

ASCII accelerators borrow the text's UTF-8 bytes once per search and read the
view directly, without allocating or calling the runtime for each byte.
Scalar decoding and assertions retain their Unicode boundary semantics.

Captures requiring subgroup positions use the Pike VM, with prefix filtering
where applicable. Unsupported patterns continue to use the VM. Each search
allocates scratch; a `Cursor` reuses it. No new public API or opt-in flag is
needed. The [regex-redux measurements](../../development/benchmarks-game.md)
exercise cleaning, counting and replacement with the same patterns as the
Rust `regex` reference; remaining gaps include Unicode DFA transitions and
the remaining scalar scan lowering (issue #760).

## Choices

The module uses its own matcher rather than the Rust `regex` crate because
that crate allocates infallibly and would abort on allocation failure, and
its Unicode tables would enlarge every program that imports `std.regex`.
The narrower `memchr::memmem::find` primitive uses constant space and no
allocation. Its SIMD byte search and portable fallbacks justify native code;
Plenty's current SIMD API has no byte-vector search operations. Literal
extraction, batching, work limits and all matching policy remain Plenty. The
syntax is the subset of the Rust crate's syntax with the same meaning, except
that Perl classes and `\b` are ASCII-only, which the tests check against
the crate.

## Acknowledgements

- Syntax, match semantics and the prefilter/fallback strategy follow the Rust [`regex`](https://docs.rs/regex/1.12.3/regex/) crate 1.12.3.
- Matching uses Rob Pike's virtual machine, as described by Russ Cox in ["Regular Expression Matching: the Virtual Machine Approach"](https://swtch.com/~rsc/regexp/regexp2.html) (2009).

- Native byte substring search uses [`memchr`](https://docs.rs/memchr/2.8.0/) 2.8.0 (MIT OR Unlicense), including its SIMD implementations and portable fallbacks.
