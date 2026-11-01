---
name: std-conventions
description: For maintenance of the Plenty compiler repository. Check a proposed or changed standard library API (a new `std` module, a new public function or method, or a PR under review) against the written std conventions for naming, errors, time and cancellation, limits, byte types, streams, formatting, integer types and argument ownership. Use when designing or reviewing `std` signatures. Reports conformance and deviations; it does not change the conventions.
---

# Check std APIs against the conventions

The rules live in `book/src/design/stdlib/conventions.md`. Read it first;
this skill only says how to apply it. Do not restate rules from memory.

## 1. Collect the signatures

- New or changed code: `git diff main -- stdlib/std` in the task worktree,
  keeping only `pub` declarations.
- Existing module: its section of `book/src/design/stdlib/signatures.md`; the
  `## Modules` list gives each section's line range.

The index is generated. After changing `pub` declarations, regenerate it with
`UPDATE_STDLIB_INDEX=1 cargo test --locked --test integration test_stdlib_index::`
and review its diff too.

## 2. Check each section of the page

Go through the page's sections in order and note, for each signature, the
rule it meets or breaks. Compare with siblings before deciding a name is
new: an operation that already exists elsewhere in `std` uses the same name,
parameter order and error shape.

Mechanical checks (run from the worktree root; `S=book/src/design/stdlib/signatures.md`,
or point them at the changed `.plenty` files):

```sh
rg -n 'timeout_ms|_ms:|milliseconds: i64|seconds: i64|leeway' $S        # raw time units
rg -n 'deadline: &Deadline' $S | rg -v 'CancellationToken'               # deadline without a token
rg -n 'bounds: &' $S                                                     # limits parameter spelling
rg -n '\| AllocError\]|AllocError \|' $S                                 # AllocError unions
rg -n 'list\[u8\]' $S                                                    # byte data that should be Bytes
rg -n 'data: &Bytes|source: &Bytes' $S                                   # read-only input that could be &[u8]
rg -n 'def (format|to_string|display)\(' $S                              # formatting names
rg -n 'def [a-z_]+\(self\) -> bool' $S | rg -v 'def (is|has)_'           # predicates without is_/has_
rg -n 'def (len|count|capacity)\(self\) -> u64' $S                       # sizes not i64
rg -n ': bool[,)]' $S                                                    # required positional booleans
rg -n 'def close\(' $S                                                   # close return types
rg -n 'def (watch|register)\(' $S                                        # readiness shapes
```

A hit is a candidate, not a finding: check it against the page, including
its listed exceptions.

## 3. Check what the index cannot show

- Ownership on failure: an operation that consumes a value and can fail
  before using it returns the value in its error.
- Invalid run-time arguments produce a named `Err` variant, never a clamp or
  silent no-op.
- Every input-driven allocation has a cap, and allocation failure reaches the
  caller.
- The module page documents ownership, allocation, errors and limits for each
  new function, and a wrapped native library is justified (see `AGENTS.md`).
- Nothing new is a `pub` hook for another std module.

## 4. Report

List deviations as `signature: rule broken, suggested signature`. Separate
deviations the change introduces from existing ones it only touches. An
existing deviation without a tracking issue (the page's table says "Not yet
tracked") may be worth an issue; search `issues.db` first as `CLAUDE.md`
describes. If a convention itself looks wrong, say so; changing it is an edit
to the page, reviewed like any reference change.
