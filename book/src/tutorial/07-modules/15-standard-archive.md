# Read and write archives

`std.archive` reads and writes TAR and ZIP archives over
[`std.io`](../../design/stdlib/io.md) streams, and extracts them safely into
a new directory.

## Write and list a TAR archive

A `TarWriter` wraps any writer. Each `add_` call writes one entry, and
`finish` writes the end of the archive. A `TarReader` returns the entries in
order; between calls to `next`, it is a reader of the current entry's
content:

```plenty
import std.archive
import std.bytes
import std.io
import std.path

def main() -> Result[(), IoError | std.io.Partial | std.archive.ArchiveError | std.bytes.BytesError]:
    mut writer = std.archive.tar_writer(std.io.cursor(std.bytes.empty()?))?
    writer.add_dir(&std.path.from_text("notes")?, 493, 1700000000)?
    writer.add_file(&std.path.from_text("notes/todo.txt")?, &std.bytes.from_text("buy milk")?, 420, 1700000000)?
    writer.finish()?
    packed = writer.inner.data.copy()?
    print(packed.len())?
    mut reader = std.archive.tar_reader(std.io.cursor(packed))?
    while True:
        match reader.next()?:
            case Some(entry):
                mut content = std.bytes.empty()?
                drop(std.io.read_up_to(&mut reader, &mut content, 1000)?)
                print(decode_utf8(entry.name.as_bytes())?, entry.size)?
                if content.len() > 0:
                    print(content.to_text()?)?
            case Nothing:
                break
    Ok(())
```
```output
2560
notes/ 0
notes/todo.txt 8
buy milk
```

TAR stores each header and each file's content in 512-byte blocks and ends
with two empty blocks, so even a tiny archive takes a few kilobytes. To
compress it, wrap the writer in a
[`std.compress`](14-standard-compress.md) `Compressor`.

## ZIP archives

A ZIP archive keeps a directory of its entries at the end, so a
`ZipReader` needs a stream it can seek: a file or a cursor. `entries` lists
everything; `open` selects one entry to read:

```plenty
import std.archive
import std.bytes
import std.io
import std.path
from std.archive import Method

def main() -> Result[(), IoError | std.io.Partial | std.archive.ArchiveError | std.bytes.BytesError]:
    mut writer = std.archive.zip_writer(std.io.cursor(std.bytes.empty()?))?
    mut text = std.bytes.empty()?
    for _ in range(0, 50):
        text.extend(&std.bytes.from_text("all work and no play ")?)?
    writer.add_file(&std.path.from_text("story.txt")?, &text, Method.Deflate, 420, 1700000000)?
    writer.add_file(&std.path.from_text("short.txt")?, &std.bytes.from_text("hi")?, Method.Stored, 420, 1700000000)?
    writer.finish()?
    mut reader = std.archive.zip_reader(std.io.cursor(writer.inner.data.copy()?), 100)?
    entries = reader.entries()
    for entry in entries:
        print(decode_utf8(entry.name.as_bytes())?, entry.size, entry.compressed)?
    reader.open(1)?
    mut content = std.bytes.empty()?
    drop(std.io.read_up_to(&mut reader, &mut content, 100)?)
    print(content.to_text()?)?
    Ok(())
```
```output
story.txt 1050 35
short.txt 2 2
hi
```

The reader checks each entry's size and CRC-32 when its content ends.

## Extract safely

An archive is untrusted input. Its names may point outside the target
(`../../.bashrc`), its links may lead elsewhere, and a small archive may
expand into terabytes. `extract_tar` and `extract_zip` check every entry
against a `Policy`, write into a hidden staging directory, and rename it to
the destination only when everything succeeded:

```plenty
import std.archive
import std.bytes
import std.fs
import std.io
import std.path
from std.sync import CancellationToken

type ArchiveOperationError = std.archive.ArchiveError | std.bytes.BytesError | std.fs.FsError

def main() -> Result[(), AllocError | IoError | ArchiveOperationError]:
    parent = std.fs.root(&std.path.from_text(".")?)?
    mut scratch = std.fs.temporary_dir(&parent, &std.path.from_text("archive-lesson-")?, 8)?
    mut writer = std.archive.tar_writer(std.io.cursor(std.bytes.empty()?))?
    writer.add_file(&std.path.from_text("site/index.html")?, &std.bytes.from_text("<h1>hi</h1>")?, 420, 0)?
    writer.add_symlink(&std.path.from_text("site/secret")?, &std.path.from_text("/etc/passwd")?, 0)?
    writer.finish()?
    packed = writer.inner.data.copy()?

    cancel = CancellationToken()?
    mut policy = std.archive.policy()
    mut first = std.archive.tar_reader(std.io.cursor(packed.copy()?))?
    match std.archive.extract_tar(&mut first, scratch.root(), &std.path.from_text("out")?, &policy, &cancel):
        case Ok(_):
            print("extracted")?
        case Err(error):
            print(error)?
    policy.skip_links = True
    mut second = std.archive.tar_reader(std.io.cursor(packed))?
    summary = std.archive.extract_tar(&mut second, scratch.root(), &std.path.from_text("out")?, &policy, &cancel)?
    print(summary.files, summary.bytes, summary.skipped)?

    std.fs.remove_tree(scratch.root(), &std.path.from_text("out")?, 10, 4, &cancel)?
    scratch.cleanup()?
    Ok(())
```
```output
std.archive.ArchiveError.Link
1 11 1
```

The first attempt fails on the link and leaves nothing behind. The second
skips it. Extracted files get private permissions (0600); stored modes and
times are not applied.

The default policy allows 10000 entries, 1 GiB per file, 4 GiB in total,
and a compression ratio of 100 for large ZIP entries. Lower the limits to
what your program expects.

See the [`std.archive` reference](../../design/stdlib/archive.md) for the
format rules, every error, and memory costs.
