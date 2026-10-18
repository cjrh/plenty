# std.archive

`import std.archive` reads and writes TAR and ZIP archives over
[`std.io`](io.md) streams, and extracts them into a new directory under a
[`std.fs`](fs.md) `Root`. It has one error type, `ArchiveError`.

The module is ordinary Plenty over [`std.bytes`](bytes.md),
[`std.compress`](compress.md) (DEFLATE and CRC-32) and `std.fs`; it adds no
runtime code. It is available on the supported x86-64 Linux GNU target.

## Functions

| Function | Result | Contract |
|---|---|---|
| `tar_reader(inner: R)` | `Result[TarReader[R], ArchiveError]` | A streaming TAR reader over any `Reader` |
| `tar_writer(inner: W)` | `Result[TarWriter[W], ArchiveError]` | A streaming TAR writer over any `Writer` |
| `zip_reader(inner: R, max_entries: i64)` | `Result[ZipReader[R], ArchiveError]` | Reads the central directory of a seekable `inner` |
| `zip_writer(inner: W)` | `Result[ZipWriter[W], ArchiveError]` | A ZIP writer over any `Writer`; it never seeks |
| `policy()` | `Policy` | The default extraction limits |
| `extract_tar(archive: &mut TarReader[R], parent: &Root, destination: &Path, policy: &Policy, cancel: &CancellationToken)` | `Result[Summary, ArchiveError]` | Extract every remaining entry into the new directory `parent/destination` |
| `extract_zip(archive: &mut ZipReader[R], parent: &Root, destination: &Path, policy: &Policy, cancel: &CancellationToken)` | `Result[Summary, ArchiveError]` | Extract every entry into the new directory `parent/destination` |
| `stream_error(error: ArchiveError)` | `StreamError` | How a reader reports `error` through the `Reader` protocol |

## Entries

```text
pub enum EntryKind:
    File
    Directory
    Symlink
    Hardlink
    Other

pub class Entry:
    pub name: Path
    pub kind: EntryKind
    pub size: i64
    pub compressed: i64
    pub mode: i64
    pub mtime: i64
    pub link: Path
```

`name` holds the raw bytes stored in the archive; it is not checked or
decoded until extraction. `size` is the length of the content, `compressed`
its length in the archive (equal to `size` in TAR), `mode` the Unix
permission bits (0 to 4095, or 0 for ZIP entries made on other systems),
and `mtime` the modification time in Unix seconds. `link` is a TAR link
target; ZIP stores a symbolic link's target as its content. `Other` covers
devices, FIFOs, and unknown TAR types.

## TarReader

`TarReader[R]` reads POSIX ustar, pax, GNU, and V7 archives. `inner` is
public.

| Method | Result | Contract |
|---|---|---|
| `next()` | `Result[Option[Entry], ArchiveError]` | The next entry, skipping unread content; `Nothing` at the end |
| `read(buffer: &mut Bytes, start: i64, stop: i64)` | `Result[i64, StreamError]` | The current entry's content; 0 at its end |
| `failure()` | `Option[ArchiveError]` | The failure that ended the archive, if any |

Format rules:

- Each header's checksum is verified; old writers' signed sums are accepted.
  A mismatch is `Checksum`.
- Numbers are octal, or GNU base-256 for values beyond 8 GiB. Any other
  digit, a negative base-256 value, or an overflow is `Corrupt`.
- Long names and link targets come from the ustar prefix, from GNU `L` and
  `K` entries, and from pax `path` and `linkpath` records; pax `size`
  replaces the header size. Other pax keys and global pax headers are
  ignored. A pax header or GNU long name larger than 64 KiB is `Limit`.
- Only regular files and unknown types have content; links, directories,
  and devices have none, whatever their size field says.
- The archive ends with two zero blocks. Input that ends before them is
  `Truncated`; one zero block followed by anything else is `Corrupt`.

A malformed header ends the archive: `next` and `read` repeat the error, and
`failure` names it. An error from `inner` is returned unchanged and leaves
the reader usable, so `Interrupted` may be retried.

## TarWriter

`TarWriter[W]` writes POSIX ustar headers with user and group 0 and no
owner names. A name or link target longer than 100 bytes is stored in a
pax extended header; a size beyond 8 GiB uses GNU base-256.

| Method | Result | Contract |
|---|---|---|
| `add_file(name: &Path, data: &Bytes, mode: i64, mtime: i64)` | `Result[(), ArchiveError]` | A regular file holding `data` |
| `add_from(name: &Path, size: i64, source: &mut S, mode: i64, mtime: i64)` | `Result[(), ArchiveError]` | A regular file of exactly `size` bytes read from `source` |
| `add_dir(name: &Path, mode: i64, mtime: i64)` | `Result[(), ArchiveError]` | A directory |
| `add_symlink(name: &Path, target: &Path, mtime: i64)` | `Result[(), ArchiveError]` | A symbolic link; `target` is stored unchecked |
| `finish()` | `Result[(), ArchiveError]` | Write the end blocks and flush `inner` |

Names are normalised: empty and `.` components are dropped, and directories
get a trailing `/`. A name that is empty after that, absolute, or contains
`..`, NUL, or a backslash is `UnsafePath`, so a writer cannot produce an
archive that extraction would refuse. A `mode` outside 0 to 4095, a negative
size or time, or any call after `finish` is `InvalidInput`. A `source` that
ends early in `add_from` is `Truncated` and leaves the archive unusable.

## ZipReader

`ZipReader[R]` needs a stream with `seek`, such as `FileStream` or `Cursor`.
`zip_reader` locates the end record, reads the whole central directory, and
keeps one `Entry` per member; more than `max_entries` entries is `Limit`.
`inner` is public.

| Method | Result | Contract |
|---|---|---|
| `len()` | `i64` | The number of entries |
| `entries()` | `&list[Entry]` | Every entry, in central-directory order |
| `open(index: i64)` | `Result[(), ArchiveError]` | Select entry `index` for `read` |
| `read(buffer: &mut Bytes, start: i64, stop: i64)` | `Result[i64, StreamError]` | The selected entry's content; 0 at its end |
| `failure()` | `Option[ArchiveError]` | The failure that ended the selected entry, if any |

Format rules:

- The end record must be the last one whose comment reaches the end of the
  input exactly. ZIP64 end records are read when a count, size, or offset is
  saturated, and ZIP64 extra fields supply each entry's sizes and offset.
- Spanned (multi-disk) archives are `Unsupported`.
- Entries may not overlap: each entry's local header, name, and data must fit
  before the next entry's local header or the central directory. Overlapping
  entries, the basis of some ZIP bombs, are `Corrupt`.
- The local header must repeat the central name. Only methods 0 (stored)
  and 8 (DEFLATE) are read; others and encrypted entries are `Unsupported`.
- The content must be exactly the declared size, its CRC-32 must match
  (`Checksum`), and a DEFLATE stream must end exactly at the declared
  compressed size (`Corrupt`).
- An entry is a directory when its name ends in `/` or its attributes say
  so; Unix attributes (host 3) also give `mode`, symbolic links, and special
  files. Times are DOS times in local time, read as UTC.

`open` clears an earlier failure. A content failure ends the entry: `read`
repeats it and `failure` names it. CRC and size are checked when the content
ends, so treat bytes as unverified until `read` returns 0. Errors from
`inner` are returned unchanged, as in `TarReader`.

## ZipWriter

`ZipWriter[W]` writes each entry's header, then its data, and the central
directory at `finish`. It never seeks, so it can write to a pipe.

| Method | Result | Contract |
|---|---|---|
| `add_file(name: &Path, data: &Bytes, method: Method, mode: i64, mtime: i64)` | `Result[(), ArchiveError]` | A regular file; `Method.Deflate` compresses at level 6, `Method.Stored` does not |
| `add_dir(name: &Path, mode: i64, mtime: i64)` | `Result[(), ArchiveError]` | A directory |
| `add_symlink(name: &Path, target: &Path, mtime: i64)` | `Result[(), ArchiveError]` | A symbolic link |
| `finish()` | `Result[(), ArchiveError]` | Write the central directory and end records, and flush `inner` |

Names follow the `TarWriter` rules. A name with non-ASCII bytes that is valid
UTF-8 sets the UTF-8 flag. Attributes are Unix (host 3) with `mode`.
Times are stored as DOS times, clamped to 1980 through 2107, with
two-second resolution. ZIP64 fields are written for an entry whose size or
offset reaches 2³²−1, and ZIP64 end records when there are 65535 or more
entries or the central directory lies beyond 2³²−1.

An error while writing an entry or the directory to `inner` leaves the
archive unusable: that call returns the error, and every later `add_*` or
`finish` call returns `InvalidInput` without writing.

## Extraction

`extract_tar` and `extract_zip` create `destination`, a single name, under
`parent`. Nothing appears at `destination` until every entry has been
extracted:

1. A staging directory named `.plenty-extract-` and 32 random hex digits is
   created under `parent`.
2. For each entry, in archive order: check `cancel`, the entry count, and
   the name; refuse or skip links and special files; check the size limits;
   then create the directory or write the file inside the staging
   directory. Files are written through `std.fs.copy_from`, so each is
   staged anonymously and published with a no-replace link.
3. The staging directory is renamed to `destination` without replacing
   anything. An existing `destination` is `Exists`.

On any error the staging tree is removed and the first error returned.
Removal allocates; if it fails too, the staging directory keeps its
`.plenty-extract-` name for the caller to remove.

Every path is resolved through `std.fs.Root` handles that refuse symbolic
links, so an entry cannot reach outside the staging directory. Name rules:

- An absolute name, a `..` component, NUL, or a backslash is `UnsafePath`.
- Empty and `.` components are dropped; a file whose name is then empty is
  `UnsafePath`, and such a directory is ignored.
- A name longer than `max_name` bytes, or with more than `max_depth`
  components, is `Limit`.
- Two entries with the same file name, or a file and a directory with the
  same name, are `Duplicate`. Repeated directory entries are allowed.
- Names are byte strings; they are not decoded or normalised.

Files are created with mode 0600 and directories with 0700. Stored modes,
times, and owners are not applied.

## Policy

```text
pub class Policy:
    pub max_entries: i64
    pub max_entry_size: i64
    pub max_total_size: i64
    pub max_ratio: i64
    pub max_depth: i64
    pub max_name: i64
    pub skip_links: bool
    pub skip_special: bool
```

| Field | Default | Meaning |
|---|---|---|
| `max_entries` | 10000 | Entries of any kind, including skipped ones |
| `max_entry_size` | 1 GiB | Bytes in one file |
| `max_total_size` | 4 GiB | Bytes in all files |
| `max_ratio` | 100 | Largest `size / compressed` for ZIP entries over 1 MiB |
| `max_depth` | 32 | Components in a name, 1 to 62 |
| `max_name` | 4096 | Bytes in a name |
| `skip_links` | `False` | Skip symbolic and hard links instead of failing with `Link` |
| `skip_special` | `False` | Skip devices, FIFOs, and unknown types instead of failing with `Special` |

Sizes are checked against the declared size before any byte is written, and
a file that turns out longer or shorter than declared fails. A field out of
range is `InvalidInput`. The ratio check applies to ZIP entries only: a
compressed TAR stream is bounded by its `std.compress` decompressor's limit.

`Summary` reports `entries` (all entries), `files`, `bytes` (file content
written), and `skipped`.

## Errors

```text
pub enum ArchiveError:
    Stream(StreamError)
    Io(IoError)
    Fs(FsError)
    Allocation(AllocError)
    Corrupt
    Checksum
    Truncated
    Unsupported
    UnsafePath
    Link
    Special
    Duplicate
    Exists
    Limit
    Cancelled
    InvalidInput
```

| Variant | Meaning |
|---|---|
| `Stream(cause)` | `inner` or a source stream failed |
| `Io(cause)` | A directory operation failed |
| `Fs(cause)` | A `std.fs` failure with no closer variant |
| `Allocation(cause)` | A buffer, name, or list could not be allocated |
| `Corrupt` | Malformed archive structure |
| `Checksum` | A TAR header checksum or ZIP CRC-32 mismatch |
| `Truncated` | The input ended inside the archive |
| `Unsupported` | Encryption, a compression method other than stored or DEFLATE, or a spanned ZIP |
| `UnsafePath` | A name that could escape the destination |
| `Link` | A link entry, when `skip_links` is false |
| `Special` | A device, FIFO, or unknown entry, when `skip_special` is false |
| `Duplicate` | A name already extracted |
| `Exists` | `destination` already exists |
| `Limit` | A policy or format limit was exceeded |
| `Cancelled` | `cancel` was set |
| `InvalidInput` | A bad argument, policy, or destination, or a writer call after `finish` |

`stream_error` maps `Stream(cause)` to `cause`, `Truncated` to
`UnexpectedEof`, `Allocation(cause)` to `Allocation(cause)`, `Io(cause)` to
`Io(cause)`, and everything else to `Io(IoError.InvalidInput)`.

## Ownership and threads

Readers and writers own `inner` and their buffers; dropping one releases them
and does not flush or close `inner`. None can be copied. The module has no
global state, passes the ordinary worker-effect checks, and may be used from
any thread.

## Costs

| Value | Allocates |
|---|---|
| `TarReader`, `TarWriter` | One 512-byte block |
| `ZipReader` | Up to 64 KiB to find the end record, one `Entry` and record per member, a 16 KiB input buffer, and an `Inflater` (about 62 KiB) |
| `ZipReader.open` | A fresh `Inflater` for a DEFLATE entry |
| `ZipWriter` | The central directory, about 50 bytes plus the name per entry |
| `ZipWriter.add_file` | The compressed copy of `data` for `Method.Deflate` |
| Extraction | Each entry's name components, one directory handle per level, and an 8 KiB copy buffer per file |

Reading content allocates nothing; time is linear in the archive size.
A TAR archive is read once, front to back; a ZIP archive is read once for
the central directory and once per opened entry.

## Limits

- No link creation: links are refused or skipped.
- No restoring of modes, times, or owners.
- No appending to an existing archive or rewriting entries.
- No streaming ZIP writes: each `add_file` holds the whole entry in memory,
  and data descriptors (sizes after the data) are not written.
- No ZIP encryption, and no methods other than stored and DEFLATE.
- No sparse TAR files, and no TAR archives split across volumes.
- One target, x86-64 Linux GNU.
