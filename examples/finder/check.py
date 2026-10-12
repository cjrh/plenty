#!/usr/bin/env python3
"""Compile Plenty, then exercise the finder experiment against independent oracles."""

import argparse
import itertools
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


def wildcard_oracle(pattern, text):
    """Dynamic programming, independent of the Plenty greedy/backtracking matcher."""
    previous = [True] + [False] * len(text)
    for token in pattern:
        current = [token == "*" and previous[0]] + [False] * len(text)
        for index, character in enumerate(text, 1):
            if token == "*":
                current[index] = previous[index] or current[index - 1]
            else:
                current[index] = previous[index - 1] and (
                    token == "?" or token == character
                )
        previous = current
    return previous[-1]


class FinderChecks(unittest.TestCase):
    compiler = None

    @classmethod
    def setUpClass(cls):
        cls.example = Path(__file__).resolve().parent
        cls.temporary = tempfile.TemporaryDirectory(prefix="plenty-finder-check-")
        cls.addClassCleanup(cls.temporary.cleanup)
        cls.directory = Path(cls.temporary.name)
        cls.binary = cls.directory / "finder"
        cls.compile(cls.example / "main.plenty", cls.binary)
        # Empty strings cannot be path records. This driver also tests those cases.
        cls.driver_source = cls.directory / "matcher.plenty"
        cls.driver_source.write_text(
            "import matching\n"
            "def main() -> Result[(), IoError]:\n"
            "    while True:\n"
            "        match input()?:\n"
            "            case Some(pattern):\n"
            "                match input()?:\n"
            "                    case Some(text):\n"
            "                        print(matching.glob_matches(&pattern, &text, 0))?\n"
            "                    case Nothing:\n"
            "                        return Err(IoError.InvalidInput)\n"
            "            case Nothing:\n"
            "                break\n"
            "    flush_stdout()?\n"
            "    Ok(())\n",
            encoding="utf-8",
        )
        cls.driver = cls.directory / "matcher"
        # The entry must live under the module root, so keep all sources together.
        (cls.directory / "matching.plenty").write_bytes(
            (cls.example / "matching.plenty").read_bytes()
        )
        cls.compile(cls.driver_source, cls.driver)
        cls.byte_driver_source = cls.directory / "byte-matcher.plenty"
        cls.byte_driver_source.write_text(
            "import binary\n"
            "def main() -> Result[(), IoError]:\n"
            "    while True:\n"
            "        match input_bytes(0u8)?:\n"
            "            case Some(pattern):\n"
            "                match input_bytes(0u8)?:\n"
            "                    case Some(text):\n"
            "                        print(binary.glob_matches(&pattern, &text, 0))?\n"
            "                    case Nothing:\n"
            "                        return Err(IoError.InvalidInput)\n"
            "            case Nothing:\n"
            "                break\n"
            "    flush_stdout()?\n"
            "    Ok(())\n",
            encoding="utf-8",
        )
        (cls.directory / "binary.plenty").write_bytes((cls.example / "binary.plenty").read_bytes())
        cls.byte_driver = cls.directory / "byte-matcher"
        cls.compile(cls.byte_driver_source, cls.byte_driver)

    @classmethod
    def compile(cls, source, output):
        result = subprocess.run(
            [str(cls.compiler), "--compile", str(source), "-o", str(output)],
            capture_output=True,
            text=True,
            timeout=60,
        )
        if result.returncode:
            raise RuntimeError(result.stdout + result.stderr)

    def run_finder(self, *arguments, data=b""):
        return subprocess.run(
            [str(self.binary), *arguments], input=data, capture_output=True, timeout=10
        )

    def assert_paths(self, arguments, data, expected):
        result = self.run_finder(*arguments, data=data.encode("utf-8"))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, b"")
        self.assertEqual(result.stdout.decode("utf-8"), expected)

    def test_literal_basename_and_full_path(self):
        data = "src/main.plenty\nmain/no.txt\nother/main.txt\n"
        self.assert_paths(["--paths", "main"], data, "src/main.plenty\nother/main.txt\n")
        self.assert_paths(["--paths", "--full-path", "main/"], data, "main/no.txt\n")
        self.assert_paths(["--paths", "absent"], data, "")

    def test_globs_and_literal_metacharacters(self):
        data = "src/a.plenty\nsrc/a.txt\nsrc/.private.plenty\nsrc/é.plenty\n"
        self.assert_paths(
            ["--paths", "--glob", "?.plenty"], data, "src/a.plenty\nsrc/é.plenty\n"
        )
        self.assert_paths(["--paths", "--glob", "--full-path", "src/*.txt"], data, "src/a.txt\n")
        self.assert_paths(["--paths", "*"], "a*b\nab\n", "a*b\n")
        self.assert_paths(["--paths", "--glob", "[ab]"], "a\n[ab]\n", "[ab]\n")

    def test_hidden_components_and_relative_prefixes(self):
        data = "./src/a\n../src/b\n.hidden/c\nsrc/.secret/d\nsrc/.../e\nsrc/a.b\n"
        self.assert_paths(["--paths"], data, "./src/a\n../src/b\nsrc/a.b\n")
        self.assert_paths(["--paths", "--hidden"], data, data)

    def test_lines_spaces_and_unterminated_final_record(self):
        self.assert_paths(["--paths"], "\r\na b\r\n leading \nlast", "a b\n leading \nlast\n")
        self.assert_paths(["--paths"], "", "")
        self.assert_paths(["--paths", ""], "foo\nbar\n", "foo\nbar\n")
        self.assert_paths(["--paths", "--glob"], "foo\nbar\n", "foo\nbar\n")

    def test_help_options_and_exit_status(self):
        result = self.run_finder("--help")
        self.assertEqual(result.returncode, 0)
        self.assertIn(b"directory", result.stdout.lower())
        for arguments in [("--unknown",), ("--paths", "x", "root"), ("x", "root", "extra")]:
            with self.subTest(arguments=arguments):
                result = self.run_finder(*arguments)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(result.stdout, b"")
                self.assertIn(b"finder:", result.stderr)
        self.assert_paths(["--paths", "--", "-name"], "-name\nother\n", "-name\n")

    def test_invalid_utf8_keeps_io_error_details(self):
        result = self.run_finder("--paths", data=b"\xff\n")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, b"")
        self.assertIn(b"InvalidUtf8", result.stderr)
        result = subprocess.run(
            [bytes(self.binary), b"--paths", b"\xff"], capture_output=True, timeout=10
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn(b"InvalidUtf8", result.stderr)

    def test_real_directory_fixture_with_native_traversal(self):
        tree = self.directory / "tree"
        for name in ["src/main.plenty", "src/notes.txt", "docs/guide.plenty", ".git/config", "src/.cache/private.plenty"]:
            path = tree / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("fixture", encoding="utf-8")
        (tree / "linked.plenty").symlink_to("src/main.plenty")
        paths = sorted(path.relative_to(tree).as_posix() for path in tree.rglob("*"))
        expected = ["docs/guide.plenty", "linked.plenty", "src/main.plenty"]
        self.assert_paths(
            ["--paths", "--glob", "*.plenty"],
            "\n".join(paths) + "\n",
            "\n".join(expected) + "\n",
        )
        (tree / "src" / "ancestor").symlink_to("..", target_is_directory=True)
        (tree / "directory-link").symlink_to("src", target_is_directory=True)
        result = subprocess.run(
            [str(self.binary), "--glob", "*.plenty", "."], cwd=tree,
            capture_output=True, text=True, timeout=10,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(sorted(result.stdout.splitlines()), ["./" + name for name in expected])
        if fd := shutil.which("fd"):
            result = subprocess.run(
                [fd, "--glob", "--no-ignore", "--color", "never", "*.plenty"],
                cwd=tree,
                capture_output=True,
                text=True,
                timeout=10,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(sorted(result.stdout.splitlines()), expected)

    def test_directory_capability_probes(self):
        for name in ["read_dir", "metadata"]:
            result = subprocess.run(
                [str(self.compiler), "--check", str(self.example / "probes" / f"{name}.plenty")],
                capture_output=True,
                text=True,
                timeout=10,
            )
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_explicit_hidden_and_symlink_roots_are_searchable(self):
        root = self.directory / ".explicit-root"
        root.mkdir()
        (root / "visible.txt").write_text("fixture")
        (root / ".hidden.txt").write_text("fixture")
        linked = self.directory / "linked-root"
        linked.symlink_to(root, target_is_directory=True)
        for supplied in [root, linked]:
            result = self.run_finder("", str(supplied))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.decode(), str(supplied / "visible.txt") + "\n")

    @unittest.skipUnless(os.name == "posix", "raw Unix filenames")
    def test_nul_framed_real_filenames_and_raw_patterns(self):
        tree = bytes(self.directory / "raw-tree")
        os.makedirs(tree)
        names = [b"space name", b"tab\tname", b"CR\r", b"LF\nname", b"bad\xff", b".hidden\xfe", b"\xc3\xa9"]
        for name in names:
            with open(tree + b"/" + name, "wb") as file:
                file.write(b"fixture")
        names = sorted(os.listdir(tree))
        traversed = subprocess.run(
            [bytes(self.binary), b"--null", b"--hidden", b"", tree],
            capture_output=True, timeout=10,
        )
        self.assertEqual(traversed.returncode, 0, traversed.stderr)
        self.assertEqual(sorted(traversed.stdout.rstrip(b"\0").split(b"\0")), [tree + b"/" + name for name in names])
        data = b"\0".join(names) + b"\0"
        visible = [name for name in names if not name.startswith(b".")]
        for options, pattern, expected in [
            ([], None, visible),
            (["--hidden"], None, names),
            ([], b"\xff", [b"bad\xff"]),
            ([], b"\n", [b"LF\nname"]),
            (["--glob"], b"?", []),
            (["--glob"], b"??", [b"\xc3\xa9"]),
            (["--glob"], b"bad?", [b"bad\xff"]),
        ]:
            arguments = ["--paths", "--null", *options]
            if pattern is not None:
                arguments += ["--", pattern]
            result = self.run_finder(*arguments, data=data)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, b"".join(name + b"\0" for name in expected))
        result = self.run_finder("--paths", "--null", data=b"\0a\r\n\xff")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, b"a\r\n\xff\0")
        # -- ends option parsing, including recognition of --null.
        self.assert_paths(["--paths", "--", "--null"], "--null\nother\n", "--null\n")

    def test_byte_wildcards_against_independent_oracle(self):
        patterns = [b""] + [bytes(items) for size in range(1, 5) for items in itertools.product(b"a\xff*?", repeat=size)]
        texts = [b""] + [bytes(items) for size in range(1, 5) for items in itertools.product(b"a\xff", repeat=size)]
        pairs = list(itertools.product(patterns, texts)) + [
            (b"?", b"\xc3\xa9"), (b"??", b"\xc3\xa9"), (b"*\n?", b"a\n\xff"),
            (b"*", b"x" * 5000), (b"*a*b", b"a" * 2000 + b"b"),
        ]
        result = subprocess.run([str(self.byte_driver)], input=b"".join(pattern + b"\0" + text + b"\0" for pattern, text in pairs), capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        answers = result.stdout.decode("ascii").splitlines()
        self.assertEqual(len(answers), len(pairs))
        # Translate byte tokens to scalar tokens solely for the independent oracle.
        for (pattern, text), actual in zip(pairs, answers):
            self.assertEqual(actual, str(wildcard_oracle(pattern.decode("latin1"), text.decode("latin1"))), (pattern, text))

    def test_wildcards_against_independent_oracle(self):
        patterns = [""] + [
            "".join(items)
            for size in range(1, 5)
            for items in itertools.product("ab*?", repeat=size)
        ]
        texts = [""] + [
            "".join(items)
            for size in range(1, 5)
            for items in itertools.product("ab", repeat=size)
        ]
        pairs = list(itertools.product(patterns, texts)) + [
            ("?", "é"), ("?", "🦀"), ("?", "e\u0301"), ("??", "e\u0301"),
            ("*界?", "hé界🦀"), ("a*b*c", "axbybzbc"), ("*a?b", "aabxaab"),
            ("a*b", "a/dir/b"), ("*", "a" * 5000), ("*a*b", "a" * 2000 + "b"),
        ]
        result = subprocess.run(
            [str(self.driver)],
            input="".join(f"{pattern}\n{text}\n" for pattern, text in pairs),
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        answers = result.stdout.splitlines()
        self.assertEqual(len(answers), len(pairs))
        for (pattern, text), actual in zip(pairs, answers):
            self.assertEqual(actual, str(wildcard_oracle(pattern, text)), (pattern, text))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plenty", type=Path, default=Path(__file__).resolve().parents[2] / "target/debug/plenty")
    options, remaining = parser.parse_known_args()
    FinderChecks.compiler = options.plenty.resolve()
    if not FinderChecks.compiler.is_file():
        parser.error("compiler missing; run cargo build or pass --plenty PATH")
    unittest.main(argv=[__file__, *remaining])
