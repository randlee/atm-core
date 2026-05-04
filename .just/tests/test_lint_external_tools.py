from __future__ import annotations

from pathlib import Path
import sys
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_cargo_deny import build_command as build_cargo_deny_command
from lint_cargo_shear import build_command as build_cargo_shear_command
from lint_codespell import build_command as build_codespell_command


class ExternalLintToolTests(unittest.TestCase):
    def test_build_cargo_deny_command_targets_workspace_manifest(self) -> None:
        repo_root = Path("/tmp/example")
        self.assertEqual(
            build_cargo_deny_command(repo_root),
            [
                "cargo-deny",
                "check",
                "advisories",
                "bans",
                "sources",
            ],
        )

    def test_build_cargo_shear_command_targets_workspace_manifest(self) -> None:
        repo_root = Path("/tmp/example")
        self.assertEqual(
            build_cargo_shear_command(repo_root),
            ["cargo-shear"],
        )

    def test_build_codespell_command_uses_repo_config(self) -> None:
        repo_root = Path("/tmp/example")
        command = build_codespell_command(repo_root)
        self.assertEqual(command[:2], [sys.executable, "-c"])
        self.assertIn("codespell_lib", command[2])
        self.assertEqual(len(command), 3)


if __name__ == "__main__":
    unittest.main()
