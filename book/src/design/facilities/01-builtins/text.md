# Text

`str` is immutable valid UTF-8 representing Unicode scalar values. Embedded
U+0000 is ordinary content (`\0` is a literal escape). Equality and hashing
compare all bytes without normalization; strings have no public terminator.
Ordering (`<`, `<=`, `>`, `>=`) is lexicographic by Unicode scalar value,
matching the order used by `list[str].sort()`. Prefixes precede longer strings;
comparison observes both inputs and allocates nothing.
`len(text)` counts scalars, including combining marks individually, rather than
bytes or grapheme clusters. `text.byte_len() -> i64` is the length of the UTF-8
encoding in bytes: `len("héllo")` is `5` and `"héllo".byte_len()` is `6`. It
observes the receiver, takes constant time, and allocates nothing; use it to
enforce a size limit before copying or encoding text. `str` has no `len`
method, so the two counts cannot be confused at a call site.

`s[a:b]?` copies the scalars from `a` to `b` into a new `str`;
`&s[a:b]` borrows them in place as a `&strview`, which loops, compares, and
searches without allocating. Text methods, mixed concatenation and membership,
`ord`, and numeric `T.parse` accept views directly; methods returning owned text
keep their fallible result types. Both count bounds in scalars; see
[substring views](../../language/04-expressions/slices.md#substring-views).

`text.as_bytes() -> &[u8]` borrows the exact UTF-8 encoding without copying,
retaining, allocating, or counting scalars. It works on `str`, string references,
and substring views. A byte view's length and positions count bytes; iterating
it yields `&u8` items, read as values with `*byte`. The string's storage remains
borrowed until the view's last use. Bind a temporary string to a name before
saving its byte view; an immediate call may borrow a temporary's bytes for the
full expression. Use `std.bytes.from_text(text)?` for an independent owned byte
copy, including when `text` is a substring view.

String literals require no allocation. Results of at most seven UTF-8 bytes,
including every single character and the empty string, also require no heap
allocation. Longer constructed results use fallible storage. This threshold does
not change the public type or the result type of an operation.

For ownership, assignment costs, mutation, and views across languages, see
[Plenty Text Compared to Python, Rust, and C](text-compared.md).

## Passing, assigning, and copying text

`str` is a copyable **owning value**. In the local operations below, copying
it keeps the source usable and never duplicates a large text buffer: heap
strings retain shared immutable storage, tiny strings copy their inline bytes,
and literals need no retain.
A `&str` borrows a whole string; a `&strview` borrows a run within it. Neither
borrow independently keeps the string alive. `&substr` remains a compatible
alias for `&strview`. A plain `&s` still has type `&str`; a view annotation or
view parameter can convert it to `&strview`.

The tables assume `s: str`, `r: &str` initialized as `r = &s`, scalar bounds
`a` and `b`, and a new, unannotated binding `x`. **R** abbreviates
`Result[str, AllocError]`. Costs include evaluating the shown expression;
compiler optimizations may remove unnecessary handle copies or retains.

### Function arguments

These are ordinary typed function parameters. Some built-ins and methods
explicitly observe either values or references; their individual signatures
and contracts describe that behavior.

The code cells are schematic excerpts: `...` stands for the function body,
and `s: str` stands for an existing string binding. Calls belong inside a
function; bounds `a` and `b` are assumed to be available.

<div class="table-wrapper">
<table>
<thead>
<tr><th>Call and parameter</th><th>Accepted?</th><th>Ownership and storage</th></tr>
</thead>
<tbody>
<tr>
<td><pre><code class="language-plenty">def f(val: str) -&gt; ():
    ...
&#10;s: str
f(s)</code></pre></td>
<td>Yes</td>
<td>The parameter gets an owning string value; heap storage is shared without allocation. <code>s</code> stays usable.</td>
</tr>
<tr>
<td><pre><code class="language-plenty">def f(val: str) -&gt; ():
    ...
&#10;s: str
f(&amp;s)</code></pre></td>
<td>No</td>
<td>A reference is not implicitly copied into an owning parameter. Write <code>f(s)</code> instead.</td>
</tr>
<tr>
<td><pre><code class="language-plenty">def f(val: str) -&gt; ():
    ...
&#10;s: str
r = &amp;s
f(r)</code></pre></td>
<td>No</td>
<td>Naming the reference does not turn it into an owning value. Write <code>f(*r)</code> to copy the string handle.</td>
</tr>
<tr>
<td><pre><code class="language-plenty">def f(val: str) -&gt; ():
    ...
&#10;s: str
r = &amp;s
f(*r)</code></pre></td>
<td>Yes</td>
<td>Dereferencing obtains an owning copy without duplicating the text buffer. Both <code>s</code> and <code>r</code> stay usable.</td>
</tr>
<tr>
<td><pre><code class="language-plenty">def f(val: &amp;str) -&gt; ():
    ...
&#10;s: str
f(s)</code></pre></td>
<td>Yes</td>
<td>Implicitly borrows <code>s</code> for the call; no owning copy, retain, or allocation.</td>
</tr>
<tr>
<td><pre><code class="language-plenty">def f(val: &amp;str) -&gt; ():
    ...
&#10;s: str
f(&amp;s)</code></pre></td>
<td>Yes</td>
<td>Explicitly borrows the whole string; no owning copy, retain, or allocation.</td>
</tr>
<tr>
<td><pre><code class="language-plenty">def f(val: &amp;str) -&gt; ():
    ...
&#10;s: str
r = &amp;s
f(r)</code></pre></td>
<td>Yes</td>
<td>Reborrows the existing reference; no owning copy, retain, or allocation.</td>
</tr>
<tr>
<td><pre><code class="language-plenty">def f(val: &amp;str) -&gt; ():
    ...
&#10;s: str
f(&amp;s[a:b])</code></pre></td>
<td>No</td>
<td>A substring view is <code>&amp;strview</code>, not <code>&amp;str</code>. Use a <code>&amp;strview</code> parameter, or create an owned slice with <code>s[a:b]?</code> and lend it to the call.</td>
</tr>
<tr>
<td><pre><code class="language-plenty">def f(val: &amp;strview) -&gt; ():
    ...
&#10;s: str
f(s)</code></pre></td>
<td>Yes</td>
<td>Implicitly borrows the whole string as a view for the call; no text copy, retain, or allocation. <code>s</code> stays usable.</td>
</tr>
<tr>
<td><pre><code class="language-plenty">def f(val: &amp;strview) -&gt; ():
    ...
&#10;s: str
f(&amp;s)</code></pre></td>
<td>Yes</td>
<td>Converts the whole string borrow to a view; no text copy, retain, or allocation.</td>
</tr>
<tr>
<td><pre><code class="language-plenty">def f(val: &amp;strview) -&gt; ():
    ...
&#10;s: str
r = &amp;s
f(r)</code></pre></td>
<td>Yes</td>
<td>Converts the existing <code>&amp;str</code> to a view of the whole string; no text copy, retain, or allocation.</td>
</tr>
<tr>
<td><pre><code class="language-plenty">def f(val: &amp;strview) -&gt; ():
    ...
&#10;f("hello")</code></pre></td>
<td>Yes</td>
<td>Borrows the whole literal as a view; no text copy, retain, or allocation.</td>
</tr>
<tr>
<td><pre><code class="language-plenty">def f(val: &amp;strview) -&gt; ():
    ...
&#10;s: str
f(&amp;s[a:b])</code></pre></td>
<td>Yes</td>
<td>Borrows just the selected text; no text copy, retain, or allocation. Finding scalar bounds may scan the text.</td>
</tr>
<tr>
<td><pre><code class="language-plenty">def f(val: &amp;strview) -&gt; ():
    ...
&#10;s: str
v = &amp;s[a:b]
f(v)</code></pre></td>
<td>Yes</td>
<td>Reborrows the existing substring view; no text copy, retain, or allocation.</td>
</tr>
<tr>
<td><pre><code class="language-plenty">def f(val: &amp;strview) -&gt; ():
    ...
&#10;def make_text() -&gt; str:
    ...
&#10;f(make_text())</code></pre></td>
<td>Yes</td>
<td>Implicitly borrows the returned string as a whole view; <code>f(&amp;make_text())</code> also works. The temporary owner lives through the full expression. View conversion adds no text copy, retain, or allocation; producing the string has its own costs.</td>
</tr>
</tbody>
</table>
</div>

A `&strview` parameter is the most general borrowed text interface among these
three types: it accepts whole strings and substring views. Use it when its
supported operations are sufficient. It currently has fewer operations than
`&str`: for example, it supports `find`, but not `rfind`, `strip`, or `upper`.
See [substring views](../../language/04-expressions/slices.md#substring-views)
for the complete operation list. A view does not provide an owning string
handle; use `val[:]?` when an owned copy is needed.

An owning `str` parameter can be returned or stored independently of the
caller's binding. A `&str` parameter lends access; obtaining an owned string
from it requires a value copy such as `*value`. See
[borrowing](../../language/05-ownership/02-borrowing.md) and
[returned references](../../language/05-ownership/04-returned-references.md).

**Efficiency note:** For a heap-backed string, an owning copy can require an
atomic reference-count increment and a later release. Borrowing as `&str` or
`&strview` avoids that ownership bookkeeping. Tiny inline strings and literals need no
reference counting, and compiler optimizations can eliminate unnecessary
retains and releases. Prefer `&strview` when its supported operations suffice
and the function should accept both whole strings and substrings; use `&str`
when it needs the full string operations. Use `str` when it needs an owning
value, for example to keep it after the call.
References can introduce indirection, so they are not guaranteed to be faster
in every case; measure when the cost matters.

### Bindings and slices

<div class="text-bindings-table">

| Expression | Type of `x` | Ownership and storage |
| --- | --- | --- |
| `x = s` | `str` | Another owning value; no text-buffer copy or allocation. Both names remain usable. |
| `x = r` | `&str` | Reborrows the reference; it does not turn the borrowed text into an owner. Use `x = *r` for an owning string value. |
| `x = s[a:b]` | **R** | Evaluates the copying slice now. On success, owns a new string containing the selected text; it does not retain the original buffer. Handle the result later. |
| `x = s[a:b]?` | `str` | The same copying slice, with its successful string extracted; allocation failure returns from the enclosing function. |
| `x = &s` | `&str` | Borrows the whole string binding without copying its text or retaining its heap storage. |
| `x = &s[a:b]` | `&strview` | Borrows the selected run in place, without a text copy, retain, or allocation. Finding scalar bounds may scan the text. |
| `x: str = s[a:b]` | Rejected | The slice returns **R**, not `str`. Use `?` or match the result. |
| `x = &mut s[a:b]` | Rejected | String contents are immutable; there is no mutable substring view. Use a shared view or construct changed text. |

</div>

`?` handles a result; it does not request the allocation. Both copying-slice
forms above evaluate the same operation. Successful slices of at most seven
UTF-8 bytes fit inline; longer ones allocate a new buffer, even when the bounds
select the entire string. Neither form leaves the original buffer pinned by
the result. A result bound without `?` must still be handled or explicitly
discarded; it is not an error merely to bind it.

An owning copy remains valid if `s` is later reassigned or destroyed. A borrow
requires `s` to remain valid and prevents reassignment while that borrow is
still used. `mut s` permits replacing the binding, not editing the text. See
[substring views](../../language/04-expressions/slices.md#substring-views) for
the operations and lifetime limits of `&strview`.

### Explicit `copy`

| Expression | Return type / validity | Ownership and storage |
| --- | --- | --- |
| `copy(s)` | **R** | Returns `Ok` containing another owning string value. No allocation or text-buffer duplication; immutable backing storage is shared. |
| `copy(&s)` or `copy(r)` | **R** | Observes the borrowed string and returns an owning `str` inside `Ok`, not a reference. No allocation or text-buffer duplication. |
| `copy(s)?` | `str` | Extracts the owning copy. Prefer `s`; this spelling produces a `redundant-copy` warning. |
| `copy(&s)?` or `copy(r)?` | `str` | Extracts an owning copy of borrowed text. Prefer `s` or `*r`, respectively; this spelling produces a `redundant-copy` warning. No text-buffer duplication. |
| `copy(s[a:b])` | `Result[R, AllocError]` | Evaluates the copying slice, then copies its result into an outer `Ok`. The successful string shares the slice's new buffer; there is no second text copy. A slice failure becomes `Ok(Err(...))`. |
| `copy(s[a:b]?)` | **R** | Evaluates the copying slice and propagates its failure first, then returns another owning handle to the successful slice inside `Ok`. No second text copy. Prefer `s[a:b]?` when you want a `str`. |
| `copy(&s[a:b])` | Rejected | `copy` does not accept a substring view. Bind `v = &s[a:b]`, then use `v[:]?` to create an owned `str`, or write `s[a:b]?` directly. |

The `Result` wrappers above do not themselves allocate. `copy` has a uniform
fallible signature, but copying an existing `str` cannot fail: it only copies
inline data or shares immutable storage. The slice expressions can still fail
while constructing their new text. `copy` does not force a private buffer for
a string, and adding it around a copying slice does not make that slice safer
or more independent. Prefer ordinary assignment, dereferencing, or the copying
slice itself. See [explicit copying](construction-copy.md#explicit-copying)
and [moves and copies](../../language/05-ownership/01-bindings-moves-and-copies.md).

[Actor message transfer](../../stdlib/actor.md#limits) has a separate ownership
contract: it can duplicate string bytes into the receiving actor's heap.

## Escapes

String literals and f-string text accept the escapes `\0`, `\n`, `\r`, `\t`,
`\\`, `\'`, `\"`, and `\u{H...}`. The last names one Unicode scalar by code
point with 1 to 6 hexadecimal digits of either case, so invisible and combining
scalars stay visible in source:

```plenty
def main() -> Result[(), IoError]:
    print(len("e\u{301}"), "e\u{301}" == "é", "\u{1F600}".byte_len())?
    Ok(())
```
```output
2 False 4
```

It is the spelling the [`!a` conversion](../../language/04-expressions/format-strings.md)
writes, so `!a` output pastes back into source as the same text. A surrogate
(`\u{d800}` to `\u{dfff}`) or a value above `\u{10ffff}` is not a scalar and is
rejected, as are `\u` without braces, an empty or longer digit run, and every
other escape, including Python's `\x`, `\uXXXX`, `\U`, and `\N{...}`. A
[byte literal](../../language/03-types/bytes.md) rejects `\u{...}`: it holds
bytes, so write the UTF-8 encoding with `\xHH`.

| Source | Diagnostic |
| --- | --- |
| `"\u200d"` | `` `\u` needs 1 to 6 hexadecimal digits in braces, as in `\u{200d}` `` |
| `"\u{d800}"` | `` `\u{d800}` is a surrogate code point, not a Unicode scalar value `` |
| `"\u{110000}"` | `` `\u{110000}` is beyond the largest Unicode scalar value, `\u{10ffff}` `` |

## Character lookup

`text[index] -> str` accepts an `i64` scalar index, with negative indices counted
from the end. A missing index traps. `text.get(index) -> Option[str]` instead
returns `Nothing`, including for either extreme `i64` index.

Both observe the text and index once in source order, accept references, and
produce an independent single-scalar string without allocation. ASCII lookup is
constant-time; other text requires scanning to the scalar boundary. String
iteration likewise yields one-scalar strings without allocation.

`ord(text) -> Result[u32, ParseError]` converts a one-scalar string to its
Unicode code point. Empty text or multiple scalars return `ParseError.Invalid`.
`chr(code: u32) -> Result[str, ParseError]` performs the reverse conversion;
surrogate code points and values above U+10FFFF return `ParseError.OutOfRange`.
Both allocate nothing, including for non-ASCII scalars. Unsuffixed `chr`
arguments infer `u32`; other integer widths require explicit conversion.
Indexing and iteration retain the existing single-scalar `str` representation;
there is no separate `char` type.

## Classification

`text.isascii() -> bool` is true for empty text and for every ASCII character,
including controls and NUL. `text.isspace() -> bool` requires nonempty text whose
characters all have Unicode's White_Space property. NUL and U+200B are not
whitespace. The bundled Unicode tables also govern trimming; Python's additional
whitespace classifications are not promised.

`text.isdigit()`, `text.isalpha()` and `text.isalnum()` require nonempty text
consisting entirely of ASCII digits (`0`–`9`), ASCII letters (`A`–`Z`,
`a`–`z`), or either, respectively. Underscores, punctuation, non-ASCII letters
and digits (`é`, `١`, `１`) and combining marks are false. They deliberately do
not implement Python's Unicode digit/letter predicates; Unicode letter and
number classification is part of [backlog item B47](../../../backlog.md).

All classification methods observe their receiver, take no arguments, and allocate nothing.

`text.lower()` and `text.upper()` return `Result[str, AllocError]`, applying
Unicode's per-scalar case mappings without locale-specific or contextual rules.
Mappings may expand a scalar (`ß` becomes `SS` in uppercase). They observe the
receiver and allocate only the final output, fallibly; short results stay inline.
They do not normalize or implement case folding.
Other operations are grouped under [search](text-search.md),
[building](text-building.md), and [splitting](text-splitting.md).
