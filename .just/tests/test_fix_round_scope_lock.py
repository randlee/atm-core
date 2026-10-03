"""A fix verification dispatches exactly the reviewer(s) that filed the finding.

Rand's rulings (2026-10-03): a review of an assigned fix is not a sprint
review, whatever its round number, and plan review QA-2 and later is fix
verification too. It dispatches only the filing reviewer,
locked to the original finding id; there is no automatic ``req-qa``,
``arch-qa``, ``rust-qa-agent`` or screening panel. The codex QA assignment
takes ``filing_reviewers`` and ``triage_records`` and refuses a ``fix/`` branch
or a later plan round without them; the subjective reviewers' and
``plan-scope-reviewer``'s assignment templates refuse to render above round 1
without their own ids. Each
case renders the template through the real ``sc-compose`` binary.
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
PLAN_SCOPE_TEMPLATE = ".claude/skills/codex-orchestration/plan-scope-reviewer-assignment.json.j2"
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


class PlanScopeReviewerScopeLockTests(unittest.TestCase):
    BASE = {"phase_root_doc": "p.md", "plan_docs": ["s.md"], "reference_docs": ["g.md"],
            "worktree_path": "/w", "branch": "plan/phase-x", "commit": "abc1234"}

    def test_plan_qa_one_renders_an_open_review(self) -> None:
        data = rendered_json(PLAN_SCOPE_TEMPLATE, {**self.BASE, "round_index": 1})
        self.assertIs(data["findings_scope_locked"], False)

    def test_later_plan_round_with_own_ids_is_scope_locked(self) -> None:
        data = rendered_json(PLAN_SCOPE_TEMPLATE, {**self.BASE, "round_index": 2,
                                                   "carry_forward_findings_json": '["PSR-002"]'})
        self.assertIs(data["findings_scope_locked"], True)
        self.assertEqual(data["carry_forward_findings"], ["PSR-002"])

    def test_later_plan_round_without_ids_refuses_to_render(self) -> None:
        for scope in (None, "[]", ""):
            with self.subTest(scope=scope):
                values: dict[str, object] = {**self.BASE, "round_index": 2}
                if scope is not None:
                    values["carry_forward_findings_json"] = scope
                completed = render(PLAN_SCOPE_TEMPLATE, values)
                self.assertNotEqual(completed.returncode, 0, completed.stdout)


class CodexQaAssignmentFixVerificationTests(unittest.TestCase):
    # The XML task templates use {% autoescape %}, which `--strict` rejects as an
    # undeclared token on develop already, so these render without it.
    def qa(self, **values: object) -> subprocess.CompletedProcess[str]:
        return render(QA_TEMPLATE, {**QA_BASE, **values}, strict=False)

    @staticmethod
    def step_e(stdout: str) -> str:
        start = stdout.index('<step id="e">')
        return stdout[start:stdout.index("</step>", start)]

    @staticmethod
    def filing_reviewers(stdout: str) -> list[str]:
        start = stdout.index("<filing-reviewers>") + len("<filing-reviewers>")
        listed = stdout[start:stdout.index("</filing-reviewers>", start)]
        return [name.strip() for name in listed.split(",") if name.strip()]

    def test_sprint_rounds_one_and_two_stay_sprint_reviews(self) -> None:
        for round_index in (1, 2):
            with self.subTest(round_index=round_index):
                completed = self.qa(round_index=round_index)
                self.assertEqual(completed.returncode, 0, completed.stderr)
                step = self.step_e(completed.stdout)
                self.assertIn("This is a full review", step)
                self.assertIn("render every reviewer assignment with `qa_round` = 1", step)
                self.assertNotIn("<filing-reviewers>", completed.stdout)

    def test_fix_branch_dispatches_exactly_the_filing_reviewer(self) -> None:
        completed = self.qa(round_index=3, branch="fix/s-1-x",
                            triage_records=".triage/phase-x/findings/RBQA-004.ttl",
                            filing_reviewers="ruthless-boundary-qa")
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertEqual(self.filing_reviewers(completed.stdout), ["ruthless-boundary-qa"])
        step = self.step_e(completed.stdout)
        self.assertIn("This is fix verification, not a sprint review", step)
        self.assertIn("dispatch exactly the filing reviewer(s) `ruthless-boundary-qa`", step)
        self.assertIn("and no other reviewer", step)
        self.assertIn("(`round_index` for `plan-scope-reviewer`) = 3", step)
        for sprint_reviewer in ("req-qa", "arch-qa", "rust-qa-agent", "ceremony-finding-screen"):
            self.assertNotIn(sprint_reviewer, step)
        self.assertNotIn("This is a full review", step)

    def test_first_qa_of_a_fix_branch_is_fix_verification(self) -> None:
        completed = self.qa(round_index=1, branch="fix/s-1-x",
                            triage_records=".triage/phase-x/findings/ARCH-001.ttl",
                            filing_reviewers="arch-qa, rust-best-practices-agent")
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertEqual(self.filing_reviewers(completed.stdout), ["arch-qa", "rust-best-practices-agent"])
        step = self.step_e(completed.stdout)
        self.assertIn("This is fix verification", step)
        self.assertIn("(`round_index` for `plan-scope-reviewer`) = 2", step)
        for unfiled in ("req-qa", "rust-qa-agent", "ceremony-finding-screen"):
            self.assertNotIn(unfiled, step)

    def test_fix_verification_without_findings_or_filers_refuses_to_render(self) -> None:
        cases = (
            {"branch": "fix/s-1-x"},
            {"branch": "fix/s-1-x", "triage_records": ".triage/phase-x/findings/X-1.ttl"},
            {"branch": "fix/s-1-x", "triage_records": ".triage/phase-x/findings/X-1.ttl",
             "filing_reviewers": "  \n"},
            {"branch": "fix/s-1-x", "filing_reviewers": "req-qa"},
            {"filing_reviewers": "req-qa", "triage_records": "  \n"},
        )
        for values in cases:
            with self.subTest(**values):
                completed = self.qa(round_index=2, **values)
                self.assertNotEqual(completed.returncode, 0, completed.stdout)

    def test_plan_qa_one_is_a_full_review(self) -> None:
        completed = self.qa(round_index=1, review_mode="plan", branch="plan/phase-x")
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("This is a full review", self.step_e(completed.stdout))
        self.assertNotIn("<filing-reviewers>", completed.stdout)

    def test_plan_round_two_dispatches_only_the_filers(self) -> None:
        completed = self.qa(round_index=2, review_mode="plan", branch="plan/phase-x",
                            triage_records=".triage/phase-x/findings/PSR-002.ttl",
                            filing_reviewers="plan-scope-reviewer")
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertEqual(self.filing_reviewers(completed.stdout), ["plan-scope-reviewer"])
        step = self.step_e(completed.stdout)
        self.assertIn("This is fix verification", step)
        self.assertIn("dispatch exactly the filing reviewer(s) `plan-scope-reviewer`", step)
        for unfiled in ("req-qa", "arch-qa", "ceremony-qa", "ceremony-finding-screen"):
            self.assertNotIn(unfiled, step)

    def test_plan_round_two_without_filers_refuses_to_render(self) -> None:
        completed = self.qa(round_index=2, review_mode="plan", branch="plan/phase-x",
                            triage_records=".triage/phase-x/findings/PSR-002.ttl")
        self.assertNotEqual(completed.returncode, 0, completed.stdout)

    def test_missing_round_refuses_to_render(self) -> None:
        completed = self.qa()
        self.assertNotEqual(completed.returncode, 0, completed.stdout)
        self.assertIn("missing required variable: round_index", completed.stderr)


if __name__ == "__main__":
    unittest.main()
