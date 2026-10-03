"""Fix rounds never run the subjective reviewers open-ended.

Rand's ruling (2026-10-02): on a fix-verification round,
``rust-best-practices-agent``, ``ruthless-boundary-qa`` and
``rust-service-hardening-agent`` only re-check their own carried finding ids.
Their assignment templates require ``qa_round`` and refuse to render above
round 1 without those ids, and the codex QA assignment requires
``round_index`` and refuses a round above 1 without triage records. Each case
renders the template through the real ``sc-compose`` binary.
"""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import tempfile
import unittest

REPO_ROOT = Path(__file__).resolve().parents[2]
SUBJECTIVE_TEMPLATES = (
    ".claude/assets/sc-rust/quality-mgr/templates/rust-best-practices-assignment.json.j2",
    ".claude/assets/sc-rust/quality-mgr/templates/rust-service-hardening-assignment.json.j2",
    ".claude/skills/codex-orchestration/ruthless-boundary-qa-assignment.json.j2",
)
QA_TEMPLATE = ".claude/skills/codex-orchestration/qa-template.xml.j2"
REVIEWER_BASE = {"review_mode": "sprint_review", "worktree_path": "/w", "review_targets": ["src/"]}
QA_BASE = {
    "task_id": "t", "sprint": "s", "sprint_doc": "d.md", "review_mode": "sprint",
    "description": "x", "pr_number": "1", "branch": "sprint/s-1-x", "worktree_path": "/w",
    "commits": "abc1234", "review_targets": "src", "references": "docs/project-plan.md",
}


def render(template: str, values: dict[str, object], strict: bool = True) -> subprocess.CompletedProcess[str]:
    with tempfile.NamedTemporaryFile("w", suffix=".json", encoding="utf-8", delete=False) as handle:
        json.dump(values, handle)
    command = ["sc-compose", "render", "--root", str(REPO_ROOT), "--file", str(REPO_ROOT / template),
               "--var-file", handle.name]
    try:
        return subprocess.run(command + (["--strict"] if strict else []),
                              capture_output=True, text=True, check=False)
    finally:
        Path(handle.name).unlink()


def rendered_json(template: str, values: dict[str, object]) -> dict[str, object]:
    completed = render(template, values)
    if completed.returncode != 0:
        raise AssertionError(f"{template} failed to render: {completed.stderr.strip()}")
    return json.loads(completed.stdout)


class SubjectiveReviewerScopeLockTests(unittest.TestCase):
    def test_round_one_renders_an_open_review(self) -> None:
        for template in SUBJECTIVE_TEMPLATES:
            with self.subTest(template=template):
                data = rendered_json(template, {**REVIEWER_BASE, "qa_round": 1})
                self.assertIs(data["findings_scope_locked"], False)
                self.assertEqual(data["carry_forward_findings"], [])

    def test_fix_round_with_own_ids_renders_scope_locked(self) -> None:
        for template in SUBJECTIVE_TEMPLATES:
            with self.subTest(template=template):
                data = rendered_json(template, {**REVIEWER_BASE, "qa_round": 2,
                                                "carry_forward_findings_json": '["RBQA-004"]'})
                self.assertIs(data["findings_scope_locked"], True)
                self.assertEqual(data["carry_forward_findings"], ["RBQA-004"])

    def test_fix_round_with_empty_scope_refuses_to_render(self) -> None:
        for template in SUBJECTIVE_TEMPLATES:
            for scope in (None, "[]", "[ ]", "", "  ", "null"):
                with self.subTest(template=template, scope=scope):
                    values: dict[str, object] = {**REVIEWER_BASE, "qa_round": 2}
                    if scope is not None:
                        values["carry_forward_findings_json"] = scope
                    completed = render(template, values)
                    self.assertNotEqual(completed.returncode, 0, completed.stdout)
                    self.assertIn("ERR_RENDER_JSON_MALFORMED", completed.stderr)

    def test_missing_round_refuses_to_render(self) -> None:
        for template in SUBJECTIVE_TEMPLATES:
            with self.subTest(template=template):
                completed = render(template, {**REVIEWER_BASE, "carry_forward_findings_json": '["RBQA-004"]'})
                self.assertNotEqual(completed.returncode, 0, completed.stdout)
                self.assertIn("missing required variable: qa_round", completed.stderr)


class CodexQaAssignmentRoundTests(unittest.TestCase):
    # The XML task templates use {% autoescape %}, which `--strict` rejects as an
    # undeclared token on develop already, so these render without it.
    def qa(self, **values: object) -> subprocess.CompletedProcess[str]:
        return render(QA_TEMPLATE, {**QA_BASE, **values}, strict=False)

    def test_round_one_sprint_branch_runs_the_full_set(self) -> None:
        completed = self.qa(round_index=1)
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("This is round 1: render every reviewer assignment with `qa_round` = 1.", completed.stdout)
        self.assertNotIn("This is a fix round", completed.stdout)

    def test_fix_round_with_triage_records_is_scope_locked(self) -> None:
        completed = self.qa(round_index=3, triage_records=".triage/phase-x/findings/X-1.ttl")
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("This is a fix round", completed.stdout)
        self.assertIn("rendered with `qa_round` = 3", completed.stdout)

    def test_first_qa_of_a_fix_branch_is_a_fix_round(self) -> None:
        completed = self.qa(round_index=1, branch="fix/s-1-x")
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("This is a fix round", completed.stdout)
        self.assertIn("rendered with `qa_round` = 2", completed.stdout)
        self.assertNotIn("This is round 1", completed.stdout)

    def test_fix_round_without_triage_records_refuses_to_render(self) -> None:
        for records in (None, "", "  \n"):
            with self.subTest(triage_records=records):
                values: dict[str, object] = {"round_index": 2}
                if records is not None:
                    values["triage_records"] = records
                completed = self.qa(**values)
                self.assertNotEqual(completed.returncode, 0, completed.stdout)

    def test_missing_round_refuses_to_render(self) -> None:
        completed = self.qa()
        self.assertNotEqual(completed.returncode, 0, completed.stdout)
        self.assertIn("missing required variable: round_index", completed.stderr)


if __name__ == "__main__":
    unittest.main()
