# Explicit applications, modules, and executable lessons

Status: the explicit binary entrypoint is implemented: parameterless `main`
returns `()` or `i32`. Absolute imports, aliases, `pub` visibility, and library
checking are also implemented. Module scope contains declarations and imports.
Multi-file Markdown examples are executable; generated lessons remain proposed.
See [DESIGN.md](../../DESIGN.md) for the implemented contract.

## Current behavior and the proposed entry point

At the research baseline (`b690afe`), `frontend::compile` collected top-level
statements into `__plenty_entry`; their bindings were startup locals rather
than globals. The implementation now calls the declared `main` instead. The
native runtime still supplies the platform entry point separately.

Recommend requiring an entry-module function for every binary:

```text
def main() -> ():
    print("Hello, Plenty!")
```

Start with zero parameters and return `()` or `i32`. Unit means successful exit;
`i32` supplies the process status, subject to the host's exit-status conventions.
`main` need not be `pub`, and imported functions named `main` are ordinary
functions. A library has no entry-point requirement. Checking a library module
must not produce a missing-main error; checking/building a binary must.

Once error propagation and allocation-free error reporting are ready, consider
`main() -> Result[(), E]`. Do not silently add an implicit formatting/protocol
requirement on arbitrary `E` now. Applications can initially use a small `i32`
main that explicitly matches a fallible application function's result.

Allow only declarations and imports at module scope initially. Defer module-level
constants until their permitted constant expressions are specified. Reject
executable statements with a diagnostic suggesting moving them into `main`.
There should be one application rule for compile-and-run and binary output, not
an implicit script fallback when `main` is absent. This is an intentional breaking
change on the language-development branch, accompanied by migrated examples.

Python makes top-level execution meaningful both for scripts and imports, and
documents the `__name__ == '__main__'` convention. Plenty can adopt familiar
spelling without importing executable-module initialization semantics.
See [Python's entry-module documentation](https://docs.python.org/3/library/__main__.html).

## Imports name source modules; they do not execute them

Proposed source layout, with `src` selected explicitly as the module root:

```text
src/
    app/
        main.plenty
        geometry.plenty
        storage/
            files.plenty
```

Use absolute paths and familiar name binding:

```text
import app.geometry
import app.storage.files as files
from app.geometry import Point
from app.geometry import distance as measure
```

`import app.geometry` makes `app.geometry` reachable. An alias binds exactly the
alias; a `from` import binds the requested declaration. Importing a module does
not inject its members into the caller's unqualified scope. A file's symbols must
come from its declarations, lexical bindings, documented built-ins, or explicit
imports. Importing a protocol must never silently add methods to unrelated types.

Directories initially serve only as namespaces, without executable `__init__`
files or implicit package initialization. Resolve `a.b` to `<root>/a/b.plenty`;
reject ambiguous file/directory module collisions. No relative imports, wildcard
imports, import hooks, conditional imports, or automatic transitive re-exports.
Importing `app.geometry` must not grant access to a sibling module simply because
some other file imported that sibling.

Python separates finding a module from binding its name and has extensive dynamic
loading rules. We need its visible syntax, not that entire runtime system.
See [Python import statements](https://docs.python.org/3/reference/simple_stmts.html#import)
and [the import system](https://docs.python.org/3/reference/import.html).

Initially use one source root: an explicit build/CLI option, defaulting to the
entry file's directory for standalone programs. A nested entry file that imports
`app.geometry` therefore needs the root above `app`, not an implicit search through
parent directories. Resolve against the configured root, never ambient Python
paths or arbitrary environment search paths. A project manifest can eventually
make the root and dependency mapping durable; no package manager is necessary yet.

Load each reachable module once by canonical module identity; include paths in
all diagnostics. Initially reject import cycles with the actual cycle path.
Collect public declarations before checking bodies in dependency order. Later,
declaration cycles may be supported independently of recursive value layouts.

## Private by default, with explicit public API

Use `pub` for top-level functions, types, class fields, and methods:

```text
pub class Point:
    pub x: f64
    pub y: f64

    pub def length_squared(self) -> f64:
        self.x * self.x + self.y * self.y
```

Recommend module privacy initially: private names and members are accessible
throughout their defining module, including sibling helper functions, but not
from other modules. Directory ancestry grants no extra access. This borrows
Rust's explicit `pub` vocabulary with a smaller privacy model; Rust itself has
additional descendant and restricted-visibility rules.
See [Rust's visibility reference](https://doc.rust-lang.org/reference/visibility-and-privacy.html).

Rules needed alongside parsing the keyword:

- A public class does not automatically expose its fields or ordinary methods.
- Public signatures cannot leak private nominal types; transparent aliases must
  not bypass this check. Resolve actual type identity before enforcing visibility.
- A public enum exposes its variants as part of its exhaustively matchable API
  in the first version. Hiding variants requires a separate non-exhaustive design.
- A generated field constructor is externally callable only when the class and
  all fields are public. Otherwise require a public `__init__` or public factory
  to establish invariants. An explicit `__init__` has its own visibility.
- `__del__` remains a compiler-invoked lifecycle hook, never a public callable
  destructor. Access restrictions must not prevent automatic cleanup.
- Borrowing private fields, generated structural printing/equality, and generic
  protocol checks must respect encapsulation. Initially keep compiler-generated
  printing/equality behavior documented as structural; do not mistake privacy
  for data secrecy. Decide whether opaque public classes should later opt out.
- Imports are private bindings. Defer `pub import` re-exports rather than letting
  ordinary imports accidentally widen a module's API.
- `pub` controls Plenty visibility, not C ABI exports. Foreign exports require
  their own explicit declaration and ABI validation.

The main implementation work is resolved symbol identity and access checking,
not accepting a new keyword. Give every nominal type/function a `ModuleId` plus
local identity instead of using an unqualified string as global identity. Local
aliases preserve identity; two unrelated modules may each define `Point`.

## Compiler changes that keep later features possible

Introduce a path-aware compilation session owning sources, module IDs, an import
graph, public signature tables, and diagnostics. Keep source-string APIs for
isolated checks/tests, with explicit virtual-module identity and an optional
resolver; do not make them silently read unrelated filesystem modules.

Resolve imported type/member syntax before lowering runtime expressions. A
qualified symbol path and a field access may look similar but are not the same
operation. Likewise, importing a module is not loading a DLL; both may be needed
to use a foreign library, and should remain separate concepts.

Initially lower the reachable source graph into one object using the existing
pipeline. Stable internal symbol IDs should allow later per-module object caching
and generic instantiation caches without promising separate compilation now.
Public signatures include ownership, borrowing, and future fallibility contracts;
callers must not inspect callee bodies to infer them. Module visibility checks
must run before both ordinary and generic calls are emitted.

## The tutorial is already executable, but its source can improve

At this snapshot, `TUTORIAL.md` contains 43 successful programs and 11 expected
compile errors. `tests/test_tutorial.rs` extracts them directly. It invokes both
compile-and-run and explicit AOT compilation; successful binaries must produce
the documented output, and invalid programs must produce the expected diagnostic
without effects. Prose is not semantically checked. The Markdown is currently the
source of truth; the lack of standalone files is an authoring/reuse limitation.

The proposed literate-source approach is workable. Recommend one independently
compilable `.plenty` file per example, with ordered lesson metadata and optional
named display regions:

```text
#| ## Choose the width of an integer
#|
#| Suffixes make the width explicit.
# docs:begin example
def main() -> ():
    count: u8 = 42u8
    print(count)
# docs:end example
```

The `#|` lines become Markdown prose; region markers remain inert comments to
the compiler. Use a small manifest to order files and identify run-pass versus
compile-fail examples, expected stdout/diagnostics, fixture stdin, and any module
root. A chapter may comprise multiple example files. Prose-only sections can
also be listed. Do not concatenate independent programs into one execution unit.

The first lesson shows the full required `main`. Later lessons may show named
regions, with a link to the complete runnable source and an explicit statement
that the displayed region is an excerpt. An example with hidden setup is no
longer honestly described as independently copyable from the rendered snippet.
Negative examples are separately labeled intentionally invalid source files.

Proposed build flow:

1. Parse the manifest, prose, and regions; reject duplicate IDs, unmatched
   markers, missing files, invalid paths, and orphan expectations.
2. Compile/run each complete source with isolated temporary outputs. Validate
   expected stdout, exit status, and stderr/diagnostics. Use bounded execution
   time and deterministic fixtures so an accidental loop cannot hang CI.
3. Render validated actual outputs and code regions into `TUTORIAL.md`. Preserve
   source locations in failure reports and links to complete examples.
4. A `--check` mode renders to memory and fails on differences from the checked-in
   tutorial. A generation command writes it deliberately. CI never updates it.

Keep reviewed expectations, not only whatever output happened to be generated.
Otherwise a compiler regression would rewrite the book and appear successful.
Snapshot acceptance must be an explicit author action. Preserve the existing
two-command validation when moving it into a shared harness. Once migrated,
edit prose/code in the source lessons and treat `TUTORIAL.md` as generated; do
not maintain duplicate editable examples in both places.

TinyTemplate can handle ordering and presentation, but it does not execute or
validate programs. Its variables, loops, and template calls are sufficient for
precomputed lesson records; region markers should be parsed by the doc builder,
not treated as executable template commands. A simple renderer may need fewer
moving parts initially. If using TinyTemplate, disable HTML escaping for trusted
Markdown/code fragments, and choose fences that safely contain backticks.
See [TinyTemplate's overview](https://docs.rs/tinytemplate/latest/tinytemplate/)
and [formatter configuration](https://docs.rs/tinytemplate/latest/tinytemplate/struct.TinyTemplate.html#method.set_default_formatter).

## Recommended staging and decisions

1. Clarify the current tutorial entry behavior immediately without claiming a
   language change. This is documentation of implemented behavior.
2. Implement required binary `main` and migrate executable examples together.
3. Add modules, stable identities, and privacy in one coherent change; then
   expose standard-library operations through explicit imports.
4. Move lessons to runnable literate files with output snapshots and generated
   Markdown. This can happen alongside step 2 if the migration is easier once.

Decisions to confirm before implementation: module-private versus class-private
members; whether initial `main` accepts both `()` and `i32`; source-root CLI
spelling; and whether lesson output expectations are comment blocks or sidecar
files. These choices do not block recording the architecture above.
