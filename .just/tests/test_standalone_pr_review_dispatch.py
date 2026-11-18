"""A standalone PR review reaches every reviewer the full review dispatches.

The codex QA assignment for a standalone PR (a small fix off ``develop`` with
no sprint doc) carries the reviewed scope text in the assignment itself
(``pr_scope``, as the lead captured it, judged at ``pr_head_sha``) because the
PR body on GitHub is live, editable metadata. quality-mgr then renders the
structured reviewer assignments from the same inputs: ``req-qa`` and
``arch-qa`` take ``pr_scope`` in place of ``sprint_doc``; the Rust reviewers,
``ruthless-boundary-qa`` and ``flaky-test-qa`` never took a sprint doc. Every
case renders through the real ``sc-compose`` wheel, end to end: first the QA
assignment, then each reviewer assignment from what it carries.
"""

from __future__ import annotations

import json
from pathlib import Path
import re
import sys
import unittest

REPO_ROOT = Path(__file__).resolve().parents[2]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts.sc_compose_render import render_file

CODEX = ".claude/skills/codex-orchestration"
RUST = ".claude/assets/sc-rust/quality-mgr/templates"
QA_TEMPLATE = f"{CODEX}/qa-template.xml.j2"
REQ_QA = f"{CODEX}/req-qa-assignment.json.j2"
ARCH_QA = f"{CODEX}/arch-qa-assignment.json.j2"
# Reviewers of the full implementation set that take no scope document at all.
DOCLESS_REVIEWERS = (
    (f"{CODEX}/ruthless-boundary-qa-assignment.json.j2", {"qa_round": 1}),
    (f"{CODEX}/flaky-test-qa-assignment.json.j2", {}),
    (f"{RUST}/rust-qa-assignment.json.j2", {}),
    (f"{RUST}/rust-best-practices-assignment.json.j2", {"qa_round": 1}),
    (f"{RUST}/rust-service-hardening-assignment.json.j2", {"qa_round": 1}),
)

PR_SCOPE = (
    'Native doctor can report the default Herdr socket twice, including a "null" session.\n\n'
    "Acceptance:\n"
    "- `atm doctor` lists the default socket once\n"
    "- `python -m pytest .claude/skills/daemon-switch/tests` passes (Windows host)\n"
    "Console-close termination is not addressed."
)
HEAD = "0de74ac4c8bd470d3ce6924da2f668b11fa5fc90"
# What the lead passes to qa-template.xml.j2 for the live case.
LEAD_INPUTS = {
    "task_id": "qa-2457", "sprint": "fix/daemon-switch-cross-platform", "review_mode": "sprint",
    "review_kind": "full", "description": "QA of PR #2457", "pr_number": "2457", "pr_head_sha": HEAD,
    "pr_scope": PR_SCOPE, "branch": "fix/daemon-switch-cross-platform", "worktree_path": "/w/daemon-switch",
    "commits": HEAD, "review_targets": ".claude/skills/daemon-switch/", "references": "docs/requirements.md",
    "round_index": 1,
}


def render_xml(values: dict[str, object]) -> str:
    return render_file(REPO_ROOT / QA_TEMPLATE, values, root=REPO_ROOT, strict=False)


def render_json(template: str, values: dict[str, object]) -> dict[str, object]:
    rendered = render_file(REPO_ROOT / template, values, root=REPO_ROOT, strict=True)
    try:
        return json.loads(rendered)
    except json.JSONDecodeError as error:
        raise AssertionError(f"{template} rendered malformed JSON: {error}\n{rendered}") from error


def refuses(template: str, values: dict[str, object]) -> bool:
    try:
        json.loads(render_file(REPO_ROOT / template, values, root=REPO_ROOT, strict=True))
    except Exception:
        return True
    return False


def element(xml: str, name: str) -> str:
    match = re.search(rf"<{name}\b[^>]*>(.*?)</{name}>", xml, re.DOTALL)
    if match is None:
        raise AssertionError(f"<{name}> missing from the rendered assignment")
    return match.group(1)


def reviewer_inputs(xml: str) -> dict[str, object]:
    """Build what quality-mgr hands every reviewer, taken from the rendered assignment."""
    source = re.search(r'<scope-source kind="pr" pr="([^"]*)" head="([^"]*)">', xml)
    assert source is not None, "no PR scope source in the rendered assignment"
    return {
        "review_mode": "sprint_review",
        "worktree_path": element(xml, "worktree"),
        "branch": element(xml, "branch"),
        "commit": element(xml, "commits"),
        "review_targets": [element(xml, "review-targets").strip()],
        "reference_docs": [element(xml, "references").strip()],
        "pr_number": source.group(1),
        "pr_head_sha": source.group(2),
        "pr_scope": element(xml, "pr-scope").strip("\n"),
    }


class StandalonePrReviewDispatchTests(unittest.TestCase):
    def test_qa_assignment_carries_the_captured_scope_text_verbatim(self) -> None:
        xml = render_xml(LEAD_INPUTS)
        self.assertIn('<scope-source kind="pr" pr="2457" head="' + HEAD + '">', xml)
        self.assertEqual(element(xml, "pr-scope").strip("\n"), PR_SCOPE)
        self.assertNotIn("<authoritative-sprint-doc>", xml)
        self.assertIn("This is a full review", element(xml, "workflow"))

    def test_req_qa_and_arch_qa_render_from_the_assignment_with_the_pr_scope(self) -> None:
        inputs = reviewer_inputs(render_xml(LEAD_INPUTS))
        expected = {"pr_number": "2457", "head_sha": HEAD, "text": PR_SCOPE}
        for template in (REQ_QA, ARCH_QA):
            with self.subTest(template=template):
                data = render_json(template, inputs)
                self.assertIsNone(data["authoritative_sprint_doc"])
                self.assertEqual(data["pr_scope"], expected)
                self.assertEqual(data["worktree_path"], "/w/daemon-switch")
                self.assertEqual(data["branch"], "fix/daemon-switch-cross-platform")
                self.assertEqual(data["commit"], HEAD)
                self.assertEqual(data["review_targets"], [".claude/skills/daemon-switch/"])

    def test_every_other_full_review_reviewer_renders_without_a_sprint_doc(self) -> None:
        inputs = reviewer_inputs(render_xml(LEAD_INPUTS))
        for template, extra in DOCLESS_REVIEWERS:
            with self.subTest(template=template):
                values = {k: v for k, v in inputs.items()
                          if k in ("review_mode", "worktree_path", "review_targets")}
                data = render_json(template, {**values, **extra})
                self.assertEqual(data["worktree_path"], "/w/daemon-switch")
                self.assertNotIn("authoritative_sprint_doc", data)
                self.assertNotIn("pr_scope", data)

    def test_sprint_reviews_still_hand_req_qa_and_arch_qa_the_sprint_doc(self) -> None:
        inputs = {"review_mode": "sprint_review", "worktree_path": "/w", "branch": "sprint/s-1-x",
                  "review_targets": ["src/"], "reference_docs": ["docs/project-plan.md"], "sprint_doc": "d.md"}
        for template in (REQ_QA, ARCH_QA):
            with self.subTest(template=template):
                data = render_json(template, inputs)
                self.assertEqual(data["authoritative_sprint_doc"], "d.md")
                self.assertIsNone(data["pr_scope"])

    def test_req_qa_refuses_an_open_ended_or_contradictory_scope(self) -> None:
        base = {"reference_docs": ["docs/requirements.md"], "review_targets": ["src/"]}
        pr = {"pr_scope": PR_SCOPE, "pr_number": "2457", "pr_head_sha": HEAD}
        cases = {
            "neither": {},
            "blank sprint doc": {"sprint_doc": " \n"},
            "both": {"sprint_doc": "d.md", **pr},
            "pr scope without head": {**pr, "pr_head_sha": ""},
            "pr scope without pr number": {**pr, "pr_number": ""},
        }
        for name, values in cases.items():
            with self.subTest(case=name):
                self.assertTrue(refuses(REQ_QA, {**base, **values}))

    def test_arch_qa_refuses_a_contradictory_or_unpinned_pr_scope(self) -> None:
        base = {"review_mode": "sprint_review", "worktree_path": "/w", "branch": "b",
                "review_targets": ["src/"], "reference_docs": ["docs/requirements.md"]}
        pr = {"pr_scope": PR_SCOPE, "pr_number": "2457", "pr_head_sha": HEAD}
        for name, values in {"both": {"sprint_doc": "d.md", **pr},
                             "pr scope without head": {**pr, "pr_head_sha": ""},
                             "pr scope without pr number": {**pr, "pr_number": ""}}.items():
            with self.subTest(case=name):
                self.assertTrue(refuses(ARCH_QA, {**base, **values}))
        # arch-qa never required a task-level document; a scan with neither still renders.
        self.assertIsNone(render_json(ARCH_QA, base)["pr_scope"])


if __name__ == "__main__":
    unittest.main()
