# Finder feasibility experiment

The repository's `examples/finder/` attempts a small `fd`-style search tool using
the current language. Its README records the assessment, commands, validation,
and remaining limits. `main.plenty` and `matching.plenty` implement a working
filter for supplied paths, with literal basename search, `*`/`?` wildcards,
full-path matching, hidden-component filtering, and CLI errors.

```sh
cargo build --offline
target/debug/plenty --compile examples/finder/main.plenty -o /tmp/plenty-finder
printf 'src/main.plenty\nnotes.txt\n' | /tmp/plenty-finder --paths --glob '*.plenty'
python3 -B examples/finder/check.py -v
```

The program processes one UTF-8 path per line. It borrows strings while matching,
avoids allocating basename slices, and preserves concrete I/O errors through
`Result[i32, IoError]`. `?` in a glob matches one Unicode scalar. Matching is
case-sensitive; omitted patterns match all nonempty visible paths.

Without `--paths`, the finder searches `[PATTERN [ROOT]]`, defaulting to `.`.
It uses the embedded `std.fs` walker over native enumeration, prunes hidden
directories before descent, and never follows discovered directory links.
Directories, regular files, symlinks and other types are eligible output.
An explicit root can itself resolve through a link. Traversal errors stop with
the attempted path, operation and concrete IoError. Python supplies test fixtures
and an independent traversal oracle, not the application's candidate paths.
The capability probes now check working directory and metadata APIs.

Use `--null` for lossless Unix paths: `args_bytes()` preserves raw patterns,
`input_bytes(0u8)` reads NUL records, and `write_stdout_bytes` emits exact names
followed by NUL. CR, LF, and invalid UTF-8 are preserved. Matching borrows
`list[u8]`; in this mode `?` matches one byte, so UTF-8 `é` takes two `?` tokens.
The default text mode retains its Unicode-scalar and line semantics.

Issue #61's transport acceptance cases are covered by a real Unix fixture with
spaces, tabs, CR/LF, and invalid UTF-8, plus raw argument patterns and a byte
wildcard oracle. Native traversal preserves those names through Path, joining
and filesystem lookup. Broader compact byte buffers and portable path/library
APIs remain within issue #49.

See the current [standard stream and argument](../facilities/02-io/streams-args.md),
[binary transport](../facilities/02-io/bytes-paths.md),
[paths](../stdlib/path.md),
[directory enumeration](../stdlib/fs.md),
[walking](../stdlib/fs/walking.md),
[text](../facilities/01-builtins/text.md), and
[foreign text adapter](../facilities/04-interop/borrowing-text.md) contracts.
