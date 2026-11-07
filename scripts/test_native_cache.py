"""Optional native compiler wrapping preserves explicitly selected tools."""
from pathlib import Path
import unittest
from unittest.mock import patch
import native_cache


class NativeCache(unittest.TestCase):
    def setUp(self):
        support = patch("native_cache.supports_namespace", return_value=True)
        support.start()
        self.addCleanup(support.stop)

    @patch("native_cache.shutil.which", return_value="/usr/bin/ccache")
    def test_native_only_wrapper_and_safe_cache_settings(self, _):
        original = {"PATH": "/usr/bin", "TARGET_CC": "cross-cc"}
        self.assertEqual(native_cache.environment(Path("/repo"), original), original)
        env = native_cache.environment(Path("/repo"), {"PATH": "/usr/bin"})
        self.assertEqual(env["HOST_CC"], "ccache cc")
        self.assertNotIn("CC", env)
        self.assertEqual(env["CCACHE_DIR"], "/repo/target/native-ccache")
        self.assertEqual(env["CCACHE_COMPILERCHECK"], "content")
        self.assertEqual(env["CCACHE_NODIRECT"], "true")
        self.assertEqual(env["CCACHE_SLOPPINESS"], "")

    @patch("native_cache.shutil.which", return_value="/usr/bin/ccache")
    def test_explicit_tools_and_disabled_cache_are_preserved(self, _):
        for key, value in [("CC", "clang"), ("HOST_CC", "wrapper cc"),
                           ("CC_aarch64_unknown_linux_gnu", "cross-cc"),
                           ("PLENTY_NATIVE_CACHE", "off"), ("CCACHE_IGNOREOPTIONS", "-D*"),
                           ("CFLAGS", "-fplugin=custom.so"), ("RUSTC_WRAPPER", "sccache")]:
            env = {key: value}
            self.assertEqual(native_cache.environment(Path("/repo"), env), env)
        env = native_cache.environment(Path("/repo"), {"CCACHE_DIR": "/custom"})
        self.assertEqual(env["CCACHE_DIR"], "/custom")

    @patch("native_cache.shutil.which", return_value=None)
    def test_missing_optional_cache_uses_existing_build(self, _):
        self.assertEqual(native_cache.environment(Path("/repo"), {}), {})

    @patch("native_cache.shutil.which", return_value="/usr/bin/ccache")
    def test_older_ccache_without_namespaces_preserves_the_original_build(self, _):
        with patch.object(native_cache, "supports_namespace", return_value=False):
            self.assertEqual(native_cache.environment(Path("/repo"), {}), {})

    def test_companion_tool_namespace_tracks_contents_and_falls_back_if_unreadable(self):
        import os
        import subprocess
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            tool = Path(tmp) / "cc1"
            tool.write_bytes(b"first")
            modified = tool.stat().st_mtime_ns
            with patch.object(native_cache.subprocess, "run",
                              return_value=subprocess.CompletedProcess([], 0, str(tool))):
                first = native_cache.compiler_namespace()
                tool.write_bytes(b"other")
                os.utime(tool, ns=(modified, modified))
                self.assertNotEqual(first, native_cache.compiler_namespace())
                tool.write_bytes(b"first")
                self.assertEqual(first, native_cache.compiler_namespace())
        with patch.object(native_cache.shutil, "which", return_value="/usr/bin/ccache"), \
                patch.object(native_cache, "compiler_namespace", side_effect=OSError("unreadable")):
            self.assertEqual(native_cache.environment(Path("/repo"), {}), {})


@unittest.skipUnless(native_cache.shutil.which("ccache") and native_cache.shutil.which("cc")
                     and native_cache.supports_namespace(),
                     "optional ccache and C compiler are unavailable")
class NativeInvalidation(unittest.TestCase):
    def test_preprocessed_headers_new_headers_flags_and_compiler_contents(self):
        import os
        import shlex
        import subprocess
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "sdk").mkdir()
            header = root / "sdk/value.h"
            header.write_text("#define VALUE 1\n")
            source = root / "fixture.c"
            source.write_text('#include <value.h>\n'
                              '#if __has_include("optional.h")\n#include "optional.h"\n#endif\n'
                              '#ifndef EXTRA\n#define EXTRA 0\n#endif\n'
                              'int result(void) { return VALUE + EXTRA; }\n')
            compiler = root / "cc"
            real = shlex.quote(native_cache.shutil.which("cc"))
            compiler.write_text(f'#!/bin/sh\nexec {real} "$@"\n')
            compiler.chmod(0o755)
            env = native_cache.environment(root, {"PATH": os.environ["PATH"]})
            env["CCACHE_CONFIGPATH"] = "/dev/null"

            def compile(extra=()):
                subprocess.run(["ccache", str(compiler), "-c", str(source), "-isystem",
                                str(root / "sdk"), "-o", str(root / "fixture.o"), *extra],
                               env=env, check=True, capture_output=True)
                return (root / "fixture.o").read_bytes()

            original = compile()
            self.assertEqual(original, compile())
            old_mtime = header.stat().st_mtime_ns
            header.write_text("#define VALUE 2\n")
            os.utime(header, ns=(old_mtime, old_mtime))
            self.assertNotEqual(original, compile())
            header.write_text("#define VALUE 1\n")
            self.assertEqual(original, compile())
            (root / "optional.h").write_text("#undef VALUE\n#define VALUE 3\n")
            self.assertNotEqual(original, compile())
            (root / "optional.h").unlink()
            self.assertNotEqual(original, compile(["-DEXTRA=4"]))
            self.assertEqual(original, compile())
            # The wrapper's version output is unchanged; content identity must
            # still distinguish its changed effective compiler command.
            compiler.write_text(f'#!/bin/sh\nexec {real} -DEXTRA=5 "$@"\n')
            self.assertNotEqual(original, compile())
