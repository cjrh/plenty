import contextlib
import io
from pathlib import Path
import tempfile
import unittest

import check_links


class CheckLinksTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "guide").mkdir()
        (self.root / "target.md").touch()
        (self.root / "image with space.png").touch()
        (self.root / "target(v2).md").touch()

    def page(self, text):
        path = self.root / "guide" / "page.md"
        path.write_text(text, encoding="utf-8")
        return path

    def test_paths_images_references_and_url_suffixes(self):
        page = self.page(
            '[page](../target.md?raw=1#heading "Title")\n'
            '![image](../image%20with%20space.png)\n'
            '[angle](<../image with space.png>)\n'
            '[balanced](../target(v2).md)\n'
            '[escaped](../target\\(v2\\).md)\n'
            '[root](/target.md)\n'
            '[reference][ref]\n[ref]: ../target.md "Title"\n'
            '[heading](#heading) [site](https://example.com/missing)\n'
            '[protocol relative](//example.com/missing) [mail](mailto:a@example.com)\n'
        )
        self.assertEqual(check_links.check(self.root, page), (1, 7, []))

    def test_code_comments_and_fence_lengths_are_ignored(self):
        page = self.page(
            '`[inline](missing.md)` and ``[code ` tick](missing.md)``\n'
            '<!-- [comment](missing.md)\n[comment](missing.md) -->\n'
            '````markdown\n```\n[fenced](missing.md)\n````\n'
            '~~~text\n[fenced](missing.md)\n~~~\n'
            '    [indented](missing.md)\n'
            '\t[indented](missing.md)\n'
            '[real](../target.md)\n'
        )
        self.assertEqual(check_links.check(self.root, page), (1, 1, []))

    def test_list_content_and_lazy_continuations_are_prose(self):
        page = self.page(
            '- Item one\n\n    - [Nested](missing-nested.md)\n\n'
            'Some paragraph text\n    [continued](missing-lazy.md)\n\n'
            '1. Item\n\n   ```\n   [fenced](missing.md)\n   ```\n\n'
            '       [indented](missing.md)\n\n'
            '# Heading\n    [indented](missing.md)\n'
        )
        self.assertEqual(
            check_links.check(self.root, page),
            (1, 2, [
                'guide/page.md:3: missing target missing-nested.md',
                'guide/page.md:6: missing target missing-lazy.md',
            ]),
        )

    def test_existing_repository_file_outside_book_is_rejected(self):
        book = self.root / "book"
        book.mkdir()
        page = book / "page.md"
        page.write_text(
            '[outside](../target.md)\n'
            '[encoded](%2e%2e/target.md)\n'
            '[root escape](/../target.md)\n', encoding="utf-8"
        )
        self.assertEqual(check_links.check(book, page), (1, 3, [
            'page.md:1: target outside book source ../target.md',
            'page.md:2: target outside book source %2e%2e/target.md',
            'page.md:3: target outside book source /../target.md',
        ]))

    def test_missing_targets_keep_original_line_numbers(self):
        page = self.page('```\n[code](ignored.md)\n```\n\n[broken](gone.md)\n')
        self.assertEqual(
            check_links.check(self.root, page),
            (1, 1, ['guide/page.md:5: missing target gone.md']),
        )

    def test_directory_and_file_scopes_and_exit_status(self):
        self.page('[ok](../target.md)\n')
        (self.root / "broken.md").write_text('[bad](gone.md)', encoding="utf-8")
        with contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(check_links.main([str(self.root), "guide"]), 0)
            self.assertEqual(check_links.main([str(self.root), "guide/page.md"]), 0)
            self.assertEqual(check_links.main([str(self.root)]), 1)
        self.assertIn('broken.md:1: missing target gone.md', output.getvalue())

    def test_invalid_scope_is_an_error_instead_of_an_empty_success(self):
        for scope in ['missing', '..', 'image with space.png']:
            with self.subTest(scope=scope), contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as error:
                    check_links.main([str(self.root), scope])
                self.assertEqual(error.exception.code, 2)


if __name__ == "__main__":
    unittest.main()
