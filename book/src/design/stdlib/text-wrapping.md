# Word wrapping and indentation

The paragraph tools in [`std.text`](text.md) follow Python's
[`textwrap`](https://docs.python.org/3/library/textwrap.html). They operate on
owned UTF-8 strings through borrowed inputs, return independent results, and
report failures through `TextError`. `wrap` finds word boundaries and returns
lines; `hard_wrap` inserts literal line breaks at scalar counts.

## Quick use

```plenty
import std.text

def main() -> Result[(), IoError | std.text.TextError]:
    print(std.text.wrap("Hello there, how are you?", 12)?)?
    print(std.text.fill("Hello there, how are you?", 12)?)?
    print(std.text.shorten("Hello  world!", 11)?)?
    print(std.text.indent(&std.text.dedent("    first\n      second")?, "> ")?)?
    Ok(())
```
```output
["Hello there,", "how are you?"]
Hello there,
how are you?
Hello [...]
> first
>   second
```

| Function | Result and policy |
|---|---|
| `wrap(text, width=70, max_bytes=1048576, max_input_bytes=1048576)` | `list[str]`, without appended line terminators |
| `fill(text, width=70, max_bytes=1048576, max_input_bytes=1048576)` | The same lines joined with LF |
| `shorten(text, width, placeholder=" [...]", max_bytes=1048576, max_input_bytes=1048576, fix_sentence_endings=False, break_long_words=True, break_on_hyphens=True)` | Collapse Unicode whitespace, then keep enough whole chunks to fit one line with the placeholder when truncated |
| `dedent(text, max_bytes=1048576, max_input_bytes=1048576)` | Remove a common literal space/tab prefix from nonblank LF-delimited lines |
| `indent(text, prefix, max_bytes=1048576, max_input_bytes=1048576)` | Prefix each nonblank line, retaining original line endings |
| `indent_with(text, prefix, predicate, max_bytes=1048576, max_input_bytes=1048576)` | Prefix lines selected by a `Callable[[&str], bool]` |

Text and prefix arguments are borrowed `&str`. Boolean options on `shorten`
are independent flags; pass changed flags by name. `shorten` first collapses
runs of Unicode White_Space to single ASCII spaces and trims the ends. Its
wrapping and truncation then follow the same rules as a one-line wrapper;
turning off long-word breaking does not disable the width used for truncation.

`dedent` ignores whitespace-only lines when finding the common margin and
normalizes their bodies to empty strings. Tabs and spaces are different
characters; a tab is not expanded to a number of spaces. It preserves LF
positions and the rest of nonblank lines, including their CR characters.
`indent` uses the [builtin splitlines rules](../facilities/01-builtins/text-splitting.md#lines),
including CRLF and Unicode line separators. A final terminator does not create
an extra line. The predicate sees each full line, including its terminator,
and is invoked once per line, in order. To indent even blank lines:

```plenty
import std.text

def main() -> Result[(), AllocError | IoError | std.text.TextError]:
    result = std.text.indent_with("one\n\ntwo\n", "+ ", def(line: &str) -> bool: True)?
    print(str.repr(result)?)?
    Ok(())
```
```output
"+ one\n+ \n+ two\n"
```

## Reusable configuration

`text_wrapper(width=70, max_bytes=1048576, max_input_bytes=1048576)` returns a
`TextWrapper` without allocating. Its public fields below may be changed
between calls. `wrap(text)` and `fill(text)` borrow both the wrapper and input;
they retain neither results nor input and do not modify configuration.

| Field | Default | Meaning |
|---|---|---|
| `width: Positive[i64]` | `70` | Line width in Unicode scalars, including indentation |
| `initial_indent: str` | `""` | Prefix on the first produced line |
| `subsequent_indent: str` | `""` | Prefix on other lines |
| `expand_tabs: bool` | `True` | Expand tabs to tab stops before wrapping |
| `tabsize: NonNegative[i64]` | `8` | Tab stop interval; zero deletes tabs when expansion is enabled |
| `replace_whitespace: bool` | `True` | Replace ASCII tab/LF/VT/FF/CR with spaces after expansion |
| `fix_sentence_endings: bool` | `False` | Change a single inter-chunk space after an ASCII lowercase sentence ending to two spaces |
| `break_long_words: bool` | `True` | Split chunks that cannot fit on a line |
| `drop_whitespace: bool` | `True` | Discard whitespace chunks at line ends and at the start of subsequent lines |
| `break_on_hyphens: bool` | `True` | Prefer compound-hyphen and double-dash boundaries |
| `max_lines: Option[Positive[i64]]` | `Nothing` | Optional truncation limit, with a placeholder on the last line |
| `placeholder: str` | `" [...]"` | Truncation suffix; leading whitespace is removed when it must stand alone |
| `max_bytes: NonNegative[i64]` | `1048576` | Total UTF-8 output bytes |
| `max_input_bytes: NonNegative[i64]` | `1048576` | Input and expanded intermediate byte limit |

```plenty
import std.text

def main() -> Result[(), IoError | std.text.TextError]:
    mut wrapper = std.text.text_wrapper(16)
    wrapper.initial_indent = "* "
    wrapper.subsequent_indent = "  "
    print(wrapper.fill("one two three four five")?)?
    wrapper.max_lines = Some(1)
    wrapper.placeholder = "..."
    print(wrapper.fill("one two three four five")?)?
    Ok(())
```
```output
* one two three
  four five
* one two...
```

Wrap a single paragraph at a time. With whitespace replacement enabled, LF
becomes a space. With replacement disabled, original control characters may
remain inside a returned line. Leading paragraph whitespace is preserved when
it fits with content, and empty or whitespace-only paragraphs normally produce
no lines. Whitespace within a paragraph is otherwise preserved, rather than
collapsed. Disable `break_long_words` to keep long words intact even when they
exceed `width`. Indentation at least as wide as `width` can also exceed it:
long-word breaking still consumes at least one scalar per iteration.

These operations count scalars, not terminal columns or grapheme clusters;
combining marks may begin a line. Hyphen scanning uses the existing runtime
Unicode `Alphabetic` and `Numeric` properties plus underscore. These properties
follow the Rust toolchain's Unicode version, as in [`std.regex`](regex.md),
and are not a promise of exact CPython Unicode-category equivalence. Ordinary
word splitting recognizes ASCII whitespace only; nonbreaking spaces stay inside
chunks. Blank-line classification, trimming, and `shorten` use Plenty's
Unicode White_Space property, which excludes Python's extra U+001C–U+001F
whitespace classifications. Sentence detection is the intentionally limited
English heuristic of lowercase ASCII letter, `.`, `!`, or `?`, optionally a
closing quote.

## Errors, limits, and cost

All operations return `Result[..., TextError]`. Widths and `max_lines` are
`Positive[i64]`; byte limits and `tabsize` are `NonNegative[i64]`, so a literal
outside those ranges does not compile and a computed value needs
`Positive.try_from(value)?` or `NonNegative.try_from(value)?`. Indentation or a
placeholder that cannot fit the width is `InvalidLimit`. A valid cap exceeded by
input, expansion or output is `LimitExceeded`. Invalid configurations are
checked when an operation is called; constructing or editing a wrapper is
infallible. Indent strings and an active placeholder are each bounded by
`max_input_bytes`, as is the prefix for `indent` and `indent_with`.

`max_bytes` counts the sum of the UTF-8 bytes in `wrap`/`TextWrapper.wrap`
lines. For `fill`, the inserted LF bytes count too. `max_input_bytes` bounds the
original input and tab-expanded intermediate separately, so even `shorten`
rejects an input above that cap instead of allocating an unbounded temporary.
A huge tab size is checked against the cap before expanding. Chunk and line
counts are bounded by the input scalar count; sentence correction can add at
most one extra space per input scalar. The limits bound logical data, rather
than exact resident memory: list slots, geometric capacities, temporary string
copies, and old allocations during growth also take space.

Scanning and copying are O(input + expanded input + output), with storage
proportional to the capped intermediate, output, and per-chunk/per-line metadata,
plus four bytes per expanded input scalar for the splitter's lookaround array.
A split long word retains scalar and UTF-8 byte cursors, avoiding repeated
suffix scans and copies without another scalar array. Unchanged whitespace
preparation shares the immutable input; tab expansion fills each space run in
one bulk operation. Complete chunks are reused during line
assembly, and `dedent` keeps only the common ASCII margin rather than allocating
one list per line. `shorten` uses native whitespace splitting and joining and
releases that word list before wrapping. Small builder appends write bytes
directly into geometrically grown storage; empty appends allocate nothing.
Allocation failure is recoverable as `Allocation(AllocError)` or
`Bytes(BytesError)` with the original byte-buffer cause. Failure discards the
partial result and preserves borrowed inputs and wrapper configuration; a
predicate's own effects are not undone. `fill` materializes the bounded line
list before joining it, so it retains line metadata and an additional output
copy. No global state, native worker, regex engine, or additional dependency is
needed.

## Performance qualification

`examples/textwrap_bench.plenty` in the repository provides seven repeatable
workloads. Optimized Plenty executables were compared
against commit `7d253e5b` on Linux x86-64, AMD Ryzen 7 7840HS, pinned to CPU 0.
Each number below is the median time per operation from three alternating
before/after process pairs, each doing three operations, at workload scale
262144 (approximately 256 KiB of input). Input construction and printing are
outside the timed region; both binaries produced the same byte-count checksums.
Both used 8 MiB input/output limits to cover expanded tabs.

| Workload | Before | After | Speedup |
|---|---:|---:|---:|
| Prose fill | 109.39 ms | 27.71 ms | 3.95× |
| Long Unicode word | 31.67 ms | 7.02 ms | 4.51× |
| Long dash run | 94.71 ms | 15.70 ms | 6.03× |
| Tab-heavy fill | 296.89 ms | 101.99 ms | 2.91× |
| Dedent | 12.56 ms | 3.62 ms | 3.47× |
| Indent | 7.60 ms | 3.73 ms | 2.04× |
| Shorten | 110.22 ms | 18.96 ms | 5.81× |

The 16, 64 and 256 KiB scale probes showed approximately linear growth.
At the largest scale, peak whole-process RSS for dash wrapping fell from
15,684 to 4,072 KiB, and tab wrapping from 27,132 to 14,032 KiB; RSS includes
input construction, allocator retention and all three repetitions.
At scale 65536, allocation-failure budget probes required 28,912 successful
checked allocations for prose fill instead of 236,498, and 6,725 for shorten
instead of 309,813. These counts measure Plenty's injected allocation checks,
not operating-system allocation calls. The remaining costs include eager
chunk/line storage and encoding larger pieces for the builder. These are local
measurements; elapsed time and allocator retention depend on the platform.

## Acknowledgements

- [CPython `textwrap`](https://github.com/python/cpython/blob/b3714a5fb7a/Lib/textwrap.py)
  provides the algorithms and API model, adapted to bounded Plenty storage and
  a scanner instead of regex splitting. Copyright (C) 1999–2001 Gregory P. Ward;
  Copyright (C) 2002 Python Software Foundation; PSF-2.0. The complete license is
  bundled in release third-party notices.
