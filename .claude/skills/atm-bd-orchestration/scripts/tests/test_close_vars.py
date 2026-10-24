"""close-vars.py and close_fields_backfill.py: vars come from poured beads, never hand assembly (#557)."""

from __future__ import annotations

import importlib.machinery
import importlib.util
import sys
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).parents[1]
sys.path.insert(0, str(SCRIPTS))
sys.path.insert(0, str(SCRIPTS.parents[1] / "atm-beads" / "scripts"))
import close_fields  # noqa: E402
from rendering import render  # noqa: E402


def load(name: str, file: str):
    loader = importlib.machinery.SourceFileLoader(name, str(SCRIPTS / file))
    module = importlib.util.module_from_spec(importlib.util.spec_from_loader(loader.name, loader))
    loader.exec_module(module)
    return module


close_vars_cli = load("close_vars_cli", "close-vars.py")
backfill = load("close_fields_backfill", "close_fields_backfill.py")
CONFIG = {"lint_command": "just lint", "policy_path": "policy.md", "max_sanity_rounds": 5}
REPO = Path("/repo")


def poured(bead: str, role: str, **extra) -> dict:
    return {"role": role, "phase": "d", "sprint": "d-1", "sprint_bead": "t-d-1", "stack": "phase-d",
            "stack_bead": "t-phase-d.stack", "branch": "fix/d-1-f1", "worktree": "wt/fix/d-1-f1", "pr_target": "sprint/d-1",
            "base": "sprint/d-1", "checked_bead": "t-d-1.f1-fix", "checked_title": "d-1: fix f1: broken", "sanity_bead": "t-d-1.f1-sanity",
            "qa_bead": "t-d-1.f1-qa", "phase_feature": "phase-d: the feature", **CONFIG, **extra}


def row(bead: str, stage: str, role: str, status: str = "open", **extra) -> dict:
    return {"id": bead, "status": status, "labels": ["phase-d", stage], "title": bead, "metadata": poured(bead, role, **extra)}


FIX = row("t-d-1.f1-fix", "stage:fix", "fix", status="closed", commit="abc123", pr_number=2214, pr_url="u")
QA = row("t-d-1.f1-qa", "stage:qa", "qa", layer=34, round=1, reviewers_round1=["req-qa"])


class CloseVarsTests(unittest.TestCase):
    def test_close_vars_come_from_the_one_bead_and_resolve_the_worktree(self):
        role, values, remaining = close_fields.close_vars(row("t-d-1.f1-fix", "stage:fix", "fix"), REPO)
        self.assertEqual(role, "fix")
        self.assertEqual(values["sanity_task_id"], "t-d-1.f1-sanity")
        self.assertEqual(values["worktree_path"], str((REPO / "wt/fix/d-1-f1").resolve()))
        self.assertNotIn("sanity_bead", remaining)

    def test_unpoured_and_mismatched_beads_are_refused_with_the_backfill_fix(self):
        bare = {"id": "t-x", "labels": ["stage:dev"], "metadata": {"role": "dev"}}
        with self.assertRaises(close_fields.CloseFieldsError) as caught:
            close_fields.close_vars(bare, REPO)
        self.assertEqual(caught.exception.code, "CLOSE_VARS.NOT_POURED")
        self.assertIn("close_fields_backfill.py", caught.exception.fix)
        with self.assertRaises(close_fields.CloseFieldsError) as caught:
            close_fields.close_vars(row("t-d-1.f1-fix", "stage:fix", "dev"), REPO)
        self.assertEqual(caught.exception.code, "CLOSE_VARS.ROLE_MISMATCH")

    def test_sanity_close_takes_its_round_from_started_runs_and_reviewers_from_its_qa_bead(self):
        sanity = row("t-d-1.f1-sanity", "stage:dev-sanity", "sanity")
        rows = {r["id"]: r for r in (sanity, QA)}
        original = close_vars_cli.read
        close_vars_cli.read = lambda repo, bead, fallback: rows[bead]
        try:
            result = close_vars_cli.build(REPO, sanity["id"], runs=lambda task: 2)
        finally:
            close_vars_cli.read = original
        self.assertEqual(result["vars"]["round"], 2)
        self.assertEqual(result["vars"]["reviewers_round1"], ["req-qa"])


class QaAssignmentTests(unittest.TestCase):
    def test_pr_number_comes_from_the_checked_bead_when_the_qa_bead_lacks_it(self):
        # #2214: a hand-built QA vars file dropped pr_number and the assignment rendered an empty PR.
        values = close_fields.qa_assignment_vars(QA, FIX, REPO)
        self.assertEqual(values["pr_number"], "2214")
        self.assertEqual((values["commit"], values["branch"], values["layer"]), ("abc123", "fix/d-1-f1", 34))
        self.assertEqual(values["carry_forward"], FIX["id"])

    def test_the_built_vars_render_the_qa_template_with_its_pr(self):
        result = render(SCRIPTS.parent / "templates/qa-template.xml.j2", close_fields.qa_assignment_vars(QA, FIX, REPO))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("<pr-number><![CDATA[2214]]></pr-number>", result.stdout)

    def test_a_checked_bead_without_a_recorded_pr_is_refused_not_sent_empty(self):
        unrecorded = {**FIX, "metadata": {k: v for k, v in FIX["metadata"].items() if k != "pr_number"}}
        with self.assertRaises(close_fields.CloseFieldsError) as caught:
            close_fields.qa_assignment_vars(QA, unrecorded, REPO)
        self.assertEqual(caught.exception.code, "ASSIGN_VARS.NOT_RECORDED")
        self.assertIn("pr_number", str(caught.exception))

    def test_a_stale_qa_value_and_an_unlinked_layer_are_refused(self):
        stale = {**QA, "metadata": {**QA["metadata"], "pr_number": 2199}}
        with self.assertRaises(close_fields.CloseFieldsError) as caught:
            close_fields.qa_assignment_vars(stale, FIX, REPO)
        self.assertEqual(caught.exception.code, "ASSIGN_VARS.CONFLICT")
        unlinked = {**QA, "metadata": {k: v for k, v in QA["metadata"].items() if k != "layer"}}
        with self.assertRaises(close_fields.CloseFieldsError) as caught:
            close_fields.qa_assignment_vars(unlinked, FIX, REPO)
        self.assertEqual(caught.exception.code, "ASSIGN_VARS.NOT_PLANNED")


class BackfillTests(unittest.TestCase):
    def group(self, **dev_meta):
        container = {"id": "t-d-1", "status": "open", "labels": ["phase-d"], "title": "d-1: the work", "metadata": {}}
        carried = {"phase": "d", "sprint": "d-1", "sprint_bead": "t-d-1", "stack": "phase-d", "branch": "sprint/d-1-work",
                   "worktree": "wt/sprint/d-1-work", "pr_target": "integrate/phase-d", "difficulty": "normal"}
        dev = {"id": "t-d-1.group-dev", "status": "in_progress", "labels": ["phase-d", "stage:dev"], "title": "dev",
               "metadata": {"role": "dev", **carried, **dev_meta}}
        sanity = {"id": "t-d-1.group-sanity", "status": "open", "labels": ["phase-d", "stage:dev-sanity"], "title": "sanity",
                  "metadata": {"dev_bead": dev["id"], **carried},
                  "dependencies": [{"issue_id": "t-d-1.group-sanity", "depends_on_id": dev["id"], "type": "blocks"}]}
        qa = {"id": "t-d-1.group-qa", "status": "open", "labels": ["phase-d", "stage:qa"], "title": "qa",
              "metadata": {"role": "qa", "checked_bead": dev["id"], "round": 1, "reviewers_round1": ["req-qa"], **carried}}
        finding = {"id": "t-d-1.qa1-f1", "status": "open", "labels": ["phase-d", "stage:finding"], "title": "f1", "metadata": {}}
        return [container, dev, sanity, qa, finding]

    def plan(self, rows):
        return backfill.plan(rows, phase_feature="phase-d: the feature", stack_bead="t-phase-d.stack", config=CONFIG)

    def test_fills_every_open_task_bead_from_its_group_and_skips_unstarted_findings(self):
        fills, problems = self.plan(self.group())
        self.assertEqual(problems, [])
        by_bead = {fill["bead"]: fill["set_metadata"] for fill in fills}
        self.assertEqual(sorted(by_bead), ["t-d-1.group-dev", "t-d-1.group-qa", "t-d-1.group-sanity"])
        dev = by_bead["t-d-1.group-dev"]
        self.assertEqual((dev["sanity_bead"], dev["qa_bead"], dev["checked_title"]),
                         ("t-d-1.group-sanity", "t-d-1.group-qa", "d-1: the work"))
        self.assertEqual(by_bead["t-d-1.group-sanity"]["checked_bead"], "t-d-1.group-dev")

    def test_a_conflicting_set_field_is_reported_and_nothing_is_overwritten(self):
        fills, problems = self.plan(self.group(base="sprint/elsewhere"))
        self.assertTrue(any("metadata.base" in problem and "left unchanged" in problem for problem in problems), problems)
        self.assertNotIn("base", {k for fill in fills for k in fill["set_metadata"] if fill["bead"] == "t-d-1.group-dev"})


if __name__ == "__main__":
    unittest.main()
