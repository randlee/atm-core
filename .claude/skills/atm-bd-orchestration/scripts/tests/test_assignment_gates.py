from __future__ import annotations

from contextlib import redirect_stderr, redirect_stdout
import importlib.util
import io
import json
from pathlib import Path
import subprocess
from types import SimpleNamespace
import unittest

MODULE = Path(__file__).parents[1] / "assignment-gates.py"
FIXTURES = Path(__file__).parent / "fixtures"
SPEC = importlib.util.spec_from_file_location("assignment_gates", MODULE)
assert SPEC and SPEC.loader
gates = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gates)


class FakeRunner:
    """Answers by (cwd, argv) first, then argv; records every call's cwd."""
    def __init__(self, responses): self.responses, self.calls = responses, []
    @property
    def cwds(self): return [cwd for cwd, _ in self.calls]
    def __call__(self, args, **kwargs):
        self.calls.append((kwargs.get("cwd"), tuple(args)))
        code, stdout, *stderr = self.responses.get((kwargs.get("cwd"), tuple(args)), self.responses.get(tuple(args), (0, "")))
        return subprocess.CompletedProcess(args, code, stdout, "".join(stderr))


STACKS = (str(gates.PRIMARY), gates.STACKS)


def HEADS(flag, ref):
    return ("gh", "pr", "list", flag, ref, "--state", "open", "--json", "headRefName")


def pr(number, head, state="open", merged_at=None):
    return {"number": number, "state": state, "draft": False, "merged_at": merged_at, "head": {"ref": head, "sha": "s"}}


def stacks(*prs, base="integrate", is_open=True, others=()):
    """A faked `gh api repos/{owner}/{repo}/stacks --paginate --jq '.[]'`: one stack object per line, PRs bottom first."""
    rows = [*others, {"number": 1, "base": {"ref": base}, "open": is_open, "pull_requests": list(prs)}]
    return {STACKS: (0, "\n".join(json.dumps(row) for row in rows))}


def ns(kind, **overrides):
    values = dict(kind=kind, root="atm-phase-d", bead="bead", pr_target="target", identity="terra",
                  pr_number="7", commit="head", checked_bead="checked")
    values.update(overrides)
    return SimpleNamespace(**values)


def dumped(value): return json.dumps(value)


def dev_runner(overrides=None):
    data = {
        (gates.VALIDATE_PLAN, "--root", "atm-phase-d"): (0, ""),
        ("bd", "ready", "-n", "0", "--json"): (0, dumped([{"id": "bead"}])),
        ("bd", "show", "bead", "--json"): (0, dumped([{"status": "open", "assignee": "", "metadata": {"difficulty": "normal", "pr_target": "target"}}])),
        ("atm", "members", "--json"): (0, dumped([{"identity": "terra", "model": "gpt-6-terra"}])),
        ("git", "merge-base", "--is-ancestor", "origin/target", "HEAD"): (0, ""),
    }; data.update(overrides or {}); return FakeRunner(data)


# `bd history --json`: snapshots newest first, each `{CommitHash, ..., Issue}`.
HISTORY_NEVER_PASSED = [
    {"CommitHash": "c3", "Issue": {"id": "bead", "status": "open", "notes": "FAIL at abc: 1 findings; earlier PASS of the parent"}},
    {"CommitHash": "c2", "Issue": {"id": "bead", "status": "closed", "close_reason": "not_reproducible: PASS at fix"}},
    {"CommitHash": "c1", "Issue": {"id": "bead", "status": "open", "close_reason": "PASS at stale-field"}},
]
HISTORY_PASSED = [{"CommitHash": "c4", "Issue": {"id": "bead", "status": "open"}},
                  {"CommitHash": "c3", "Issue": {"id": "bead", "status": "closed", "close_reason": "PASS at abc1234"}}]


def sanity_runner(overrides=None):
    data = {
        ("bd", "show", "bead", "--json"): (0, dumped([{"metadata": {}}])),
        ("gh", "pr", "view", "7", "--json", "baseRefName,headRefName,headRefOid"): (0, dumped({"baseRefName": "target", "headRefName": "branch", "headRefOid": "head"})),
        **stacks(pr(5, "merged", "closed", "2026-10-01T00:00:00Z"), pr(6, "target"), pr(7, "branch")),
        HEADS("--head", "target"): (0, dumped([{"headRefName": "target"}])),
        ("git", "fetch", "origin"): (0, ""),
        ("git", "rev-parse", "target"): (0, "base"),
        ("git", "rev-parse", "origin/target"): (0, "base"),
        ("git", "log", "--format=%H", "origin/target..head"): (0, "delta"),
        ("git", "status", "--porcelain", "--untracked-files=no"): (0, "?? .beads.gate.lock"),
        ("bd", "history", "bead", "--json"): (0, dumped(HISTORY_NEVER_PASSED)),
    }; data.update(overrides or {}); return FakeRunner(data)


def qa_runner(overrides=None):
    data = {
        ("bd", "show", "bead", "--json"): (0, dumped([{"metadata": {"checked_bead": "checked", "pr_target": "target"}}])),
        ("bd", "list", "-l", "stage:dev-sanity", "--status", "closed", "-n", "0", "--json"): (0, dumped([
            {"id": "checked-sanity-old", "close_reason": "PASS at 0ld0ld0", "closed_at": "2026-10-01T00:00:00Z",
             "metadata": {"dev_bead": "checked"}},
            {"id": "checked-sanity", "close_reason": "PASS at abc1234", "closed_at": "2026-10-02T00:00:00Z",
             "metadata": {"dev_bead": "checked"}},
            {"id": "other-sanity", "close_reason": "PASS at fff9999", "closed_at": "2026-10-03T00:00:00Z",
             "metadata": {"dev_bead": "other"}}])),
        ("gh", "pr", "view", "7", "--json", "baseRefName,headRefOid"): (0, dumped({"baseRefName": "target", "headRefOid": "abc1234def"})),
        ("git", "rev-parse", "HEAD"): (0, "abc1234def"),
    }; data.update(overrides or {}); return FakeRunner(data)


class AssignmentGateTests(unittest.TestCase):
    def expected(self, name): return json.loads((FIXTURES / name).read_text())["expected"]

    def assert_fixture(self, name, gate_args, runner):
        code = gates.evaluate(gate_args, runner)
        self.assertEqual(code, self.expected(name))
        output = io.StringIO()
        argv = [gate_args.kind, "--root", gate_args.root, "--bead", gate_args.bead, "--pr-target", gate_args.pr_target,
                "--identity", gate_args.identity, "--pr-number", gate_args.pr_number, "--commit", gate_args.commit,
                "--checked-bead", gate_args.checked_bead]
        with redirect_stdout(output): exit_code = gates.main(argv, runner)
        self.assertEqual(output.getvalue().strip(), code)
        self.assertEqual(exit_code, 0 if code == "READY" else 5)

    def test_dev_refusals_and_ready(self):
        cases = [
            ("plan-invalid.json", dev_runner({(gates.VALIDATE_PLAN, "--root", "atm-phase-d"): (5, "bad")})),
            ("not-ready.json", dev_runner({("bd", "ready", "-n", "0", "--json"): (0, "[]")})),
            ("unclaimable.json", dev_runner({("bd", "show", "bead", "--json"): (0, dumped([{"status": "open", "assignee": "other", "metadata": {"difficulty": "normal"}}]))})),
            ("wrong-base.json", dev_runner({("git", "merge-base", "--is-ancestor", "origin/target", "HEAD"): (1, "")})),
            ("difficulty-mismatch.json", dev_runner({("bd", "show", "bead", "--json"): (0, dumped([{"status": "open", "assignee": "", "metadata": {"difficulty": "hard"}}]))})),
            ("dev-ready.json", dev_runner()),
        ]
        for fixture, runner in cases:
            with self.subTest(fixture=fixture): self.assert_fixture(fixture, ns("dev"), runner)

    def test_a_poured_dev_bead_takes_its_sprint_containers_pr_target(self):
        poured = {("bd", "show", "bead", "--json"): (0, dumped([{"status": "open", "assignee": "", "labels": ["stage:dev"],
                  "metadata": {"difficulty": "normal", "sprint_bead": "container"}}]))}
        for target, ancestor, expected in (("target", 1, "READY"), ("lower", 0, "READY"), ("other", 1, "PR_TARGET_MISMATCH"), ("other", 128, "GATE_CANNOT_RUN")):
            runner = dev_runner({**poured, ("bd", "show", "container", "--json"): (0, dumped([{"metadata": {"pr_target": target}}])),
                                 ("git", "merge-base", "--is-ancestor", f"origin/{target}", "origin/target"): (ancestor, "")})
            with self.subTest(container_target=target, ancestor=ancestor):
                self.assertEqual(gates.evaluate(ns("dev"), runner), expected)
        fix = {("bd", "show", "bead", "--json"): (0, dumped([{"status": "open", "assignee": "", "labels": ["stage:fix"],
               "metadata": {"difficulty": "normal", "sprint_bead": "container"}}])),
               ("bd", "show", "container", "--json"): (0, dumped([{"metadata": {"pr_target": "other"}}]))}
        self.assertEqual(gates.evaluate(ns("dev"), dev_runner(fix)), "READY")  # a fix layer's target is set at dispatch

    def test_dev_gate_runs_validate_plan_in_primary_and_git_in_worktree(self):
        runner = dev_runner({("git", "-C", "/wt", "merge-base", "--is-ancestor", "origin/target", "HEAD"): (0, "")})
        self.assertEqual(gates.evaluate(ns("dev", worktree="/wt"), runner), "READY")
        self.assertEqual(runner.cwds[0], str(gates.PRIMARY))
        behind = dev_runner({("git", "-C", "/other", "merge-base", "--is-ancestor", "origin/target", "HEAD"): (1, "")})
        self.assertEqual(gates.evaluate(ns("dev", worktree="/other"), behind), "WRONG_BASE")

    def test_dev_gate_names_why_the_plan_check_refused(self):
        plan = (gates.VALIDATE_PLAN, "--root", "atm-phase-d")
        for code, stderr, expected in ((5, "e-1: missing sprint bead", "PLAN_INVALID"),
                                       (2, "phase-d.toml does not exist", "GATE_CANNOT_RUN")):
            with self.subTest(code=code):
                err = io.StringIO()
                with redirect_stderr(err):
                    self.assertEqual(gates.evaluate(ns("dev"), dev_runner({plan: (code, "", stderr)})), expected)
                self.assertIn(stderr, err.getvalue())

    def test_sanity_refusals_and_ready(self):
        cases = [
            ("pr-required.json", ns("sanity", pr_number=""), sanity_runner()),
            ("not-stacked.json", ns("sanity"), sanity_runner({("gh", "pr", "view", "7", "--json", "baseRefName,headRefName,headRefOid"): (0, dumped({"baseRefName": "wrong", "headRefName": "branch", "headRefOid": "head"}))})),
            ("not-stacked.json", ns("sanity"), sanity_runner({("gh", "pr", "view", "7", "--json", "baseRefName,headRefName,headRefOid"): (0, dumped({"baseRefName": "target", "headRefName": "branch", "headRefOid": "moved"}))})),
            ("not-stacked.json", ns("sanity"), sanity_runner({STACKS: (0, "")})),
            ("not-stacked.json", ns("sanity"), sanity_runner(stacks(pr(6, "target")))),
            ("not-stacked.json", ns("sanity"), sanity_runner(stacks(pr(6, "target"), pr(7, "branch"), is_open=False))),
            ("not-stacked.json", ns("sanity"), sanity_runner(stacks(pr(6, "target"), pr(8, "other"), pr(7, "branch")))),
            ("sanity-ready.json", ns("sanity"), sanity_runner(stacks(pr(6, "target"), pr(8, "closed-unmerged", "closed"), pr(7, "branch"),
                                                                     others=[{"number": 2, "base": {"ref": "main"}, "open": True, "pull_requests": [pr(9, "x")]}]))),
            ("not-stacked.json", ns("sanity", pr_target="planned"), sanity_runner({("git", "merge-base", "--is-ancestor", "origin/planned", "origin/target"): (1, "")})),
            ("sanity-ready.json", ns("sanity", pr_target="planned"), sanity_runner({("git", "merge-base", "--is-ancestor", "origin/planned", "origin/target"): (0, "")})),
            ("not-rebased.json", ns("sanity"), sanity_runner({("git", "merge-base", "--is-ancestor", "origin/target", "head"): (1, "")})),
            ("zero-delta.json", ns("sanity"), sanity_runner({("git", "log", "--format=%H", "origin/target..head"): (0, "")})),
            ("dirty-tree.json", ns("sanity"), sanity_runner({("git", "status", "--porcelain", "--untracked-files=no"): (0, " M tracked.py")})),
            ("sanity-frozen.json", ns("sanity"), sanity_runner({("bd", "history", "bead", "--json"): (0, dumped(HISTORY_PASSED))})),
            ("sanity-ready.json", ns("sanity"), sanity_runner()),
        ]
        for fixture, gate_args, runner in cases:
            with self.subTest(fixture=fixture): self.assert_fixture(fixture, gate_args, runner)

    def test_sanity_checks_the_assigned_base_and_runs_git_in_the_worktree(self):
        self.assertEqual(gates.evaluate(ns("sanity", base="target"), sanity_runner()), "READY")
        self.assertEqual(gates.evaluate(ns("sanity", base="assigned-elsewhere"), sanity_runner()), "NOT_STACKED")
        moved = {("git", "-C", "/wt", "log", "--format=%H", "origin/target..head"): (0, "")}
        runner = sanity_runner(moved)
        self.assertEqual(gates.evaluate(ns("sanity", worktree="/wt"), runner), "ZERO_DELTA")
        git_calls = [argv for _, argv in runner.calls if argv[0] == "git"]
        self.assertTrue(git_calls and all(argv[1:3] == ("-C", "/wt") for argv in git_calls), git_calls)

    def test_sanity_rebase_check_that_cannot_run_is_not_a_refusal(self):
        runner = sanity_runner({("git", "merge-base", "--is-ancestor", "origin/target", "head"): (128, "")})
        self.assertEqual(gates.evaluate(ns("sanity"), runner), "GATE_CANNOT_RUN")

    def test_sanity_history_that_is_not_snapshots_cannot_run(self):
        """Malformed `bd history` output is GATE_CANNOT_RUN naming the output, never an empty history (READY)."""
        for output in (dumped({"verdict": "PASS"}), dumped([{"description": "lint passes"}]), dumped([{"Issue": "closed"}]),
                       dumped([HISTORY_NEVER_PASSED[0], "PASS at abc1234"]), "not json"):
            with self.subTest(output=output):
                err = io.StringIO()
                with redirect_stderr(err):
                    code = gates.evaluate(ns("sanity"), sanity_runner({("bd", "history", "bead", "--json"): (0, output)}))
                self.assertEqual(code, "GATE_CANNOT_RUN")
                self.assertIn(output, err.getvalue())
        self.assertEqual(gates.evaluate(ns("sanity"), sanity_runner({("bd", "history", "bead", "--json"): (0, "[]")})), "READY")

    def test_sanity_stack_checks_that_cannot_run_are_not_refusals(self):
        for override in ({STACKS: (1, "", "gh: Not Found (HTTP 404)")}, {STACKS: (0, "not json")},
                         {("git", "merge-base", "--is-ancestor", "origin/planned", "origin/target"): (128, "")}):
            with self.subTest(override=override):
                self.assertEqual(gates.evaluate(ns("sanity", pr_target="planned"), sanity_runner(override)), "GATE_CANNOT_RUN")

    def test_the_first_open_layer_is_based_on_the_trunk(self):
        runner = sanity_runner({**stacks(pr(5, "merged", "closed", "2026-10-01T00:00:00Z"), pr(7, "target")),("gh", "pr", "view", "7", "--json", "baseRefName,headRefName,headRefOid"): (0, dumped({"baseRefName": "integrate", "headRefName": "target", "headRefOid": "head"})),
                                ("git", "merge-base", "--is-ancestor", "origin/target", "origin/integrate"): (1, ""),
                                ("git", "log", "--format=%H", "origin/integrate..head"): (0, "delta")})
        self.assertEqual(gates.evaluate(ns("sanity", pr_target="integrate"), runner), "READY")
        self.assertEqual(gates.evaluate(ns("sanity"), runner), "NOT_STACKED")  # the trunk does not descend from a planned layer above it

    def test_layer_0_awaiting_layer_1_is_accepted_only_on_the_trunk(self):
        unlinked = {STACKS: (0, ""), HEADS("--head", "integrate"): (0, "[]"),
                    ("gh", "pr", "view", "7", "--json", "baseRefName,headRefName,headRefOid"): (0, dumped({"baseRefName": "integrate", "headRefName": "layer-0", "headRefOid": "head"})),
                    ("git", "log", "--format=%H", "origin/integrate..head"): (0, "delta")}
        self.assertEqual(gates.evaluate(ns("sanity", pr_target="integrate"), sanity_runner(unlinked)), "READY")
        self.assertEqual(gates.evaluate(ns("sanity", pr_target="integrate"), sanity_runner(
            {**unlinked, ("git", "merge-base", "--is-ancestor", "origin/integrate", "head"): (1, "")})), "NOT_REBASED")
        # based on another layer's branch, or on a planned layer above the trunk: it must be in a stack
        self.assertEqual(gates.evaluate(ns("sanity", pr_target="integrate"), sanity_runner(
            {**unlinked, HEADS("--head", "integrate"): (0, dumped([{"headRefName": "integrate"}]))})), "NOT_STACKED")
        self.assertEqual(gates.evaluate(ns("sanity", pr_target="planned"), sanity_runner(unlinked)), "NOT_STACKED")
        self.assertEqual(gates.evaluate(ns("sanity", pr_target="integrate"), sanity_runner(
            {**unlinked, HEADS("--head", "integrate"): (1, "", "HTTP 502")})), "GATE_CANNOT_RUN")

    def test_sanity_reads_the_stack_from_githubs_stacks_api_not_local_tracking(self):
        # The dev's worktree has no gh-stack tracking (the dev never runs gh stack), so `gh stack view` exits 2 there.
        runner = sanity_runner({("/wt", ("gh", "stack", "view", "--json")): (2, ""),
                                ("git", "-C", "/wt", "log", "--format=%H", "origin/target..head"): (0, "delta")})
        self.assertEqual(gates.evaluate(ns("sanity", worktree="/wt"), runner), "READY")
        self.assertIn(STACKS, runner.calls)
        self.assertFalse([call for call in runner.calls if call[1][:3] == ("gh", "stack", "view")])

    def test_a_lower_bound_whose_branch_is_gone_holds_only_when_its_pr_merged(self):
        gone = {("git", "merge-base", "--is-ancestor", "origin/planned", "origin/target"): (128, ""),
                ("git", "rev-parse", "--verify", "--quiet", "refs/remotes/origin/planned"): (1, "")}
        merged = ("gh", "pr", "list", "--head", "planned", "--state", "merged", "--json", "mergeCommit")
        in_base = ("git", "merge-base", "--is-ancestor", "m1", "origin/target")
        listed = {**gone, merged: (0, dumped([{"mergeCommit": {"oid": "old"}}, {"mergeCommit": {"oid": "m1"}}])),
                  ("git", "merge-base", "--is-ancestor", "old", "origin/target"): (1, "")}
        self.assertEqual(gates.evaluate(ns("sanity", pr_target="planned"), sanity_runner({**listed, in_base: (0, "")})), "READY")
        # an older merged PR of a reused branch name whose merge commit is not in the base does not satisfy the bound
        self.assertEqual(gates.evaluate(ns("sanity", pr_target="planned"), sanity_runner({**listed, in_base: (1, "")})), "GATE_CANNOT_RUN")
        self.assertEqual(gates.evaluate(ns("sanity", pr_target="planned"), sanity_runner({**gone, merged: (0, "[]")})), "GATE_CANNOT_RUN")

    def stack_top(self, responses, target="integrate"):
        output = io.StringIO()
        with redirect_stdout(output):
            code = gates.main(["stack-top", "--pr-target", target], FakeRunner(responses))
        return output.getvalue().strip(), code

    def test_stack_top_is_the_last_open_pr_of_the_one_matching_stack(self):
        other = {"number": 9, "base": {"ref": "main"}, "open": True, "pull_requests": [pr(9, "elsewhere")]}
        layers = (pr(5, "merged", "closed", "2026-10-01T00:00:00Z"), pr(6, "target"), pr(7, "branch"), pr(8, "dropped", "closed"))
        cases = [
            ("trunk match", stacks(*layers, others=[other]), "integrate", ("branch", 0)),
            ("head match", stacks(*layers, others=[other]), "target", ("branch", 0)),
            ("merged head match", stacks(*layers), "merged", ("branch", 0)),
            ("no open PR left", stacks(pr(5, "merged", "closed", "2026-10-01T00:00:00Z")), "merged", ("integrate", 0)),
            ("closed stack ignored", {**stacks(*layers, is_open=False), HEADS("--base", "integrate"): (0, "[]")}, "integrate", ("integrate", 0)),
            ("empty", {STACKS: (0, ""), HEADS("--base", "integrate"): (0, "[]")}, "integrate", ("integrate", 0)),
            ("unlinked layer 0", {STACKS: (0, ""), HEADS("--base", "integrate"): (0, dumped([{"headRefName": "layer-0"}])),
                                  HEADS("--base", "layer-0"): (0, "[]")}, "integrate", ("layer-0", 0)),
            ("unlinked layers 0 and 1", {STACKS: (0, ""), HEADS("--base", "integrate"): (0, dumped([{"headRefName": "layer-0"}])),
                                         HEADS("--base", "layer-0"): (0, dumped([{"headRefName": "layer-1"}])),
                                         HEADS("--base", "layer-1"): (0, "[]")}, "integrate", ("layer-1", 0)),
            ("unlinked chain branches", {STACKS: (0, ""), HEADS("--base", "integrate"): (0, dumped([{"headRefName": "layer-0"}])),
                                         HEADS("--base", "layer-0"): (0, dumped([{"headRefName": "a"}, {"headRefName": "b"}]))}, "integrate", ("STACK_AMBIGUOUS", 5)),
            ("unlinked chain cycles", {STACKS: (0, ""), HEADS("--base", "integrate"): (0, dumped([{"headRefName": "layer-0"}])),
                                       HEADS("--base", "layer-0"): (0, dumped([{"headRefName": "integrate"}]))}, "integrate", ("STACK_AMBIGUOUS", 5)),
            ("unlinked chain lookup fails", {STACKS: (0, ""), HEADS("--base", "integrate"): (0, dumped([{"headRefName": "layer-0"}])),
                                             HEADS("--base", "layer-0"): (1, "", "HTTP 502")}, "integrate", ("GATE_CANNOT_RUN", 2)),
            ("several unlinked PRs", {STACKS: (0, ""), HEADS("--base", "integrate"): (0, dumped([{"headRefName": "a"}, {"headRefName": "b"}]))}, "integrate", ("STACK_AMBIGUOUS", 5)),
            ("unlinked lookup fails", {STACKS: (0, ""), HEADS("--base", "integrate"): (1, "", "HTTP 502")}, "integrate", ("GATE_CANNOT_RUN", 2)),
            ("several stacks on one trunk", stacks(*layers, others=[{**other, "base": {"ref": "integrate"}}]), "integrate", ("STACK_AMBIGUOUS", 5)),
            ("API failure", {STACKS: (1, "", "gh: Not Found (HTTP 404)")}, "integrate", ("GATE_CANNOT_RUN", 2)),
            ("not json", {STACKS: (0, "<html>")}, "integrate", ("GATE_CANNOT_RUN", 2)),
        ]
        for name, responses, target, expected in cases:
            with self.subTest(name):
                self.assertEqual(self.stack_top(responses, target), expected)

    def test_gate_kinds_still_require_root_and_bead(self):
        with self.assertRaises(SystemExit):
            gates.main(["dev", "--pr-target", "target"], dev_runner())

    def test_qa_refusals_and_ready(self):
        cases = [
            ("stale-sanity.json", qa_runner({("gh", "pr", "view", "7", "--json", "baseRefName,headRefOid"): (0, dumped({"baseRefName": "target", "headRefOid": "0ld0ld0aaa"}))})),
            ("stale-sanity.json", qa_runner({("bd", "list", "-l", "stage:dev-sanity", "--status", "closed", "-n", "0", "--json"): (0, dumped([
                {"id": "checked-sanity", "close_reason": "FAIL at abc1234", "closed_at": "2026-10-02T00:00:00Z", "metadata": {"dev_bead": "checked"}}]))})),
            ("qa-head-mismatch.json", qa_runner({("git", "rev-parse", "HEAD"): (0, "other")})),
            ("pr-target-mismatch.json", qa_runner({("git", "merge-base", "--is-ancestor", "origin/target", "origin/top"): (1, ""),
                                                   ("gh", "pr", "view", "7", "--json", "baseRefName,headRefOid"): (0, dumped({"baseRefName": "top", "headRefOid": "abc1234def"}))})),
            ("qa-ready.json", qa_runner({("git", "merge-base", "--is-ancestor", "origin/target", "origin/top"): (0, ""),
                                         ("gh", "pr", "view", "7", "--json", "baseRefName,headRefOid"): (0, dumped({"baseRefName": "top", "headRefOid": "abc1234def"}))})),
            ("qa-ready.json", qa_runner()),
        ]
        for fixture, runner in cases:
            with self.subTest(fixture=fixture): self.assert_fixture(fixture, ns("qa"), runner)


    def test_a_quick_fix_qa_skips_only_the_sanity_pass_check(self):
        no_pass = {("bd", "list", "-l", "stage:dev-sanity", "--status", "closed", "-n", "0", "--json"): (0, "[]")}
        quick = {("bd", "show", "bead", "--json"): (0, dumped([{"metadata": {"checked_bead": "checked", "pr_target": "target", "quick_fix": True}}]))}
        self.assertEqual(gates.evaluate(ns("qa"), qa_runner({**no_pass, **quick})), "READY")
        self.assertEqual(gates.evaluate(ns("qa"), qa_runner(no_pass)), "SANITY_STALE")
        self.assertEqual(gates.evaluate(ns("qa"), qa_runner({**no_pass, ("bd", "show", "bead", "--json"): (0, dumped([
            {"metadata": {"checked_bead": "checked", "pr_target": "target", "quick_fix": False}}]))})), "SANITY_STALE")
        self.assertEqual(gates.evaluate(ns("qa"), qa_runner({**no_pass, **quick, ("git", "rev-parse", "HEAD"): (0, "other")})), "QA_HEAD_MISMATCH")
        self.assertEqual(gates.evaluate(ns("qa", pr_number=""), qa_runner({**no_pass, **quick})), "PR_REQUIRED")
        self.assertEqual(gates.evaluate(ns("qa"), qa_runner({**no_pass, **quick, ("git", "merge-base", "--is-ancestor", "origin/target", "origin/top"): (1, ""),
                                                            ("gh", "pr", "view", "7", "--json", "baseRefName,headRefOid"): (0, dumped({"baseRefName": "top", "headRefOid": "abc1234def"}))})),
                         "PR_TARGET_MISMATCH")

    def test_a_quick_fix_layer_on_the_stack_top_passes_its_lower_bound(self):
        # R4: the quick fix is a new layer on the stack top; its QA bead's pr_target is the lowest branch it needs.
        quick = {("bd", "show", "bead", "--json"): (0, dumped([{"metadata": {"checked_bead": "finder", "pr_target": "sprint/d-2", "quick_fix": True}}])),
                 ("bd", "list", "-l", "stage:dev-sanity", "--status", "closed", "-n", "0", "--json"): (0, "[]"),
                 ("gh", "pr", "view", "7", "--json", "baseRefName,headRefOid"): (0, dumped({"baseRefName": "sprint/d-5", "headRefOid": "abc1234def"})),
                 ("git", "merge-base", "--is-ancestor", "origin/sprint/d-2", "origin/sprint/d-5"): (0, "")}
        self.assertEqual(gates.evaluate(ns("qa", pr_target="sprint/d-2", checked_bead=""), qa_runner(quick)), "READY")
        self.assertEqual(gates.evaluate(ns("qa", pr_target="sprint/d-2", checked_bead=""), qa_runner(
            {**quick, ("git", "merge-base", "--is-ancestor", "origin/sprint/d-2", "origin/sprint/d-5"): (1, "")})), "PR_TARGET_MISMATCH")

    def test_qa_base_check_that_cannot_run_is_not_a_refusal(self):
        runner = qa_runner({("git", "merge-base", "--is-ancestor", "origin/target", "origin/top"): (128, ""),
                            ("gh", "pr", "view", "7", "--json", "baseRefName,headRefOid"): (0, dumped({"baseRefName": "top", "headRefOid": "abc1234def"}))})
        self.assertEqual(gates.evaluate(ns("qa"), runner), "GATE_CANNOT_RUN")


if __name__ == "__main__": unittest.main()
