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


BOOK_ASSETS = ("index.html", "404.html", "print.html", "searchindex-*.js",
               "searcher-*.js", "highlight-*.js", "theme/plenty-highlight*.js",
               "theme/plenty-highlight*.css")
SITE_ASSETS = ("index.html", "404.html", "assets/site.css", "assets/plenty.svg",
               "assets/plenty.png", "docs/versions.json")


def check(root, prefix="/", *, site_root=None, site_prefix=None, required=BOOK_ASSETS):
    root = root.resolve()
    site_root = (site_root or root).resolve()
    site_prefix = site_prefix or prefix
    errors = []
    for pattern in required:
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
                if not path.startswith(site_prefix):
                    errors.append(f"{page.relative_to(root)}: outside site prefix: {value}")
                    continue
                target = site_root / path[len(site_prefix):]
            else:
                target = page.parent / path
            target = target.resolve()
            if not target.is_relative_to(site_root) or not target.exists():
                errors.append(f"{page.relative_to(root)}: missing/escaping target: {value}")
    return len(pages), sorted(set(errors))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--prefix", default="/", help="Published site mount path")
    args = parser.parse_args()
    required = SITE_ASSETS if (args.directory / "docs/versions.json").is_file() else BOOK_ASSETS
    count, errors = check(args.directory, args.prefix, required=required)
    for error in errors:
        print(error)
    print(f"Checked {count} generated HTML pages; {len(errors)} errors.")
    raise SystemExit(bool(errors))
