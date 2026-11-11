"""close-vars.py and close_fields_backfill.py: vars come from poured beads, never hand assembly (#557)."""

from __future__ import annotations

import importlib.machinery
import importlib.util
import json
import os
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

from subprocess_helpers import isolated_git_env, run_fixture_git, set_fixture_hooks_path

SCRIPTS = Path(__file__).parents[1]
sys.path.insert(0, str(SCRIPTS))
sys.path.insert(0, str(SCRIPTS.parents[1] / "atm-beads" / "scripts"))
sys.path.insert(0, str(SCRIPTS / "tests"))
import close_fields  # noqa: E402
from rendering import render  # noqa: E402
from task_close_world import World as CloseHookWorld  # noqa: E402


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
CURRENT_PR = {"number": 2214, "base": "fix/d-1-f2", "head": "fix/d-1-f1", "head_sha": "def456789"}


def test_rebased_fixture_layer_updates_qa_vars_and_sanity_close_context(tmp_path: Path, monkeypatch) -> None:
    fixture = tmp_path / "rebased-layer"
    fixture.mkdir()

    git_env, _ = isolated_git_env(fixture)
    for key in tuple(os.environ):
        if key.startswith("GIT_"):
            monkeypatch.delenv(key, raising=False)
    for key, value in git_env.items():
        monkeypatch.setenv(key, value)

    def git(*args: str) -> str:
        completed = run_fixture_git(["git", *args], cwd=fixture, env=git_env)
        return completed.stdout.strip()

    git("init", "-b", "stack-old")
    set_fixture_hooks_path(fixture, env=git_env)
    git("config", "user.name", "ATM Test")
    git("config", "user.email", "atm-test@example.invalid")
    git("config", "commit.gpgsign", "false")
    (fixture / "base.txt").write_text("old stack base\n")
    git("add", "base.txt")
    git("commit", "-m", "old stack base")
    git("switch", "-c", "fix/d-1-f1")
    (fixture / "fix.txt").write_text("layer change\n")
    git("add", "-A")
    git("commit", "-m", "layer change")
    first_head = git("rev-parse", "HEAD")
    git("switch", "-c", "stack-new", "stack-old")
    (fixture / "new-base.txt").write_text("new stack base\n")
    git("add", "-A")
    git("commit", "-m", "new stack base")
    new_base = git("rev-parse", "HEAD")
    git("switch", "fix/d-1-f1")
    git("rebase", "stack-new")
    rebased_head = git("rev-parse", "HEAD")
    assert git("rev-parse", "HEAD^") == new_base
    assert rebased_head != first_head

    current_pr = {**CURRENT_PR, "base": "stack-new", "head": "fix/d-1-f1", "head_sha": rebased_head}
    recorded = {**FIX, "metadata": {**FIX["metadata"], "branch": "fix/d-1-f1", "pr_base": "stack-old",
                                      "commit": first_head}}
    rows = {QA["id"]: QA, recorded["id"]: recorded}

    with patch.object(close_vars_cli, "read", lambda repo, bead, fallback, **_: rows[bead]):
        qa_assignment = close_vars_cli.build_qa_assignment(
            fixture, QA["id"], pr_reader=lambda repo, number: current_pr)
    qa_vars = qa_assignment["vars"]
    assert qa_vars["base"] == "stack-new"
    assert qa_vars["commit"] == rebased_head

    hook_root = tmp_path / "close-hook"
    hook_root.mkdir()
    close_hook = CloseHookWorld(hook_root, monkeypatch)
    close_hook.values.update(base=qa_vars["base"], commit=qa_vars["commit"], branch=qa_vars["branch"])
    close_hook.actual_branch = qa_vars["branch"]
    close_hook.head = qa_vars["commit"]
    checked = close_hook.beads[close_hook.values["checked_bead"]]
    checked["labels"] = ["stage:finding"]
    checked["metadata"].update(role="fix", branch=qa_vars["branch"])
    api_path = f"repos/example/repo/pulls?head=example:{qa_vars['branch']}&state=open"
    close_hook.overrides[("gh", "api", api_path)] = (
        0, json.dumps([{"number": qa_vars["pr_number"], "head": {"sha": rebased_head},
                        "base": {"ref": "stack-new"}}]))
    close_hook.overrides[("git", "rev-parse", "--verify", "origin/stack-new^{commit}")] = (0, new_base)

    result = close_hook.run("fix")
    assert "ok" in result, result
    close_context = next(message["values"]["close_context"] for message in close_hook.messages
                         if message["type"] == "dev-sanity-task")
    assert close_context["base"] == new_base
    assert close_context["commit"] == rebased_head


class CloseVarsTests(unittest.TestCase):
    def test_not_poured_cli_names_complete_dispatch_repair(self):
        import contextlib
        import io
        import json

        error = close_fields.CloseFieldsError("CLOSE_VARS.NOT_POURED", "finding lacks fields", "old fix")
        output = io.StringIO()
        with patch.object(close_vars_cli, "build", side_effect=error), contextlib.redirect_stdout(output):
            code = close_vars_cli.main(["finding", "--out", "/unused"])
        self.assertEqual(code, 2)
        fix = json.loads(output.getvalue())["error"]["fix"]
        for command in ("bd update <finding> --status in_progress --assignee <dev>", "--metadata",
                        "close_fields_backfill.py --phase <x> --apply", "bd_cache.py --root . drain"):
            self.assertIn(command, fix)

    def test_close_vars_come_from_the_one_bead_and_resolve_the_worktree(self):
        role, values, remaining = close_fields.close_vars(row("t-d-1.f1-fix", "stage:fix", "fix"), REPO)
        self.assertEqual(role, "fix")
        self.assertEqual(values["sanity_task_id"], "t-d-1.f1-sanity")
        self.assertEqual(values["worktree_path"], str((REPO / "wt/fix/d-1-f1").resolve()))
        self.assertNotIn("sanity_bead", remaining)

    def test_a_stamped_finding_closes_as_a_fix_and_an_unstamped_one_names_the_backfill(self):
        # fix-assignment step f needs lead close vars for an important or minor finding (stage:finding), phase bg.
        role, values, remaining = close_fields.close_vars(row("t-d-1.qa1-f1", "stage:finding", "fix"), REPO)
        self.assertEqual(role, "fix")
        self.assertEqual((values["task_id"], values["sanity_task_id"], values["qa_task_id"]),
                         ("t-d-1.qa1-f1", "t-d-1.f1-sanity", "t-d-1.f1-qa"))
        self.assertEqual(values["worktree_path"], str((REPO / "wt/fix/d-1-f1").resolve()))
        self.assertNotIn("sanity_bead", remaining)
        unstamped = {"id": "t-d-1.qa1-f2", "labels": ["phase-d", "stage:finding"], "metadata": {"severity": "minor"}}
        with self.assertRaises(close_fields.CloseFieldsError) as caught:
            close_fields.close_vars(unstamped, REPO)
        self.assertEqual(caught.exception.code, "CLOSE_VARS.NOT_POURED")
        self.assertIn("close_fields_backfill.py", caught.exception.fix)

    def test_unpoured_and_mismatched_beads_are_refused_with_the_backfill_fix(self):
        bare = {"id": "t-x", "labels": ["stage:dev"], "metadata": {"role": "dev"}}
        with self.assertRaises(close_fields.CloseFieldsError) as caught:
            close_fields.close_vars(bare, REPO)
        self.assertEqual(caught.exception.code, "CLOSE_VARS.NOT_POURED")
        self.assertIn("close_fields_backfill.py", caught.exception.fix)
        with self.assertRaises(close_fields.CloseFieldsError) as caught:
            close_fields.close_vars(row("t-d-1.f1-fix", "stage:fix", "dev"), REPO)
        self.assertEqual(caught.exception.code, "CLOSE_VARS.ROLE_MISMATCH")

    def test_sanity_close_takes_its_round_from_assignment_events_and_reviewers_from_its_qa_bead(self):
        sanity = row("t-d-1.f1-sanity", "stage:dev-sanity", "sanity")
        rows = {r["id"]: r for r in (sanity, QA)}
        from unittest import mock
        with mock.patch.object(close_vars_cli, "read", lambda repo, bead, fallback, **_: rows[bead]):
            result = close_vars_cli.build(REPO, sanity["id"], events_for_task=lambda task: [
                {"event": "assigned", "message_id": "assignment-1"},
                {"event": "started"},
                {"event": "reassigned", "message_id": "assignment-2"},
            ])
        self.assertEqual(result["vars"]["round"], 2)
        self.assertEqual(result["vars"]["reviewers_round1"], ["req-qa"])

    def test_sanity_round_matches_hook_assignment_producer_after_reassignment(self):
        events = [
            {"event": "assigned", "message_id": "assignment-1"},
            {"event": "started"},
            {"event": "reassigned", "message_id": "assignment-2"},
        ]
        close_vars_round = close_vars_cli.sanity_round_from_events("q", events)
        hook_round = close_vars_cli.task_close_validation.assignment_details(events, "q")[2]
        self.assertEqual(close_vars_round, hook_round)
        self.assertEqual(close_vars_round, 2)

    def test_sanity_round_matches_hook_assignment_producer_after_reopen(self):
        events = [
            {"event": "assigned", "message_id": "assignment-1"},
            {"event": "started"},
            {"event": "reopened", "message_id": "reopen-2"},
        ]
        close_vars_round = close_vars_cli.sanity_round_from_events("q", events)
        hook_round = close_vars_cli.task_close_validation.assignment_details(events, "q")[2]
        self.assertEqual(close_vars_round, hook_round)
        self.assertEqual(close_vars_round, 2)


class QaAssignmentTests(unittest.TestCase):
    def test_pr_number_comes_from_the_checked_bead_when_the_qa_bead_lacks_it(self):
        # #2214: a hand-built QA vars file dropped pr_number and the assignment rendered an empty PR.
        values = close_fields.qa_assignment_vars(QA, FIX, REPO, CURRENT_PR)
        self.assertEqual(values["pr_number"], "2214")
        self.assertEqual((values["commit"], values["branch"], values["layer"]), ("def456789", "fix/d-1-f1", 34))
        self.assertEqual(values["carry_forward"], FIX["id"])

    def test_the_built_vars_render_the_qa_template_with_its_pr(self):
        result = render(SCRIPTS.parent / "templates/qa-template.xml.j2",
                        close_fields.qa_assignment_vars(QA, FIX, REPO, CURRENT_PR))
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
            close_fields.qa_assignment_vars(unlinked, FIX, REPO, CURRENT_PR)
        self.assertEqual(caught.exception.code, "ASSIGN_VARS.NOT_PLANNED")
        self.assertIn("1-based position", caught.exception.fix)
        self.assertIn("metadata.layers", caught.exception.fix)
        self.assertIn("bd update", caught.exception.fix)
        self.assertIn("--metadata", caught.exception.fix)


    def test_rebased_checked_pr_supplies_current_base_and_head_sha(self):
        # First-close values are stale after the PR is rebased; QA must inspect the current layer.
        recorded = {**FIX, "metadata": {**FIX["metadata"], "pr_base": "fix/d-1-f0"}}
        current = {**CURRENT_PR, "base": "fix/new-stack-top", "head_sha": "fedcba987"}
        values = close_fields.qa_assignment_vars(QA, recorded, REPO, current)
        self.assertEqual(values["base"], "fix/new-stack-top")
        self.assertEqual(values["commit"], "fedcba987")
        self.assertEqual(values["branch"], "fix/d-1-f1")

    def test_missing_first_close_commit_is_replaced_by_the_current_pr_head(self):
        recorded = {**FIX, "metadata": {k: v for k, v in FIX["metadata"].items() if k != "commit"}}
        values = close_fields.qa_assignment_vars(QA, recorded, REPO, CURRENT_PR)
        self.assertEqual(values["commit"], "def456789")

    def test_missing_current_pr_is_a_typed_refusal_instead_of_first_close_context(self):
        with self.assertRaises(close_fields.CloseFieldsError) as caught:
            close_fields.qa_assignment_vars(QA, FIX, REPO)
        self.assertEqual(caught.exception.code, "ASSIGN_VARS.PR_UNAVAILABLE")
        self.assertIn("base and head SHA", str(caught.exception))

    def test_current_pr_must_match_the_recorded_number_and_branch(self):
        wrong = {**CURRENT_PR, "number": 999, "head": "fix/other-branch"}
        with self.assertRaises(close_fields.CloseFieldsError) as caught:
            close_fields.qa_assignment_vars(QA, FIX, REPO, wrong)
        self.assertEqual(caught.exception.code, "ASSIGN_VARS.PR_MISMATCH")
        self.assertIn("pr_number/branch", caught.exception.fix)

    def test_qa_assignment_builder_reads_the_live_pr_for_a_rebased_checked_bead(self):

        rows = {QA["id"]: QA, FIX["id"]: FIX}
        current = {**CURRENT_PR, "base": "fix/new-stack-top", "head_sha": "fedcba987"}
        with patch.object(close_vars_cli, "read", lambda repo, bead, fallback, **_: rows[bead]):
            result = close_vars_cli.build_qa_assignment(REPO, QA["id"],
                                                        pr_reader=lambda repo, number: current)
        self.assertEqual(result["vars"]["base"], "fix/new-stack-top")
        self.assertEqual(result["vars"]["commit"], "fedcba987")

    def test_current_pr_read_has_a_timeout_and_maps_live_fields(self):
        import json
        import subprocess

        completed = subprocess.CompletedProcess([], 0, json.dumps({"number": 2214, "baseRefName": "top",
                                                                    "headRefName": "fix/d-1-f1", "headRefOid": "sha"}), "")
        with patch.object(close_vars_cli.subprocess, "run", return_value=completed) as run:
            current = close_vars_cli.read_current_pr(REPO, "2214")
        self.assertEqual(current, {"number": 2214, "base": "top", "head": "fix/d-1-f1", "head_sha": "sha"})
        self.assertEqual(run.call_args.kwargs["timeout"], close_vars_cli.GH_PR_TIMEOUT_S)

    def test_current_pr_read_failure_is_typed_and_actionable(self):
        import subprocess

        failed = subprocess.CompletedProcess([], 1, "", "not found")
        with patch.object(close_vars_cli.subprocess, "run", return_value=failed):
            with self.assertRaises(close_fields.CloseFieldsError) as caught:
                close_vars_cli.read_current_pr(REPO, "2214")
        self.assertEqual(caught.exception.code, "ASSIGN_VARS.PR_READ_FAILED")
        self.assertIn("gh pr view 2214", caught.exception.fix)


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

    def test_a_dispatched_finding_takes_its_layer_from_its_imported_sanity_and_qa_beads(self):
        layer = {"branch": "fix/d-1-qa1-f1", "worktree": "wt/fix/d-1-qa1-f1", "pr_target": "sprint/d-1-work"}
        finding = {"id": "t-d-1.qa1-f1", "status": "in_progress", "labels": ["phase-d", "stage:finding"], "title": "f1",
                   "metadata": {"phase": "d", "sprint": "d-1", "sprint_bead": "t-d-1", "stack": "phase-d", "severity": "important",
                                "reviewer": "req-qa", "found_at_commit": "abc123"}}
        sanity = {"id": "t-d-1.qa1-f1-sanity", "status": "open", "labels": ["phase-d", "stage:dev-sanity"], "title": "s",
                  "metadata": {"dev_bead": finding["id"], "phase": "d", "sprint": "d-1", "sprint_bead": "t-d-1",
                               "stack": "phase-d", "difficulty": "fast", **layer},
                  "dependencies": [{"issue_id": "t-d-1.qa1-f1-sanity", "depends_on_id": finding["id"], "type": "blocks"}]}
        qa = {"id": "t-d-1.qa1-f1-qa", "status": "open", "labels": ["phase-d", "stage:qa"], "title": "q",
              "metadata": {"role": "qa", "checked_bead": finding["id"], "round": 1, "reviewers_round1": ["req-qa"],
                           "phase": "d", "sprint": "d-1", "sprint_bead": "t-d-1", "stack": "phase-d", "difficulty": "fast", **layer}}
        fills, problems = self.plan([*self.group()[:4], finding, sanity, qa])
        self.assertEqual(problems, [])
        stamped = {fill["bead"]: fill["set_metadata"] for fill in fills}[finding["id"]]
        self.assertEqual({k: stamped[k] for k in layer}, layer)
        self.assertEqual((stamped["role"], stamped["base"], stamped["sanity_bead"], stamped["qa_bead"]),
                         ("fix", "sprint/d-1-work", sanity["id"], qa["id"]))

    def test_a_conflicting_set_field_is_reported_and_nothing_is_overwritten(self):
        fills, problems = self.plan(self.group(base="sprint/elsewhere"))
        self.assertTrue(any("metadata.base" in problem and "left unchanged" in problem for problem in problems), problems)
        self.assertNotIn("base", {k for fill in fills for k in fill["set_metadata"] if fill["bead"] == "t-d-1.group-dev"})

    def test_superseded_sanity_does_not_block_other_group(self):
        import copy
        import json

        first = self.group()
        original = first[2]
        original["status"] = "closed"
        original["notes"] = "superseded by t-d-1.group-sanity-r2: rebased"
        replacement = copy.deepcopy(original)
        replacement.update(id="t-d-1.group-sanity-r2", status="open", notes="")
        first[1]["metadata"]["sanity_bead"] = original["id"]
        first[3]["metadata"]["sanity_bead"] = original["id"]
        second = json.loads(json.dumps(self.group()).replace("t-d-1", "t-d-2").replace("d-1", "d-2"))
        fills, problems = self.plan([*first, replacement, *second])
        self.assertEqual(problems, [])
        self.assertTrue(any(item["bead"] == "t-d-2.group-dev" for item in fills))
        self.assertFalse(any("sanity_bead" in problem for problem in problems))
        self.assertEqual(first[1]["metadata"]["sanity_bead"], original["id"])
        self.assertEqual(first[3]["metadata"]["sanity_bead"], original["id"])

    def test_empty_sanity_fields_choose_live_highest_round_over_superseded(self):
        import copy

        group = self.group()
        superseded = group[2]
        superseded.update(status="closed", notes="superseded by t-d-1.group-sanity-r2: rebased")
        replacement = copy.deepcopy(superseded)
        replacement.update(id="t-d-1.group-sanity-r2", status="open", notes="")
        replacement["metadata"]["round"] = 2
        fills, problems = self.plan([*group, replacement])

        self.assertEqual(problems, [])
        by_bead = {item["bead"]: item["set_metadata"] for item in fills}
        self.assertEqual(by_bead["t-d-1.group-dev"]["sanity_bead"], replacement["id"])
        self.assertEqual(by_bead["t-d-1.group-qa"]["sanity_bead"], replacement["id"])

    def test_empty_qa_sanity_uses_live_replacement_when_work_reference_is_superseded(self):
        import copy

        group = self.group()
        superseded = group[2]
        superseded.update(status="closed", notes="superseded by t-d-1.group-sanity-r2: rebased")
        group[1]["metadata"]["sanity_bead"] = superseded["id"]
        replacement = copy.deepcopy(superseded)
        replacement.update(id="t-d-1.group-sanity-r2", status="open", notes="")
        replacement["metadata"]["round"] = 2

        fills, problems = self.plan([*group, replacement])

        self.assertEqual(problems, [])
        by_bead = {item["bead"]: item["set_metadata"] for item in fills}
        self.assertEqual(by_bead["t-d-1.group-qa"]["sanity_bead"], replacement["id"])

    def test_superseded_sanity_without_live_replacement_is_a_problem(self):
        group = self.group()
        superseded = group[2]
        superseded.update(status="closed", notes="superseded by t-d-1.group-sanity-r2: rebased")

        fills, problems = self.plan(group)

        self.assertFalse(any(item["bead"] in ("t-d-1.group-dev", "t-d-1.group-qa") for item in fills))
        self.assertTrue(any("t-d-1.group-dev" in problem and "sanity_bead" in problem for problem in problems), problems)
        self.assertTrue(any("t-d-1.group-qa" in problem and "sanity_bead" in problem for problem in problems), problems)

    def test_bad_group_is_reported_without_blocking_good_group(self):
        import json

        broken = self.group(base="wrong")
        healthy = json.loads(json.dumps(self.group()).replace("t-d-1", "t-d-2").replace("d-1", "d-2"))
        fills, problems = self.plan([*broken, *healthy])
        self.assertTrue(any("t-d-1.group-dev" in p and "bd update" in p for p in problems), problems)
        self.assertFalse(any(item["bead"].startswith("t-d-1.") for item in fills))
        self.assertTrue(any(item["bead"] == "t-d-2.group-dev" for item in fills))

    def test_invalid_member_round_is_reported_without_blocking_other_groups(self):
        import copy
        import json

        healthy = json.loads(json.dumps(self.group()).replace("t-d-1", "t-d-2").replace("d-1", "d-2"))
        for index in (2, 3):
            for invalid in ("two", None, True, 0, []):
                with self.subTest(member=index, invalid=invalid):
                    broken = self.group()
                    malformed = copy.deepcopy(broken[index])
                    malformed["id"] += "-r2"
                    malformed["metadata"]["round"] = invalid
                    fills, problems = self.plan([*broken, malformed, *healthy])
                    self.assertTrue(any(malformed["id"] in p and "metadata.round" in p and "bd update" in p
                                        for p in problems), problems)
                    self.assertFalse(any(item["bead"].startswith("t-d-1.") for item in fills))
                    self.assertTrue(any(item["bead"] == "t-d-2.group-dev" for item in fills))

    def test_invalid_round_on_only_open_member_is_reported(self):
        for index in (2, 3):
            with self.subTest(member=index):
                group = self.group()
                group[1]["status"] = "closed"
                group[5 - index]["status"] = "closed"
                group[index]["metadata"]["round"] = "bad"
                fills, problems = self.plan(group)
                self.assertFalse(fills)
                self.assertTrue(any(group[index]["id"] in p and "metadata.round" in p and "bd update" in p
                                    for p in problems), problems)

    def test_apply_journals_healthy_groups_even_when_another_has_problems(self):
        import contextlib
        import io
        from unittest.mock import Mock

        cache = Mock()
        cache.get.return_value = {"title": "phase-d"}
        cache.write.return_value = {"ok": True}
        fill = {"bead": "good", "status": "open", "set_metadata": {"role": "dev"}}
        with (patch.object(backfill.repo_config, "repo_root", return_value=REPO),
              patch.object(backfill.repo_config, "load", return_value=CONFIG),
              patch.object(backfill.sprint_index_common, "load_phase_config", return_value={"root": "t-phase-d"}),
              patch.object(backfill.bd_cache, "BdCache", return_value=cache),
              patch.object(backfill, "plan", return_value=([fill], ["bad: repair with bd update"])),
              contextlib.redirect_stdout(io.StringIO())):
            result = backfill.main(["--phase", "d", "--apply", "--actor", "dev"])
        self.assertEqual(result, 2)
        self.assertEqual(cache.write.call_args.args[0], "good")


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
        self.assertEqual(result["read_fallback"][0],
                         "ASSIGN_VARS.NOT_PLANNED on cached rows (t-qa: metadata lacks layer); informational: "
                         "the cache lagged bd; the row was re-read; no action needed")

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
            self.assertEqual(self.run_with(outcome, lambda: close_vars_cli.task_events("q")), "CLOSE_VARS.EVENTS_FAILED", outcome)
