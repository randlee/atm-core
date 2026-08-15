from __future__ import annotations

from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]


TEMPLATES_WITH_UNQUOTED_INTERPOLATION = (
    (
        ".claude/assets/sc-rust/quality-mgr/templates/rust-best-practices-assignment.json.j2",
        '"review_mode": {{ review_mode }},',
    ),
    (
        ".claude/assets/sc-rust/quality-mgr/templates/rust-qa-assignment.json.j2",
        '"worktree_path": {{ worktree_path }},',
    ),
    (
        ".claude/assets/sc-rust/quality-mgr/templates/rust-service-hardening-assignment.json.j2",
        '"review_mode": {{ review_mode }},',
    ),
    (
        ".claude/skills/codex-orchestration/arch-qa-assignment.json.j2",
        '"review_mode": {{ review_mode }},',
    ),
    (
        ".claude/skills/codex-orchestration/flaky-test-qa-assignment.json.j2",
        '"worktree_path": {{ worktree_path }},',
    ),
    (
        ".claude/skills/codex-orchestration/req-qa-assignment.json.j2",
        '"authoritative_sprint_doc": {{ sprint_doc }},',
    ),
    (
        ".claude/skills/codex-orchestration/ruthless-boundary-qa-assignment.json.j2",
        '"worktree_path": {{ worktree_path }},',
    ),
)


class ScComposeTemplateEscapeTests(unittest.TestCase):
    def test_every_json_template_migration_uses_unquoted_interpolation(self) -> None:
        for relative_path, expected_fragment in TEMPLATES_WITH_UNQUOTED_INTERPOLATION:
            with self.subTest(template=relative_path):
                content = (REPO_ROOT / relative_path).read_text(encoding="utf-8")
                self.assertIn(expected_fragment, content)


if __name__ == "__main__":
    unittest.main()
