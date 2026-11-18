import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from build_book_examples import build


class BuildFences(unittest.TestCase):
    def test_inline_links_without_launching(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            page = root / 'book/src/example.md'
            page.parent.mkdir(parents=True)
            page.write_text('```plenty fragment\nskip\n```\n```plenty build\nsource\n```\n')

            def compile_only(args, cwd, check):
                self.assertEqual(args, ['plenty', 'build', '--offline'])
                self.assertEqual((cwd / 'src/main.plenty').read_text(), 'source\n')
                binary = cwd / 'target/optimized/listing'
                binary.parent.mkdir(parents=True)
                binary.touch()

            with patch('build_book_examples.subprocess.run', side_effect=compile_only) as runner:
                build(root, 'plenty')
            self.assertEqual(runner.call_count, 1)

    def test_missing_include_names_listing(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            page = root / 'book/src/example.md'
            page.parent.mkdir(parents=True)
            page.write_text('Example:\n```plenty build\n{{#include missing.plenty}}\n```\n')
            with self.assertRaisesRegex(SystemExit, 'book/src/example.md:2:.*missing.plenty'):
                build(root, 'plenty')
