import tempfile
import unittest
from pathlib import Path

from check_book_output import check


class BookOutputTests(unittest.TestCase):
    def test_missing_rendered_chapter_and_escaping_links_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "index.html").write_text(
                '<a href="missing.html">missing</a><a href="../secret">escape</a>'
                '<a href="https://example.com/">remote</a>'
            )
            count, errors = check(root)
            self.assertEqual(count, 1)
            self.assertTrue(any("missing.html" in error for error in errors))
            self.assertTrue(any("../secret" in error for error in errors))
            self.assertFalse(any("example.com" in error for error in errors))

    def test_pages_prefix_and_nested_relative_assets(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for filename in ("index.html", "404.html", "print.html", "searchindex-a.js",
                             "searcher-b.js", "highlight-c.js", "theme/plenty-highlight-d.js",
                             "theme/plenty-highlight-e.css", "nested/chapter.html"):
                path = root / filename
                path.parent.mkdir(parents=True, exist_ok=True)
                path.touch()
            (root / "404.html").write_text('<a href="/plenty/index.html">home</a>')
            (root / "nested/chapter.html").write_text(
                '<script src="../theme/plenty-highlight-d.js"></script>'
            )
            self.assertEqual(check(root), (4, []))
            (root / "404.html").write_text('<a href="/index.html">wrong prefix</a>')
            self.assertTrue(any("outside site prefix" in error for error in check(root)[1]))


if __name__ == "__main__":
    unittest.main()
