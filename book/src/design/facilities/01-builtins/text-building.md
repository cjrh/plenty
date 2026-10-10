# Building text

These methods observe their inputs once, receiver first, and return
`Result[str, AllocError]`. Inputs remain unchanged on success and failure; the
result outlives them. References are accepted. Length/layout overflow returns
`CapacityOverflow`; storage exhaustion returns `OutOfMemory`. Only the final
text needs storage, with no intermediate strings; [small outputs](text.md)
require no heap allocation.

| Operation | Behavior |
| --- | --- |
| `text.concat(other)` or `text + other` | Concatenate two strings |
| `separator.join(parts)` | Insert the separator between consecutive strings in a `list[str]` |
| `text.repeat(count)` | Repeat exact contents; `count` is `i64`; zero or negative yields empty |
| `text.slice(start, stop)` | Forward scalar slice with required `i64` bounds |
| `text.replace(old, new)` | Replace all non-overlapping literal matches, left to right |
| `text.removeprefix(prefix)` | Remove at most one exact prefix |
| `text.removesuffix(suffix)` | Remove at most one exact suffix |
| `text.strip()` | Remove Unicode White_Space at both ends |
| `text.lstrip()` / `text.rstrip()` | Remove White_Space at the left/right end |

`join` accepts a list or list reference, not arbitrary iterables. An empty list
produces empty text; a singleton adds no separator. Empty pieces are retained.
Empty input to `repeat` produces empty output for any count.

`slice` includes start and excludes stop. Negative bounds count from the end;
bounds clamp to `[0, len(text)]`, without overflow for extreme `i64` values.
Reversed bounds yield empty text. ASCII bounds map directly to bytes; other
text requires scanning scalar boundaries. Slice syntax and steps are unsupported.

`replace` observes receiver, old, and new in that order. Inserted text is not
searched again. An empty old string matches every scalar boundary, including
both ends; on empty input it inserts new once. Empty new text removes matches.
There is no count limit, regex interpretation, or normalization.

Prefix/suffix removal uses exact strings, including NUL, without repeated
removal. Empty or missing patterns preserve the contents. Trimming takes no
character-set argument, leaves interior text unchanged, and uses the same
White_Space classification as [isspace](text.md).
