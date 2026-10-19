#!/usr/bin/env python3
"""Assemble stable, tagged and development books into one complete Pages artifact."""

import argparse
import html
import io
import json
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import tomllib

from build_book import build_book, development_metadata, git
from check_book_output import check


def load_releases(repo, path=None):
    manifest = json.loads((path or repo / "book/releases.json").read_text())
    versions = set()
    for release in manifest["releases"]:
        version = release["version"]
        if not re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", version):
            raise ValueError(f"Release must be a stable numeric version: {version}")
        if version in versions:
            raise ValueError(f"Duplicate release: {version}")
        versions.add(version)
        if release["tag"] != "v" + version or not re.fullmatch(r"[0-9a-f]{40}", release["commit"]):
            raise ValueError(f"Invalid release identity: {version}")
        if git(repo, "rev-parse", release["tag"] + "^{commit}") != release["commit"]:
            raise ValueError(f"Tag moved since admission: {release['tag']}")
    if manifest["stable"] is not None and manifest["stable"] not in versions:
        raise ValueError("Stable must name an admitted release")
    if subprocess.check_output(["mdbook", "--version"], text=True).strip() != "mdbook v" + manifest["mdbook"]:
        raise ValueError("Install the mdBook version pinned in book/releases.json")
    return manifest


def extract_release(repo, release, destination):
    # Includes may refer outside book/ (the runtime README does today). Extract
    # the exact tracked tree; only mdBook's output enters the Pages artifact.
    archive = subprocess.check_output(["git", "-C", str(repo), "archive", release["commit"]])
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        # Only ordinary source files/directories; reject links even inside the tree.
        if any(not member.isfile() and not member.isdir() for member in tar.getmembers()):
            raise ValueError("Release documentation archive contains a special file")
        tar.extractall(destination, filter="data")
    version = tomllib.loads((destination / "Cargo.toml").read_text())["package"]["version"]
    if version != release["version"]:
        raise ValueError(f"Tag/package version mismatch: {version}")
    # v0.1.1 predates the SUMMARY repair in PR346. This explicit rendering-only
    # overlay restores omitted chapters without substituting current prose.
    summary = destination / "book/src/SUMMARY.md"
    content = summary.read_text()
    for path in release.get("summary_indent_repairs", []):
        pattern = r"(?m)^(- \[[^\n]+\]\(" + re.escape(path) + r"\))$"
        content, count = re.subn(pattern, r"        \1", content)
        if count != 1:
            raise ValueError(f"Historical SUMMARY repair no longer applies: {path}")
    summary.write_text(content)


def page_names(root):
    return {p.relative_to(root).as_posix() for p in root.rglob("*.html") if p.name != "toc.html"}


def destination(version, chapter):
    return version["url"] + (chapter if chapter in version["pages"] else "index.html")


def add_navigation(root, current, versions, prefix):
    for chapter in current["pages"]:
        page = root / chapter
        links = []
        for version in versions:
            target = destination(version, chapter)
            same_page = chapter in version["pages"]
            links.append('<li><a href="' + html.escape(target, quote=True) + '"'
                         + (' data-same-chapter="true"' if same_page else '')
                         + (' aria-current="true"' if current["id"] == version["id"] else '')
                         + '>' + html.escape(version["label"]) + '</a></li>')
        navigation = ('<details class="docs-versions"><summary>Choose documentation version</summary>'
                      '<ul>' + ''.join(links) + '</ul></details>')
        content = page.read_text()
        content = content.replace('</aside>', navigation + '</aside>', 1)
        content = content.replace('</body>', '<script src="' + prefix + 'docs-switcher.js"></script>\n</body>', 1)
        page.write_text(content)


def redirect_page(target):
    safe = html.escape(target, quote=True)
    return ('<!doctype html><html lang="en"><head><meta charset="utf-8">'
            '<title>Documentation moved</title></head><body>'
            '<p>This page is available in development documentation. '
            f'<a id="destination" href="{safe}">Continue to the page</a>.</p>'
            '<script>location.replace(document.getElementById("destination").href'
            '+location.search+location.hash);</script></body></html>')


def assemble(repo, output, prefix, manifest):
    versions = []
    stable = next((r for r in manifest["releases"] if r["version"] == manifest["stable"]), None)
    development = development_metadata(repo)
    if stable is None:
        build_book(repo, output, prefix, development)
        versions.append(dict(id="default", label=development["label"] + " (default)",
                             url=prefix, pages=page_names(output), root=output))
    # Root is a complete stable book, so existing chapter URLs keep working.
    ordered = ([stable] if stable else []) + sorted(manifest["releases"], key=lambda r: tuple(map(int, r["version"].split('.'))), reverse=True)
    with tempfile.TemporaryDirectory(prefix="plenty-docs-sources-") as directory:
        sources = Path(directory)
        for index, release in enumerate(ordered):
            alias = stable is not None and index == 0
            source = sources / str(index)
            extract_release(repo, release, source)
            version = release["version"]
            label = "Plenty " + version + (" (latest stable)" if alias else "")
            path = output if alias else output / version
            url = prefix if alias else prefix + version + "/"
            metadata = dict(label=label, channel="release", package_version=version,
                            revision=release["commit"], tag=release["tag"], dirty=False,
                            summary_indent_repairs=release.get("summary_indent_repairs", []))
            build_book(source, path, url, metadata)
            versions.append(dict(id="default" if alias else version, label=label,
                                 url=url, pages=page_names(path), root=path))
    build_book(repo, output / "dev", prefix + "dev/", development)
    versions.append(dict(id="dev", label=development["label"], url=prefix + "dev/",
                         pages=page_names(output / "dev"), root=output / "dev"))
    for version in versions:
        add_navigation(version["root"], version, versions, prefix)
    # GitHub serves the site's root404 for all missing paths, even inside dev/.
    # Hand off known version prefixes to their own correctly labelled404 page.
    routes = {v["url"]: v["url"] + "404.html" for v in versions if v["id"] != "default"}
    script = '''"use strict";
document.querySelectorAll(".docs-versions a[data-same-chapter]").forEach(link => {
    link.href += location.search + location.hash;
});
const routes = ROUTES;
const path = location.pathname;
if (document.title.startsWith("Page not found")) {
    for (const [prefix, target] of Object.entries(routes)) {
        if (path.startsWith(prefix) && path !== target) {
            location.replace(target);
            break;
        }
    }
}
'''.replace("ROUTES", json.dumps(routes))
    (output / "docs-switcher.js").write_text(script)
    for chapter in versions[-1]["pages"] - versions[0]["pages"]:
        target = output / chapter
        if target.exists():
            raise ValueError(f"Development redirect collides with stable asset: {chapter}")
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(redirect_page(prefix + "dev/" + chapter))
    catalog = [{k: sorted(v) if k == "pages" else v for k, v in version.items() if k != "root"} for version in versions]
    (output / "versions.json").write_text(json.dumps(catalog, indent=2) + "\n")
    for version in versions:
        count, errors = check(version["root"], version["url"], site_root=output, site_prefix=prefix)
        if errors:
            raise ValueError("\n".join(errors[:30]))
        print(f"{version['id']}: checked {count} HTML pages")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path)
    parser.add_argument("--prefix", default="/plenty/")
    parser.add_argument("--manifest", type=Path, help="Alternate release manifest for admission checks")
    args = parser.parse_args()
    repo = args.source.resolve()
    output = (args.output or repo / "book/site").resolve()
    if not re.fullmatch(r"/(?:[A-Za-z0-9_-]+/)+", args.prefix):
        raise ValueError("Prefix must be a site path such as /plenty/")
    if output.exists() and (not (output / "versions.json").is_file()):
        raise ValueError("Refusing to replace a directory not built by this helper")
    manifest = load_releases(repo, args.manifest)
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".docs-site-", dir=output.parent) as directory:
        staging = Path(directory) / "site"
        assemble(repo, staging, args.prefix, manifest)
        # Failed builds leave the previous complete artifact intact.
        if output.exists():
            shutil.rmtree(output)
        shutil.move(str(staging), output)
    print(f"Complete Pages site: {output}")


if __name__ == "__main__":
    main()
