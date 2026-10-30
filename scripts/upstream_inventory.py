#!/usr/bin/env python3
"""List the upstream code, data and tools Plenty pins, for the upstream-check skill.

Everything is read from the tree, so the inventory follows the manifests:

- direct dependencies of every Cargo manifest, with the version its lockfile
  pins (git dependencies show their locked revision);
- vendored C libraries under plenty-runtime/native/NAME-VERSION;
- pinned data: the Unicode version std.unicode reports, and the official tz
  package tag the book pins (with the IANA tzdata release that tag bundles,
  when a tz checkout is found);
- toolchain pins: the mdBook version in book/releases.json, the release Rust
  toolchain in scripts/release/toolchain.env, and rust-toolchain files;
- every bullet of the std reference pages' Acknowledgements sections, with
  the versions it names.

Entries are grouped for one research agent each; see GROUPS. --advisories
also runs `cargo deny check advisories` on each lockfile and lists the
RustSec findings. Nothing is updated.

Usage: upstream_inventory.py [--json] [--group NAME ...] [--advisories] [--tz-repo DIR]
"""

import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib

REPO = Path(__file__).resolve().parent.parent
MANIFESTS = {
    # manifest -> lockfile; the runtime is a member of the root workspace.
    "Cargo.toml": "Cargo.lock",
    "plenty-runtime/Cargo.toml": "Cargo.lock",
    "plenty-dataframe/Cargo.toml": "plenty-dataframe/Cargo.lock",
    "plenty-linalg/Cargo.toml": "plenty-linalg/Cargo.lock",
}
NATIVE = "plenty-runtime/native"
ACK_PAGES = "book/src/design/stdlib"

# One research agent per group. Crates not listed fall into "support".
GROUPS = {
    "codegen": "cranelift-codegen cranelift-frontend cranelift-module cranelift-object "
    "cranelift-native regalloc2 target-lexicon object iced-x86",
    "crypto-tls": "hmac sha1 sha2 subtle rustls rustls-webpki rustls-pki-types ring blake3 siphasher",
    "compression": "zlib-rs brotli",
    "sqlite": "sqlite",
    "dataframe": "polars chrono",
    "linalg": "faer dyn-stack",
    "data": "unicode tz",
    "toolchain": "mdbook rust",
}
GROUP_OF = {name: group for group, names in GROUPS.items() for name in names.split()}
# Transitive crates important enough to list beside the direct dependencies.
TRANSITIVE = {"Cargo.toml": ("regalloc2",), "plenty-runtime/Cargo.toml": ("rustls-webpki", "rustls-pki-types")}
# Rerun rules from book/src/development/workflow.md#dependency-versions and
# the reference pages' update policies.
UPDATE_NOTES = {
    "codegen": "Rerun object-identity checks and compiler performance comparisons "
    "(book/src/development/workflow.md#dependency-versions).",
    "linalg": "faer is a fork; recheck allocation denial, residuals, concurrency and the "
    "non-AVX fallback (book/src/design/stdlib/linalg.md).",
    "compression": "Runtime crates are vendored unmodified in plenty-runtime/vendor and listed "
    "in build.rs VENDORED; Brotli updates need source/license/digest and security review "
    "(book/src/design/stdlib/compress-brotli.md).",
    "crypto-tls": "Runtime crates are vendored unmodified in plenty-runtime/vendor and listed "
    "in build.rs VENDORED; update steps in book/src/design/stdlib/tls.md#supply.",
    "sqlite": "Vendored C sources in plenty-runtime/native; build.rs names the directory.",
    "dataframe": "plenty-dataframe has its own Cargo.lock; check that an advisory's crate is "
    "reachable with the enabled features (cargo tree --locked -e normal -i CRATE in plenty-dataframe).",
    "data": "Every Unicode version named in the tree should agree unless a page explains why.",
}
# Versions, not licence names such as Apache-2.0 or Unicode-3.0.
VERSION = re.compile(r"(?<![\w.-])\d+\.\d+(?:\.\d+)?[a-z]?\b")


def group_of(name):
    if name.startswith("cranelift-"):
        return "codegen"
    return GROUP_OF.get(name, "support")


def locked_packages(lockfile):
    """name -> [(version, source)] from a Cargo.lock."""
    packages = {}
    for package in tomllib.loads(lockfile.read_text()).get("package", []):
        packages.setdefault(package["name"], []).append((package["version"], package.get("source", "")))
    return packages


def requirement(spec):
    if isinstance(spec, str):
        return spec
    if "git" in spec:
        ref = spec.get("branch") or spec.get("tag") or spec.get("rev") or "default branch"
        return f"{spec.get('version', '')} git {spec['git']} {ref}".strip()
    return spec.get("version", "path")


def crate_entries(repo=REPO):
    entries = []
    for manifest, lockfile in MANIFESTS.items():
        data = tomllib.loads((repo / manifest).read_text())
        locked = locked_packages(repo / lockfile)
        sections = [("dependencies", data.get("dependencies", {})),
                    ("dev-dependencies", data.get("dev-dependencies", {}))]
        sections.append(("transitive", {name: None for name in TRANSITIVE.get(manifest, ())}))
        for section, deps in sections:
            for name, spec in deps.items():
                package = spec.get("package", name) if isinstance(spec, dict) else name
                versions = locked.get(package, [])
                entries.append({
                    "group": group_of(package),
                    "name": package,
                    "kind": "crate",
                    "pinned": ", ".join(v for v, _ in versions) or "not locked",
                    "requirement": "" if spec is None else requirement(spec),
                    "source": ", ".join(sorted({s for _, s in versions if s.startswith("git+")})),
                    "where": f"{manifest} [{section}], {lockfile}",
                })
    return entries


def native_entries(repo=REPO):
    entries = []
    for path in sorted((repo / NATIVE).iterdir()):
        match = re.fullmatch(r"([a-z0-9_]+)-(\d+(?:\.\d+)+)", path.name)
        if path.is_dir() and match:
            entries.append({
                "group": group_of(match[1]),
                "name": match[1],
                "kind": "vendored C",
                "pinned": match[2],
                "requirement": "",
                "source": "",
                "where": f"{NATIVE}/{path.name}, build.rs",
            })
    return entries


def primary_worktree(repo=REPO):
    try:
        output = subprocess.run(
            ["git", "-C", str(repo), "worktree", "list", "--porcelain"],
            check=True, capture_output=True, text=True,
        ).stdout
    except (OSError, subprocess.CalledProcessError):
        return repo
    first = output.splitlines()[0] if output else ""
    return Path(first[len("worktree "):]) if first.startswith("worktree ") else repo


def data_entries(repo=REPO, tz_repo=None):
    entries = []
    unicode_source = (repo / "stdlib/std/unicode.plenty").read_text()
    match = re.search(r'return "(\d+\.\d+\.\d+)"', unicode_source)
    entries.append({
        "group": "data", "name": "Unicode Character Database", "kind": "data",
        "pinned": match[1] if match else "unknown", "requirement": "", "source": "",
        "where": "stdlib/std/unicode.plenty version(), plenty-runtime vendored tables; "
        "other Unicode versions named in Acknowledgements are listed there",
    })
    mentions = {}
    for root in ("book/src/design", "stdlib/std"):
        for path in sorted((repo / root).rglob("*")):
            if path.suffix in (".md", ".plenty") and path.is_file():
                for version in re.findall(r"Unicode \*{0,2}(\d+\.\d+\.\d+)", path.read_text()):
                    mentions.setdefault(version, []).append(path.relative_to(repo).as_posix())
    for version, paths in sorted(mentions.items()):
        entries.append({
            "group": "data", "name": f"Unicode {version} (named)", "kind": "data",
            "pinned": version, "requirement": "", "source": "",
            "where": ", ".join(dict.fromkeys(paths)),
        })
    tags = set()
    for page in ("book/src/design/toolchain/01-building/packages.md", "book/src/design/stdlib/time.md"):
        tags.update(re.findall(r'plentylang/tz\.git", tag = "(v[^"]+)"', (repo / page).read_text()))
    tz = {"group": "data", "name": "tz package (plentylang/tz)", "kind": "package",
          "pinned": ", ".join(sorted(tags)) or "unknown", "requirement": "", "source": "",
          "where": "book/src/design/toolchain/01-building/packages.md, book/src/design/stdlib/time.md, "
          "book/src/tutorial/07-modules/16-packages.md"}
    entries.append(tz)
    tz_repo = Path(tz_repo) if tz_repo else primary_worktree(repo).parent / "plentylang/tz"
    generator = "scripts/generate.py"
    for tag in sorted(tags) or [None]:
        text, where = None, f"tz checkout not found at {tz_repo}; pass --tz-repo"
        if tag and (tz_repo / ".git").exists():
            shown = subprocess.run(["git", "-C", str(tz_repo), "show", f"{tag}:{generator}"],
                                   capture_output=True, text=True)
            if shown.returncode == 0:
                text, where = shown.stdout, f"{tz_repo} {tag}:{generator} TZDB_VERSION"
        match = re.search(r'^TZDB_VERSION = "([^"]+)"', text or "", re.MULTILINE)
        entries.append({
            "group": "data", "name": "IANA tzdata", "kind": "data",
            "pinned": match[1] if match else "unknown", "requirement": "", "source": "",
            "where": where,
        })
    return entries


def toolchain_entries(repo=REPO):
    releases = json.loads((repo / "book/releases.json").read_text())
    entries = [{"group": "toolchain", "name": "mdbook", "kind": "tool",
                "pinned": releases.get("mdbook", "unknown"), "requirement": "", "source": "",
                "where": "book/releases.json"}]
    env = repo / "scripts/release/toolchain.env"
    if env.is_file():
        for key, value in re.findall(r"^([A-Z_]+_VERSION)=(\S+)", env.read_text(), re.MULTILINE):
            entries.append({"group": "toolchain", "name": key.removesuffix("_VERSION").lower(),
                            "kind": "tool", "pinned": value, "requirement": "", "source": "",
                            "where": "scripts/release/toolchain.env"})
    for name in ("rust-toolchain", "rust-toolchain.toml"):
        if (repo / name).is_file():
            entries.append({"group": "toolchain", "name": "rust (dev)", "kind": "tool",
                            "pinned": (repo / name).read_text().strip(), "requirement": "",
                            "source": "", "where": name})
    return entries


def acknowledgements(markdown):
    """Bullets of a page's final `## Acknowledgements` section."""
    match = re.search(r"^## Acknowledgements\n(.*?)(?=^#{1,2} |\Z)", markdown, re.MULTILINE | re.DOTALL)
    if not match:
        return []
    bullets, current = [], None
    for line in match[1].splitlines():
        if line.startswith("- "):
            current = line[2:].strip()
            bullets.append(current)
        elif current is not None and line.startswith("  ") and line.strip():
            bullets[-1] += " " + line.strip()
    return bullets


def ack_kind(bullet):
    text = bullet.lower()
    if text.startswith("wraps") or "vendored" in text:
        return "wrapped"
    if any(word in text for word in ("ported", "generated from", "tables", "data from", "derived")):
        return "ported"
    return "modelled"


LINK = re.compile(r"\[([^\]]+)\]\((https?://[^)\s]+)\)")


def ack_items(bullet):
    """(name, url, version) for each linked upstream in a bullet.

    A version is the one written right after the link (`[sha2](...) 0.10.9`);
    a bullet without one yields its first link and the bullet's first version.
    """
    items = []
    for match in LINK.finditer(bullet):
        version = re.match(r"\s+(" + VERSION.pattern + ")", bullet[match.end():])
        if version:
            items.append((match[1], match[2], version[1]))
    if not items:
        first = LINK.search(bullet)
        versions = VERSION.findall(LINK.sub("", bullet))
        version = versions[0] if versions else ""
        items.append((first[1], first[2], version) if first else (bullet[:40], "", version))
    return [(name.replace("`", "").replace('"', ""), url, version) for name, url, version in items]


def ack_entries(repo=REPO):
    entries = []
    for page in sorted((repo / ACK_PAGES).rglob("*.md")):
        for bullet in acknowledgements(page.read_text()):
            for name, url, version in ack_items(bullet):
                entries.append({
                    "group": "acknowledgements",
                    "name": name,
                    "kind": ack_kind(bullet),
                    "pinned": version or "-",
                    "requirement": "",
                    "source": url,
                    "where": page.relative_to(repo).as_posix(),
                    "text": bullet,
                })
    return entries


def inventory(repo=REPO, tz_repo=None):
    return (crate_entries(repo) + native_entries(repo) + data_entries(repo, tz_repo)
            + toolchain_entries(repo) + ack_entries(repo))


def parse_deny(lines, lockfile):
    """Advisory findings from `cargo deny --format json` output lines."""
    findings = []
    for line in lines:
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        fields = record.get("fields", {})
        advisory = fields.get("advisory")
        if record.get("type") != "diagnostic" or not advisory:
            continue
        spans = [label.get("span", "") for label in fields.get("labels", [])]
        solution = next((n for n in fields.get("notes", []) if n.startswith("Solution:")), "")
        findings.append({
            "lockfile": lockfile,
            "id": advisory.get("id"),
            "package": spans[0].split(" registry+")[0].split(" git+")[0] if spans else advisory.get("package"),
            "title": advisory.get("title"),
            "kind": advisory.get("informational") or "vulnerability",
            "aliases": advisory.get("aliases", []),
            "solution": solution.removeprefix("Solution: "),
        })
    return findings


def advisories(repo=REPO):
    findings, errors = [], []
    for manifest, lockfile in dict.fromkeys((m, l) for m, l in MANIFESTS.items() if m != "plenty-runtime/Cargo.toml"):
        try:
            result = subprocess.run(
                ["cargo", "deny", "--format", "json", "--manifest-path", str(repo / manifest),
                 "check", "--hide-inclusion-graph", "advisories"],
                capture_output=True, text=True, timeout=600,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            errors.append(f"{lockfile}: {error}")
            continue
        found = parse_deny(result.stderr.splitlines() + result.stdout.splitlines(), lockfile)
        if result.returncode not in (0, 1) or (result.returncode == 1 and not found):
            errors.append(f"{lockfile}: cargo deny exited {result.returncode}: {result.stderr.strip()[-400:]}")
        findings.extend(found)
    return findings, errors


def drift(entries):
    """Wrapped upstreams whose acknowledged version differs from the pin."""
    pins = {}
    for e in entries:
        if e["kind"] in ("crate", "vendored C"):
            pins.setdefault(e["name"].lower(), set()).add(e["pinned"])
    return [
        {"name": e["name"], "acknowledged": e["pinned"], "pinned": sorted(pins[e["name"].lower()]),
         "where": e["where"]}
        for e in entries
        if e["group"] == "acknowledgements" and e["kind"] == "wrapped" and e["pinned"] != "-"
        and e["name"].lower() in pins and e["pinned"] not in pins[e["name"].lower()]
    ]


def table(entries):
    lines = []
    order = [*GROUPS, "acknowledgements", "support"]
    for group in sorted(dict.fromkeys(e["group"] for e in entries), key=order.index):
        lines.append(f"## {group}")
        if group in UPDATE_NOTES:
            lines.append(f"   update rule: {UPDATE_NOTES[group]}")
        for e in (e for e in entries if e["group"] == group):
            extra = f" ({e['source']})" if e["source"] and e["kind"] == "crate" else ""
            lines.append(f"   {e['name']:<28} {e['kind']:<10} {e['pinned'] or '-':<14} {e['where']}{extra}")
    return "\n".join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--group", action="append", default=[], help="only this group; repeatable")
    parser.add_argument("--advisories", action="store_true", help="also run cargo deny on each lockfile")
    parser.add_argument("--tz-repo", help="checkout of plentylang/tz (default: beside the primary worktree)")
    arguments = parser.parse_args(argv)
    everything = inventory(tz_repo=arguments.tz_repo)
    entries = [e for e in everything if not arguments.group or e["group"] in arguments.group]
    result = {"entries": entries, "update_notes": UPDATE_NOTES, "drift": drift(everything)}
    if arguments.advisories:
        result["advisories"], result["advisory_errors"] = advisories()
    if arguments.json:
        print(json.dumps(result, indent=1))
        return 0
    print(table(entries))
    print("## acknowledged version differs from pin")
    for item in result["drift"]:
        print(f"   {item['name']:<28} says {item['acknowledged']}, pinned {', '.join(item['pinned'])}: {item['where']}")
    if not result["drift"]:
        print("   none")
    if arguments.advisories:
        print("## advisories (cargo deny)")
        for finding in result["advisories"]:
            print(f"   {finding['id']:<18} {finding['kind']:<14} {finding['package']:<24} "
                  f"{finding['lockfile']}: {finding['title']} [{finding['solution']}]")
        for error in result["advisory_errors"]:
            print(f"   error: {error}")
        if not result["advisories"] and not result["advisory_errors"]:
            print("   none")
    return 0


if __name__ == "__main__":
    sys.exit(main())
