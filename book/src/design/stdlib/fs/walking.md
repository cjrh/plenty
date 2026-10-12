# Walking a tree

`std.fs.walk(&root, top_down) -> Result[Walk, AllocError]` constructs an explicit
Plenty walker over [directory enumeration](../fs.md). It copies the root
and allocates its pending stack, but opens no directory until advancement.
`walker.next() -> Result[bool, WalkError]` updates `walker.current` when it returns
True. False means exhaustion or explicit close. `walker.close()` clears pending
work and current entry lists without allocation and is repeatable.

`current` is a `WalkDir` with `path: Path`, `directories: list[Path]`, and
`files: list[DirEntry]`. Directories contains only non-followed directory paths;
files contains every other native type, including directory links, dangling
links, FIFOs and sockets. Non-directory does not mean regular file.

In top-down mode, next yields the parent before any child is opened. Remove or
reorder `current.directories` before advancing again; the walker descends only
into the remaining paths, in their resulting order. It consumes that list on
the next call. Thus pruning happens before even attempting access to an
unreadable child. In bottom-up mode, children have already been visited when
the parent is yielded, so editing its directories cannot prune prior descent.
Native sibling order remains unspecified; there is no implicit sorting.

The walker never follows discovered directory links. An explicitly supplied
root can resolve through a link, just as ordinary path lookup can. This initial
walker has no follow-links option or visited-identity set; an ancestor link
therefore does not cause recursive descent. Relative roots retain their spelling
and the walker never changes the working directory.

`WalkError.Io(path, operation, cause)` retains the attempted Path, a fixed
operation label (`read_dir`, `next`, `file_type`, or `entry.path`), and concrete
IoError. The failing frame has already been removed. Handle Err and call next
again to skip that subtree and continue pending siblings; propagate with `?`
to stop and drop the walker. A partially scanned directory produces no partial
row. `WalkError.Allocation(error)` reports fallible stack/list/path storage;
no completeness guarantee applies after an allocation failure. If allocating
the error's entry path also fails, the context reports the containing root and
`entry.path` instead.

Traversal uses a Plenty list of frames and no recursive native call stack.
At most one directory stream is open during a scan, and none is retained between
next calls. A row allocates one directory's lists. Top-down storage includes
pending sibling paths along the active traversal depth and the current row;
bottom-up additionally retains parent rows along that depth. It streams the
tree rather than collecting the entire tree in advance; a very wide directory
still needs fallible per-directory storage.

Python `os.walk` supplies top-down/bottom-up and pruning precedent. Grouping
directory symlinks with files follows `pathlib.Path.walk` with link following
disabled. The explicit next/error continuation is Plenty's adaptation of an
error callback; failures are visible by default. The implementation is embedded
Plenty source in `stdlib/std/fs.plenty`, not a separate native traversal engine.
