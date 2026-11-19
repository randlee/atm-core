"""A standalone PR review reaches every reviewer the full review dispatches.

The codex QA assignment for a standalone PR (a small fix off ``develop`` with
no sprint doc) carries the reviewed scope text in the assignment itself
(``pr_scope``, as the lead captured it, judged at ``pr_head_sha``) because the
PR body on GitHub is live, editable metadata. The text sits in a CDATA section
so XML metacharacters and a literal ``</pr-scope>`` round-trip exactly, and it
is emitted byte for byte apart from one normalization, CRLF and lone CR to LF
(XML readers normalize line ends anyway, so every reviewer must see the same
bytes): trimming is only the empty check, so a Markdown code block's
indentation and blank lines survive.
quality-mgr then renders the structured reviewer assignments from the same
inputs: ``req-qa`` and ``arch-qa`` take ``pr_scope`` in place of
``sprint_doc``; the Rust reviewers, ``ruthless-boundary-qa`` and
``flaky-test-qa`` never took a sprint doc and get the same scope of record and
head through their ``notes``. Every case renders through the real
``sc-compose`` wheel, end to end: first the QA assignment, then each reviewer
assignment from what it carries.
"""

from __future__ import annotations

import json
from pathlib import Path
import re
import sys
import unittest
import xml.etree.ElementTree as ET

REPO_ROOT = Path(__file__).resolve().parents[2]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts.sc_compose_render import render_file

CODEX = ".claude/skills/codex-orchestration"
RUST = ".claude/assets/sc-rust/quality-mgr/templates"
QA_TEMPLATE = f"{CODEX}/qa-template.xml.j2"
REQ_QA = f"{CODEX}/req-qa-assignment.json.j2"
ARCH_QA = f"{CODEX}/arch-qa-assignment.json.j2"
# Reviewers of the full implementation set that take no scope document; the
# scope of record reaches them through `notes`.
NOTES_REVIEWERS = (
    (f"{CODEX}/ruthless-boundary-qa-assignment.json.j2", {"qa_round": 1}),
    (f"{CODEX}/flaky-test-qa-assignment.json.j2", {}),
    (f"{RUST}/rust-qa-assignment.json.j2", {}),
    (f"{RUST}/rust-best-practices-assignment.json.j2", {"qa_round": 1}),
    (f"{RUST}/rust-service-hardening-assignment.json.j2", {"qa_round": 1}),
)
# A CDATA-only element body: each section ends at its first `]]>`, which is
# exactly where the template's `]]>` split puts it.
PR_SCOPE_ELEMENT = re.compile(r"<pr-scope>(?:<!\[CDATA\[.*?\]\]>)+</pr-scope>", re.DOTALL)

PR_SCOPE = (
    'Native doctor can report the default Herdr socket twice, including a "null" session.\n\n'
    "Acceptance:\n"
    "- `atm doctor` lists the default socket once\n"
    "- `python -m pytest .claude/skills/daemon-switch/tests` passes (Windows host)\n"
    "Console-close termination is not addressed."
)
# Every delimiter and metacharacter that could truncate or break the carrier,
# plus the whitespace a trim would eat: leading and trailing blank lines, a
# four-space Markdown code block, and trailing spaces (a Markdown line break),
# plus CRLF and lone-CR line endings as a Windows capture would carry them.
HOSTILE_SCOPE = (
    "\r\n\n"
    "    atm doctor --json | jq .sockets  \r\n"
    "    # expected: one entry\r"
    "Reject </pr-scope> early & keep <tag attr=\"x\"> literal; the body shows &amp; and &lt;.  \n"
    "A criterion with ]]> inside, then ]]>]]> twice, and a trailing <![CDATA[ opener.\r\n"
    "Acceptance: `grep -c '</pr-scope>' out.xml` prints 0 and \"quotes\" 'survive'.\n"
    "\r\n"
)
# The template's one normalization: CRLF and lone CR become LF, nothing else changes.
EXPECTED_SCOPE = HOSTILE_SCOPE.replace("\r\n", "\n").replace("\r", "\n")
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


def pr_scope_text(xml: str) -> str:
    """The captured scope as an XML reader gets it: the parsed `<pr-scope>` element's text."""
    match = PR_SCOPE_ELEMENT.search(xml)
    if match is None:
        raise AssertionError("<pr-scope> is not a well-formed CDATA-only element")
    return ET.fromstring(match.group(0)).text or ""


def scope_note(pr_number: str, head: str, text: str) -> str:
    """What step b tells quality-mgr to put in every other reviewer's `notes`."""
    return f"PR scope of record (PR #{pr_number}, head {head}): {text}"


def reviewer_inputs(xml: str) -> dict[str, object]:
    """Build what quality-mgr hands every reviewer, taken from the rendered assignment."""
    source = re.search(r'<scope-source kind="pr" pr="([^"]*)" head="([^"]*)">', xml)
    assert source is not None, "no PR scope source in the rendered assignment"
    pr_number, head = source.group(1), source.group(2)
    text = pr_scope_text(xml)
    return {
        "review_mode": "sprint_review",
        "worktree_path": element(xml, "worktree"),
        "branch": element(xml, "branch"),
        "commit": element(xml, "commits"),
        "review_targets": [element(xml, "review-targets").strip()],
        "reference_docs": [element(xml, "references").strip()],
        "pr_number": pr_number,
        "pr_head_sha": head,
        "pr_scope": text,
        "notes": scope_note(pr_number, head, text),
    }


def notes_only(inputs: dict[str, object]) -> dict[str, object]:
    return {k: v for k, v in inputs.items() if k in ("review_mode", "worktree_path", "review_targets", "notes")}


class StandalonePrReviewDispatchTests(unittest.TestCase):
    def test_qa_assignment_carries_the_captured_scope_text_verbatim(self) -> None:
        xml = render_xml(LEAD_INPUTS)
        self.assertIn('<scope-source kind="pr" pr="2457" head="' + HEAD + '">', xml)
        self.assertEqual(pr_scope_text(xml), PR_SCOPE)
        self.assertNotIn("<authoritative-sprint-doc>", xml)
        workflow = element(xml, "workflow")
        self.assertIn("This is a full review", workflow)
        self.assertIn(f"`{scope_note('2457', HEAD, '')}` followed by the pr-scope text verbatim", workflow)
        self.assertIn("No reviewer runs without the captured scope and the pinned head", workflow)

    def test_hostile_scope_text_round_trips_through_the_xml_and_every_reviewer(self) -> None:
        # The probe must keep what a trim would eat and carry both foreign line
        # endings, or the round trip proves nothing.
        self.assertNotEqual(HOSTILE_SCOPE, HOSTILE_SCOPE.strip())
        self.assertTrue(HOSTILE_SCOPE.startswith("\r\n\n    "))
        self.assertIn("  \r\n", HOSTILE_SCOPE)
        self.assertIn("  \n", HOSTILE_SCOPE)
        self.assertRegex(HOSTILE_SCOPE, r"\r[^\n]")
        self.assertTrue(HOSTILE_SCOPE.endswith(".\n\r\n"))
        self.assertNotIn("\r", EXPECTED_SCOPE)
        self.assertEqual(EXPECTED_SCOPE.count("\n"), HOSTILE_SCOPE.count("\n") + 1)
        self.assertTrue(EXPECTED_SCOPE.startswith("\n\n    ") and EXPECTED_SCOPE.endswith(".\n\n"))
        xml = render_xml({**LEAD_INPUTS, "pr_scope": HOSTILE_SCOPE})
        self.assertEqual(pr_scope_text(xml), EXPECTED_SCOPE)
        self.assertNotIn("\r", PR_SCOPE_ELEMENT.search(xml).group(0))
        # quality-mgr reads the text from the assignment; the reviewer templates
        # also normalize on their own, so a raw capture given to them agrees.
        inputs = reviewer_inputs(xml)
        self.assertEqual(inputs["pr_scope"], EXPECTED_SCOPE)
        for template in (REQ_QA, ARCH_QA):
            for label, text in (("from assignment", EXPECTED_SCOPE), ("raw capture", HOSTILE_SCOPE)):
                with self.subTest(template=template, given=label):
                    data = render_json(template, {**inputs, "pr_scope": text})
                    self.assertEqual(data["pr_scope"]["text"], EXPECTED_SCOPE)
        for template, extra in NOTES_REVIEWERS:
            with self.subTest(template=template):
                notes = render_json(template, {**notes_only(inputs), **extra})["notes"]
                self.assertTrue(notes.endswith(scope_note("2457", HEAD, EXPECTED_SCOPE)), notes)
                self.assertNotIn("\r", notes)

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

    def test_standalone_req_qa_assignment_satisfies_the_req_qa_input_contract(self) -> None:
        # The live case: the lead names no phase or sprint documents at all.
        inputs = {**reviewer_inputs(render_xml(LEAD_INPUTS)), "reference_docs": []}
        data = render_json(REQ_QA, inputs)
        self.assertEqual(data["phase_or_sprint_docs"], [])
        self.assertEqual(data["phase_sprint_documents"], [])
        self.assertIsNone(data["authoritative_sprint_doc"])
        self.assertEqual(data["pr_scope"]["text"], PR_SCOPE)
        self.assertEqual(data["pr_scope"]["head_sha"], HEAD)
        # req-qa.md's contract: exactly one task-level source, and the document
        # arrays may be empty only on the pr_scope side.
        self.assertNotEqual(data["authoritative_sprint_doc"] is None, data["pr_scope"] is None)
        contract = (REPO_ROOT / ".claude/agents/req-qa.md").read_text(encoding="utf-8")
        self.assertIn("for a standalone PR\n  review (`pr_scope` set) it may be empty", contract)
        self.assertNotIn("must contain one or more repo-relative\n  paths.", contract)

    def test_every_other_full_review_reviewer_gets_the_scope_and_head_in_notes(self) -> None:
        inputs = reviewer_inputs(render_xml(LEAD_INPUTS))
        for template, extra in NOTES_REVIEWERS:
            with self.subTest(template=template):
                data = render_json(template, {**notes_only(inputs), **extra})
                self.assertEqual(data["worktree_path"], "/w/daemon-switch")
                self.assertIn(PR_SCOPE, data["notes"])
                self.assertIn(HEAD, data["notes"])
                self.assertIn("PR #2457", data["notes"])

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
