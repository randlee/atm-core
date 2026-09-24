"""Immutable checkout, provenance-mode and platform contracts for source setup."""
import importlib.util
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import yaml
import shutil

SCRIPT = Path(__file__).resolve().parents[1] / 'setup_sc_lint_source.py'
spec = importlib.util.spec_from_file_location('setup_source', SCRIPT)
setup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(setup)


class SourceSetupTests(unittest.TestCase):
    def test_refs_are_full_immutable_sha_only(self):
        for ref in ('main', 'v0.6.0', 'HEAD', 'abc123', 'a'*39, 'z'*40, 'a'*40+'\n', '--help', None):
            with self.subTest(ref=ref), self.assertRaises(ValueError):
                setup.revision(ref)
        self.assertEqual(setup.revision('a'*40), 'a'*40)

    def test_execution_smoke_retains_reported_findings(self):
        result = {'ok': True, 'data': {'status': 'fail', 'diagnostics': ['visible finding']}}
        with contextlib.redirect_stdout(io.StringIO()) as output:
            setup.report_lint_smoke(result)
        self.assertIn('visible finding', output.getvalue())
        self.assertIn('reported lint status=fail', output.getvalue())
        for value in ({'ok': False}, {'ok': True, 'error': {'code': 'CLI.CONFIG_ERROR'}}):
            with contextlib.redirect_stdout(io.StringIO()), self.assertRaisesRegex(ValueError, 'root discovery failed'):
                setup.report_lint_smoke(value)

    def test_paths_match_actual_unix_and_windows_helper_lookup(self):
        root = Path('consumer/.sc-lint/venv')
        self.assertEqual(setup.python_path(root, False), root / 'bin/python3')
        self.assertEqual(setup.python_path(root, True), root / 'Scripts/python.exe')

    def test_manifest_selection_is_opt_in_and_conflicts_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'manifest.toml'
            self.assertEqual(setup.selected_revision('', path), '')
            path.write_text('[project]\nsc_lint_source_revision="' + 'a'*40 + '"\n')
            self.assertEqual(setup.selected_revision('', path), 'a'*40)
            self.assertEqual(setup.selected_revision('a'*40, path), 'a'*40)
            with self.assertRaisesRegex(ValueError, 'differs'):
                setup.selected_revision('b'*40, path)
            for value in ('"main"', '""', 'false', '12'):
                path.write_text('[project]\nsc_lint_source_revision=' + value)
                with self.assertRaises(ValueError):
                    setup.selected_revision('', path)

    def test_real_git_checkout_must_match_exact_requested_commit(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            origin = root / 'origin'
            origin.mkdir()
            setup.run(['git', 'init', '-q'], origin)
            (origin/'Cargo.lock').write_text('reviewed source')
            setup.run(['git', 'add', '.'], origin)
            setup.run(['git', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.test', 'commit', '-qm', 'source'], origin)
            sha = setup.run(['git', 'rev-parse', 'HEAD'], origin)
            target = root / 'checkout'
            setup.checkout(str(origin), sha, target)
            setup.verify_checkout(target, sha)
            with self.assertRaisesRegex(ValueError, 'does not match'):
                setup.verify_checkout(target, '0'*40)
            (target/'Cargo.lock').write_text('changed')
            with self.assertRaises(subprocess.CalledProcessError):
                setup.verify_checkout(target, sha)
            with self.assertRaises(subprocess.CalledProcessError):
                setup.checkout(str(origin), '0'*40, root/'missing')

    def test_release_cannot_silently_reuse_source_install(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'.sc-lint').mkdir()
            (root/'.sc-lint/source-install.json').write_text('{}')
            env = dict(os.environ, GITHUB_WORKSPACE=str(root), GITHUB_ENV=str(root/'env'), SC_LINT_SOURCE_REVISION_INPUT='')
            result = subprocess.run([sys.executable, SCRIPT, 'resolve'], env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('cannot reuse', result.stderr)
            self.assertFalse((root/'env').exists())

    def test_default_action_resolution_needs_no_python_on_unix_or_windows(self):
        action = SCRIPT.parents[1] / 'actions/setup-sc-lint/action.yml'
        steps = yaml.safe_load(action.read_text())['runs']['steps']
        for label, shell in (('Unix', 'bash'), ('Windows', 'pwsh')):
            if not shutil.which(shell):
                continue
            with self.subTest(platform=label), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root/'release').mkdir()
                (root/'release/publish-artifacts.toml').write_text('[project]\nname="old-consumer"\n')
                # Any helper invocation fails: default mode must not reach Python.
                helper = root/'.github/scripts/setup_sc_lint_source.py'
                helper.parent.mkdir(parents=True)
                helper.write_text('raise RuntimeError("default release must not invoke source helper")')
                step = next(x for x in steps if x['name'] == f'Resolve immutable sc-lint source ({label})')
                script = root / ('resolve.ps1' if label == 'Windows' else 'resolve.sh')
                script.write_text(step['run'])
                env = dict(os.environ, GITHUB_ENV=str(root/'env'), SC_LINT_SOURCE_REVISION_INPUT='')
                cmd = [shell, '-NoProfile', '-File', str(script)] if label == 'Windows' else [shell, str(script)]
                result = subprocess.run(cmd, cwd=root, env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual((root/'env').read_text().strip(), 'SC_LINT_SOURCE_REVISION=')

    def test_real_subprocess_timeout_fails_with_command_context(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, 'timed out.*sleep') as caught:
                setup.run([sys.executable, '-c', 'import time; time.sleep(5)'], Path(directory), timeout=0.05)
            self.assertIsInstance(caught.exception.__cause__, subprocess.TimeoutExpired)
        self.assertEqual(setup.BUILD_TIMEOUT_SECONDS, 1800)
        self.assertLess(setup.PROBE_TIMEOUT_SECONDS, setup.SETUP_TIMEOUT_SECONDS)

    def test_existing_python_environment_rejected_before_fetch(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'.sc-lint/venv').mkdir(parents=True)
            with self.assertRaisesRegex(ValueError, 'fresh consumer'):
                setup.install('example/unavailable', 'a'*40, root, root)
            self.assertEqual(list(root.glob('sc-lint-source-*')), [])


if __name__ == '__main__':
    unittest.main()
