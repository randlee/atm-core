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
from lint_common import classify_rust_test_scope
from lint_common import LintDirectivePolicy
from lint_common import lint_slug
from lint_common import line_is_suppressed
from lint_common import load_lint_config
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

    def test_load_lint_config_reads_repo_policy(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            just_dir = repo_root / ".just"
            just_dir.mkdir()
            (just_dir / "lint-config.toml").write_text(
                "[identities]\nforbidden_literals = [\"team-lead\"]\n",
                encoding="utf-8",
            )

            config = load_lint_config(repo_root)

            self.assertEqual(config["identities"]["forbidden_literals"], ["team-lead"])

    def test_line_is_suppressed_supports_tool_key_and_rule_aliases(self) -> None:
        lines = [
            "// lint-identities: allow-next-line",
            "let _ = \"team-lead\";",
            "// rule-009: allow-start",
            "let _ = \"arch-ctm\";",
            "// lint-identities: allow-end",
            "let _ = \"quality-mgr\";",
        ]
        policy = LintDirectivePolicy(tool_key="identities", aliases=("rule-008", "rule-009"))

        self.assertTrue(line_is_suppressed(2, lines, policy))
        self.assertTrue(line_is_suppressed(4, lines, policy))
        self.assertFalse(line_is_suppressed(6, lines, policy))

    def test_line_is_suppressed_supports_multiline_comment_directives(self) -> None:
        lines = [
            "/* lint-identities: allow-start */",
            "let _ = \"team-lead\";",
            "* lint-identities: allow-end",
            "let _ = \"arch-ctm\";",
        ]
        policy = LintDirectivePolicy(tool_key="identities", aliases=("rule-008", "rule-009"))

        self.assertTrue(line_is_suppressed(2, lines, policy))
        self.assertFalse(line_is_suppressed(4, lines, policy))

    def test_classify_rust_test_scope_marks_cfg_test_block_only(self) -> None:
        lines = [
            "pub fn production() {}",
            "#[cfg(test)]",
            "mod tests {",
            "    #[test]",
            "    fn example() {}",
            "}",
        ]

        scope = classify_rust_test_scope(lines)

        self.assertEqual(scope, [False, True, True, True, True, True])


if __name__ == "__main__":
    unittest.main()
