# Compare text with std.diff

`std.diff` finds what changed between two versions of a text and reports it as
edit operations: runs that stayed equal, were deleted, or were inserted.

## Show a unified diff

`std.diff.lines` compares line by line. `unified` formats the result the way
`diff -u` does, with the given number of unchanged lines around each change:

```plenty
import std.diff

def main() -> Result[(), AllocError | IoError]:
    old = "name = \"stats\"\nversion = \"0.1.0\"\nauthors = []\nlicense = \"MIT\"\n"
    new = "name = \"stats\"\nversion = \"0.2.0\"\nauthors = []\nlicense = \"MIT\"\nreadme = \"README.md\"\n"
    changes = std.diff.lines(old, new)?
    print(changes.has_changes())?
    print(changes.unified(1, "plenty.toml", "plenty.toml (edited)")?)?
    Ok(())
```
```output
True
--- plenty.toml
+++ plenty.toml (edited)
@@ -1,4 +1,5 @@
 name = "stats"
-version = "0.1.0"
+version = "0.2.0"
 authors = []
 license = "MIT"
+readme = "README.md"

```

A hunk header `@@ -1,4 +1,5 @@` says that four lines starting at line 1 of
the old text became five lines starting at line 1 of the new one. Equal texts
give empty text, so check `has_changes` first when that matters.

## Walk the operations

`words` splits text into runs of whitespace and runs of other characters, and
`chars` into single characters. Each `Op` gives a range of old tokens and a
range of new tokens; a deletion has an empty new range and an insertion an
empty old range:

```plenty
import std.diff
from std.diff import Kind

def main() -> Result[(), AllocError | IoError]:
    changes = std.diff.words("the quick brown fox", "the quick red fox jumps")?
    old = changes.old_tokens()
    new = changes.new_tokens()
    for op in changes.ops():
        match op.kind:
            case Kind.Equal:
                pass
            case Kind.Delete:
                for index in range(op.old_start, op.old_end):
                    print("removed", str.repr(old[index])?)?
            case Kind.Insert:
                for index in range(op.new_start, op.new_end):
                    print("added", str.repr(new[index])?)?
    Ok(())
```
```output
removed "brown"
added "red"
added " "
added "jumps"
```

`ops` returns a reference, so bind the `TextDiff` to a name before calling it.
`std.diff.diff` compares any two lists whose elements can be dictionary keys,
and `std.diff.diff_bytes` compares `Bytes`.

The default algorithm finds the fewest deletions and insertions. Passing
`Algorithm.Patience` as the last argument anchors the comparison on lines that
occur once in each text, which often reads better for source code. See the
[`std.diff` reference](../../design/stdlib/diff.md) for both algorithms and
their costs.
