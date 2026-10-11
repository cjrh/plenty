# Splitting text

Both operations observe their text and arguments without consuming them. They
return independently owned lines or pieces in `Result[list[str], AllocError]`.
All list storage and long output strings are constructed fallibly. Failure
reclaims the initialized result prefix and preserves the inputs.

## Explicit separators

`text.split(separator)` requires one nonempty string, including a reference.
Empty separators trap; this invalid-operation error is not an `AllocError`.
Maximum-split arguments are unsupported.

Matches are literal, non-overlapping, and scanned left to right. Leading,
trailing, and adjacent separators produce empty pieces. Empty input yields
`[""]`; no match yields one piece containing the input's contents. Multibyte
separators and NUL work without normalization.

## Whitespace

`text.split()` with no separator splits on runs of Unicode White_Space, using
the same bundled property as `isspace()` and trimming. It omits leading,
trailing, and repeated empty pieces; empty or whitespace-only text yields an
empty list. NUL and U+200B are ordinary content. The output remains fallible
and independently owned, like explicit-separator splitting.

## Lines

`text.splitlines()` discards terminators. `text.splitlines(True)` retains each
original terminator without translation; the optional argument is `bool` and
positional.

Recognized boundaries are LF, CR, CRLF, vertical tab, form feed, U+001C–U+001E,
U+0085, U+2028, and U+2029. Empty text yields no lines; interior blank lines are
retained; a final terminator adds no extra empty line. This is broader than
[file universal-newline reading](../02-io/reading.md), which recognizes CR/LF only.
