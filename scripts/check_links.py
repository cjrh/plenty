#!/usr/bin/env python3
"""Check local Markdown link targets: BOOK_SRC [SUBDIR_OR_FILE].

Checks inline links, images, and reference definitions. Ignores code, comments,
remote URLs, and fragment-only links; does not validate heading anchors or HTML.
"""

import argparse
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit


# Destinations may be angle-wrapped, escaped, or contain one level of balanced
# parentheses. Optional link titles do not affect the destination.
DESTINATION = r"(?:<([^<>\n]+)>|((?:\\.|[^\\\s()]|\([^()\n]*\))+))"
INLINE_LINK = re.compile(r"(?<!\\)\]\(\s*" + DESTINATION)
REFERENCE = re.compile(r"^ {0,3}\[[^\]\n]+\]:\s*" + DESTINATION, re.MULTILINE)
INLINE_CODE = re.compile(r"(?<!`)(`+)(?!`)(.*?)\1(?!`)", re.DOTALL)


def blank(text):
    """Preserve offsets and line numbers while hiding non-prose content."""
    return re.sub(r"[^\n]", " ", text)


def prose(markdown):
    lines = []
    fence = None
    for line in markdown.splitlines(keepends=True):
        marker = re.match(r"^ {0,3}(`{3,}|~{3,})(.*)$", line.rstrip("\r\n"))
        if fence:
            lines.append(blank(line))
            if (
                marker
                and marker[1][0] == fence[0]
                and len(marker[1]) >= len(fence)
                and not marker[2].strip()
            ):
                fence = None
        elif marker:
            fence = marker[1]
            lines.append(blank(line))
        elif line.startswith("    ") or line.startswith("\t"):
            lines.append(blank(line))
        else:
            lines.append(line)
    text = re.sub(
        r"<!--.*?-->", lambda match: blank(match[0]), "".join(lines), flags=re.DOTALL
    )
    return INLINE_CODE.sub(lambda match: blank(match[0]), text)


def link_targets(markdown):
    text = prose(markdown)
    for pattern in (INLINE_LINK, REFERENCE):
        for match in pattern.finditer(text):
            target = match[1] if match[1] is not None else match[2]
            # Markdown escapes are resolved before URL decoding, so literal
            # parentheses and percent-encoded spaces refer to the real filename.
            target = re.sub(
                r"\\([!\"#$%&'()*+,\-./:;<=>?@\[\]^_`{|}~\\])", r"\1", target
            )
            yield markdown.count("\n", 0, match.start()) + 1, target


def check(root, scope):
    files = [scope] if scope.is_file() else sorted(scope.rglob("*.md"))
    missing = []
    checked = 0
    for page in files:
        for line, target in link_targets(page.read_text(encoding="utf-8")):
            url = urlsplit(target)
            if url.scheme or url.netloc or not url.path:
                continue
            path = unquote(url.path)
            destination = (
                root / path.lstrip("/") if path.startswith("/") else page.parent / path
            )
            checked += 1
            if not destination.exists():
                missing.append(f"{page.relative_to(root)}:{line}: missing target {target}")
    return len(files), checked, missing


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("book_src", type=Path)
    parser.add_argument("scope", nargs="?", default=".", type=Path)
    args = parser.parse_args(argv)
    root = args.book_src.resolve()
    scope = (root / args.scope).resolve()
    if not root.is_dir():
        parser.error(f"Book source directory does not exist: {root}")
    if not scope.is_relative_to(root) or not scope.exists():
        parser.error("Scope must be an existing file or directory within BOOK_SRC.")
    if scope.is_file() and scope.suffix != ".md":
        parser.error("A file scope must be Markdown (.md).")
    try:
        files, targets, missing = check(root, scope)
    except (OSError, ValueError) as error:
        parser.exit(2, f"{error}\n")
    for error in missing:
        print(error)
    print(f"Checked {files} Markdown files and {targets} local link targets; {len(missing)} missing.")
    return 1 if missing else 0


if __name__ == "__main__":
    sys.exit(main())
