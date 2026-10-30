#!/usr/bin/env python3
"""Assemble the plentylang/skills repository: OUT_DIR [--version X.Y.Z].

The `plenty` skill is developed in `.agents/skills/plenty/`, where its links
point at book pages in this checkout. Publishing copies it to
`skills/plenty/` with copies of the book pages it relies on (the syntax cheat
sheet, the signature index, every std module page, builtins, I/O and packages)
under `references/`, so the published tree is self-contained and matches one
compiler version. The marketplace and plugin manifests and the README exist
only in the published repository; their sources are in
`scripts/agent_skill_repo/`.

While copying, links to bundled pages become relative links inside OUT_DIR;
links to other pages keep only their text, since the published tree must not
depend on this repository. Contributor-only book passages and issue notes are
dropped.
"""

import argparse
import json
from pathlib import Path
import posixpath
import re
import shutil
import sys
import tomllib
from urllib.parse import urlsplit

from check_links import INLINE_LINK, prose

REPO = Path(__file__).resolve().parent.parent
SKILL_SOURCE = ".agents/skills/plenty"
REPO_FILES = REPO / "scripts/agent_skill_repo"
SKILL = "skills/plenty"
REFERENCES = f"{SKILL}/references"

# Book path (file or directory, relative to the repository) -> path in OUT_DIR.
BUNDLE = {
    "book/src/design/examples/cheatsheet.md": f"{REFERENCES}/cheatsheet.md",
    "book/src/design/toolchain/01-building/packages.md": f"{REFERENCES}/packages.md",
    "book/src/design/stdlib": f"{REFERENCES}/std",
    "book/src/design/facilities/01-builtins": f"{REFERENCES}/builtins",
    "book/src/design/facilities/02-io": f"{REFERENCES}/io",
}

# Book passages addressed to compiler contributors. See "Contributor-only
# passages" in book/src/development/agent-skill.md.
CONTRIBUTOR_OPEN = "<!-- contributors-only -->"
CONTRIBUTOR_CLOSE = "<!-- /contributors-only -->"
CONTRIBUTOR_BLOCK = re.compile(
    re.escape(CONTRIBUTOR_OPEN) + r".*?" + re.escape(CONTRIBUTOR_CLOSE) + r"\n*", re.DOTALL
)
# Notes such as `<!-- issue #575 -->` mark skill text that works around a local
# issue; published readers cannot see the register.
_NOTE = r"<!--(?:(?!-->).)*\bissues? #\d(?:(?!-->).)*-->"
ISSUE_NOTE_LINE = re.compile(r"^" + _NOTE + r"\n+", re.DOTALL | re.MULTILINE)
ISSUE_NOTE = re.compile(r"[ \t]*" + _NOTE, re.DOTALL)
# The in-repo skill describes the checkout's compiler; the copy names its release.
IN_REPO_VERSION = "the Plenty compiler in this repository"


def compiler_version():
    return tomllib.loads((REPO / "Cargo.toml").read_text())["package"]["version"]


def published_pages(repo=REPO):
    """Every copied page (relative to the repository) -> its output path."""
    pages = {}
    source = repo / SKILL_SOURCE
    for path in sorted(source.rglob("*")):
        if path.is_file():
            relative = path.relative_to(source).as_posix()
            pages[f"{SKILL_SOURCE}/{relative}"] = f"{SKILL}/{relative}"
    outputs = set(pages.values())
    for book_path, output in BUNDLE.items():
        source = repo / book_path
        if source.is_dir():
            found = {}
            for page in sorted(source.rglob("*.md")):
                relative = page.relative_to(source).as_posix()
                found[f"{book_path}/{relative}"] = f"{output}/{relative}"
        elif source.is_file():
            found = {book_path: output}
        else:
            raise SystemExit(f"bundled book path is missing: {book_path}")
        clash = outputs & set(found.values())
        if clash:
            raise SystemExit(f"bundled book pages would replace skill files: {sorted(clash)}")
        pages.update(found)
    return pages


def link_end(markdown, start):
    """Index just past the `)` closing the link destination starting at `start`."""
    depth = 1
    index = start
    while index < len(markdown):
        char = markdown[index]
        if char == "\\":
            index += 2
            continue
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
            if depth == 0:
                return index + 1
        index += 1
    raise ValueError("unterminated link")


def link_start(markdown, close):
    """Index of the `[` that opens the link text ending at `close`."""
    depth = 1
    index = close - 1
    while index >= 0:
        char = markdown[index]
        if char == "]":
            depth += 1
        elif char == "[" and (index == 0 or markdown[index - 1] != "\\"):
            depth -= 1
            if depth == 0:
                return index
        index -= 1
    raise ValueError("link text has no opening bracket")


def rewrite_links(markdown, page, pages):
    """Retarget the links of `page` (repository-relative) for its published copy."""
    output_dir = posixpath.dirname(pages[page])
    visible = prose(markdown)
    pieces = []
    position = 0
    for match in INLINE_LINK.finditer(visible):
        target = match[1] or match[2]
        parts = urlsplit(target)
        if parts.scheme or parts.netloc or not parts.path:
            continue
        close = match.start()
        opening = link_start(visible, close)
        end = link_end(visible, match.start() + 2)
        text = markdown[opening + 1:close]
        resolved = posixpath.normpath(posixpath.join(posixpath.dirname(page), parts.path))
        fragment = "#" + parts.fragment if parts.fragment else ""
        if resolved in pages:
            relative = posixpath.relpath(pages[resolved], output_dir)
            replacement = f"[{text}]({relative}{fragment})"
        else:
            # Images would be lost; the published pages have none today.
            if opening > 0 and markdown[opening - 1] == "!":
                raise ValueError(f"{page}: image {target} is not bundled")
            replacement = text
        pieces.append(markdown[position:opening])
        pieces.append(replacement)
        position = end
    pieces.append(markdown[position:])
    return "".join(pieces)


def strip_contributor_notes(markdown, page):
    if markdown.count(CONTRIBUTOR_OPEN) != markdown.count(CONTRIBUTOR_CLOSE):
        raise SystemExit(f"{page}: unbalanced {CONTRIBUTOR_OPEN} markers")
    return CONTRIBUTOR_BLOCK.sub("", markdown)


def published_page(page, pages, version, repo=REPO):
    markdown = (repo / page).read_text()
    markdown = strip_contributor_notes(markdown, page)
    markdown = ISSUE_NOTE.sub("", ISSUE_NOTE_LINE.sub("", markdown))
    if page.startswith(SKILL_SOURCE + "/"):
        if page == f"{SKILL_SOURCE}/SKILL.md":
            if markdown.count(IN_REPO_VERSION) != 1:
                raise SystemExit(f"{page} must name {IN_REPO_VERSION!r} once")
            markdown = markdown.replace(IN_REPO_VERSION, f"Plenty {version}")
        return rewrite_links(markdown, page, pages)
    markdown = re.sub(r"<!-- Generated by [^>]*-->\n\n?", "", markdown)
    header = (
        f"<!-- Copied from the Plenty {version} book page {page}. "
        "Edit the book, not this copy. -->\n\n"
    )
    return header + rewrite_links(markdown, page, pages)


def build(out_dir, version, repo=REPO):
    out_dir = Path(out_dir)
    if out_dir.exists():
        others = [child for child in out_dir.iterdir() if child.name != ".git"]
        if others and not (out_dir / ".claude-plugin").is_dir():
            raise SystemExit(f"{out_dir} exists and is not a skills tree; refusing to replace it")
        # Keep the published repository's history when syncing into a clone.
        for child in out_dir.iterdir():
            if child.name == ".git":
                continue
            if child.is_dir() and not child.is_symlink():
                shutil.rmtree(child)
            else:
                child.unlink()
    for source in sorted(REPO_FILES.rglob("*")):
        if source.is_dir():
            continue
        destination = out_dir / source.relative_to(REPO_FILES)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(source.read_text().replace("@VERSION@", version))
    for licence in ("LICENSE-APACHE", "LICENSE-MIT"):
        shutil.copy2(repo / licence, out_dir / licence)
    pages = published_pages(repo)
    for page, output in pages.items():
        destination = out_dir / output
        destination.parent.mkdir(parents=True, exist_ok=True)
        if page.endswith(".md"):
            destination.write_text(published_page(page, pages, version, repo))
        else:
            shutil.copy2(repo / page, destination)
    for manifest in (out_dir / ".claude-plugin").glob("*.json"):
        json.loads(manifest.read_text())
    return out_dir


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("out_dir")
    parser.add_argument("--version", default=None, help="defaults to Cargo.toml's version")
    arguments = parser.parse_args(argv)
    version = arguments.version or compiler_version()
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?", version):
        raise SystemExit(f"version must be MAJOR.MINOR.PATCH[-PRERELEASE]: {version}")
    print(build(arguments.out_dir, version))


if __name__ == "__main__":
    sys.exit(main())
