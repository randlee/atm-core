from __future__ import annotations

from contextlib import redirect_stderr
from io import StringIO
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_sc_boundary import main as sc_boundary_main
from sc_lint_binary import exec_lint
from sc_lint_binary import lint_argv
from sc_lint_binary import resolve_sc_lint_binary


class ScLintBinaryTests(unittest.TestCase):
    def test_explicit_managed_binary_builds_analyzer_argv(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            binary = Path(tempdir) / "sc-lint"
            binary.touch(mode=0o755)
            repo_root = Path("/repo")
            with mock.patch.dict("os.environ", {"SC_LINT_BIN": str(binary)}, clear=True):
                resolved, argv = lint_argv(repo_root, "function-length", ["--write-baseline"])

        self.assertEqual(resolved, binary)
        self.assertEqual(
            argv,
            [str(binary), "lint", "function-length", "--root", str(repo_root), "--write-baseline"],
        )

    def test_explicit_managed_binary_ignores_a_path_decoy(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            managed = root / "managed" / "sc-lint"
            decoy = root / "decoy" / "sc-lint"
            managed.parent.mkdir()
            decoy.parent.mkdir()
            managed.touch(mode=0o755)
            decoy.touch(mode=0o755)
            with mock.patch.dict(
                "os.environ",
                {"SC_LINT_BIN": str(managed), "PATH": str(decoy.parent)},
                clear=True,
            ), mock.patch("sc_lint_binary.subprocess.run", return_value=mock.Mock(returncode=0)) as run:
                self.assertEqual(sc_boundary_main(["lint_sc_boundary.py", "--root", "/repo"]), 0)

        run.assert_called_once_with(
            [
                str(managed),
                "lint",
                "sc-boundary",
                "--root",
                "/repo",
                "--config",
                "/repo/sc-lint.toml",
            ],
            check=False,
        )
        self.assertNotIn(str(decoy), run.call_args.args[0])

    def test_windows_default_resolves_exe_and_propagates_child_exit_code(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            local_app_data = Path(tempdir) / "AppData" / "Local"
            binary = local_app_data / "sc-lint" / "bin" / "sc-lint.exe"
            binary.parent.mkdir(parents=True)
            binary.touch(mode=0o755)
            with mock.patch("sc_lint_binary.sys.platform", "win32"), mock.patch.dict(
                "os.environ", {"LOCALAPPDATA": str(local_app_data)}, clear=True
            ), mock.patch("sc_lint_binary.subprocess.run", return_value=mock.Mock(returncode=23)) as run:
                self.assertEqual(exec_lint(Path("/repo"), "sc-boundary", []), 23)

        run.assert_called_once_with(
            [str(binary), "lint", "sc-boundary", "--root", "/repo"],
            check=False,
        )

    def test_missing_managed_binary_tells_the_user_to_setup(self) -> None:
        with mock.patch.dict("os.environ", {"SC_LINT_INSTALL_DIR": "/missing"}, clear=True):
            with self.assertRaisesRegex(FileNotFoundError, "run just setup"):
                resolve_sc_lint_binary()

    def test_boundary_wrapper_reports_missing_binary_without_a_traceback(self) -> None:
        stderr = StringIO()
        with mock.patch.dict("os.environ", {"SC_LINT_INSTALL_DIR": "/missing"}, clear=True), redirect_stderr(stderr):
            self.assertEqual(sc_boundary_main(["lint_sc_boundary.py", "--root", "/repo"]), 1)

        self.assertEqual(
            stderr.getvalue(),
            "sc-lint binary is unavailable at /missing/sc-lint; run just setup\n",
        )


if __name__ == "__main__":
    unittest.main()
