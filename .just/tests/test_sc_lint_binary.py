from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

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

    def test_missing_managed_binary_tells_the_user_to_setup(self) -> None:
        with mock.patch.dict("os.environ", {"SC_LINT_INSTALL_DIR": "/missing"}, clear=True):
            with self.assertRaisesRegex(FileNotFoundError, "run just setup"):
                resolve_sc_lint_binary()


if __name__ == "__main__":
    unittest.main()
