#!/usr/bin/env python3
"""Check the generated Pages artifact, including links the source check cannot see."""

import argparse
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit


class Links(HTMLParser):
    def __init__(self):
        super().__init__()
        self.urls = []

    def handle_starttag(self, tag, attrs):
        self.urls.extend(value for key, value in attrs if key in ("href", "src") and value)


def check(root, prefix="/plenty/"):
    root = root.resolve()
    errors = []
    for pattern in ("index.html", "404.html", "print.html", "searchindex-*.js",
                    "searcher-*.js", "highlight-*.js", "theme/plenty-highlight*.js",
                    "theme/plenty-highlight*.css"):
        if not any(root.glob(pattern)):
            errors.append(f"missing required output: {pattern}")
    for path in root.rglob("*"):
        if path.is_symlink() or path.name in ("issues.db", ".git", ".github", "Cargo.toml"):
            errors.append(f"unexpected artifact entry: {path.relative_to(root)}")
    pages = list(root.rglob("*.html"))
    for page in pages:
        parser = Links()
        parser.feed(page.read_text(encoding="utf-8"))
        for value in parser.urls:
            url = urlsplit(value)
            if url.scheme or url.netloc or not url.path:
                continue
            path = unquote(url.path)
            if path.startswith("/"):
                if not path.startswith(prefix):
                    errors.append(f"{page.relative_to(root)}: outside site prefix: {value}")
                    continue
                target = root / path[len(prefix):]
            else:
                target = page.parent / path
            target = target.resolve()
            if not target.is_relative_to(root) or not target.exists():
                errors.append(f"{page.relative_to(root)}: missing/escaping target: {value}")
    return len(pages), sorted(set(errors))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    count, errors = check(args.directory)
    for error in errors:
        print(error)
    print(f"Checked {count} generated HTML pages; {len(errors)} errors.")
    raise SystemExit(bool(errors))
