# std.diff

`import std.diff` compares two sequences and returns the edit operations that
turn the old one into the new one. It diffs generic lists, bytes, and text split
into lines, words, or Unicode scalars, and formats text differences as unified
diffs. It is ordinary embedded Plenty code: every allocation failure is reported
as `AllocError`.

| Operation | Result | Contract |
|---|---|---|
| `std.diff.diff(&old, &new, algorithm)` | `Result[list[Op], AllocError]` | Edit operations between two `list[T]`; `T` must be usable as a dictionary key |
| `std.diff.diff_bytes(&old, &new, algorithm)` | `Result[list[Op], AllocError]` | Edit operations between the bytes of two `Bytes` values |
| `std.diff.lines(old, new, algorithm)` | `Result[TextDiff, AllocError]` | Compare `&str` texts line by line |
| `std.diff.words(old, new, algorithm)` | `Result[TextDiff, AllocError]` | Compare runs of whitespace and runs of other scalars |
| `std.diff.chars(old, new, algorithm)` | `Result[TextDiff, AllocError]` | Compare Unicode scalar by scalar |
| `text_diff.ops()` | `&list[Op]` | The operations, indexing the tokens |
| `text_diff.old_tokens()`, `text_diff.new_tokens()` | `&list[str]` | Each side's tokens |
| `text_diff.has_changes()` | `bool` | `False` when the texts are equal |
| `text_diff.unified(context, old_name, new_name)` | `Result[str, AllocError]` | Unified diff text, or `""` for equal texts |

`algorithm` is optional and defaults to `Algorithm.Myers`. Import `Algorithm`,
`Kind`, `Op`, and `TextDiff` from `std.diff` to name them.

## Operations

An `Op` has public `kind`, `old_start`, `old_end`, `new_start`, and `new_end`
fields. It says that `old[old_start:old_end]` becomes `new[new_start:new_end]`.
`op.old_len()`, `op.new_len()`, and `op.is_equal()` are shorthands.

| `kind` | Old range | New range |
|---|---|---|
| `Kind.Equal` | Nonempty | Equal elements, same length |
| `Kind.Delete` | Nonempty, removed | Empty, at the position of the removal |
| `Kind.Insert` | Empty, at the insertion position | Nonempty, added |

Operations are in order and cover both sequences without gaps: each starts
where the previous one ended. No operation is empty, two adjacent operations
never have the same kind, and a deletion comes before an adjacent insertion.
There is no separate replace kind; a replacement is a deletion followed by an
insertion. Equal inputs give one `Equal` operation, or none when both are empty.

## Algorithms

`Algorithm.Myers` finds a shortest edit script: the number of equal elements is
the length of a longest common subsequence. It uses the linear-space
divide-and-conquer form of Myers' algorithm, after removing the common prefix
and suffix. It takes O((N+M)·D) time and O(N+M) space, where D is the number of
deleted and inserted elements. Small changes to large inputs are fast; two
large inputs with nothing in common take time quadratic in their size. There is
no time limit or cost bound.

`Algorithm.Patience` matches the elements that occur exactly once in each
sequence, keeps the longest increasing run of those matches as anchors, and
runs Myers between anchors. The script is valid but not always shortest. It
often reads better for source code, where repeated lines such as braces and
blank lines otherwise pull unrelated blocks into alignment.

Both algorithms first intern elements to integer ids through a dictionary, so
comparisons during the search are integer comparisons. Text tokens are `str`
copies that share storage with each other but not with the input text.

## Tokens

Each side's tokens concatenate to its original text.

- `lines` splits after each LF. A line keeps its LF, and a CR before it, so
  a final line without an LF differs from the same line with one.
- `words` alternates runs of whitespace (`str.isspace`) and runs of other
  scalars.
- `chars` makes one token per Unicode scalar, not per grapheme cluster.

## Unified format

`unified` follows the output of GNU `diff -U`:

- The text starts with `--- old_name` and `+++ new_name` lines. No timestamp is
  added.
- Each hunk header is `@@ -start,count +start,count @@`, with 1-based starts. A
  count of 1 is omitted, and an empty range gives the line before it as its
  start, as in `@@ -0,0 +1 @@`.
- Each token is written as one line prefixed by a space, `-`, or `+`. A token
  that does not end in LF is followed by an LF and the line
  `\ No newline at end of file`.
- `context` unchanged tokens surround each change. Changes separated by at most
  `2 * context` unchanged tokens share a hunk. A negative context counts as
  zero.

`unified` is meant for `lines`. For word or scalar diffs, every token still
becomes its own output line.

There is no inline (word-level) rendering, patch parsing or application, or
three-way merge. See the runnable [diff lesson](../../tutorial/07-modules/20-standard-diff.md).
