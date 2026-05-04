from __future__ import annotations

from datetime import datetime, timezone
from pathlib import Path
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_common import build_report
from lint_common import lint_slug
from lint_common import make_log_path
from lint_common import relative_log_path


class LintCommonTests(unittest.TestCase):
    def test_lint_slug_normalizes_names(self) -> None:
        self.assertEqual(lint_slug("Rule 8 / identities"), "rule-8-identities")

    def test_build_report_writes_log(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            started_at = datetime(2026, 5, 4, 3, 15, 0, tzinfo=timezone.utc)
            report = build_report(
                lint_name="manifests",
                repo_root=repo_root,
                passed=True,
                summary="manifest policy satisfied",
                findings=[],
                transcript_lines=["no manifest violations found"],
                started_at=started_at,
                duration_seconds=0.42,
            )

            self.assertTrue(report.log_path.is_file())
            self.assertIn("summary: manifest policy satisfied", report.log_path.read_text(encoding="utf-8"))
            self.assertEqual(relative_log_path(repo_root, report.log_path), ".just/logs/20260504031500-manifests.log")

    def test_make_log_path_uses_timestamp_and_slug(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            started_at = datetime(2026, 5, 4, 3, 16, 0, tzinfo=timezone.utc)
            path = make_log_path(repo_root, "Boundary Check", started_at)
            self.assertEqual(path, repo_root / ".just/logs/20260504031600-boundary-check.log")


if __name__ == "__main__":
    unittest.main()
