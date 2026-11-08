# An integrated source formatter: `plenty fmt`

> **Status: Not implemented.** This proposes a formatter and its editor adapter.
> The [language server](../design/toolchain/01-building/language-server.md)
> describes current editor support; it has no formatting request today.
> Future scope belongs to **B17** in the [backlog](../backlog.md).

Provide `plenty fmt` in the existing toolchain, with one predictable style,
no setup for ordinary use, and the same output from the CLI and editor.
Use Astral's Ruff as the principal example for the experience. Prefer a
conservative implementation whose safety properties are easy to test over
reproducing every transformation a mature Python formatter performs.

## What to borrow from Ruff

[Ruff's formatter](https://docs.astral.sh/ruff/formatter/) combines an opinionated
style, in-place formatting, a read-only check mode, and few style options.
Its [launch account](https://astral.sh/blog/the-ruff-formatter) describes sharing
lexer/parser infrastructure, adapting Biome's printer, and extensive edge-case
testing. This is substantial language tooling, not a whitespace substitution.
Its Black-compatibility work also solves a problem Plenty does not have.

Borrow the integrated experience and disciplined testing. Do not run Ruff on
Plenty: similar syntax does not make Python's grammar appropriate for Plenty's
ownership annotations, fallible expressions, declarations, or type syntax.
Do not initially port Ruff/Biome's printer, compatibility rules, caching,
configuration discovery, or lint framework. A small local layout engine is the
recommended starting point; reconsider a dependency only if a prototype shows
that it materially reduces the code and maintenance burden.

Ruff also deliberately keeps trailing comments near their original code instead
of collapsing them together; see its [comment handling rationale](https://docs.astral.sh/ruff/formatter/black/).
Adopt that principle. Comments are part of the source's meaning to its reader.

## Command contract

```text
plenty fmt
plenty fmt src tests
plenty fmt example.plenty interface.pi
plenty fmt --check
plenty fmt --diff src
plenty fmt -
plenty fmt --stdin-filename src/main.plenty -
```

With no paths, recursively format the current directory. Explicit files and
directories work outside a package; formatting needs neither `plenty.toml` nor
resolved imports. Recognize `.plenty`, `.p`, `.plentyi`, and `.pi`, including
standard-library declaration syntax in interface files. Directory traversal
skips hidden directories, `target`, and `.git`, does not follow symlinks, and
visits paths in sorted order. Deduplicate overlapping arguments. Explicit
regular files bypass directory exclusions; reject symlink arguments. No Git
index, ignore-file engine, package cache scan, or dependency fetch is involved.
Repository integrations can pass selected paths if they need different discovery.

Default operation writes changed files. `--check` writes nothing and reports
paths that would change. `--diff` prints unified diffs without writing; it is
mutually exclusive with `--check`. Both return 1 when changes are needed.
Return 0 for successful formatting or an already formatted check, and 2 for
invalid arguments, syntax errors, read/write failures, or failed safety checks.
An error takes precedence over a formatting difference in the aggregate status.
Continue checking independent files and report all errors; successfully formatted
files may already have been written. There is no whole-directory transaction.

A single `-` reads UTF-8 source from stdin and writes only formatted source to
stdout. Do not combine stdin with path arguments, `--check`, or `--diff` in the
first version. `--stdin-filename` supplies a diagnostic name and source kind;
it does not open that file or resolve its dependencies. Without it, treat stdin
as an ordinary source module. Send diagnostics and summaries to stderr.

A parse error leaves that file untouched. Type errors, unresolved names and
missing dependencies do not prevent formatting syntactically valid source.
Do not repair invalid syntax, invoke code generation, execute source, update a
lockfile, or require a successful ownership check.

## A deliberately small style

Use four spaces for each suite level, conventional spaces around binary
operators and after commas, and no spaces just inside delimiters. Distinguish
unary operators, borrows, generic arguments, slices, and type unions through
syntax rather than text heuristics. Match the language's accepted syntax.

Use an 88-column target, following Ruff's default. Treat it as a best-effort
limit. Break comma-separated groups inside existing delimiters when they do
not fit; indent continuations by four spaces and place the closing delimiter
at the group's base indentation. Retain necessary hard breaks around comments.
Do not introduce parentheses to wrap a long expression or invent a continuation
syntax. A long unbreakable expression is acceptable in the first version.
Count Unicode scalar values consistently for the initial width calculation;
terminal display-cell width can differ.

Preserve every non-layout token's exact spelling and order: names, operators,
parentheses, trailing commas, numeric bases and separators, strings, bytes,
f-strings, and docstrings. Do not sort imports, simplify expressions, normalize
quotes, rewrite escapes, or change f-string fields. In particular, reformatting
a debug `=` field can change its displayed source text. Literal tokens remain
opaque, including their internal newlines and indentation.

Existing trailing commas remain, but do not acquire a separate rule that forces
expansion; no commas are added or removed. This means equivalent inputs with
different literal spellings or redundant parentheses need not converge. The
formatter promises deterministic, idempotent layout, not one spelling for every
equivalent program. This is the principal simplification from Ruff's scope.

Remove trailing whitespace outside protected text and normalize ordinary code
line endings to LF. End a nonempty ordinary module with one newline; keep an
empty file empty. Preserve literal bytes, including embedded CRLF. Use one blank
line between top-level declarations, at most one blank line within a suite,
and retain comment-only groups with their associated code. Never reflow comment
text or insert blank lines between decorators and their declaration.

Preserve a script's shebang and embedded manifest byte-for-byte, using the
existing [script header contract](completed/single-file-scripts.md).
Do not interpret header text as ordinary Plenty syntax. Preserve its boundary
newlines as well as its contents.

No style configuration is proposed initially: no quote, indent, width, or
per-directory overrides. Expose choices only after concrete use demonstrates
that the fixed defaults prevent adoption. Formatter style changes must be
called out in compiler release notes; pin the toolchain in CI for stable output.

## Editors and implementation

Formatting belongs to a shared library entry point. `plenty fmt` and
`plenty lsp` are adapters around it. Provide whole-document formatting over the
current unsaved buffer through `textDocument/formatting`; advertise it only
when the core is ready. Return no edits for an unchanged document, otherwise
a single whole-document edit using the negotiated position encoding. Never
write the file from the language server. Reject invalid syntax without edits
and honor cancellation and document versions so stale results are discarded.

The fixed style overrides client indentation preferences; document that choice
in the editor setup. Range formatting, on-type indentation, import organization,
and formatting embedded Markdown/docstring examples are outside the initial
contract. See the [implementation and validation design](plenty-fmt/implementation.md)
for source preservation, alternatives, and acceptance criteria.
