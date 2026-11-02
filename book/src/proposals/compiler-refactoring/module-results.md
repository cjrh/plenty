# Replayable module-lowering results

[Parent proposal](../compiler-refactoring.md). No lowered-module cache is
implemented by this design. The
[module-cache investigation](../../development/module-cache-investigation.md)
is the evidence baseline; this page describes the boundary it requires.

## Evidence and limits

The recorded investigation at `00d32a13`, on an AMD Ryzen 7 7840HS with
seven repetitions and pinned physical cores, measured a synthetic ten-module
application importing `std.http.server`:

| Object-build mode | Median |
| --- | ---: |
| Cache disabled | 45.50 ms |
| Warm function cache | 44.54 ms |
| Unchanged program cache | 3.99 ms |
| Ordinary body edit | 44.00 ms |

These exclude native linking and are not current-main benchmark results.
The body-edit profile recorded parsing 8.35 ms, resolution 3.32 ms and program
lowering 19.28 ms; lower-function time is contained in lowering. Do not sum
nested rows or promise the sum as achievable savings.

A scalar separate-compilation spike was slower than the normal compiler.
There is no evidence that independently packaging current modules as native
libraries solves frontend reuse. Existing whole-program and compiled-function
caches remain the default.

## Relevant implementation boundaries

- `src/frontend/modules.rs` and its children load/resolve imports and produce
  `Resolved`; source identities and access maps are part of that environment.
- `src/frontend.rs::lower` consumes `Resolved` but mutates shared string,
  function and generic state while producing one `Program`.
- `src/frontend/generics.rs` and `functions.rs` own pending instances and
  body-derived facts.
- `src/frontend/references.rs` infers returned-field projections that callers
  use for borrow checking.
- `src/cache.rs`, `src/cache/program.rs`, `src/cache/compiled.rs` and
  `src/cache/executable.rs` cover existing cache layers; a module artifact
  must not silently reuse incompatible keys or change cache-off behavior.

## Proposed replay contract

A result needs explicit inputs, outputs and identity relocation, not simply a
serialized `Vec<Op>`.

| Part | Required information |
| --- | --- |
| Inputs | Canonical module identity/text, import resolution, compiler/settings identity and imported semantic dependencies |
| Declarations | Signatures, aliases, nominal layouts, visibility, protocols and generated methods |
| Definitions | Generic templates and instantiated bodies, including their dependency provenance |
| Derived interface | Returned-reference projections and any inferred capture/effect facts consumed by other bodies |
| Lowered content | Typed operations, referenced strings/types/functions and native substitutions |
| Attribution | Source identities, original declaration positions, instantiation origins and deterministic diagnostic order |

Replay must remap local string/type/function/source identities into the new
environment in a deterministic order. Reject unsupported combinations rather
than quietly sharing mutable tables between saved and replayed compilations.
Handle cyclic nominal graphs without recursive serialization loops.

First extract an in-memory result for a restricted, explicitly documented
module class. Replay it against a fresh imported environment and compare both
accepted and rejected programs with normal lowering. Keeping all original
global tables alive and returning pointers into them does not demonstrate
independent replay.

Conservative full imported-source digests are acceptable initially. Precise
interface invalidation is a separate refinement: a body can change semantics
that callers need without changing its written signature.

## Critical invalidation probe

The existing `experiments/module-cache/check_dependencies.py` changes an
imported borrow-returning function from lending `pair.x` to lending
`pair.y`, keeping `select(pair: &mut Pair) -> &mut i64` unchanged.
An unchanged caller mutating `pair.y` must change from accepted to rejected.

Retain this probe, generic-body edits, imported layout/visibility changes,
import-target replacement, missing/deleted files and native substitution changes.
Compare diagnostic attribution as well as executable output.

## Alternatives and adoption gate

A conservative lowered-import result is the primary experiment. Separately
compare lowering only reachable standard-library functions, checking whether
on-demand instantiation and diagnostics remain consistent. Streaming functions
can reduce peak memory but does not by itself create a module cache; mutually
dependent inference/effects must still complete before consumers need them.

Only add persistence after fresh-environment replay is correct and faster on
a real std-heavy application. Account for hashing, validation, decoding,
relocation and merge allocations. A large theoretical reusable frontend share
does not establish net benefit. Corrupt/version-mismatched artifacts must miss
safely; invalidation must not suppress errors. No remote cache follows without
a separately agreed trust contract.
