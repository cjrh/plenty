# Package language editions

> **Status: Not implemented.** Exploratory candidate under
> [B17](../backlog.md), not an implementation commitment. Plenty currently has
> one language mode; its manifest rejects `edition`. The
> [package reference](../design/toolchain/01-building/packages.md) describes
> implemented behavior. All edition names, settings and examples below are
> hypothetical.

## Assessment

Plenty could plausibly support packages written in different language editions
in one compilation. The existing package-aware module loader is a useful
foundation. A narrow mechanism for syntax and name-resolution changes looks
like moderate compiler/tooling work, with little reason for extra runtime cost.
It is not just a manifest field, and it is not a cheap way to preserve arbitrary
historical language behavior.

Recommendation: keep this option open, but wait for a concrete incompatible
change before implementing it. Prefer additive features and contextual keywords
where they solve the problem. If an edition becomes worthwhile, require the
first change to demonstrate mixed-edition interoperability using one type,
ownership and runtime model. Do not promise a release cadence or permanent
support policy before understanding that maintenance cost.

## What editions buy

An application should be able to adopt a new spelling or resolution rule without
waiting for every dependency to migrate. Conversely, an older application should
be able to use a newer-edition library, provided its compiler supports both.
The application selects its own edition, not the edition of the whole graph.

Rust uses opt-in editions for otherwise incompatible language changes. Crates
from different editions interoperate and lower to a common internal
representation. That constrains which changes can be edition-specific; it does
not require a separate runtime per edition. See the
[Rust Edition Guide](https://doc.rust-lang.org/edition-guide/editions/index.html).

For Plenty, distinguish four independent settings:

| Setting | Meaning |
| --- | --- |
| Package `version` | Version of the library/application and its API |
| `plenty-version` | Minimum compiler version needed to build the package |
| `manifest-version` | Format of the package metadata |
| Proposed `edition` | Rules used to interpret this package's source |

A new compiler can support old and new editions. An old edition may still use
new additive features, and an old compiler need not understand a new edition.
Editions do not pin compiler bugs, standard-library releases, dependency versions
or native ABI compatibility. They would not make a new compiler link arbitrary
objects emitted by old compilers: the initial contract is compilation from
source with one supported toolchain and runtime.

## A package author's experience

Use illustrative names `e1` and `e2`, not proposed release dates:

```toml
# app/plenty.toml
manifest-version = 1

[package]
name = "app"
version = "0.1.0"
edition = "e2"

[dependencies]
legacy = { path = "../legacy", version = "^0.1.0" }
```

The `legacy` package independently declares `edition = "e1"`. All its modules,
library targets, executable targets and tests use that edition; importing it
does not reinterpret its source as `e2`. Workspace members choose independently.
Start without per-file or per-target edition overrides.

For a deliberately hypothetical change, suppose `e2` reserves the word `memo`
while `e1` permits it as a local name:

```text
# legacy/src/lib.plenty — e1
pub def answer() -> int:
    memo = 42
    return memo

# app/src/main.plenty — e2
import legacy

def main() -> Result[(), IoError]:
    print(legacy.answer())?
    Ok(())
```

The application's call works; only writing a local named `memo` in the `e2`
application is rejected. Migrating `legacy` could rename its local variable,
then change its manifest. No caller needs to change its edition.

Public names make even this example harder: an `e1` library might export a
function named `memo`. Before reserving it, Plenty would need a way for `e2`
callers to name that existing API, such as contextual member-name parsing or
escaped identifiers. Neither spelling is selected here. Renaming a public API
is not a harmless automatic migration. A new keyword that makes old exports
inaccessible fails the interoperability requirement.

## Boundaries that keep this manageable

| Candidate change | Edition suitability |
| --- | --- |
| New reserved spelling, old syntax translated to existing operations | Promising, with public-name access and migration rules |
| Different import lookup or implicit names | Possible, if resolved in the defining package before combination |
| New opt-in feature that accepts all existing code | Usually no edition needed |
| Changed type identity, value layout, calling convention or allocator contract | Outside the narrow design; needs a shared representation or a separate interoperability design |
| Weaker historical borrow/safety checks, different destruction or concurrency guarantees | Do not preserve through this mechanism; cross-package safety obligations must still agree |
| Removing a standard-library API or changing its result/error type | Ordinary API compatibility problem; an edition label alone does not solve it |

Every package must expose types and callable contracts that the other editions
understand. Protocol satisfaction, borrows, generic arguments and errors must
continue to compose. A generic function's body must retain its defining
package's interpretation when specialized for a caller from another edition;
the caller must not change how that body resolves names. Prefer resolving and
normalizing edition differences before shared checking and specialization.

Safety fixes apply to every edition even if previously accepted code stops
compiling. More general compatibility exceptions need an explicit policy;
"old edition" cannot mean "all past compiler behavior is frozen".

Embedded `std` should use the edition selected by the compiler distribution,
not the importing package's edition. Its exported contracts remain usable from
every supported edition. Generated test harnesses and interface source also
need an explicit interpretation; never inherit an unrelated caller's setting.

## Selection and tooling

If adopted, missing edition metadata should mean a fixed baseline, never
"whatever is newest today". New-package commands should write the latest
supported edition explicitly. Reject unknown editions with the package name
and supported values; do not silently choose a nearby version. Validate the
minimum compiler requirement independently.

Standalone compilation needs an equivalent explicit option, provisionally
`plentyc --edition e2 FILE`, with the same fixed baseline when absent. A package
graph supplies editions per package; reject a conflicting global override
instead of applying it to dependencies. Compiler library APIs, the language
server and documentation checks must use the same selection rules.

The proposed [single-file script header](single-file-scripts.md) could carry
`[package].edition`. It should share the package schema and default rather than
invent another source directive. An edition does not require a script runner.

Resolution continues to choose package sources/versions. Edition is metadata
of the selected package, not a request to compile that dependency in multiple
editions. A lockfile is not the authority for source interpretation. Generated
package graphs must preserve the edition, and build reuse must be invalidated
when it changes, including when no source bytes change.

## Fit with the current implementation

This is a source inspection at local `main` `2d6998b4`, not an implemented
prototype or a measured delivery estimate:

| Area | Existing foundation and required work |
| --- | --- |
| `src/project/manifest.rs` | Strict keys and minimum compiler metadata exist; tests explicitly reject `edition`. Add validated metadata, defaults and generator support. |
| `src/project.rs` | `SourcePackage`/`SourceGraph` already carry package identity, roots and dependency edges. Carry edition through resolution, graph serialization and synthetic test packages. Version the graph format so older readers cannot silently drop semantics; preserve the old format as baseline input if supported. |
| `src/frontend/modules.rs` | `Loader::file` already receives a package index. `parse`/`parse_buffered` create a lexer/parser without an edition. Pass the owning edition through both buffered parsing paths and retain it wherever interpretation remains deferred. |
| Lexer, parser and checker | Gate only the intended source rule and normalize it; audit generic specialization, interfaces and source-only entry points for caller-dependent interpretation. |
| `src/project/fingerprint.rs` and compiler caches | Target fingerprints cover sources, selected graph fields and compiler identity, but currently have no edition. Include effective editions in relevant build/cache inputs and version affected formats. |
| CLI, IDE and test tooling | Agree on package discovery, explicit standalone selection, generated code and diagnostics. Migration tooling is additional work, not supplied by storing the field. |

Module loading currently deduplicates by `ModuleSource`. If one physical file
can be assigned to different package contexts, implementation must either
reject conflicting ownership/editions or include that context in module
identity. Taking whichever edition happened to load first would be incorrect.

The inexpensive part is plumbing a value to the parser. The larger recurring
cost is maintaining old rules and proving that their combinations still work.
A syntax-only first edition should not need a second backend, runtime or
dependency resolver. Rules that spread throughout checking or require adapters
at package boundaries are a reason to reconsider the change, not evidence that
editions in general are easy.

## Evidence required before adoption

An implementation experiment should use a real proposed incompatible rule and
prove old-to-new and new-to-old calls, including a diamond graph. It should
cover public names that collide with new keywords, shared class/enum values,
protocol satisfaction, borrows and cross-package generic specialization. Merely
accepting two manifest values does not establish mixed-edition support.

Check omitted/unknown editions, independent workspace members, test harnesses,
embedded `std`, interfaces, direct compiler/API use and IDE diagnostics. Warm
builds must invalidate after changing only a root or dependency edition; graph
round trips must retain it. Run existing baseline tests unchanged and measure
compile-time/cache effects before claiming negligible cost.

Migration should first make source valid in both editions, then change the
manifest and rerun checks/tests. Rust's
[migration workflow](https://doc.rust-lang.org/edition-guide/editions/advanced-migrations.html)
uses compatibility diagnostics and verified edits; it also documents limits for
generated code and configurations. Plenty could begin with diagnostics and a
short manual migration guide for a small change. Automated public-API rewrites
or claims of complete migration coverage need substantially more evidence.

Before committing, choose the first change, edition naming, baseline/default
policy, supported-edition lifetime, and public-name escape policy. With no
pressing incompatible change, retaining one language mode is the simpler
choice. Scheduling and priority remain solely in [the backlog](../backlog.md).
