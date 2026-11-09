# Ordinary-path directory creation

`std.fs.create_dirs(path: &Path, unix_mode: u32 = 0o700u32) -> Result[(), IoError]`
creates missing directories using ordinary OS pathname resolution. It borrows
the lossless native-byte path without retaining it and returns unit; success
does not require opening the result as a readable `Root`.

```plenty
import std.fs as fs
import std.path as path

def main() -> Result[(), AllocError | IoError]:
    state = path.from_text("app/state/runs")?
    fs.create_dirs(&state)?
    fs.create_dirs(&state)?
    print(fs.metadata(&state)?.is_dir())?
    Ok(())
```
```output
True
```

## Choosing the path contract

| | `fs.create_dirs(&path)` | [`root.create_dirs(&path)`](helpers.md#creating-a-directory-tree) |
| --- | --- | --- |
| Starting point | OS root for absolute paths; working directory for relative paths | Already-open `Root` |
| Absolute paths | Accepted | Rejected |
| `.` and `..` | Ordinary OS resolution | Rejected |
| Symlink components, including final link to a directory | Followed | Rejected |
| Result | `Result[(), IoError]` | `Result[Root, IoError]` |
| Existing directories | Accepted; permissions unchanged | Accepted; permissions unchanged |
| New-directory mode | Requests 0700 by default, subject to OS policy | Same existing policy |

Neither operation is a sandbox. Ordinary-path creation provides no containment
guarantee. Rooted traversal does not isolate callers from every concurrent
rename. Opening the initial [`Root`](helpers.md) rejects a final symlink but
resolves ancestor components normally; these contracts are unchanged.

## Resolution and permissions

Relative paths start at the process working directory; the function never
changes it. Absolute paths start at the filesystem root. Repeated and trailing
separators, filesystem root and `.` are accepted. No UTF-8 conversion, shell
expansion (`~`, variables), lexical simplification or canonicalization occurs.

Components are processed in order. If `link` points to `/mnt/storage/a`, then
`link/../cache` creates `/mnt/storage/cache`. Likewise, `new/../cache` can create
both `new` and `cache`. Existing intermediate and final symlinks are followed.
Dangling links and link loops fail; a missing symlink target is never constructed
implicitly. Files and other non-directories fail without replacement or truncation.

Every directory created by this call receives the requested mode **at creation**,
without a broader temporary mode or a later `chmod`. Only ordinary bits
`0o000u32..0o777u32` are accepted. Existing directories, including symlink targets,
retain their permissions, ownership and ACLs. The library never reads or changes
the process-global umask. Umask, default ACLs, setgid inheritance and filesystem
rules still apply. Thus 0700 is a requested creation mode, not a guarantee about
existing directories or an override of OS permission policy; it does not certify
ownership or suitability for secrets.

No directory read permission is needed just to enumerate or return a handle.
An existing write-and-search-only parent can support creation. Restrictive modes
can prevent creation of later components; the OS error is returned without
broadening permissions.

## Errors, races and limits

Empty paths, embedded NUL and invalid mode bits return `IoError.InvalidInput`
before any creation, even when the destination already exists. Native errors
remain `IoError.System(code)`; allocation failure is
`IoError.Data(DataError.Allocation(...))`. Supported targets match the rooted
creation APIs (64-bit Linux); other targets return `IoError.Unsupported` for
valid arguments. Each native prefix lookup is subject to Linux pathname limits,
including `PATH_MAX` (4096 bytes including its terminating NUL).

Concurrent creators are accepted only after an existing entry is verified to
resolve to a directory. Lookup and creation are separate operations; symlink
and rename races have ordinary pathname semantics. There is no protection from
an attacker changing components. Calls can block and have no deadline or
cancellation parameter.

The operation is not atomic. A later error leaves earlier directories in place;
there is no rollback. Retrying after correcting the problem uses those existing
directories. Traversal and retry policy are iterative Plenty code. Temporary
storage is bounded by the input path length, and no directory descriptors are
opened. Narrow native `stat` and `mkdir` primitives provide OS resolution and
permission behavior. Calls are permitted in workers under the normal
[path-effect rules](../../facilities/03-concurrency/effects.md).
