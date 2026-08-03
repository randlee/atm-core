from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_daemon_signing_coupling import collect_violations
from lint_daemon_signing_coupling import recipe_blocks


class DaemonSigningCouplingTests(unittest.TestCase):
    def test_current_justfile_couples_build_and_benchmark(self) -> None:
        self.assertEqual(collect_violations(JUST_DIR.parent), [])

    def test_missing_benchmark_hook_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / "Justfile").write_text(
                "benchmark:\n    cargo build --release -p atm-daemon\n",
                encoding="utf-8",
            )

            violations = collect_violations(repo_root)

        self.assertEqual(len(violations), 1)
        self.assertEqual(violations[0].recipe, "benchmark")
        self.assertIn("Justfile:2", violations[0].render())

    def test_hook_in_same_recipe_satisfies_coupling(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / "Justfile").write_text(
                "benchmark:\n"
                "    cargo build --release -p atm-daemon\n"
                "    python3 .just/sign_daemon_dev.py\n",
                encoding="utf-8",
            )

            self.assertEqual(collect_violations(repo_root), [])

    def test_non_daemon_build_is_not_checked(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / "Justfile").write_text(
                "build-cli:\n    cargo build --release -p agent-team-mail\n",
                encoding="utf-8",
            )

            self.assertEqual(collect_violations(repo_root), [])

    def test_recipe_parser_keeps_body_boundaries(self) -> None:
        blocks = recipe_blocks(
            "build:\n    cargo build --workspace\n\n"
            "benchmark *args:\n    cargo build --release -p atm-daemon\n"
        )

        self.assertEqual([name for name, _line, _body in blocks], ["build", "benchmark"])
        self.assertEqual(blocks[1][2][0][0], 5)


if __name__ == "__main__":
    unittest.main()
