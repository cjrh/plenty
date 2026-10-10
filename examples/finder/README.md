# Finder: a realistic Plenty experiment

**Result:** Plenty can implement the search/matching core, but its current
shipped APIs cannot implement a standalone directory finder. Directory
enumeration and file-type metadata are missing. This example preserves the
working core, the failed capability probes, and a repeatable assessment.
It adds no compiler/runtime features and is not a replacement for `fd`.

## Run it

From the repository root, with Rust dependencies available locally:

```sh
cargo build --offline
target/debug/plenty --compile examples/finder/main.plenty -o /tmp/plenty-finder
printf 'src/main.plenty\nsrc/notes.txt\ndocs/guide.plenty\n' | /tmp/plenty-finder --paths --glob '*.plenty'
python3 -B examples/finder/check.py -v
```

Expected finder output:

```text
src/main.plenty
docs/guide.plenty
```

Pass `--plenty PATH` to `check.py` to use another built compiler. Compile to a
native executable before supplying application arguments: the compiler's
compile-and-run command does not forward arbitrary finder flags.

You can try it on actual paths using an **external** enumerator:

```sh
find . -print | /tmp/plenty-finder --paths --glob '*.plenty'
```

Here `find` discovers paths; Plenty only filters them. This pipeline is suitable
only for names compatible with the line protocol below. Invoking the finder
without `--paths` exits 2 and explains the traversal blocker. It does not silently
accept a root argument and pretend to search it.

## Implemented behavior

| Surface | Behavior |
| --- | --- |
| `--paths [PATTERN]` | Read paths from stdin; match a literal, case-sensitive substring in each basename |
| `--glob` | Match the whole basename: `*` means zero or more Unicode scalars; `?` means exactly one |
| `--full-path` | Match the entire supplied path, including `/`; in glob mode wildcards may cross separators |
| `--hidden` | Include dot-prefixed components, otherwise skipped; structural `.` and `..` components are allowed |
| `--` | End option parsing, allowing a pattern beginning with `-` |
| `--help`, `-h` | Explain the experimental interface and traversal limitation |
| Omitted pattern | Match all nonempty supplied paths, including with `--glob` |
| Exit status | 0 for successful processing/help, including no matches; 2 for usage/traversal errors; 1 for an I/O error |

In glob mode, brackets, backslashes, and other characters are literal. There are
no regexes, character classes, escape syntax, special `**`, normalization, or
case folding. `?` matches a Unicode scalar, not a grapheme: `é` matches one `?`,
while decomposed `e` plus a combining accent requires two.

Input is one UTF-8 path per LF/CRLF line, with empty records skipped and a final
unterminated record accepted. Spaces are preserved. Records should have no
trailing `/`. Supplied paths need not exist; file types, symlinks, permissions,
ignore files, and recursion are the producer's responsibility. Results retain
input spelling/order and duplicates. No sorting or filesystem operation occurs.

This framing cannot safely carry arbitrary Unix names: embedded LF splits a
name, CR before LF is stripped, invalid UTF-8 is rejected, and there is no
NUL-delimited mode. Do not treat it as a lossless filesystem protocol.

## What worked and what was awkward

The algorithm and CLI are entirely Plenty: arguments, option parsing, borrowed
strings, literal search, a greedy wildcard matcher with retries, hidden-component
filtering, incremental stdin consumption, stdout flushing, and typed error
propagation. `main() -> Result[i32, IoError]` preserves the actual I/O cause and
supports distinct usage status without a new process-exit API. The matcher uses
no recursion and allocates no basename slices, token arrays, or search trees.
Memory grows with arguments and the current input path, not all candidate paths.

Explicit borrowing was manageable here; no ownership workaround or `unwrap()`
was needed. Fallible I/O composes cleanly with `?`. The absence of
`Option.unwrap_or` required small explicit matches, already tracked by issue #35.
A supplied relative path can be processed with strings, though a dedicated path
API would avoid reimplementing separator and component rules in each tool.

The main obstacles are library/runtime capabilities:

| Gap | Evidence and consequence | Local issue |
| --- | --- | --- |
| Directory entries and metadata | The preserved `read_dir` and `metadata` probes are rejected; no equivalent operation was found in the implemented I/O surface. There is nothing for a Plenty walker to enumerate or classify. | #60 |
| Lossless OS paths and binary streams | Both stdin and `args()` reject invalid UTF-8; line framing loses legal Unix filename characters. A real finder needs a path/bytes representation and NUL framing. | #61 |
| Convenient optional-value defaults | `unwrap_or` is rejected; explicit matches work. | #35 |
| Buffered/reusable I/O and iterator/view facilities | The current stdin backend reads without read-ahead; indexed non-ASCII characters scan scalar boundaries. These costs matter for larger workloads. | Existing B12 scope and issue #49 |

Issues #60 and #61 are concrete motivating gaps within issue #49's broader
standard-library catalogue. They remain open; this example does not fix them.
The reference's current text/interop contracts still apply.

I also checked the C interoperability surface. It can pass text **into** C and
hold opaque pointers, but cannot directly access C records such as `dirent`,
decode a returned C string, or receive mutable byte buffers. A trusted native
bridge could provide the missing OS operations, but would require new supporting
code and a safe entry-name transport. Existing FFI declarations alone do not
provide an ordinary safe `readdir` binding. See the reference's
`facilities/04-interop/trusted-declarations.md` and `borrowing-text.md` contracts.

## Validation and performance limits

`check.py` compiles both the application and a matcher driver, then runs nine
checks: CLI statuses/options, literal/basename/full-path matching, wildcards,
hidden components, line boundaries, UTF-8 failures in input and arguments,
a real directory fixture, and the two expected capability failures.
It compares **10,581** wildcard cases against an independent dynamic-programming
oracle, including empty strings, retries, Unicode, and long names. When `fd` is
installed, the fixture's `*.plenty` result also agrees with `fd --glob --no-ignore`.
Python supplies the test fixture and candidate enumeration, never the matching.
The capability checks should be revised when traversal support lands.

A local smoke measurement on 2026-10-10, using a compiler built from main
`08c73bc`, sent 10,000 repeated paths to the standalone executable, captured all
output, and took the median of three runs:

| Input record | Query | Median elapsed |
| --- | --- | --- |
| `src/module.plenty` | Literal `plenty` | 0.094 s |
| `src/module.plenty` | Glob `*.plenty` | 0.107 s |
| `src/é界🦀.plenty` | Glob `*.plenty` | 0.117 s |

These are short-path filter measurements, not an `fd` speed comparison or
filesystem benchmark. They do not establish memory accounting or large-tree
performance. The wildcard algorithm may retry a suffix many times; it is not
guaranteed linear-time. Non-ASCII scalar indexing can repeatedly rescan the text,
making long Unicode names more expensive. Buffered input and an efficient
character cursor/view are useful next capabilities; regex support is unnecessary
for this experiment.
