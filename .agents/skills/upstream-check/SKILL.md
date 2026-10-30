---
name: upstream-check
description: For maintenance of the Plenty compiler repository. Check the upstream code, data and tools Plenty pins (Cranelift and regalloc2, crypto and TLS crates, compression, SQLite, polars, faer, Unicode and tzdata, mdBook and the release toolchain) for new releases, security advisories (RustSec, GHSA, CVE), breaking changes and relevant fixes, using one small research agent per group. Use for a periodic dependency review or before a release. Reports ranked recommendations; never updates a pin.
---

# Upstream check

Find out which pinned upstreams need attention, why, and what updating would
cost. The result is a ranked report. This skill never runs `cargo update`,
edits `Cargo.toml`, lockfiles, vendored sources or version pins, or files
issues; the user decides, and an update is its own change in a worktree.

## 1. Build the inventory

Run [`scripts/upstream_inventory.py`](../../../scripts/upstream_inventory.py)
from the worktree being checked:

```sh
python3 -B scripts/upstream_inventory.py --advisories
python3 -B scripts/upstream_inventory.py --advisories --json > "$SCRATCH/upstream.json"
python3 -B scripts/upstream_inventory.py --group codegen --group crypto-tls
```

It reads the tree, so nothing in it is maintained by hand except the group
map and update notes at the top of the script:

- every direct dependency in the three manifests (`Cargo.toml`,
  `plenty-runtime/Cargo.toml`, and the engine crates `plenty-dataframe` and
  `plenty-linalg`, which have their own lockfiles), with its locked version,
  plus named transitive crates such as regalloc2;
- vendored C sources under `plenty-runtime/native` (SQLite, Brotli);
- data pins: the Unicode version from `std.unicode`, every Unicode version a
  std page names, the tz package tag in the book, and the tzdata release in
  that tag of the tz repository (default `../plentylang/tz` beside the primary
  worktree; `--tz-repo` overrides it, and `unknown` means it was not found);
- tool pins: mdBook in `book/releases.json`, `scripts/release/toolchain.env`,
  and any `rust-toolchain` file;
- every upstream named in a std page's Acknowledgements section (wrapped,
  ported or modelled), and any acknowledged version that differs from its pin;
- with `--advisories`, `cargo deny check advisories` against each lockfile.
  This is the authoritative RustSec result; if `cargo deny` is missing, say so
  and let the agents search RustSec instead.

Each group has an update rule, printed with it. Repeat the rule in the report
for any update you recommend; for example, Cranelift or regalloc2 changes need
the object-identity and performance reruns in
[the development workflow](../../../book/src/development/workflow.md#dependency-versions).

Groups `support` (other direct dependencies) and `acknowledgements` (mostly
APIs Plenty is modelled on) need no research agent by default: `cargo deny`
covers the support crates' advisories, and modelled-on designs rarely need
updates. Include them only if the user asks.

## 2. Confirm the plan

Name the groups to research and the agent count: one per group, normally
`codegen`, `crypto-tls`, `compression`, `sqlite`, `dataframe`, `linalg`,
`data` and `toolchain` (8). Launch up to 5 without asking; for more, get the
user's go-ahead or offer a subset (for example the groups with advisories plus
`codegen` and `crypto-tls`).

## 3. Launch research agents

Use parallel `Agent` calls in one message, each with `model: "haiku"` and the
general-purpose agent type. Agents need web access (WebSearch and WebFetch, or
the web-fetch agent); check that the session has it. If the Workflow tool is
available and the user approved the count, a workflow with
`agent(prompt, {model: 'haiku', schema})` per group works too.

Prompt for each group, filling the braces with that group's inventory rows:

````text
Research upstream releases for one group of dependencies pinned by the
Plenty compiler. Work read-only: do not edit files, run cargo update, or
change any repository state. Use web search and fetch, and read-only commands
in the repository if useful.

Repository: {worktree absolute path}
Today: {date}
Group: {group}. Update rule: {update rule or "none"}
Pinned items (name, kind, pinned version, where pinned):
{rows}
cargo deny advisories already found for this group:
{matching advisories, or "none"}

For each item, find:
1. The latest stable release and its date. Prefer the project's release
   page, changelog or crates.io (https://crates.io/api/v1/crates/NAME), not
   blog posts.
2. Security advisories affecting the pinned version: RustSec
   (https://rustsec.org/packages/NAME.html), GitHub advisories (GHSA) and
   CVEs. Give the ID, affected range, fixed version, and whether the
   affected code path is plausible for this use (say why, or "unknown").
3. Breaking changes between the pinned and latest versions: MSRV, removed or
   renamed APIs, changed defaults, licence changes.
4. Fixes or performance changes relevant to how Plenty uses it (codegen
   quality and compile speed for Cranelift; allocation failure handling;
   data versions for Unicode and tzdata).

Cite a URL for every claim. If you could not confirm something, say so; do
not guess versions.

Before answering, check that the JSON parses by piping it to
python3 -c 'import json,sys; json.load(sys.stdin)'.

Final answer: only a JSON object, no other text:
{"group": "{group}",
 "items": [{"name": "...", "pinned": "...", "latest": "... or unknown",
            "latest_date": "YYYY-MM-DD or unknown",
            "advisories": [{"id": "...", "affects_pinned": true,
                            "fixed_in": "...", "relevance": "...", "url": "..."}],
            "breaking_changes": ["... (url)"],
            "relevant_changes": ["... (url)"],
            "suggested_priority": "urgent|recommended|informational",
            "confidence": "high|medium|low"}],
 "notes": "..."}
````

## 4. Verify what matters

Small agents misread changelogs and confuse similarly named crates. Before
ranking:

1. Parse each JSON answer; repair an obvious bracket slip, and rerun a group
   once if the answer is missing or unusable.
2. For every advisory and every item marked `urgent` or `recommended`, check
   the cited source yourself (fetch the advisory or release page) and confirm
   the affected range against the pinned version.
3. For a RustSec finding in an engine lockfile, check that the crate is in
   the normal dependency graph with the enabled features:
   `cargo tree --locked -e normal -i CRATE` in that crate's directory. A
   reachable crate is not proof the vulnerable function is called; say which
   you established.
4. Check `latest` versions from the source, not from the agent. For crates,
   one request per crate is enough:
   `curl -s -A plenty-upstream-check https://crates.io/api/v1/crates/NAME`
   (`max_stable_version`); crates.io rejects requests without a user agent.
   For Unicode, `https://www.unicode.org/Public/VERSION/ucd/ReadMe.txt` says
   "final data files" once a version is released; news and draft pages are
   not enough.
5. For a transitive advisory, establish whether an update is possible:
   `https://crates.io/api/v1/crates/PARENT/VERSION/dependencies` gives the
   requirement the latest parent release accepts. If it excludes the fixed
   version, the next step is to wait for or ask upstream, not `cargo update`.

Group hints from earlier runs:

- `codegen`: the `cranelift-*` crates release together with Wasmtime. Map a
  Cranelift version to its Wasmtime release through that tag's workspace
  `Cargo.toml`; there is no fixed offset. Most Wasmtime advisories concern the
  Wasm runtime; a Cranelift miscompilation advisory applies to Plenty too.
- `dataframe`: polars pins its own transitive crates, so its advisories are
  usually blocked until a polars release relaxes a requirement.
- `data`: tzdata releases are listed at `https://www.iana.org/time-zones`;
  the tz package is a separate repository with its own tags.

## 5. Report

Rank every item that needs attention:

- **Urgent**: a confirmed advisory affecting the pinned version on a
  plausibly reachable path.
- **Recommended**: an advisory with a doubtful path, an unmaintained crate,
  a pin well behind with relevant fixes, or a data release (Unicode, tzdata)
  that changes results.
- **Informational**: new releases without a clear reason to move, and
  inconsistencies such as different Unicode versions on different pages.

For each: the item, pinned and latest versions, the evidence (advisory ID or
release note, with URL), and a concrete next step that names the update rule,
for example "update ring in `plenty-runtime/vendor` and `Cargo.lock`, then rerun
`tests/test_std_tls.rs`" or "wait for polars to release with zstd 0.14". Finish
with the groups checked, the groups skipped, and anything unverified.

Offer, in one line each, to file a local issue for an urgent or recommended
item (search `issues.db` first, as `CLAUDE.md` describes) or to start the
update in a new worktree and local PR. Do neither unless asked.
