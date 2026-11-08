# Formatter implementation and validation

> Companion to the [unimplemented `plenty fmt` proposal](../plenty-fmt.md).
> This is an architecture recommendation, not an implementation result or a
> separate work queue. Future scope is tracked under B17 in the [backlog](../../backlog.md).

## Keep source information before laying it out

The current compiler is not a source-preserving printer. In
`src/frontend/lexer.rs`, comments are skipped, literal values are decoded,
and `Token` records a starting line and column rather than a complete byte
range. `src/frontend/parser.rs` drops layout inside delimiters and builds the
lowering-oriented tree in `src/frontend/syntax.rs`. Reprinting that tree alone
would lose information a formatter must preserve.

Add an optional source-preservation mode to the existing lexical machinery:
record byte ranges for raw tokens, comments, whitespace and physical newlines,
while retaining the compiler's ordinary significant-token stream. Keep literal
recognition shared; do not build a second string/f-string scanner. Store the
extra ranges in a formatting sidecar so normal compilation does not retain an
entire concrete syntax tree or pay a per-node memory cost without evidence.

Expose a parse-only path that accepts a source module, interface, or script
body without resolving imports or lowering types. Record just the syntax facts
needed for layout against token ranges: suite boundaries, delimiter groups,
unary/binary operator roles, declaration boundaries, and comment attachment
points. Some parser productions live beside their feature implementation, so
this boundary needs an audit; do not assume the existing parser is already a
standalone public API. Avoid a second independent Plenty grammar.

A full lossless concrete syntax tree is an alternative if the sidecar becomes
more complicated than the tree it avoids. Do not start a compiler-wide tree
replacement merely to implement formatting. Prototype source capture and
comment attachment on representative syntax before committing to that redesign.

## A small printer with strict limits

Use a small document representation: source slice, space, hard line, soft line,
indentation, concatenation, and a group that chooses flat or broken layout.
Build groups only where existing delimiters permit continuation. Significant
newlines and suite boundaries stay significant. Preserve punctuation, so the
renderer does not need an expression-rewriting or precedence engine.

A bounded lookahead determines whether a group fits; stop measuring once the
remaining width is exceeded. Avoid enumerating combinations of break points
or repeatedly measuring arbitrarily large subtrees. Bound nesting and work,
using the parser's existing nesting limit where applicable, and fail cleanly
rather than exhausting the stack. Start single-threaded without a persistent
cache. Measure real source trees before adding concurrency or incremental work.

Attach each comment exactly once using source ranges. Keep an end-of-line
comment after the same code token on the same logical line; force a break
after it. Keep a standalone comment at the same boundary between significant
tokens and at its original suite depth. A comment inside a delimiter can force
that group to remain broken. Do not collapse separated comments onto one line,
move comments across statements, or infer prose wrapping.

For the first version, leave suppression directives as ordinary comments and
document that no `fmt: off/on/skip` behavior exists. These directives need
precise statement and indentation semantics to be safe; silently copying
Ruff's names without that contract would create a false escape hatch. The
conservative literal and punctuation policy already preserves many hand-tuned
spellings, but aligned tables will still be reformatted. Suppressions require a
separate explicit design decision if that limitation blocks adoption.

## Safety checks before replacement

For each candidate result, re-lex and re-parse before exposing output or
replacing a file. Require the same ordered raw non-layout token spellings,
the same sequence of significant newline/indent/dedent tokens (compare suite
structure rather than indentation widths), and the same syntax acceptance.
Only whitespace and permitted physical newlines may differ. These checks are
possible because the initial style never adds or removes punctuation.

Also compare comment text, ordering, suite attachment, and adjacent significant
token anchors; verify whether each comment is standalone or trailing. Verify
that script headers and opaque literal spans are identical. Re-parsing alone
cannot establish these properties. The grammar's layout token comparison must
cover comment-only lines without treating them as statements.

If a check fails, report a formatter error and retain the original. Internal
errors must not produce partial source on stdout or an LSP edit. Test
idempotence separately across the corpus; a production second formatting pass
is not necessary once layout stability is established.

Stage each changed file in its directory, preserve permissions, and replace it
only after validation and a check that the original still matches the bytes
read. Use the project's platform-safe replacement facilities; fail if the
platform cannot preserve that contract. Detect intervening changes before
replacement and report them. This is best-effort conflict detection, not a
claim to an atomic compare-and-swap against an uncooperative concurrent writer.
Do not truncate the destination in place or replace symlinks. Unchanged files
keep their timestamps. Test error cleanup and script executable permissions.

## Evidence required for adoption

Snapshot tests should exercise both attractive ordinary output and difficult
syntax: nested declarations, decorators, generics and value arguments,
protocols, borrows, slices, patterns, closures, operators with different roles,
fallible expressions, and interface declarations. Cover comments before/after
operators and delimiters, empty/comment-only files, docstrings, Unicode,
multiline bytes/strings, debug f-strings, mixed line endings, and script headers.

Property tests and fuzzing should require no panic, bounded work, unchanged
significant tokens and comment anchors, parseable output, and idempotence.
Deliberately malformed and deeply nested source must fail without writing.
Keep regression cases for every discovered preservation failure.

Run the formatter over the standard library, examples, and complete runnable
book fixtures. Confirm token/layout equivalence and idempotence for all accepted
inputs; then run their existing compiler and executable tests. These complement
the structural checks rather than replacing them. Review a representative diff
for idiomatic layout. Do not silently rewrite the repository in the feature PR;
separate the tool's acceptance from an explicitly reviewed formatting rollout.

CLI tests cover traversal, deduplication, exclusions, stdin purity, exit statuses,
invalid UTF-8, errors mixed with valid files, permissions, and failed/concurrent
writes. LSP tests compare CLI output with unsaved-buffer formatting, including
UTF-8/16/32 positions, unchanged documents, parse errors, and stale versions.
Use Neovim's existing headless integration to exercise format-on-save.

Report wall time and peak memory on representative modules and a whole source
tree; do not promise Ruff's performance figures for Plenty. A prototype should
show whether source preservation and syntax annotations stay small. The largest
cost is expected to be grammar coverage and comment correctness, not the CLI
or the flat/broken layout choice. There is no evidence yet for a reliable
line count or delivery estimate.

## Why this scope

A regex formatter is smaller to start but cannot reliably distinguish strings,
comments, unary operators, and meaningful indentation. An unconstrained token
spacer misses the syntax needed for those decisions. A lowering-tree printer
loses source details. Reusing the compiler's recognizers with raw ranges and
small syntax annotations addresses those failures without adopting all of
Ruff's transformations.

The tradeoff is visible: long expressions without suitable delimiters stay
long, quotes and redundant parentheses persist, and formatting exclusions are
limited. If those costs prove unacceptable, extend the contract deliberately
with a normalized syntax comparison before allowing punctuation changes.
Do not weaken the preservation checks to make a prettier snapshot pass.
