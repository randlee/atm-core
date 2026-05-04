from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from run_lint import build_tasks
from run_lint import extract_count
from run_lint import prioritize_error_lines
from run_lint import resolve_task_names


class RunLintTests(unittest.TestCase):
    def test_resolve_task_names_all_includes_new_targets(self) -> None:
        names = resolve_task_names("all")
        self.assertIn("boundaries", names)
        self.assertIn("manifests", names)
        self.assertIn("deny", names)
        self.assertIn("shear", names)
        self.assertIn("spell", names)
        self.assertIn("pytests", names)

    def test_resolve_task_names_rejects_unknown_target(self) -> None:
        with self.assertRaises(ValueError):
            resolve_task_names("unknown")

    def test_extract_count_understands_total_violations(self) -> None:
        self.assertEqual(extract_count(["total violations: 58"]), 58)

    def test_prioritize_error_lines_prefers_actual_failures(self) -> None:
        lines = [
            "Updating crates.io index",
            "Downloaded crate",
            "error[E0432]: unresolved import `uuid`",
            "could not compile `agent-team-mail`",
        ]

        self.assertEqual(
            prioritize_error_lines(lines),
            [
                "error[E0432]: unresolved import `uuid`",
                "could not compile `agent-team-mail`",
            ],
        )

    def test_build_tasks_contains_expected_commands(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            tasks = build_tasks(repo_root)
            self.assertEqual(tasks["boundaries"].command[-1], str(repo_root / ".just/lint_boundaries.py"))
            self.assertEqual(tasks["manifests"].command[-1], str(repo_root / ".just/lint_manifests.py"))
            self.assertEqual(tasks["deny"].command[-1], str(repo_root / ".just/lint_cargo_deny.py"))
            self.assertEqual(tasks["shear"].command[-1], str(repo_root / ".just/lint_cargo_shear.py"))
            self.assertEqual(tasks["spell"].command[-1], str(repo_root / ".just/lint_codespell.py"))
            self.assertEqual(tasks["pytests"].command[-1], str(repo_root / ".just/run_pytests.py"))


if __name__ == "__main__":
    unittest.main()
