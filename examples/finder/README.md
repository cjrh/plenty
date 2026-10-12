# Finder: a realistic Plenty experiment

Plenty now searches a directory root itself through `std.fs` and `std.path`.
The finder keeps matching, option parsing, hidden-directory pruning and traversal
policy in Plenty. Its `--paths` mode still filters paths supplied on stdin.
`--null` preserves arbitrary Linux filenames and raw patterns.

## Run it

```sh
cargo build --locked --offline
target/debug/plenty --compile examples/finder/main.plenty -o /tmp/plenty-finder
/tmp/plenty-finder --glob '*.plenty' .
/tmp/plenty-finder --null --glob '*.plenty' .
printf 'src/main.plenty\nnotes.txt\n' | /tmp/plenty-finder --paths --glob '*.plenty'
python3 -B examples/finder/check.py -v
```

Compile a native executable before supplying application arguments; the
compiler's compile-and-run command does not forward arbitrary finder flags.
Pass `--plenty PATH` to the checks to use another compiler.

## Interface

| Surface | Behavior |
| --- | --- |
| `[PATTERN [ROOT]]` | Search ROOT, default `.`; omitted pattern matches everything |
| `--paths [PATTERN]` | Filter stdin instead of accessing the filesystem |
| `--null` | NUL-framed raw output; raw input too with `--paths`; globs match bytes |
| `--glob` | Whole-basename `*`/`?` matching instead of literal substring search |
| `--full-path` | Match the full path; globs may cross separators |
| `--hidden` | Include dot-prefixed components; otherwise prune hidden directories before descent |
| `--` | End option parsing |
| `--help`, `-h` | Show usage |
| Exit status | 0 for success/help, even without matches; 2 for usage; 1 for I/O/traversal failure |

Traversal excludes the supplied root from output, emits directories and all
non-directory entries, and never follows discovered directory links. Dangling
links remain discoverable. An explicitly supplied root can itself be a symlink.
Hidden filtering applies to discovered names, not to an explicitly supplied
root's spelling. Native sibling order is unspecified. Root spelling is preserved; `.` produces
`./name`, while absolute roots produce absolute paths. No snapshot or sorting
is promised. Traversal failure stops with its attempted path, operation and
concrete IoError visible on stderr. Diagnostic paths use explicit byte escaping.

Text matching is case-sensitive and `?` means one Unicode scalar; raw matching
uses bytes, so UTF-8 `é` needs two `?` tokens. Brackets, backslashes and other
characters are literal. There are no regexes, character classes, special `**`,
normalization, case folding, ignore files or follow-links flag.

Text output uses lines and cannot safely frame names containing LF. Invalid UTF-8
causes a checked conversion error. Choose `--null` for arbitrary filenames;
the exact output bytes are separated only by NUL. In `--paths` text mode, stdin
accepts LF/CRLF and an unterminated final record, skips empty records, and
preserves spaces, input order and duplicates. Supplied paths need not exist.

## Ownership and limits

The native directory stream yields one detached entry at a time and closes on
EOF, errors or drop. Entries contain their own path bytes and cached native type,
and do not retain directory handles. The Plenty walker collects one directory's
entries for pruning, keeps pending sibling paths along the active depth, and
uses an explicit list of frames rather than recursive calls. It has at most one
native directory stream open while scanning, and none between walker advances.
Bottom-up walking is also available in the library.

Native operations currently require 64-bit Linux; metadata requires libc/kernel
statx support. The runtime adapts Rust `read_dir`/`DirEntry` and Python `scandir`
semantics to fallible buffers and Plenty ownership. The walker follows Python's
top-down/bottom-up/pruning model and Path.walk's non-followed link grouping.
Paths preserve Linux bytes and expose checked text conversion separately from
escaped display. General `list[u8]` storage remains larger than a compact buffer.
Broader buffers and library facilities remain within issues #49/#61; optional
defaults and iterator conveniences remain in issue #35.

`check.py` runs twelve checks, including native-root traversal with ancestor
links, hidden pruning, raw filenames, CLI errors, and both formerly failing
capability probes. Python creates fixtures and independently computes expected
results; it supplies no paths to the native traversal invocation. The fixture
also agrees with installed `fd --glob --no-ignore`. The matcher checks cover
10,581 Unicode-scalar and 10,576 byte wildcard cases against independent oracles.
Compiler/runtime regressions additionally cover file/link metadata, special
types, permission denial, pruning, bottom-up order, subtree recovery, descriptor
lifetime, allocation failure, unknown-type fallback, and flat/deep trees.

The wildcard matcher may retry suffixes and is not guaranteed linear-time.
Non-ASCII text indexing can rescan scalar boundaries. Flat/deep regression
fixtures verify streaming and handle bounds, not fd-level throughput or memory
efficiency; there is no claim of a production fd replacement.

A local smoke measurement on 2026-10-12 used three native-executable runs per
fixture, an unmatched raw pattern, and `--null --hidden`. These include scanning
and matching; setup and compilation are excluded. RSS is the largest process
peak reported by `/usr/bin/time` across those runs.

| Fixture | Median elapsed | Peak RSS |
| --- | --- | --- |
| 10,000 files in one directory | 0.0659 s | 4,880 KiB |
| 256 nested child directories | 0.0128 s | 3,228 KiB |

These local fixture numbers do not establish a worst-case memory bound or a
comparison with fd. The walker intentionally allocates per-directory lists.
