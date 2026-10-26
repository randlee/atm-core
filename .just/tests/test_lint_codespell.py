from __future__ import annotations

import contextlib
import io
from pathlib import Path
import subprocess
import sys
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_codespell import main as run_codespell_lint


class CodespellDelegateTests(unittest.TestCase):
    def run_lint(self, **patch: object) -> tuple[int, str]:
        stderr = io.StringIO()
        with mock.patch("lint_codespell.subprocess.run", **patch) as run_mock:
            with contextlib.redirect_stderr(stderr):
                exit_code = run_codespell_lint(["lint_codespell.py"])
        run_mock.assert_called_once()
        return exit_code, stderr.getvalue()

    def test_delegates_to_sc_lint_codespell(self) -> None:
        with mock.patch("lint_codespell.subprocess.run",
                        return_value=subprocess.CompletedProcess([], 0)) as run_mock:
            self.assertEqual(run_codespell_lint(["lint_codespell.py", "--extra"]), 0)
        command = run_mock.call_args.args[0]
        self.assertEqual(command[1:3], ["-m", "sc_lint.lint_codespell"])
        self.assertEqual(command[-1], "--extra")

    def test_findings_exit_code_is_propagated(self) -> None:
        exit_code, stderr = self.run_lint(return_value=subprocess.CompletedProcess([], 65))
        self.assertEqual(exit_code, 65)
        self.assertEqual(stderr, "")

    def test_missing_interpreter_fails_actionably(self) -> None:
        exit_code, stderr = self.run_lint(side_effect=FileNotFoundError("python missing"))
        self.assertEqual(exit_code, 127)
        self.assertIn("executable is unavailable", stderr)
        self.assertIn("just setup", stderr)

    def test_execution_error_fails_with_distinct_diagnostic(self) -> None:
        exit_code, stderr = self.run_lint(side_effect=PermissionError("permission denied"))
        self.assertEqual(exit_code, 2)
        self.assertIn("could not execute", stderr)


if __name__ == "__main__":
    unittest.main()
