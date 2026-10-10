# Text search

All these methods observe their strings once in source order, accept references,
and allocate nothing. Matches are literal UTF-8, with no normalization, case
folding, or regex interpretation. Optional bounds are unsupported.

| Operation | Result | Behavior |
| --- | --- | --- |
| `text.startswith(prefix)` | `bool` | Exact prefix; empty prefix always matches |
| `text.endswith(suffix)` | `bool` | Exact suffix; empty suffix always matches |
| `text.find(needle)` | `Option[i64]` | First matching scalar position, or `Nothing` |
| `text.rfind(needle)` | `Option[i64]` | Last matching scalar position, or `Nothing` |
| `text.count(needle)` | `i64` | Number of non-overlapping matches, left to right |

Each method takes exactly one string argument. Prefix and suffix checks do not
accept tuples of alternatives. An empty needle matches at zero for `find`, at
`len(text)` for `rfind`, and at all `len(text) + 1` scalar boundaries for `count`.
The last `rfind` match can overlap an earlier match. Converting a found byte
position to a scalar position scans the preceding prefix.

`needle in text` and `needle not in text` provide substring membership.
