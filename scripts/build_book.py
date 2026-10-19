#!/usr/bin/env python3
"""Build an mdBook with visible, source-derived version metadata on every page."""

import argparse
import html
import json
import os
from pathlib import Path
import subprocess
import sys
import tomllib


def git(repo, *args):
    return subprocess.check_output(["git", "-C", str(repo), *args], text=True).strip()


def development_metadata(repo):
    version = tomllib.loads((repo / "Cargo.toml").read_text())["package"]["version"]
    revision = git(repo, "rev-parse", "HEAD")
    dirty = bool(git(repo, "status", "--porcelain", "--untracked-files=normal"))
    label = f"Development · {revision[:12]}" + (" · local changes" if dirty else "")
    return dict(label=label, revision=revision, package_version=version,
                channel="development", dirty=dirty)


def decorate(output, metadata):
    """Static label remains visible without JavaScript, including on print/404."""
    banner = ('<aside class="docs-version" aria-label="Documentation version">'
              f'{html.escape(metadata["label"])}'
              '</aside>')
    style = ('<style id="docs-version-style">.docs-version {padding:.65em 0;'
             'margin-bottom:1.2em;border-bottom:1px solid currentColor;'
             'font-size:.9em;overflow-wrap:anywhere}</style>')
    for page in output.rglob("*.html"):
        if page == output / "toc.html":
            continue  # mdBook's sidebar fragment, not a standalone page.
        content = page.read_text(encoding="utf-8")
        if "<main>" not in content or "</head>" not in content:
            raise ValueError(f"Unsupported mdBook page layout: {page}")
        content = content.replace("</head>", style + "\n</head>", 1)
        content = content.replace("<main>", "<main>\n" + banner, 1)
        page.write_text(content, encoding="utf-8")
    (output / "docs-version.json").write_text(json.dumps(metadata, indent=2) + "\n")


def build_book(source, output, prefix, metadata):
    env = dict(os.environ, MDBOOK_OUTPUT__HTML__SITE_URL=prefix)
    result = subprocess.run(["mdbook", "build", str(source / "book"), "--dest-dir", str(output)],
                            env=dict(env, RUST_LOG="info"), check=True,
                            text=True, capture_output=True)
    print(result.stdout, end="")
    print(result.stderr, end="", file=sys.stderr)
    # mdBook0.5.4 logs missing {{#include}} files as ERROR but exits successfully.
    if "ERROR" in result.stderr:
        raise ValueError("mdBook logged an error; refusing incomplete documentation")
    decorate(output, metadata)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path)
    parser.add_argument("--prefix", default="/plenty/")
    args = parser.parse_args()
    source = args.source.resolve()
    output = (args.output or source / "book/book").resolve()
    metadata = development_metadata(source)
    build_book(source, output, args.prefix, metadata)
    print(metadata["label"])


if __name__ == "__main__":
    main()
