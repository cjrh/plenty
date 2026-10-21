import json
from pathlib import Path
import tempfile
import subprocess
import unittest
from unittest.mock import patch

from build_book import build_book, decorate


class BookVersionTests(unittest.TestCase):
    def test_static_label_on_nested_print_and_missing_pages(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = [root / name for name in ("index.html", "print.html", "404.html", "nested/page.html")]
            for page in paths:
                page.parent.mkdir(parents=True, exist_ok=True)
                page.write_text('<html><head></head><body><main><h1>Keep me</h1></main></body></html>')
            metadata = {"label": "Development · abc123 · <local>", "revision": "abc123"}
            (root / "toc.html").write_text('<ol><li>Sidebar fragment</li></ol>')
            decorate(root, metadata)
            self.assertEqual((root / "toc.html").read_text(), '<ol><li>Sidebar fragment</li></ol>')
            for page in paths:
                content = page.read_text()
                self.assertIn('aria-label="Documentation version"', content)
                self.assertIn("&lt;local&gt;", content)
                self.assertIn("<h1>Keep me</h1>", content)
            self.assertEqual(json.loads((root / "docs-version.json").read_text()), metadata)

    def test_template_change_fails_instead_of_silently_omitting_label(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "index.html").write_text('<html><head></head><body>No main</body></html>')
            with self.assertRaisesRegex(ValueError, "Unsupported mdBook"):
                decorate(root, {"label": "Development"})

    def test_build_preserves_configured_redirect_and_labels_destination(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "book").mkdir()
            (root / "book/book.toml").write_text(
                '[output.html.redirect]\n"old/chapter.html" = "new/chapter.html"\n')
            output = root / "output"
            old = output / "old/chapter.html"
            new = output / "new/chapter.html"
            old.parent.mkdir(parents=True)
            new.parent.mkdir(parents=True)
            redirect = '<html><head><meta http-equiv="refresh" content="0; URL=../new/chapter.html"></head></html>'
            old.write_text(redirect)
            new.write_text('<html><head></head><body><main>Chapter</main></body></html>')
            result = subprocess.CompletedProcess([], 0, stdout="", stderr="")
            with patch('build_book.subprocess.run', return_value=result):
                build_book(root, output, '/plenty/', {"label": "Development"})
            self.assertEqual(old.read_text(), redirect)
            self.assertIn('aria-label="Documentation version"', new.read_text())
            # Unconfigured pages must still fail, even with redirects present.
            (output / "broken.html").write_text(redirect)
            with self.assertRaisesRegex(ValueError, "Unsupported mdBook"):
                decorate(output, {"label": "Development"}, {"old/chapter.html"})
