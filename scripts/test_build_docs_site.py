import json
import io
from contextlib import redirect_stderr
from pathlib import Path
import subprocess
import shutil
import tempfile
import tarfile
import unittest
from unittest.mock import patch

from build_book import build_book
from check_book_output import SITE_ASSETS, check
from build_docs_site import add_navigation, assemble, destination, extract_release, load_releases, redirect_page


class DocsSiteTests(unittest.TestCase):
    def test_homepage_docs_versions_and_legacy_links_share_one_checked_artifact(self):
        def book(source, output, prefix, metadata):
            output.mkdir(parents=True, exist_ok=True)
            pages = ["index.html", "404.html", "print.html", "tutorial/index.html",
                     "tutorial/00-setting-up.html", "design/index.html", "nested/chapter.html"]
            if metadata["channel"] == "development":
                pages.append("development-only.html")
            for chapter in pages:
                path = output / chapter
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('<html><head><title>Book</title></head><body>'
                                '<aside>Version</aside><main>Chapter</main></body></html>')
            for name in ("searchindex-test.js", "searcher-test.js", "highlight-test.js",
                         "theme/plenty-highlight-test.js", "theme/plenty-highlight-test.css"):
                path = output / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.touch()

        manifest = dict(stable="0.1.0", releases=[dict(version="0.1.0", tag="v0.1.0", commit="a" * 40)])
        for prefix in ("/", "/plenty/"):
            with self.subTest(prefix=prefix), tempfile.TemporaryDirectory() as directory:
                repo = Path(directory)
                shutil.copytree(Path(__file__).resolve().parents[1] / "website", repo / "website")
                shutil.copytree(Path(__file__).resolve().parents[1] / "logos", repo / "logos")
                output = repo / "site"
                with patch('build_docs_site.development_metadata', return_value=dict(label="Development", channel="development")), \
                     patch('build_docs_site.extract_release'), patch('build_docs_site.build_book', side_effect=book):
                    assemble(repo, output, prefix, manifest)
                homepage = (output / "index.html").read_text()
                self.assertIn("Native executables.", homepage)
                self.assertIn(f'src="{prefix}assets/plenty_heptagon.svg"', homepage)
                self.assertIn(f'href="{prefix}docs/"', homepage)
                self.assertNotIn("@SITE_PREFIX@", homepage)
                self.assertTrue((output / "assets/site.css").is_file())
                self.assertEqual((output / "assets/plenty.svg").read_bytes(), (repo / "logos/plenty.svg").read_bytes())
                self.assertEqual((output / "assets/plenty.png").read_bytes(), (repo / "logos/plenty.png").read_bytes())
                for extension in ("svg", "png"):
                    name = f"plenty_heptagon.{extension}"
                    self.assertEqual((output / "assets" / name).read_bytes(), (repo / "logos" / name).read_bytes())
                self.assertIn(f'href="{prefix}">Plenty home</a>', (output / "docs/index.html").read_text())
                versions = json.loads((output / "docs/versions.json").read_text())
                self.assertEqual([v["url"] for v in versions], [prefix + "docs/", prefix + "docs/0.1.0/", prefix + "docs/dev/"])
                for chapter in ("nested/chapter.html", "dev/index.html", "0.1.0/index.html"):
                    text = (output / chapter).read_text()
                    self.assertIn(f'href="{prefix}docs/{chapter}"', text)
                    self.assertIn('+location.search+location.hash', text)
                self.assertIn(f'href="{prefix}docs/dev/development-only.html"',
                              (output / "docs/development-only.html").read_text())
                fallback = (output / "404.html").read_text()
                self.assertIn(prefix + "docs/dev/404.html", fallback)
                self.assertIn(f'href="{prefix}assets/plenty_heptagon.svg"', fallback)
                self.assertIn('location.replace(destination + location.search + location.hash)', fallback)
                self.assertNotIn("@DOCS_ROUTES@", fallback)
                (output / "assets/plenty_heptagon.svg").unlink()
                _, errors = check(output, prefix, required=SITE_ASSETS)
                self.assertTrue(any("assets/plenty_heptagon.svg" in error for error in errors))

    def test_selector_preserves_chapter_or_falls_back_without_script(self):
        for prefix in ("/", "/plenty/"):
            with self.subTest(prefix=prefix):
                versions = [dict(id="default", label="Stable", url=prefix, pages={"index.html", "nested/chapter.html"}),
                            dict(id="old", label="Plenty 0.1.0", url=prefix + "0.1.0/", pages={"index.html"})]
                self.assertEqual(destination(versions[0], "nested/chapter.html"), prefix + "nested/chapter.html")
                self.assertEqual(destination(versions[1], "nested/chapter.html"), prefix + "0.1.0/index.html")
                with tempfile.TemporaryDirectory() as directory:
                    root = Path(directory)
                    for chapter in versions[0]["pages"]:
                        page = root / chapter
                        page.parent.mkdir(parents=True, exist_ok=True)
                        page.write_text('<body><aside>Stable</aside><main>Chapter</main></body>')
                    add_navigation(root, versions[0], versions, prefix)
                    text = (root / "nested/chapter.html").read_text()
                    self.assertIn(f'href="{prefix}nested/chapter.html" data-same-chapter="true"', text)
                    self.assertIn(f'href="{prefix}0.1.0/index.html">', text)
                    self.assertIn(f'<script src="{prefix}docs-switcher.js">', text)
                    self.assertIn('<details', text)

    def test_migration_redirect_has_no_javascript_fallback_and_preserves_fragment(self):
        text = redirect_page('/plenty/dev/new.html')
        self.assertIn('href="/plenty/dev/new.html"', text)
        self.assertIn('+location.search+location.hash', text)

    def test_missing_include_error_is_fatal_even_when_mdbook_succeeds(self):
        result = subprocess.CompletedProcess([], 0, stdout="", stderr="ERROR missing include\n")
        with patch('build_book.subprocess.run', return_value=result), redirect_stderr(io.StringIO()):
            with self.assertRaisesRegex(ValueError, "mdBook logged an error"):
                build_book(Path('/unused'), Path('/unused/output'), '/plenty/', {})

    def test_manifest_rejects_moved_tags_and_unknown_stable(self):
        release = dict(version="0.1.1", tag="v0.1.1", commit="a" * 40)
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            (repo / 'book').mkdir()
            path = repo / 'book/releases.json'
            path.write_text(json.dumps(dict(stable="0.1.1", mdbook="0.5.4", releases=[release])))
            with patch('build_docs_site.git', return_value='b' * 40):
                with self.assertRaisesRegex(ValueError, "Tag moved"):
                    load_releases(repo)
            with patch('build_docs_site.git', return_value='a' * 40):
                path.write_text(json.dumps(dict(stable="0.2.0", releases=[release])))
                with self.assertRaisesRegex(ValueError, "Stable must"):
                    load_releases(repo)

    def test_manifest_allows_no_released_version_and_rejects_duplicates_or_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            (repo / 'book').mkdir()
            path = repo / 'book/releases.json'
            manifest = dict(stable=None, mdbook="0.5.4", releases=[])
            path.write_text(json.dumps(manifest))
            with patch('build_docs_site.subprocess.check_output', return_value='mdbook v0.5.4\n'):
                self.assertEqual(load_releases(repo), manifest)
            manifest['releases'] = [dict(version='../escape')]
            path.write_text(json.dumps(manifest))
            with self.assertRaisesRegex(ValueError, "stable numeric"):
                load_releases(repo)
            manifest['releases'] = [dict(version='0.1.1', tag='v0.1.1', commit='a'*40)] * 2
            path.write_text(json.dumps(manifest))
            with patch('build_docs_site.git', return_value='a'*40):
                with self.assertRaisesRegex(ValueError, "Duplicate"):
                    load_releases(repo)

    def test_release_identity_and_external_include_source(self):
        archive = io.BytesIO()
        files = {'Cargo.toml': '[package]\nversion = "0.1.1"\n',
                 'book/src/SUMMARY.md': '- [Lesson](tutorial/lesson.md)\n',
                 'plenty-runtime/README.md': 'Historical included content'}
        with tarfile.open(fileobj=archive, mode='w') as tar:
            for name, content in files.items():
                data = content.encode()
                member = tarfile.TarInfo(name)
                member.size = len(data)
                tar.addfile(member, io.BytesIO(data))
        release = dict(commit='a'*40, version='0.1.1', summary_indent_repairs=['tutorial/lesson.md'])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch('build_docs_site.subprocess.check_output', return_value=archive.getvalue()):
                extract_release(root, release, root / 'source')
                self.assertEqual((root / 'source/plenty-runtime/README.md').read_text(), 'Historical included content')
                self.assertTrue((root / 'source/book/src/SUMMARY.md').read_text().startswith('        -'))
                with self.assertRaisesRegex(ValueError, 'version mismatch'):
                    extract_release(root, dict(release, version='0.2.0'), root / 'mismatch')
                with self.assertRaisesRegex(ValueError, 'repair no longer applies'):
                    extract_release(root, dict(release, summary_indent_repairs=['missing.md']), root / 'repair')
