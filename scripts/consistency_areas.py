#!/usr/bin/env python3
"""Partition the repository into areas for the consistency-check skill.

Each area is a coherent set of tracked files that one small agent can check.
Areas are derived from `git ls-files`, so new pages, modules and tests are
covered without editing a list. Only the standard-library theme map below is
maintained by hand; an unmapped module lands in `std-other`, which the output
reports so the map can be updated.

Kinds:
  read  prose and Plenty sources small enough to read in full
  scan  large Rust trees (tests, compiler, runtime); the agent searches them
        and reads around each hit

With --pattern, each area reports matching lines, areas without a hit can be
dropped (--hits-only), and an area whose hits exceed --max-hits is split.

--only/--skip select areas by id prefix, and --pack merges adjacent small
areas of one family (tutorial chapters, say) to reduce the agent count.

Usage: consistency_areas.py [--json] [--pattern REGEX ...] [--hits-only] [--pack]
                            [--only PREFIX ...] [--skip PREFIX ...]
                            [--include-history] [--budget BYTES] [--max-hits N]
"""

import argparse
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys

REPO = Path(__file__).resolve().parent.parent
TEXT_SUFFIXES = {".md", ".plenty", ".plentyi", ".rs", ".toml", ".vim", ".lua"}
# Proposals and experiments record history and rejected designs; old syntax
# there is expected. Checked only with --include-history.
HISTORY = ("book/src/proposals/", "experiments/")
EXCLUDED = (
    "plenty-runtime/vendor/",
    "plenty-runtime/native/",
    "fuzz/",
    ".github/",
    # Generated from the std sources by tests/test_stdlib_index.rs; fix the source.
    "book/src/design/stdlib/signatures.md",
)
DEFAULT_BUDGET = 400_000
DEFAULT_MAX_HITS = 80

# Standard-library themes. A module's key is its first path component under
# stdlib/std (`http/client.plenty` -> `http`) or its reference page's name up
# to the first `-` or `/` (`http-client.md`, `fs/walking.md`).
STD_THEMES = {
    "std-net": "http https net tls actor sse websocket wss url channel concurrent thread ready signal",
    "std-formats": "json toml xml csv encoding markdown template uuid mime report image",
    "std-crypto": "crypto jwt secrets random",
    "std-text": "text unicode regex fmt shlex diff bytes",
    "std-system": "io fs path tempfile env system process cli log archive compress error",
    "std-numeric": "math complex simd stat graph sketch plot",
    "std-data": "dataframe linalg kv db sqlite cache",
    "std-core": "collections deque sync atomic test time index",
}
STD_THEME = {key: theme for theme, keys in STD_THEMES.items() for key in keys.split()}

AREA_DESCRIPTIONS = {
    "top-level": "README, AGENTS.md, CLAUDE.md, book front matter and development guide",
    "editors": "Editor syntax highlighting and LSP configuration",
    "skill": "The plenty agent skill and its published-repository files",
    "design-language": "Language reference",
    "design-facilities": "Builtins and I/O facilities reference",
    "design-toolchain": "Toolchain reference",
    "design-examples": "Reference examples and the syntax cheat sheet",
    "examples": "Sample programs",
    "tests-std": "Standard-library integration tests (Plenty embedded in Rust)",
    "tests-fixtures": "Test fixture programs",
    "tests-other": "Other integration tests (Plenty embedded in Rust)",
    "src-frontend": "Compiler frontend: parser, checker, diagnostics and hints",
    "src-codegen": "Compiler codegen",
    "src-other": "Other compiler sources, including CLI and LSP messages",
    "runtime": "Rust runtime crate sources",
    "engines": "Dataframe and linalg engine crates",
    "history": "Proposals and experiments (history; old forms expected)",
    "std-other": "Standard-library modules missing from STD_THEMES",
}
SCAN = ("tests-", "src-", "runtime", "engines")


def tracked_files(repo=REPO):
    output = subprocess.run(
        ["git", "-C", str(repo), "ls-files", "-z"], check=True, capture_output=True
    ).stdout.decode()
    files = []
    for name in output.split("\0"):
        path = repo / name
        if name and path.is_file() and not path.is_symlink():
            files.append((name, path.stat().st_size))
    return files


def std_key(path):
    if path.startswith("stdlib/std/"):
        return PurePosixPath(path[len("stdlib/std/"):]).parts[0].split(".")[0]
    page = path[len("book/src/design/stdlib/"):]
    return re.split(r"[-/.]", page, maxsplit=1)[0]


def area_of(path, include_history=False):
    """The area id for a tracked path, or None if it is not checked."""
    if PurePosixPath(path).suffix not in TEXT_SUFFIXES and not path.endswith("README"):
        return None
    if path.startswith(EXCLUDED):
        return None
    if path.startswith(HISTORY):
        return "history" if include_history else None
    parts = PurePosixPath(path).parts
    if path.startswith("book/src/tutorial/"):
        return f"tutorial-{parts[3].split('.')[0]}" if len(parts) > 4 else "tutorial-index"
    if path.startswith("book/src/design/stdlib/") or path.startswith("stdlib/std/"):
        return STD_THEME.get(std_key(path), "std-other")
    if path.startswith("book/src/design/") and len(parts) > 4:
        return f"design-{parts[3]}"
    if path.startswith("book/src/") or "/" not in path:
        return "top-level"
    if path.startswith(("editors/",)):
        return "editors"
    if path.startswith((".agents/skills/plenty/", "scripts/agent_skill_repo/")):
        return "skill"
    if path.startswith("examples/"):
        return "examples"
    if path.startswith("tests/"):
        if path.startswith("tests/fixtures/"):
            return "tests-fixtures"
        return "tests-std" if path.startswith("tests/test_std") else "tests-other"
    if path.startswith("src/"):
        if path.startswith("src/frontend/"):
            return "src-frontend"
        return "src-codegen" if path.startswith("src/codegen/") else "src-other"
    if path.startswith("plenty-runtime/"):
        return "runtime"
    if path.startswith(("plenty-dataframe/", "plenty-linalg/")):
        return "engines"
    return None


def kind_of(area):
    return "scan" if area.startswith(SCAN) else "read"


def describe(area):
    if area == "tutorial-index":
        return "Tutorial index and closing pages"
    if area.startswith("tutorial-"):
        return f"Tutorial chapter {area[len('tutorial-'):]}"
    if area.startswith("std-") and area != "std-other":
        return f"Standard library sources and reference pages: {STD_THEMES[area]}"
    return AREA_DESCRIPTIONS.get(area, area)


def split(files, weight, limit):
    """Greedy, order-preserving split of [(path, size, hits)] by weight(file)."""
    parts, current, total = [], [], 0
    for item in files:
        if current and total + weight(item) > limit:
            parts.append(current)
            current, total = [], 0
        current.append(item)
        total += weight(item)
    if current:
        parts.append(current)
    return parts


def count_hits(repo, path, patterns):
    if not patterns:
        return 0
    try:
        text = (repo / path).read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return 0
    return sum(1 for line in text.splitlines() if any(p.search(line) for p in patterns))


def build_areas(files, repo=REPO, patterns=(), include_history=False,
                budget=DEFAULT_BUDGET, max_hits=DEFAULT_MAX_HITS, hits_only=False):
    """files: [(path, size)]. Returns a list of area dicts in a stable order."""
    grouped = {}
    for path, size in sorted(files):
        area = area_of(path, include_history)
        if area is not None:
            grouped.setdefault(area, []).append((path, size, count_hits(repo, path, patterns)))
    areas = []
    for area in sorted(grouped):
        members = grouped[area]
        kind = kind_of(area)
        if patterns and hits_only:
            members = [m for m in members if m[2]]
            if not members:
                continue
        if patterns and sum(m[2] for m in members) > max_hits:
            parts = split(members, lambda m: m[2], max_hits)
        elif kind == "read" and sum(m[1] for m in members) > budget:
            parts = split(members, lambda m: m[1], budget)
        else:
            parts = [members]
        for index, part in enumerate(parts, 1):
            entry = {
                "id": area if len(parts) == 1 else f"{area}.{index}",
                "kind": kind,
                "description": describe(area),
                "bytes": sum(m[1] for m in part),
                "files": [m[0] for m in part],
            }
            if patterns:
                entry["hits"] = sum(m[2] for m in part)
            areas.append(entry)
    return areas


def family(area_id):
    return area_id.split("-", 1)[0]


def pack(areas, budget=DEFAULT_BUDGET, max_hits=DEFAULT_MAX_HITS):
    """Merge adjacent small areas of one family and kind, to need fewer agents."""
    packed = []
    for area in areas:
        last = packed[-1] if packed else None
        if (
            last
            and family(last["id"]) == family(area["id"])
            and last["kind"] == area["kind"]
            and last.get("hits", 0) + area.get("hits", 0) <= max_hits
            and (area["kind"] == "scan" or last["bytes"] + area["bytes"] <= budget)
        ):
            merged = dict(last)
            merged["members"] = last.get("members", [last["id"]]) + [area["id"]]
            merged["id"] = f"{merged['members'][0]}..{area['id']}"
            merged["description"] = "Merged: " + ", ".join(merged["members"])
            merged["bytes"] = last["bytes"] + area["bytes"]
            merged["files"] = last["files"] + area["files"]
            if "hits" in area:
                merged["hits"] = last["hits"] + area["hits"]
            packed[-1] = merged
        else:
            packed.append(area)
    return packed


def select(areas, only=(), skip=()):
    def matches(area_id, prefixes):
        return any(area_id.startswith(prefix) for prefix in prefixes)

    return [
        area for area in areas
        if (not only or matches(area["id"], only)) and not matches(area["id"], skip)
    ]


def table(areas):
    lines = []
    for area in areas:
        hits = f" hits={area['hits']}" if "hits" in area else ""
        lines.append(
            f"{area['id']:<26} {area['kind']:<4} files={len(area['files']):<4} "
            f"kb={area['bytes'] // 1000:<5}{hits}  {area['description']}"
        )
    return "\n".join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--json", action="store_true", help="print areas with file lists as JSON")
    parser.add_argument("--pattern", action="append", default=[], help="Python regex; repeatable")
    parser.add_argument("--hits-only", action="store_true", help="drop files and areas without a hit")
    parser.add_argument("--include-history", action="store_true", help="add proposals and experiments")
    parser.add_argument("--budget", type=int, default=DEFAULT_BUDGET, help="read-area size limit in bytes")
    parser.add_argument("--max-hits", type=int, default=DEFAULT_MAX_HITS, help="hit limit per area")
    parser.add_argument("--only", action="append", default=[], help="keep areas with this id prefix")
    parser.add_argument("--skip", action="append", default=[], help="drop areas with this id prefix")
    parser.add_argument("--pack", action="store_true", help="merge adjacent small areas of one family")
    arguments = parser.parse_args(argv)
    if arguments.hits_only and not arguments.pattern:
        parser.error("--hits-only needs --pattern")
    patterns = [re.compile(p) for p in arguments.pattern]
    areas = build_areas(
        tracked_files(), patterns=patterns, include_history=arguments.include_history,
        budget=arguments.budget, max_hits=arguments.max_hits, hits_only=arguments.hits_only,
    )
    areas = select(areas, arguments.only, arguments.skip)
    if arguments.pack:
        areas = pack(areas, arguments.budget, arguments.max_hits)
    print(json.dumps(areas, indent=1) if arguments.json else table(areas))
    if any(area["id"].startswith("std-other") for area in areas):
        print("note: std-other holds modules missing from STD_THEMES", file=sys.stderr)


if __name__ == "__main__":
    sys.exit(main())
