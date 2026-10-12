# Prune before descending

The small companion files provide two fixture directories. Run the program from
an empty `work/` child of the directory containing these files; the root below
uses `..` to reach the fixture without changing the working directory:

```plenty-file tree/kept/item.plenty
pub const value: i64 = 1
```

```plenty-file tree/pruned/item.plenty
pub const value: i64 = 2
```

With a top-down walker, edit its directory list before asking for another row:

```plenty
import std.fs as fs
import std.path as path

def main() -> Result[(), Failure]:
    root = path.from_text("../tree")?
    mut walker = fs.walk(&root, True)?
    while walker.next()?:
        print(walker.current.path.to_text()?)?
        mut index = len(walker.current.directories) - 1
        while index >= 0:
            if walker.current.directories[index].to_text()?.endswith("/pruned"):
                drop(walker.current.directories.pop(index))
            index = index - 1
    walker.close()
    print(walker.next()?)?
    Ok(())
```

```output
../tree
../tree/kept
False
```

False passed to `fs.walk` requests bottom-up order. Editing a bottom-up row
cannot undo descent. Child directory links appear in `current.files`, and are
never followed; a link to an ancestor cannot make this walker loop. An explicit
root may itself resolve through a link.

To continue siblings after an error, match `walker.next()` instead of using `?`:

```plenty
import std.fs as fs
import std.path as path

def main() -> Result[(), Failure]:
    root = path.from_text("missing-root")?
    mut walker = fs.walk(&root, True)?
    match walker.next():
        case Ok(found):
            print(found)?
        case Err(error):
            match error:
                case fs.WalkError.Io(at, operation, cause):
                    print(at.to_text()?)?
                    print(operation)?
                    print(cause)?
                case fs.WalkError.Allocation(cause):
                    print(cause)?
    print(walker.next()?)?
    Ok(())
```

```output
missing-root
read_dir
IoError.System(2)
False
```

The failed subtree is already skipped. Another call continues any pending
siblings. Propagating the error stops traversal with normal cleanup. Only the
current scan owns a native directory stream; retaining results does not retain
directory handles. See [walking contracts](../../design/stdlib/fs/walking.md).
