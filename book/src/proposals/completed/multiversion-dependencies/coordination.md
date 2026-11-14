# Shared instances and Node experience

Companion to the [multiple-version proposal](../multiversion-dependencies.md).
The shared-resource restriction described here is implemented; see the
[package contract](../../../design/toolchain/01-building/packages.md#explicit-shared-resources). External documentation
was checked on 2026-11-09; the evidence demonstrates failure modes, not their
frequency across an ecosystem.

## What Node and npm provide

Node's CommonJS loader resolves dependencies relative to the caller and caches
modules by resolved filename. Two calls reaching one file normally receive the
same exports object; two installed copies can have separate objects and state.
It does not provide a universal process-wide singleton for a package name.
See [Node module caching](https://nodejs.org/api/modules.html#caching) and
[caching caveats](https://nodejs.org/api/modules.html#module-caching-caveats).
This is specifically the CommonJS model, not a complete account of ESM or
browser bundlers.

npm's `peerDependencies` express compatibility with a host library/tool, such
as a plugin's host. npm 7 and later install peers by default; conflicting peer
requirements can make resolution fail. Peers express a relationship at the
relevant dependency placement, not a guarantee that the whole process contains
only one copy. See [npm peer dependencies](https://docs.npmjs.com/cli/v11/configuring-npm/package-json/#peerdependencies).

There is a concrete, documented consequence. React requires a component's React
import and its renderer's React import to resolve to the same exports object.
Two copies can cause an invalid-hook-call error. Its documentation identifies
libraries incorrectly declaring React as an ordinary dependency instead of a
peer as one cause. It also says independent React copies can coexist when they
serve separate applications/widgets; the broken case is crossing the component/
renderer boundary. See [React's duplicate-copy guidance](https://react.dev/warnings/invalid-hook-call-warning#duplicate-react).

The lesson for Plenty is to distinguish independent implementations from
explicit sharing relationships. Node does not make this problem disappear,
and Plenty's static typing catches only its type-visible forms. Two functions
using separate registries can have perfectly compatible signatures.

## Explicit shared-resource restriction

A package requiring one linked instance declares `[package] shared-resources`,
a list of stable resource keys. At most one resolved package identity may claim
a given shared-resource key within the graph linked into one program. Several
edges reaching that same identity are fine. Two identities claiming the key
fail before compilation, with both dependency chains. Do not pick a winner,
force a version upgrade, or launch a solver. Authors must coordinate their
requirements when sharing is essential.

The key identifies the resource contract, not necessarily the package name:
two differently named wrappers may both claim one native runtime. A package
requiring a single copy across versions must use a stable key across those
versions. Keys contain 1–128 ASCII letters, digits, `.`, `_`, `-`, or `/` and
must be unique in the manifest. Historical versions without metadata are not
assigned an inferred key. No unmarked historical package is inferred safe, and no annotation can discover
all conflicts with unrelated unannotated code.

Check this per executable/library link closure. A workspace lock can retain
different instances for separate programs; merely appearing in one lockfile
does not mean they share a process. Dynamically loaded libraries and separately
built plugins require a host/linking contract beyond this graph-local check.
A declaration is not a runtime isolation mechanism.

Prefer explicit injected services or handles where practical: consumers then
share an application-selected registry/logger by construction. Native libraries
may also need symbol namespacing, ABI compatibility or a single-runtime rule.
Distinct Plenty package identities alone do not isolate C symbols, environment
variables, ports or process-global native state. Two unrelated top-level
libraries already face this problem; duplication increases its opportunity.

This deliberately stops short of npm-style peer selection or automatically
binding a plugin to an ancestor's package. Such contextual resolution could
require one package source to have different dependency mappings in different
contexts, complicating the proposal's source identity rule.
