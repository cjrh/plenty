#!/usr/bin/env python3
"""Check local Markdown link targets: BOOK_SRC [SUBDIR_OR_FILE].

Checks inline links, images, and reference definitions. Ignores code, comments,
remote URLs, and fragment-only links; does not validate heading anchors or HTML.
Local targets must stay within BOOK_SRC to be published with the book.
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
LIST_ITEM = re.compile(r"^([-+*]|\d{1,9}[.)])(?:( +)(?=\S)|\s*$)")
HEADING_OR_RULE = re.compile(r"^(#{1,6}(\s|$)|([-*_])(\s*\3){2,}\s*$)")
INLINE_CODE = re.compile(r"(?<!`)(`+)(?!`)(.*?)\1(?!`)", re.DOTALL)


def blank(text):
    """Preserve offsets and line numbers while hiding non-prose content."""
    return re.sub(r"[^\n]", " ", text)


def prose(markdown):
    lines = []
    fence = None
    # Content columns of open list items, innermost last. Indentation inside a
    # list item is measured from its content column, as in CommonMark.
    items = []
    paragraph = False
    previous_blank = True
    for line in markdown.splitlines(keepends=True):
        text = line.rstrip("\r\n").expandtabs(4)
        if fence:
            lines.append(blank(line))
            closing = re.match(r"^\s*(`{3,}|~{3,})\s*$", text)
            if closing and closing[1][0] == fence[0] and len(closing[1]) >= len(fence):
                fence = None
            continue
        if not text.strip():
            lines.append(line)
            paragraph, previous_blank = False, True
            continue
        indent = len(text) - len(text.lstrip(" "))
        rule = HEADING_OR_RULE.match(text[indent:])
        item = not rule and LIST_ITEM.match(text[indent:])
        # A less indented line closes list items unless it lazily continues
        # their paragraph.
        while items and indent < items[-1] and (previous_blank or item or not paragraph):
            items.pop()
        relative = indent - (items[-1] if items else 0)
        previous_blank = False
        # An indented code block cannot interrupt a paragraph, so an indented
        # line after paragraph text is a lazy continuation.
        if relative >= 4 and not paragraph:
            lines.append(blank(line))
            continue
        marker = re.match(r"^(`{3,}|~{3,})(.*)$", text[indent:])
        if marker and not (marker[1][0] == "`" and "`" in marker[2]):
            fence = marker[1]
            lines.append(blank(line))
            paragraph = False
            continue
        lines.append(line)
        if item:
            spaces = len(item[2] or "")
            width = len(item[1]) + (spaces if 1 <= spaces <= 4 else 1)
            items.append(indent + width)
            paragraph = bool(text[indent + len(item[0]):].strip())
        else:
            paragraph = not rule
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
            if not destination.resolve().is_relative_to(root.resolve()):
                missing.append(
                    f"{page.relative_to(root)}:{line}: target outside book source {target}"
                )
            elif not destination.exists():
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
