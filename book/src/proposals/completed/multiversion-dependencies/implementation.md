# Resolver and compiler implementation

Companion to the [implemented multiple-version design](../multiversion-dependencies.md).
The [package reference](../../../design/toolchain/01-building/packages.md)
owns the user contract. This page records implementation boundaries and format
migration, rather than a separate work queue.

## Resolved identity and compiler boundaries

`src/project/identity.rs` represents exact Git locator, normalized subdirectory
and full commit, or a canonical local directory. Local identities serialize
relative to the project/lock root. A path within a Git snapshot keeps the
snapshot identity. Embedded scripts have explicit `script:RELATIVE_PATH`
identities and `script+RELATIVE_PATH` source records, distinct from a local
package identity. This does not relax source-root containment: script and
dependency source roots must still be disjoint. Workspace member role is
separate metadata.

Non-Unicode local and script paths use `path-bytes:` and `script-bytes:`
identities and hex-encoded `path-bytes+HEX` and `script-bytes+HEX` source records. Native
Git subdirectories use `git-bytes:` identities and a hex `subdir-bytes` field.
These alternatives retain native path bytes (native code units on Windows);
ordinary UTF-8 encodings are unchanged.

The resolver interns source identities, rejects actual identity cycles and
checks each owner's direct declared-name uniqueness. Distinct transitive
instances may coexist, while one identity has one outgoing mapping. Existing
Git cache layout and source-containment checks remain in use.

Compiler graph IDs qualify declarations as `NAME@VERSION#FULL_SHA256` and are
checked against the complete source key. Aliases only affect source lookup.
Nominal classes/enums, generic instances and boxed protocols stay distinct
across sources; structural method matching keeps its existing rules. Source
and root-to-instance provenance accompany diagnostics and IDE results.
Synthetic test ownership is explicit rather than inferred from an ID suffix.

## Lock and graph formats

`src/project/lock.rs` writes `lock-version = 2`: each package records its full
identity, display name/version, member role and source; each owner's dependency
entry stores alias, exact source requirement and target ID. Independent owners
can retain different pins for the same mutable tag. A shared identity keeps
one dependency mapping, and conflicting retained mappings fail explicitly.

Valid version-1 locks migrate while retaining pinned commits, including embedded
script locks. `--locked` refuses the write and directs the user to `plenty fetch`
without `--locked`; targeted edge updates require this migration first. Unknown versions and malformed identities fail. Replacement
is atomic; failed updates leave the lock unchanged. Relative local locations
preserve checkout relocation. Live path content changes still use build
fingerprints rather than pretending the lock freezes a directory.

`plenty graph` emits `graph-version = 2` with opaque compiler identity, full
`source-identity`, display/provenance fields, package `directory`, direct alias
edges and explicit `test-of` where applicable. Legacy graph version 1 adapts
its opaque IDs without guessing test ownership; the legacy adapter alone
infers an omitted package directory from the parent of the source root. In
version 2, an omitted `directory` means no package directory and includes stay
within the source root, as for single-file programs and scripts. Optional
`root-bytes`, `directory-bytes` and `library-bytes` hex alternatives preserve
non-Unicode native paths (native code units on Windows); each excludes its
ordinary path field. Ordinary UTF-8 fields and version-1 input are unchanged.
Unknown graph versions fail.

## Targeted updates

Selection accepts a direct alias, a root-to-edge alias path (`a/c`), or a full
owner source ID followed by `::ALIAS`. A bare transitive name works only when
one resolved identity matches; ambiguity reports candidate IDs.

Updating one independent Git edge can split a previously shared target without
moving another consumer's edge. Updating an outgoing edge of one shared
identity affects all its consumers and reports that scope. Matching independent
Git alias/source requirements retain their pins when a parent revision changes;
new or changed source requirements resolve normally. Pin matching compares
the exact source requirement separately from its optional version guard.
Changing only the guard retains the old source pin and validates the locked
package version, without refreshing a moved tag.

Snapshot-relative paths follow their parent's repository commit. Parent updates
recompute the complete path chain in the replacement snapshot. Independently
updating such an edge fails with its owning Git boundary, rather than silently
expanding the request. Local paths remain live.

## Inspection and shared resources

`src/project/inspection.rs` provides `plenty graph --duplicates` and
`plenty graph --why SELECTOR`. Reports show name/version, full identity/source,
Git commits and every root-to-instance alias path. Default graph output stays
machine-readable.

`[package] shared-resources` declares unique stable keys of 1–128 ASCII letters,
digits, `.`, `_`, `-`, or `/`. Validation checks each member's linked dependency
closure; two identities claiming one key fail with both paths. A workspace lock
may retain different instances for independent programs. The declaration adds
neither a solver nor isolation of native symbols, process state or separately
built plugins. See [coordination and external evidence](coordination.md).

## Validation scope

Resolver fixtures use local paths and local Git repositories. Compiler coverage
checks distinct nominal/generic/boxed identities, structural protocol matching,
readable diagnostics and explicit test ownership. Resolver coverage addresses
edge-pin stability, snapshot-relative paths, migrations, graph round trips,
shared resources, script/workspace integration and failed-update atomicity.
The local PR records the checks actually run and their results. No performance
improvement is claimed by this design record.

## Measured command costs

On 2026-11-14, a debug CLI was measured with local path fixtures: a chain of
40 nodes plus the application; a diamond with 40 branches, one shared C and
the application; and 40 branches each using a distinct local package named C,
plus the application. Times are wall-clock milliseconds. Warm graph values
are medians of seven runs; warm builds are medians of three reuse runs.

| Shape | Packages | Cold graph | Warm graph | Cold `--fast-compile` build | Warm build reuse |
| --- | ---: | ---: | ---: | ---: | ---: |
| Chain | 41 | 6.077 | 6.190 | 67.502 | 22.521 |
| Shared diamond | 42 | 6.151 | 6.682 | 68.613 | 25.957 |
| Distinct same-named instances | 81 | 8.383 | 9.517 | 82.369 | 35.037 |

Graph timings include CLI startup, manifest parsing, resolution, lock
comparison/write and graph serialization. Build timings include graph work
and compiler/linker execution or executable reuse; they are not isolated
compilation measurements. No Git fetches or network were measured, and OS
caches were not flushed. These observations are not a before/after comparison
or evidence of a performance improvement.
