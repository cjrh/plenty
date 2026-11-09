# Multiple versions of transitive dependencies

**Status: Not implemented.** This proposes replacing graph-wide package-name
uniqueness with resolved package identity. The [package reference](../design/toolchain/01-building/packages.md#dependencies)
remains authoritative: today two packages with the same name, or two commits
of one Git package, conflict. Implementation scope belongs to **B17** in the
[backlog](../backlog.md). This proposal changes no compiler behavior.

## Motivation

An application P depends on A and B. Both use C internally, but A needs C 1.2
while B still needs C 1.1. Requiring one C for the whole application can prevent
an urgent security upgrade of A even when neither C version is vulnerable.
Allowing both lets these independent implementation dependencies evolve
independently. It does not make a vulnerable retained version safe: inspection
must expose every resolved instance.

```text
P
├── A ── C 1.2
└── B ── C 1.1
```

The governing rule is:

> The same resolved package shares types; distinct resolved packages have
> distinct types, regardless of whether their names match.

Extra code and separate module state are accepted costs. A type mismatch at a
library boundary is a useful compile-time error. Explicitly shared services and
native resources need additional coordination, as described under
[shared instances](multiversion-dependencies/coordination.md).

## Direct dependencies remain unambiguous

Each manifest has unique import aliases, as today, and may directly depend on
at most **one distinct resolved instance of any declared package name**.
This applies to every package, not just the application root: a library should
have the same rules when built independently or used as a dependency.
Multiple aliases for the same instance remain valid; aliases do not change
identity. Aliasing two versions as `c_old` and `c_new` does not bypass the rule.

| Shape | Proposed result |
| --- | --- |
| P directly uses A and B; A uses C 1.1, B uses C 1.2 | Allowed |
| P directly uses C 1.2 and A; A uses C 1.1 | Allowed; P's C does not override A's |
| P directly aliases C 1.1 and C 1.2 under different names | Rejected |
| A directly aliases C 1.1 and C 1.2 | Rejected for the same reason |
| A and B reach the identical C source/revision | Share one instance and its types |
| Two unrelated packages called C occur in different branches | Allowed; source identities distinguish them |

This is the conservative interpretation of “multiple versions only for
transitive dependencies.” Merely requiring unique aliases would also be
unambiguous, but would permit direct dual-version dependencies and is outside
this design. Workspace member names remain unique for command selection;
different members may resolve different versions of their dependencies.
Dependencies still are not transitively importable: P must declare C to import
it, and that declaration has no authority over A's or B's C.

## Resolved identity

Keep three concepts separate: the package's declared name/version, a caller's
import alias, and the resolved source identity used by the compiler.

| Source | Identity basis |
| --- | --- |
| Git | Repository locator, normalized package subdirectory, full commit ID |
| Local path | Canonical package directory within this build |
| Workspace member | Its local package identity, with a distinct member role |

For Git, initially preserve the resolver's exact repository locator semantics;
do not guess that SSH and HTTPS URLs or mirrors are equivalent. A path inside
a Git snapshot inherits the repository/commit/subdirectory identity, so it
shares with a direct Git edge to that exact package. Different tags resolving
to one commit share. Equal version strings or equal file contents do not imply
identity; two commits may advertise the same version. Live paths stay mutable,
with content changes handled by build fingerprints, not by pretending that a
version string pins their content.

Use a structured internal key and a deterministic serialized identity. Display
names and short hashes are labels, not equality keys. A digest used in symbols
or graph files must be checked against its full identity, not silently trusted
when it collides. Persist local locations relative to the lock root, preserving
checkout relocation without merging distinct directories during a build.

One source identity has one dependency mapping within a resolved graph.
Consumers do not privately reinterpret that package's outgoing edges. If two
retained lock contexts demand different mappings for the same source identity,
report an inconsistent lock/update request; do not silently clone that package
under the same identity. Context-dependent instances are outside this proposal.

## Types and structural protocols

A class declared by C 1.1 differs from the class with the same spelling declared
by C 1.2. This applies through generic arguments, errors, enum payloads and
protocol method signatures; aliases cannot erase it. Diagnostics should show
both versions, source/revision distinctions when needed, and paths from the
root to each declaration. Avoid an unexplained “expected Point, found Point.”

[Plenty protocols](../design/language/07-generics/protocols.md) already match
accessible inherent methods structurally, including exact receivers, parameter
and result types. A concrete class from either version can satisfy a consumer's
protocol when its methods meet that contract. No shared protocol declaration
or registration is required for that concrete structural check. This is useful
for contracts expressed in common builtin or standard-library types.

A method returning C 1.1's Point cannot satisfy a requirement returning C 1.2's
Point. Distinct protocol declarations likewise do not become interchangeable
boxed protocol identities: existing [box restrictions](../design/language/07-generics/boxed-protocols.md)
still apply. Multiple-version support changes package identity, not protocol
matching or implicit conversions. Behavioral compatibility and shared state
remain outside what signature matching proves.

## Resolution, stability and cost

Resolve each declared path/tag/revision, recursively read its manifest, and
validate its optional version requirement against that result. Deduplicate
identical resolved identities. Reject actual identity cycles, direct-name
ambiguity and explicit shared-instance conflicts. There is no search for a
single version satisfying all packages, no backtracking to older releases, and
no implicit upgrading to deduplicate compatible versions.

Plenty already has this pinned-source approach and no version solver. The
benefit is preserving that model as the ecosystem grows, not a measured speedup
over the current compiler. With identity maps and DFS visiting states, graph
bookkeeping can approach O(V + E), excluding fetching, parsing and hashing;
the current linear scans do not establish that bound. Extra versions can
increase download, compilation and executable costs. SAT solving is not
inherently required by package management; future registry/range selection is
separate from this proposal.

A targeted update preserves existing edges outside its selected scope. Updating
A may introduce a new C while B retains its old C. For independent Git
tag/revision dependencies, a replacement A should reuse prior pins where the
alias and source requirement match in the declaring package's source context.
New or changed independent requirements resolve normally; reuse an already
locked package's dependency mapping when that source identity is reached again.
Do not recursively refresh every mutable tag below A.

A path dependency inside a Git package instead follows its parent's repository
snapshot. If A moves from commit R1 to R2, its unchanged `c = { path = "../c" }`
now names C at R2, not C at R1. Recompute these paths, including chains of paths,
within the new snapshot; other branches may keep C at R1. Such an edge cannot
be updated independently of the Git package that supplies its snapshot. Local
path dependencies remain live directories resolved relative to their manifests.

Updating a shared package's own dependency edge affects all consumers of that
same package instance. Report those consumers; stability cannot promise that
an explicitly updated shared node changes for only one caller. Selecting an
independent Git edge to C can move that edge to a different C without moving
another incoming edge. A snapshot-relative path edge is not independently
retargetable. The [implementation sketch](multiversion-dependencies/implementation.md)
distinguishes these update scopes and the required lockfile changes.

## Alternatives and boundaries

Global name uniqueness is simpler but couples independent upgrades. Automatic
semver unification can reduce duplication but adds selection policy and may
move unrelated code. Both are rejected as defaults here. Identical-source
sharing remains useful without either policy.

A broad standard library may reduce small utility dependencies and offer common
boundary types. That is a plausible ecosystem effect, not evidence that Plenty
will avoid large graphs. The design must remain inspectable with many versions.
There is no nested-directory requirement: the existing Git cache is already
organized by repository and commit.

See [Node experience and shared-instance restrictions](multiversion-dependencies/coordination.md)
for evidence and limitations, and the [implementation sketch](multiversion-dependencies/implementation.md)
for current code, format migration and acceptance criteria. Implementation and
priorities are tracked only in B17, not in a separate proposal work queue.
