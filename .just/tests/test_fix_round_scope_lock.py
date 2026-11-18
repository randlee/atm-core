"""A fix verification dispatches exactly the reviewer(s) that filed the finding.

Rand's rulings (2026-10-03): a review of an assigned fix is not a sprint
review, whatever its round number, and plan review QA-2 and later is fix
verification too. It dispatches only the filing reviewer,
locked to the original finding id; there is no automatic ``req-qa``,
``arch-qa``, ``rust-qa-agent`` or screening panel. The codex QA assignment
names its purpose with ``review_kind`` (``full`` or ``fix_verification``);
the branch name decides nothing, so the first QA of a standalone ``fix/`` PR
is a full review. A fix verification takes ``filing_reviewers`` and
``triage_records`` and refuses to render without both; a full review refuses
filing reviewers. When ``review_kind`` is omitted the compatibility default is
``fix_verification`` only when ``filing_reviewers`` is given or a plan round
is past the first, otherwise ``full``; an explicit ``full`` on a plan round
past the first refuses to render. A full review's scope source is its
``sprint_doc`` (sprint-doc and ``gh stack`` gates) or, for a standalone PR
with no sprint doc, the ``pr_scope`` text the lead captured from the PR body
and linked issues, carried in the assignment and judged at ``pr_head_sha``
(no sprint-doc or stack steps; the live PR body is never the scope of
record). The subjective reviewers' and
``plan-scope-reviewer``'s assignment templates refuse to render above round 1
without their own ids. Each
case renders the template through the real ``sc-compose`` binary.
"""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
import unittest

REPO_ROOT = Path(__file__).resolve().parents[2]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts.sc_compose_render import render_file
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
    try:
        rendered = render_file(REPO_ROOT / template, values, root=REPO_ROOT, strict=strict)
        if template.endswith(".json.j2"):
            try:
                json.loads(rendered)
            except json.JSONDecodeError as error:
                return subprocess.CompletedProcess(
                    args=["sc-compose", "wheel-render", template],
                    returncode=1,
                    stdout=rendered,
                    stderr=f"ERR_RENDER_JSON_MALFORMED: {error}",
                )
        return subprocess.CompletedProcess(
            args=["sc-compose", "wheel-render", template],
            returncode=0,
            stdout=rendered,
            stderr="",
        )
    except Exception as error:
        return subprocess.CompletedProcess(
            args=["sc-compose", "wheel-render", template], returncode=1, stdout="", stderr=str(error)
        )


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
    PR_SCOPE = ('Native doctor can report the default Herdr socket twice.\n\n'
                'Acceptance: `atm doctor` lists the default socket once; '
                '`python -m pytest .claude/skills/daemon-switch/tests` passes.')
    STANDALONE_PR = {"sprint": "fix/daemon-switch-cross-platform", "branch": "fix/daemon-switch-cross-platform",
                     "sprint_doc": "", "pr_number": "2457", "pr_head_sha": "0de74ac4c8bd470d3ce6924da2f668b11fa5fc90",
                     "pr_scope": PR_SCOPE}
    FILED = {"triage_records": ".triage/phase-x/findings/RBQA-004.ttl", "filing_reviewers": "ruthless-boundary-qa"}

    def qa(self, **values: object) -> subprocess.CompletedProcess[str]:
        return render(QA_TEMPLATE, {**QA_BASE, **values}, strict=False)

    def rendered(self, **values: object) -> str:
        completed = self.qa(**values)
        self.assertEqual(completed.returncode, 0, completed.stderr)
        return completed.stdout

    @staticmethod
    def step(stdout: str, step_id: str) -> str:
        start = stdout.index(f'<step id="{step_id}">')
        return stdout[start:stdout.index("</step>", start)]

    @classmethod
    def step_e(cls, stdout: str) -> str:
        return cls.step(stdout, "e")

    @staticmethod
    def element(stdout: str, name: str) -> str:
        start = stdout.index(f"<{name}") + len(f"<{name}")
        return stdout[start:stdout.index(f"</{name}>", start)]

    @classmethod
    def filing_reviewers(cls, stdout: str) -> list[str]:
        listed = cls.element(stdout, "filing-reviewers").removeprefix(">")
        return [name.strip() for name in listed.split(",") if name.strip()]

    def assert_full_review(self, stdout: str) -> None:
        self.assertEqual(self.element(stdout, "review-kind"), ">full")
        step = self.step_e(stdout)
        self.assertIn("This is a full review", step)
        self.assertIn("render every reviewer assignment with `qa_round` = 1", step)
        self.assertNotIn("This is fix verification", step)
        self.assertNotIn("<filing-reviewers>", stdout)

    def assert_sprint_scoped(self, stdout: str) -> None:
        self.assertIn('<scope-source kind="sprint-doc">d.md</scope-source>', stdout)
        self.assertIn("<authoritative-sprint-doc>d.md</authoritative-sprint-doc>", stdout)
        self.assertIn("<stack-discipline>", stdout)
        self.assertNotIn("<review-pin>", stdout)
        self.assertIn("Read `d.md` first", self.step(stdout, "b"))
        self.assertIn("directly from `d.md`", self.step(stdout, "c"))
        self.assertIn("sets `status: complete`", self.step(stdout, "d"))
        self.assertIn("`docs/project-plan.md` includes the sprint entry", self.step(stdout, "d"))
        self.assertIn("`gh stack view --json`", self.step(stdout, "j"))

    # --- sprint reviews keep their gates -------------------------------------------------

    def test_sprint_rounds_one_and_two_stay_sprint_reviews(self) -> None:
        for round_index in (1, 2):
            for review_kind in (None, "full"):
                with self.subTest(round_index=round_index, review_kind=review_kind):
                    values: dict[str, object] = {"round_index": round_index}
                    if review_kind is not None:
                        values["review_kind"] = review_kind
                    stdout = self.rendered(**values)
                    self.assert_full_review(stdout)
                    self.assert_sprint_scoped(stdout)

    def test_sprint_round_two_may_recheck_triage_records_as_a_full_review(self) -> None:
        stdout = self.rendered(round_index=2, triage_records=".triage/phase-x/findings/RBQA-004.ttl")
        self.assert_full_review(stdout)
        self.assertIn('<step id="m">', stdout)

    def test_fix_branch_name_alone_does_not_make_a_fix_verification(self) -> None:
        # A sprint-stack fix/ layer reviewed with no carried findings is a full review:
        # the branch prefix decides nothing, so nothing is invented to satisfy it.
        for round_index in (1, 2):
            with self.subTest(round_index=round_index):
                stdout = self.rendered(round_index=round_index, branch="fix/s-1-x")
                self.assert_full_review(stdout)
                self.assert_sprint_scoped(stdout)

    # --- fix verification: filer-only dispatch -------------------------------------------

    def test_fix_verification_dispatches_exactly_the_filing_reviewer(self) -> None:
        stdout = self.rendered(round_index=3, branch="fix/s-1-x", review_kind="fix_verification", **self.FILED)
        self.assertEqual(self.element(stdout, "review-kind"), ">fix_verification")
        self.assertEqual(self.filing_reviewers(stdout), ["ruthless-boundary-qa"])
        step = self.step_e(stdout)
        self.assertIn("This is fix verification, not a sprint review", step)
        self.assertIn("dispatch exactly the filing reviewer(s) `ruthless-boundary-qa`", step)
        self.assertIn("and no other reviewer", step)
        self.assertIn("(`round_index` for `plan-scope-reviewer`) = 3", step)
        for sprint_reviewer in ("req-qa", "arch-qa", "rust-qa-agent", "ceremony-finding-screen"):
            self.assertNotIn(sprint_reviewer, step)
        self.assertNotIn("This is a full review", step)

    def test_fix_verification_on_round_one_is_fix_verification(self) -> None:
        stdout = self.rendered(round_index=1, branch="fix/s-1-x", review_kind="fix_verification",
                               triage_records=".triage/phase-x/findings/ARCH-001.ttl",
                               filing_reviewers="arch-qa, rust-best-practices-agent")
        self.assertEqual(self.filing_reviewers(stdout), ["arch-qa", "rust-best-practices-agent"])
        step = self.step_e(stdout)
        self.assertIn("This is fix verification", step)
        self.assertIn("(`round_index` for `plan-scope-reviewer`) = 2", step)
        for unfiled in ("req-qa", "rust-qa-agent", "ceremony-finding-screen"):
            self.assertNotIn(unfiled, step)

    def test_filing_reviewers_without_review_kind_default_to_fix_verification(self) -> None:
        # Compatibility default: a caller that names filers but omits review_kind
        # still gets filer-only dispatch, on any branch.
        for branch in ("sprint/s-1-x", "fix/s-1-x"):
            with self.subTest(branch=branch):
                stdout = self.rendered(round_index=2, branch=branch, **self.FILED)
                self.assertEqual(self.element(stdout, "review-kind"), ">fix_verification")
                self.assertEqual(self.filing_reviewers(stdout), ["ruthless-boundary-qa"])
                self.assertIn("This is fix verification", self.step_e(stdout))

    def test_fix_verification_without_findings_or_filers_refuses_to_render(self) -> None:
        cases = (
            {"review_kind": "fix_verification"},
            {"review_kind": "fix_verification", "branch": "fix/s-1-x"},
            {"review_kind": "fix_verification", "triage_records": ".triage/phase-x/findings/X-1.ttl"},
            {"review_kind": "fix_verification", "triage_records": ".triage/phase-x/findings/X-1.ttl",
             "filing_reviewers": "  \n"},
            {"review_kind": "fix_verification", "filing_reviewers": "req-qa"},
            {"review_kind": "fix_verification", "filing_reviewers": "req-qa", "triage_records": "  \n"},
            # the compatibility default is fix verification here and is just as strict
            {"branch": "fix/s-1-x", "filing_reviewers": "req-qa"},
            {"filing_reviewers": "req-qa", "triage_records": "  \n"},
        )
        for values in cases:
            with self.subTest(**values):
                completed = self.qa(round_index=2, **values)
                self.assertNotEqual(completed.returncode, 0, completed.stdout)

    def test_full_review_naming_filing_reviewers_refuses_to_render(self) -> None:
        for values in ({}, {"triage_records": ".triage/phase-x/findings/X-1.ttl"}):
            with self.subTest(**values):
                completed = self.qa(round_index=2, review_kind="full", filing_reviewers="req-qa", **values)
                self.assertNotEqual(completed.returncode, 0, completed.stdout)

    def test_unknown_review_kind_refuses_to_render(self) -> None:
        for review_kind in ("fix", "sprint", "FULL", "fix-verification"):
            with self.subTest(review_kind=review_kind):
                completed = self.qa(round_index=1, review_kind=review_kind)
                self.assertNotEqual(completed.returncode, 0, completed.stdout)

    # --- standalone PR reviews: PR-body scope, no sprint-doc or stack steps -------------

    def test_standalone_fix_pr_first_qa_is_a_full_review_scoped_by_the_pr(self) -> None:
        for review_kind in (None, "full"):
            with self.subTest(review_kind=review_kind):
                values: dict[str, object] = {**self.STANDALONE_PR, "round_index": 1, "linked_issues": "#2450"}
                if review_kind is not None:
                    values["review_kind"] = review_kind
                stdout = self.rendered(**values)
                self.assert_full_review(stdout)
                head = self.STANDALONE_PR["pr_head_sha"]
                self.assertIn(f'<scope-source kind="pr" pr="2457" head="{head}">', stdout)
                self.assertIn("(#2450)", self.element(stdout, "scope-source"))
                # The captured scope text travels in the assignment itself, verbatim.
                self.assertEqual(self.element(stdout, "pr-scope").strip(">\n"), self.PR_SCOPE)
                self.assertNotIn("<authoritative-sprint-doc>", stdout)
                self.assertNotIn("<stack-discipline>", stdout)
                self.assertNotIn("gh stack", stdout)
                self.assertNotIn("status: complete", stdout)
                self.assertNotIn("docs/project-plan.md` includes", stdout)
                self.assertIn(f"Review exactly head `{head}`", self.element(stdout, "review-pin"))
                step_b = self.step(stdout, "b")
                self.assertIn("Read the `pr-scope` text above first", step_b)
                self.assertIn("do not replace it with a re-fetched PR body", step_b)
                self.assertNotIn("--json title,body", step_b)
                self.assertIn("gh pr view 2457 --json headRefOid,baseRefName", step_b)
                self.assertIn("passing the pr-scope text verbatim as `pr_scope`", step_b)
                self.assertIn("never invent scope or acceptance criteria", step_b)
                self.assertIn('atm task close t refused "<what is missing>"', step_b)
                self.assertIn(f"with head `{head}`, in the verdict", self.step(stdout, "c"))
                self.assertIn("There is no sprint doc and no project-plan entry to gate on", self.step(stdout, "d"))
                step_j = self.step(stdout, "j")
                self.assertIn("gh pr view 2457 --json headRefOid,mergeable,mergeStateStatus", step_j)
                self.assertIn("gh pr checks 2457 --json name,state,bucket", step_j)

    def test_standalone_pr_fix_verification_dispatches_only_the_filers(self) -> None:
        stdout = self.rendered(**self.STANDALONE_PR, round_index=2, review_kind="fix_verification", **self.FILED)
        self.assertEqual(self.filing_reviewers(stdout), ["ruthless-boundary-qa"])
        self.assertIn("This is fix verification", self.step_e(stdout))
        self.assertNotIn("<stack-discipline>", stdout)
        self.assertNotIn("gh stack", stdout)
        self.assertIn("<review-pin>", stdout)

    def test_standalone_pr_review_without_a_pinned_head_pr_or_scope_text_refuses_to_render(self) -> None:
        # The scope text is carried in the assignment, so a PR review without it
        # (or with a sprint doc alongside it) has no single scope of record.
        for missing in ({"pr_head_sha": ""}, {"pr_head_sha": " \n"}, {"pr_number": ""},
                        {"pr_scope": ""}, {"pr_scope": " \n"}, {"sprint_doc": "d.md"}):
            with self.subTest(**missing):
                completed = self.qa(**{**self.STANDALONE_PR, **missing}, round_index=1, review_kind="full")
                self.assertNotEqual(completed.returncode, 0, completed.stdout)

    # --- plan rounds --------------------------------------------------------------------

    def test_plan_qa_one_is_a_full_review(self) -> None:
        for review_kind in (None, "full"):
            with self.subTest(review_kind=review_kind):
                values: dict[str, object] = {"round_index": 1, "review_mode": "plan", "branch": "plan/phase-x"}
                if review_kind is not None:
                    values["review_kind"] = review_kind
                self.assert_full_review(self.rendered(**values))

    def test_plan_round_two_dispatches_only_the_filers(self) -> None:
        # Explicitly assigned, and by the documented compatibility default.
        for review_kind in (None, "fix_verification"):
            with self.subTest(review_kind=review_kind):
                values: dict[str, object] = {"round_index": 2, "review_mode": "plan", "branch": "plan/phase-x",
                                             "triage_records": ".triage/phase-x/findings/PSR-002.ttl",
                                             "filing_reviewers": "plan-scope-reviewer"}
                if review_kind is not None:
                    values["review_kind"] = review_kind
                stdout = self.rendered(**values)
                self.assertEqual(self.element(stdout, "review-kind"), ">fix_verification")
                self.assertEqual(self.filing_reviewers(stdout), ["plan-scope-reviewer"])
                step = self.step_e(stdout)
                self.assertIn("This is fix verification", step)
                self.assertIn("dispatch exactly the filing reviewer(s) `plan-scope-reviewer`", step)
                for unfiled in ("req-qa", "arch-qa", "ceremony-qa", "ceremony-finding-screen"):
                    self.assertNotIn(unfiled, step)

    def test_explicit_full_review_on_a_later_plan_round_refuses_to_render(self) -> None:
        # quality-policy.md: plan QA-2 and later is fix verification only; an explicit
        # `full` there is a contradiction the template refuses, not an override.
        for round_index in (2, 3):
            with self.subTest(round_index=round_index):
                completed = self.qa(round_index=round_index, review_mode="plan", branch="plan/phase-x",
                                    review_kind="full")
                self.assertNotEqual(completed.returncode, 0, completed.stdout)

    def test_plan_round_two_without_filers_refuses_to_render(self) -> None:
        for review_kind in (None, "fix_verification"):
            with self.subTest(review_kind=review_kind):
                values: dict[str, object] = {"round_index": 2, "review_mode": "plan", "branch": "plan/phase-x",
                                             "triage_records": ".triage/phase-x/findings/PSR-002.ttl"}
                if review_kind is not None:
                    values["review_kind"] = review_kind
                completed = self.qa(**values)
                self.assertNotEqual(completed.returncode, 0, completed.stdout)

    def test_missing_round_refuses_to_render(self) -> None:
        completed = self.qa()
        self.assertNotEqual(completed.returncode, 0, completed.stdout)
        self.assertIn("missing required variable: round_index", completed.stderr)


if __name__ == "__main__":
    unittest.main()
