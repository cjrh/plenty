import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import check_fork_filetests as check


class ForkCheckTests(unittest.TestCase):
    def test_snapshot_uses_commit_not_dirty_checkout(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            repo = root / 'repo'
            repo.mkdir()
            def git(*args):
                return subprocess.check_output(['git', '-C', str(repo), *args], text=True)
            git('init', '-q')
            (repo / 'file').write_text('committed')
            git('add', 'file')
            git('-c', 'user.name=Test', '-c', 'user.email=test@example.invalid',
                'commit', '-qm', 'fixture')
            revision = git('rev-parse', 'HEAD').strip()
            (repo / 'file').write_text('dirty')
            check.snapshot(repo, revision, root / 'snapshot')
            self.assertEqual((root / 'snapshot/file').read_text(), 'committed')
            self.assertEqual((repo / 'file').read_text(), 'dirty')
            self.assertFalse((root / 'snapshot/.git').exists())

    def test_environment_never_inherits_blessing_or_rustflags(self):
        with patch.dict(os.environ, {'CRANELIFT_TEST_BLESS': '1', 'RUSTFLAGS': '-C target-cpu=native',
                                     'CARGO_ENCODED_RUSTFLAGS': 'bad'}):
            env = check.check_environment()
        self.assertNotIn('CRANELIFT_TEST_BLESS', env)
        self.assertNotIn('RUSTFLAGS', env)
        self.assertNotIn('CARGO_ENCODED_RUSTFLAGS', env)
        self.assertEqual(env['RUSTC_WRAPPER'], '')

    def test_pins_reject_registry_and_ambiguous_sources(self):
        with tempfile.TemporaryDirectory() as tmp:
            lock = Path(tmp) / 'Cargo.lock'
            lock.write_text('[[package]]\nname="regalloc2"\nsource="registry+example"\n')
            with self.assertRaises(ValueError):
                check.pinned_revision(lock, 'regalloc2', 'regalloc2')
            lock.write_text('[[package]]\nname="regalloc2"\nsource="git+https://github.com/cjrh/regalloc2?branch=fork#' + 'a' * 40 + '"\n')
            self.assertEqual(check.pinned_revision(lock, 'regalloc2', 'regalloc2'), 'a' * 40)

    def test_ignored_path_patch_is_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / 'tree'
            log.write_text('warning: patch unused\nregalloc2 v0.15.2\ncranelift-codegen v0.137.0\n')
            check.verify_allocator(log, None)
            with self.assertRaises(ValueError):
                check.verify_allocator(log, Path('/fork'))
            log.write_text('regalloc2 v0.15.2 (/fork)\ncranelift-codegen v0.137.0\n')
            check.verify_allocator(log, Path('/fork'))

    def test_filetest_differences_fail_without_blessing_and_keep_both_runs(self):
        for baseline_exit, patched_exit, expected in [(0, 0, 0), (0, 1, 1), (1, 1, 2)]:
            with self.subTest(expected=expected), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                args = argparse.Namespace(output=root, wasmtime=root, regalloc2=root)
                def run(command, cwd, env, log):
                    self.assertNotIn('CRANELIFT_TEST_BLESS', env)
                    self.assertNotIn('--bless', command)
                    if log.name.endswith('tree.log'):
                        suffix = f' ({cwd.parent / "regalloc2"})' if log.name.startswith('plenty') else ''
                        log.write_text('regalloc2 v0.15.2' + suffix + '\n')
                    if log.name.endswith('test.log'):
                        return baseline_exit if log.name.startswith('registry') else patched_exit
                    return 0
                with patch.object(check, 'snapshot'), patch.object(check.shutil, 'copyfile'), \
                        patch.object(check, 'run_logged', side_effect=run):
                    self.assertEqual(check.check(args), expected)
                reports = list(root.glob('run-*/summary.json'))
                report = json.loads(reports[0].read_text())
                self.assertEqual(len(report['runs']), 2)
                self.assertIn('test_exit', report['runs'][1])


if __name__ == '__main__':
    unittest.main()
