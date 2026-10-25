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
    spec = importlib.util.spec_from_loader(loader.name, loader)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
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
        from unittest import mock
        with mock.patch.object(close_vars_cli, "read", lambda repo, bead, fallback, **_: rows[bead]):
            result = close_vars_cli.build(REPO, sanity["id"], runs=lambda task: 2)
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


    def test_base_is_the_pr_base_the_checked_bead_recorded_not_the_poured_target(self):
        # The poured base is pr_target; the PR's actual base is the layer below, recorded at the dev or fix close.
        recorded = {**FIX, "metadata": {**FIX["metadata"], "pr_base": "fix/d-1-f0"}}
        self.assertEqual(close_fields.qa_assignment_vars(QA, recorded, REPO)["base"], "fix/d-1-f0")

    def test_base_falls_back_to_the_poured_base_when_the_checked_bead_recorded_none(self):
        self.assertEqual(close_fields.qa_assignment_vars(QA, FIX, REPO)["base"], "sprint/d-1")


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


class CacheFirstReadTests(unittest.TestCase):
    """close-vars reads the bd-cache, and bd only when a field the vars need is missing there (#557)."""

    def setUp(self):
        import os
        import tempfile
        sys.path.insert(0, str(Path(__file__).parent))
        from test_bd_cache import FakeBd, workspace
        self.saved_beads_dir = os.environ.pop("BEADS_DIR", None)
        self.tmp = tempfile.TemporaryDirectory()
        self.root = workspace(Path(self.tmp.name))
        self.bd = FakeBd()
        self.bd.add("q", metadata={"round": 1})
        self.factory = lambda repo: close_vars_cli.bd_cache.BdCache(repo, runner=self.bd)
        self.factory(self.root).get("q")  # cached before the direct update below
        self.bd.rows["q"]["metadata"]["layer"] = 37
        self.bd.touch("q")
        self.bd.calls.clear()

    def tearDown(self):
        import os
        self.tmp.cleanup()
        if self.saved_beads_dir is not None:
            os.environ["BEADS_DIR"] = self.saved_beads_dir

    def lists(self):
        return [command for command in self.bd.calls if command[1] == "list"]

    def test_a_cached_bead_is_read_without_asking_bd(self):
        fallback: list[str] = []
        row = close_vars_cli.read(self.root, "q", fallback, cache_factory=self.factory)
        self.assertEqual(row["metadata"], {"round": 1})
        self.assertEqual((fallback, self.lists()), ([], []))

    def test_a_fresh_read_takes_a_field_set_in_bd_after_the_cache_filled_and_stores_it(self):
        fallback: list[str] = []
        row = close_vars_cli.read(self.root, "q", fallback, cache_factory=self.factory, fresh=True)
        self.assertEqual(row["metadata"]["layer"], 37)
        self.assertEqual(fallback, [])
        self.assertEqual(self.factory(self.root).cached("q")["metadata"]["layer"], 37)

    def test_pending_journal_writes_are_replayed_on_cached_and_fresh_rows(self):
        self.factory(self.root).write("q", "update", {"set_metadata": {"commit": "abc123"}}, idem_key="k1", actor="t")
        cached = close_vars_cli.read(self.root, "q", [], cache_factory=self.factory)
        fresh = close_vars_cli.read(self.root, "q", [], cache_factory=self.factory, fresh=True)
        self.assertEqual(cached["metadata"]["commit"], "abc123")
        self.assertEqual((fresh["metadata"]["layer"], fresh["metadata"]["commit"]), (37, "abc123"))

    def test_a_failed_fresh_read_falls_back_to_the_cached_row_and_says_so(self):
        self.bd.fail.append((lambda command: command[1] == "list", 1))
        fallback: list[str] = []
        row = close_vars_cli.read(self.root, "q", fallback, cache_factory=self.factory, fresh=True)
        self.assertNotIn("layer", row["metadata"])
        self.assertEqual(len(fallback), 1)
        self.assertIn("using the cached row", fallback[0])

    def test_a_bead_bd_does_not_have_is_refused(self):
        with self.assertRaises(close_fields.CloseFieldsError) as raised:
            close_vars_cli.read(self.root, "missing", [], cache_factory=self.factory, fresh=True)
        self.assertEqual(raised.exception.code, "CLOSE_VARS.READ_FAILED")

    def test_a_missing_field_on_cached_rows_is_retried_once_on_fresh_rows(self):
        seen = []

        def attempt(fresh, fallback):
            seen.append(fresh)
            if not fresh:
                raise close_fields.CloseFieldsError("ASSIGN_VARS.NOT_PLANNED", "t-qa: metadata lacks layer", "fix")
            return {"read_fallback": fallback}

        result = close_vars_cli.cached_then_fresh(attempt)
        self.assertEqual(seen, [False, True])
        self.assertIn("ASSIGN_VARS.NOT_PLANNED on cached rows", result["read_fallback"][0])

    def test_other_refusals_and_a_second_miss_are_not_retried(self):
        def refuse(code):
            def attempt(fresh, fallback):
                seen.append(fresh)
                raise close_fields.CloseFieldsError(code, "x", "fix")
            return attempt

        for code, tries in (("CLOSE_VARS.ROLE_MISMATCH", [False]), ("CLOSE_VARS.NOT_POURED", [False, True])):
            seen = []
            with self.assertRaises(close_fields.CloseFieldsError) as raised:
                close_vars_cli.cached_then_fresh(refuse(code))
            self.assertEqual((raised.exception.code, seen), (code, tries))


class TypedFailureTests(unittest.TestCase):
    """A bd or atm call that hangs or cannot start is a typed refusal, never a traceback (#557)."""

    def run_with(self, outcome, call):
        import subprocess
        from unittest import mock

        def fake(*args, **kwargs):
            if isinstance(outcome, BaseException):
                raise outcome
            return subprocess.CompletedProcess(args[0], 0, outcome, "")

        with mock.patch.object(close_vars_cli.subprocess, "run", fake):
            with self.assertRaises(close_fields.CloseFieldsError) as raised:
                call()
        return raised.exception.code

    def test_bd_show_timeout_missing_binary_and_garbage_are_read_failures(self):
        import subprocess
        for outcome in (subprocess.TimeoutExpired("bd", 30), FileNotFoundError("bd"), "not json", "[]"):
            self.assertEqual(self.run_with(outcome, lambda: close_vars_cli.show(REPO, "q")), "CLOSE_VARS.READ_FAILED", outcome)

    def test_atm_events_timeout_missing_binary_and_garbage_are_events_failures(self):
        import subprocess
        for outcome in (subprocess.TimeoutExpired("atm", 10), FileNotFoundError("atm"), "not json", "{}"):
            self.assertEqual(self.run_with(outcome, lambda: close_vars_cli.started_runs("q")), "CLOSE_VARS.EVENTS_FAILED", outcome)
