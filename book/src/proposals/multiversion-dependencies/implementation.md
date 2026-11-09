# Resolver and compiler implementation sketch

Companion to the [multiple-version proposal](../multiversion-dependencies.md).
This is a source-informed design sketch, not implemented behavior or completed
validation. B17 in the [backlog](../../backlog.md) owns the remaining scope.
Source observations below refer to local main commit `0fe2bd37`.

## Current implementation

| Area | Observed behavior | Proposed change |
| --- | --- | --- |
| `src/project.rs`: `visit`, `dependency`, `same_git_package`, `merge_git` | DFS shares canonical directories, rejects repeated names and different commits of one repository/subdirectory. Some cycle bookkeeping uses names. | Intern by resolved source identity; maintain visiting/visited identity sets. Check each owner's direct names separately. Keep diagnostic chains separate from traversal keys. |
| `src/project.rs`: `source_package`, `SourcePackage`, `SourceGraph` | Compiler identity is assigned from `manifest.name`; dependency edges already target package indices. Graph files use identity strings. | Separate identity from display name/version/provenance and preserve index-based edges. Serialize stable, unique identities. |
| `src/project.rs`: `Pins`, `read_pins`, `Resolver`, `lockfile` | Pins use `(url, subdir, selector)` and a name-to-commit map. Lock edges refer to names; targeted updates invalidate pins using names/commits. | Pin each declaring package's dependency edge and target identity; avoid name/commit-wide invalidation. |
| `src/project/git.rs` | Cache separates repository URLs and full commits; a path inside a snapshot stays in that repository. | Reuse this layout and containment checks; do not introduce a nested installation tree. |
| `src/frontend/modules.rs` | Imports use each package's direct aliases; non-entry declarations are qualified by package identity. Synthetic test ownership partly relies on identity spelling. | Carry opaque package identity into declaration identity, with separate readable labels and explicit synthetic-test ownership. |
| `src/project/fingerprint.rs`, compiler caches and code generation | Build fingerprints include graph identity and package information. | Audit all type/function/symbol/cache keys and deterministic ordering for full package identity. Preserve relocation behavior and disambiguate generated symbols. |

The [current package contract](../../design/toolchain/01-building/packages.md)
resolves all manifests before compilation and checks requirements after source
resolution. No registry version selection exists. The frontend's existing
per-package alias tables are a good starting point; simply removing the
resolver's duplicate-name check would still collide compiler and lock identities.

## Lock and graph formats

Introduce an explicitly versioned lock representation with package IDs distinct
from names, source identities, package versions, and edges containing the
owner ID, alias, source requirement and target ID. Retain full Git commits and
source containment. Two owners using the same mutable tag may retain different
edge pins; a global `(url, selector)` map must not overwrite one with the other.
Store one outgoing mapping per resolved package identity.

For an ordinary build, honor every unchanged locked edge. For a replacement
package revision during an update, carry forward matching old requirements;
resolve only new/changed requirements. When a new edge reaches an existing
identity, use its already established outgoing mapping. For conflicting
retained mappings, fail with a diagnostic rather than choosing traversal order.
Fresh, identical unpinned requirements should resolve consistently within one
command. Reuse fetches per repository while keeping existing pins edge-specific.

Migrate valid old locks without re-resolving tags: old unique names make their
edges unambiguous. `--locked` must refuse a migration that rewrites the file,
with an actionable explanation. Validate format versions explicitly and reject
unknown versions. Preserve atomic replacement, relative local paths, offline
behavior and missing-commit errors. Failed updates must leave the lock intact.
Audit script preflight and embedded-manifest locks, not only project commands.

Graph files also need a versioned extension if display metadata, source identity
or explicit test ownership is added. Preserve old graph input through a defined
adapter, or reject it with a precise format diagnostic. Do not let older
compilers silently interpret a new identity contract as ordinary package names.
Tests must exercise `plenty graph` to `plentyc --package-graph` round trips.

## Update selection and inspection

Keep a bare direct dependency alias as the common update selection. Where a
transitive name identifies several instances, require an explicit identity or
root-to-edge selector and list the candidates. Final selector spelling is an
interface design detail; it must distinguish updating one incoming edge from
updating an instance's own outgoing edges. Updating the latter affects all
consumers of that instance and must report that scope.

For `P -> A -> C` and `P -> B -> C`, selecting A's edge to C may split it from
B's edge. Selecting a dependency edge owned by a shared C changes that same C
for both A and B; the tool must not manufacture context-dependent C copies.
An explicit whole-graph update may refresh all mutable selectors. A targeted
update must never expand its scope silently to satisfy a shared-instance check.

Extend inspection to show duplicate names, full identities, versions, sources,
commits and reverse dependency chains. Version alone is insufficient: two
commits can both call themselves 1.2.0. Keep the machine-readable graph available;
provide a human-readable duplicates/why view without changing existing graph
output silently. Lock diffs and update reports should expose moved edges,
retained versions and removed unreachable instances.

## Acceptance criteria

Use local path fixtures and local Git repositories; no network is necessary.
The implementation should establish these observable contracts:

- P/A/B/C resolves two transitive C revisions and executes code from both;
  identical-source diamonds share nominal types, even through different aliases.
- Direct aliases to two distinct same-named instances fail for every owner;
  root direct C and another branch's transitive C may differ.
- Equal version strings at different sources/commits remain distinct; tags
  reaching one commit and snapshot-relative paths reaching it share.
- Real cycles fail by identity, while a chain containing different same-named
  instances does not fail merely for repeating a name.
- Concrete structural protocol satisfaction works across versions with shared
  signature types; nominal C types and incompatible boxed protocols fail with
  both origins. Generic instantiations and generated symbols remain distinct.
- Targeted updates preserve unrelated pins, including two edges using the same
  tag; matching requirements survive parent revisions; explicit updates to
  shared outgoing mappings report all consumers.
- Lock migration preserves commits; locked/offline failures, malformed identities,
  failed-update atomicity, moved checkouts and deterministic serialization work.
- Scripts, workspace members, synthetic tests, graph round trips, IDE diagnostics,
  build reuse and cache invalidation preserve the new identities.
- Shared-resource conflicts fail for one linked program but do not reject
  independent workspace executables merely for sharing a lockfile.

Measure cold/warm resolution and build costs on chains, shared diamonds and
many-version graphs. Report fetching and compilation separately from resolver
bookkeeping; do not claim a performance win from eliminating a solver Plenty
never had. The proposal itself is documentation-only and does not run these
future acceptance cases.
