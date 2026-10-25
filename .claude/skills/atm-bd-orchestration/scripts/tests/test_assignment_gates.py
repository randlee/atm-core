from __future__ import annotations

from contextlib import redirect_stderr, redirect_stdout
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
from types import SimpleNamespace
import unittest
from unittest.mock import patch

MODULE = Path(__file__).parents[1] / "assignment-gates.py"
FIXTURES = Path(__file__).parent / "fixtures"
SPEC = importlib.util.spec_from_file_location("assignment_gates", MODULE)
assert SPEC is not None and SPEC.loader is not None
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


STACKS = (str(gates.PRIMARY), ("gh", "api", "repos/{owner}/{repo}/stacks", "--paginate", "--jq", ".[]"))


def ns(kind, **overrides):
    values = dict(kind=kind, root="atm-phase-d", bead="bead", pr_target="target", identity="terra",
                  pr_number="7", commit="head", checked_bead="checked")
    values.update(overrides)
    return SimpleNamespace(**values)


def dumped(value): return json.dumps(value)


def dev_runner(overrides=None):
    data = {
        ("bd", "show", gates.stack_bead_id("atm-phase-d"), "--json"): (0, dumped([{"metadata": {}}])),
        (gates.VALIDATE_PLAN, "--root", "atm-phase-d"): (0, ""),
        ("bd", "ready", "-n", "0", "--json"): (0, dumped([{"id": "bead"}])),
        ("bd", "show", "bead", "--json"): (0, dumped([{"status": "open", "assignee": "", "metadata": {"difficulty": "normal", "pr_target": "target"}}])),
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
        ("bd", "ready", "-n", "0", "--json"): (0, dumped([{"id": "bead"}])),
        ("git", "rev-parse", "HEAD"): (0, "head"),
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
    def test_runner_timeouts_are_typed_and_name_command(self):
        for timed_out in ((gates.VALIDATE_PLAN, "--root", "atm-phase-d"),
                          ("bd", "ready", "-n", "0", "--json"),
                          ("git", "merge-base", "--is-ancestor", "origin/target", "HEAD")):
            with self.subTest(command=timed_out):
                delegate = dev_runner()
                seen = []

                def runner(argv, **kwargs):
                    seen.append(kwargs["timeout"])
                    if tuple(argv) == timed_out:
                        raise subprocess.TimeoutExpired(argv, kwargs["timeout"])
                    return delegate(argv, **kwargs)

                stderr, stdout = io.StringIO(), io.StringIO()
                with patch.object(gates, "COMMAND_TIMEOUT", 7), patch.object(gates, "VALIDATE_PLAN_TIMEOUT", 7), redirect_stderr(stderr), redirect_stdout(stdout):
                    exit_code = gates.main(["dev", "--root", "atm-phase-d", "--bead", "bead",
                                            "--pr-target", "target", "--identity", "terra"], runner)
                self.assertEqual((exit_code, stdout.getvalue().strip()), (2, "GATE_CANNOT_RUN"))
                self.assertIn(timed_out[0], stderr.getvalue())
                self.assertIn("7s", stderr.getvalue())
                self.assertIn("retry once, or raise ATM_BD_GATE_TIMEOUT_S", stderr.getvalue())
                self.assertTrue(seen and all(timeout == 7 for timeout in seen))

    def test_invalid_timeout_env_is_gate_cannot_run_not_a_traceback(self):
        for bad in ("abc", "0", "-3"):
            with self.subTest(bad=bad):
                runner = FakeRunner({})
                stderr, stdout = io.StringIO(), io.StringIO()
                with patch.dict(os.environ, {"ATM_BD_GATE_TIMEOUT_S": bad}), redirect_stderr(stderr), redirect_stdout(stdout):
                    exit_code = gates.main(["dev", "--root", "r", "--bead", "b", "--pr-target", "t"], runner)
                self.assertEqual((exit_code, stdout.getvalue().strip()), (2, "GATE_CANNOT_RUN"))
                self.assertIn(f"ATM_BD_GATE_TIMEOUT_S={bad!r}", stderr.getvalue())
                self.assertIn("ATM_BD_GATE_TIMEOUT_S=180", stderr.getvalue())
                self.assertNotIn("Traceback", stderr.getvalue())
                self.assertEqual(runner.calls, [])

    def test_stack_top_timeout_is_typed(self):
        def timeout(argv, **kwargs):
            raise subprocess.TimeoutExpired(argv, kwargs["timeout"])

        stderr, stdout = io.StringIO(), io.StringIO()
        with redirect_stderr(stderr), redirect_stdout(stdout):
            exit_code = gates.main(["stack-top", "--root", "phase", "--pr-target", "target"], timeout)
        self.assertEqual((exit_code, stdout.getvalue().strip()), (2, "GATE_CANNOT_RUN"))
        self.assertIn("bd", stderr.getvalue())

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

    def test_dev_rerun_after_the_callers_own_claim(self):
        unlisted, show = ("bd", "ready", "-n", "0", "--json"), ("bd", "show", "bead", "--json")
        def dep(status, kind="blocks"): return {"id": "other", "status": status, "dependency_type": kind}
        for assignee, deps, expected in (("terra", [], "READY"), ("someone-else", [], "NOT_READY"),
                                         ("terra", [dep("open")], "NOT_READY"), ("terra", [dep("closed")], "READY"),
                                         ("terra", [dep("open", "parent-child")], "READY")):
            runner = dev_runner({unlisted: (0, "[]"), show: (0, dumped([{"status": "in_progress", "assignee": assignee,
                                 "dependencies": deps, "metadata": {"difficulty": "normal", "pr_target": "target"}}]))})
            with self.subTest(assignee=assignee, deps=deps):
                self.assertEqual(gates.evaluate(ns("dev"), runner), expected)

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
            ("sanity-frozen.json", ns("sanity"), sanity_runner({("bd", "history", "bead", "--json"): (0, dumped(HISTORY_PASSED))})),
            ("sanity-ready.json", ns("sanity"), sanity_runner()),
            ("sanity-ready.json", ns("sanity", pr_number=""), sanity_runner()),
        ]
        for fixture, gate_args, runner in cases:
            with self.subTest(fixture=fixture, pr_number=gate_args.pr_number):
                self.assert_fixture(fixture, gate_args, runner)

    def test_sanity_requires_an_assigned_commit_before_reading_state(self):
        runner = sanity_runner()
        self.assertEqual(gates.evaluate(ns("sanity", commit=""), runner), "COMMIT_REQUIRED")
        self.assertEqual(runner.calls, [])

    def test_sanity_checks_only_history_and_the_assigned_worktree_commit(self):
        runner = sanity_runner({("git", "-C", "/wt", "rev-parse", "HEAD"): (0, "head")})
        self.assertEqual(gates.evaluate(ns("sanity", worktree="/wt", base="elsewhere", pr_number=""), runner), "READY")
        self.assertEqual(runner.calls, [
            (None, ("bd", "ready", "-n", "0", "--json")),
            (None, ("bd", "history", "bead", "--json")),
            (None, ("git", "-C", "/wt", "rev-parse", "HEAD")),
        ])
        moved = sanity_runner({("git", "-C", "/wt", "rev-parse", "HEAD"): (0, "moved")})
        self.assertEqual(gates.evaluate(ns("sanity", worktree="/wt"), moved), "COMMIT_MISMATCH")

    def test_sanity_commit_check_that_cannot_run_is_not_a_refusal(self):
        runner = sanity_runner({("git", "rev-parse", "HEAD"): (128, "", "not a git repository")})
        err = io.StringIO()
        with redirect_stderr(err):
            self.assertEqual(gates.evaluate(ns("sanity"), runner), "GATE_CANNOT_RUN")
        self.assertIn("not a git repository", err.getvalue())

    def test_sanity_frozen_history_wins_even_when_the_worktree_head_changed(self):
        runner = sanity_runner({
            ("bd", "history", "bead", "--json"): (0, dumped(HISTORY_PASSED)),
            ("git", "rev-parse", "HEAD"): (0, "moved"),
        })
        self.assertEqual(gates.evaluate(ns("sanity"), runner), "SANITY_FROZEN")
        self.assertEqual(runner.calls, [(None, ("bd", "ready", "-n", "0", "--json")),
                                        (None, ("bd", "history", "bead", "--json"))])

    def test_sanity_history_that_is_not_snapshots_cannot_run(self):
        for output in (dumped({"verdict": "PASS"}), dumped([{"description": "lint passes"}]), dumped([{"Issue": "closed"}]),
                       dumped([HISTORY_NEVER_PASSED[0], "PASS at abc1234"]), "not json"):
            with self.subTest(output=output):
                err = io.StringIO()
                with redirect_stderr(err):
                    code = gates.evaluate(ns("sanity"), sanity_runner({("bd", "history", "bead", "--json"): (0, output)}))
                self.assertEqual(code, "GATE_CANNOT_RUN")
                self.assertIn(output, err.getvalue())
        self.assertEqual(gates.evaluate(ns("sanity"), sanity_runner({("bd", "history", "bead", "--json"): (0, "[]")})), "READY")

    def test_sanity_not_ready_is_a_gate_result(self):
        show = ("bd", "show", "bead", "--json")
        for bead in ({"status": "open", "assignee": ""}, {"status": "in_progress", "assignee": "someone-else"},
                     {"status": "closed", "assignee": "terra"}):
            with self.subTest(bead=bead):
                runner = sanity_runner({("bd", "ready", "-n", "0", "--json"): (0, "[]"), show: (0, dumped([bead]))})
                self.assertEqual(gates.evaluate(ns("sanity"), runner), "NOT_READY")
                self.assertEqual(runner.calls, [(None, ("bd", "ready", "-n", "0", "--json")), (None, show)])

    def test_sanity_replay_of_a_bead_the_caller_already_claimed_is_ready(self):
        runner = sanity_runner({("bd", "ready", "-n", "0", "--json"): (0, "[]"),
                                ("bd", "show", "bead", "--json"): (0, dumped([{"status": "in_progress", "assignee": "terra"}]))})
        self.assertEqual(gates.evaluate(ns("sanity"), runner), "READY")

    def test_validate_plan_alone_gets_the_longer_timeout(self):
        runner = dev_runner()
        seen = {}
        original = runner.__call__
        def spy(args, **kwargs):
            seen[args[0]] = kwargs["timeout"]
            return original(args, **kwargs)
        with patch.object(gates, "COMMAND_TIMEOUT", 7), patch.object(gates, "VALIDATE_PLAN_TIMEOUT", 99):
            self.assertEqual(gates.evaluate(ns("dev"), spy), "READY")
        self.assertEqual(seen.pop(gates.VALIDATE_PLAN), 99)
        self.assertTrue(seen and set(seen.values()) == {7})
        self.assertEqual((gates.DEFAULT_COMMAND_TIMEOUT, gates.MIN_VALIDATE_PLAN_TIMEOUT), (180, 300))

    def test_command_timeout_defaults_to_180_and_env_overrides_it(self):
        self.assertEqual(gates.command_timeout({}), 180)
        self.assertEqual(gates.command_timeout({"ATM_BD_GATE_TIMEOUT_S": ""}), 180)
        self.assertEqual(gates.command_timeout({"ATM_BD_GATE_TIMEOUT_S": "45"}), 45)
        for bad in ("0", "-5", "ninety", "1.5"):
            with self.subTest(value=bad), self.assertRaises(ValueError) as caught:
                gates.command_timeout({"ATM_BD_GATE_TIMEOUT_S": bad})
            self.assertIn("ATM_BD_GATE_TIMEOUT_S=180", str(caught.exception))

    def test_module_timeouts_follow_the_env_at_import(self):
        for env, expected in (({}, (180, 300)), ({"ATM_BD_GATE_TIMEOUT_S": "45"}, (45, 300)),
                              ({"ATM_BD_GATE_TIMEOUT_S": "600"}, (600, 600))):
            with self.subTest(env=env), patch.dict(os.environ, env, clear=False):
                if not env:
                    os.environ.pop("ATM_BD_GATE_TIMEOUT_S", None)
                assert SPEC is not None and SPEC.loader is not None
                fresh = importlib.util.module_from_spec(SPEC)
                SPEC.loader.exec_module(fresh)
                self.assertEqual((fresh.COMMAND_TIMEOUT, fresh.VALIDATE_PLAN_TIMEOUT), expected)

    def test_sanity_history_command_failure_cannot_run(self):
        runner = sanity_runner({("bd", "history", "bead", "--json"): (1, "", "database unavailable")})
        err = io.StringIO()
        with redirect_stderr(err):
            self.assertEqual(gates.evaluate(ns("sanity"), runner), "GATE_CANNOT_RUN")
        self.assertIn("database unavailable", err.getvalue())

    def test_sanity_does_not_consult_linking_dirty_state_or_github(self):
        runner = sanity_runner({
            STACKS: (1, "", "GitHub unavailable"),
            ("gh", "pr", "view", "7", "--json", "baseRefName,headRefName,headRefOid"): (1, "", "unlinked PR"),
            ("git", "status", "--porcelain", "--untracked-files=no"): (0, " M tracked.py"),
            ("git", "merge-base", "--is-ancestor", "origin/target", "head"): (1, ""),
            ("git", "log", "--format=%H", "origin/target..head"): (0, ""),
        })
        self.assertEqual(gates.evaluate(ns("sanity", pr_target="unlinked-target"), runner), "READY")
        self.assertEqual([argv for _, argv in runner.calls], [
            ("bd", "ready", "-n", "0", "--json"),
            ("bd", "history", "bead", "--json"), ("git", "rev-parse", "HEAD"),
        ])

    def test_a_lower_bound_whose_branch_is_gone_holds_only_when_its_pr_merged(self):
        gone = {("git", "merge-base", "--is-ancestor", "origin/planned", "origin/target"): (128, ""),
                ("git", "rev-parse", "--verify", "--quiet", "refs/remotes/origin/planned"): (1, "")}
        merged = ("gh", "pr", "list", "--head", "planned", "--state", "merged", "--json", "mergeCommit")
        in_base = ("git", "merge-base", "--is-ancestor", "m1", "origin/target")
        listed = {**gone, merged: (0, dumped([{"mergeCommit": {"oid": "old"}}, {"mergeCommit": {"oid": "m1"}}])),
                  ("git", "merge-base", "--is-ancestor", "old", "origin/target"): (1, "")}
        self.assertTrue(gates.descends(FakeRunner({**listed, in_base: (0, "")}), "planned", "target"))
        for responses in ({**listed, in_base: (1, "")}, {**gone, merged: (0, "[]")}):
            with self.subTest(responses=responses):
                with self.assertRaises(RuntimeError):
                    gates.descends(FakeRunner(responses), "planned", "target")

    def stack_top(self, responses, target="integrate"):
        output = io.StringIO()
        with redirect_stdout(output):
            code = gates.main(["stack-top", "--root", "atm-phase-bj", "--pr-target", target, "--worktree", "/wt"], FakeRunner(responses))
        return output.getvalue().strip(), code

    def test_stack_top_reads_the_phase_ledger_and_rejects_invalid_data(self):
        bead = ("bd", "show", "atm-phase-bj.stack", "--json")
        head = ("git", "-C", "/wt", "rev-parse", "origin/integrate")
        cases = [
            ("last layer", {bead: (0, dumped([{"metadata": {"layers": [{"branch": "first", "head": "a"}, {"branch": "last", "head": "b"}]}}]))}, ("last", 0)),
            ("empty", {bead: (0, dumped([{"metadata": {"layers": []}}])), head: (0, "abc")}, ("integrate", 0)),
            ("null metadata", {bead: (0, dumped([{"metadata": None}])), head: (0, "abc")}, ("integrate", 0)),
            ("missing bead", {bead: (0, "[]")}, ("GATE_CANNOT_RUN", 2)),
            ("multiple beads", {bead: (0, "[{}, {}]")}, ("GATE_CANNOT_RUN", 2)),
            ("invalid json", {bead: (0, "not json")}, ("GATE_CANNOT_RUN", 2)),
            ("bd fails", {bead: (1, "", "database down")}, ("GATE_CANNOT_RUN", 2)),
            ("invalid metadata", {bead: (0, dumped([{"metadata": "bad"}]))}, ("GATE_CANNOT_RUN", 2)),
            ("invalid layers", {bead: (0, dumped([{"metadata": {"layers": "bad"}}]))}, ("GATE_CANNOT_RUN", 2)),
            ("missing head", {bead: (0, dumped([{"metadata": {"layers": [{"branch": "top"}]}}]))}, ("GATE_CANNOT_RUN", 2)),
            ("missing fallback", {bead: (0, "[{}]"), head: (1, "", "unknown ref")}, ("GATE_CANNOT_RUN", 2)),
        ]
        for name, responses, expected in cases:
            with self.subTest(name):
                self.assertEqual(self.stack_top(responses), expected)

    def test_stack_top_requires_root(self):
        with self.assertRaises(SystemExit):
            gates.main(["stack-top", "--pr-target", "target"], FakeRunner({}))

    def test_stack_top_reads_bead_in_primary_and_resolves_fallback_in_worktree(self):
        runner = FakeRunner({("bd", "show", "phase.stack", "--json"): (0, "[{}]"),
                             ("git", "-C", "/wt", "rev-parse", "origin/trunk"): (0, "abc")})
        with redirect_stdout(io.StringIO()), patch.object(gates, "stack_bead_id", wraps=gates.stack_bead_id) as stack_id:
            assert gates.main(["stack-top", "--root", "phase", "--pr-target", "trunk", "--worktree", "/wt"], runner) == 0
        stack_id.assert_called_once_with("phase")
        assert runner.calls == [(str(gates.PRIMARY), ("bd", "show", "phase.stack", "--json")),
                                (None, ("git", "-C", "/wt", "rev-parse", "origin/trunk"))]

    def test_gate_kinds_still_require_root_and_bead(self):
        with self.assertRaises(SystemExit):
            gates.main(["dev", "--pr-target", "target"], dev_runner())

    def test_sanity_cli_needs_only_bead_commit_and_worktree(self):
        runner = sanity_runner({("git", "-C", "/wt", "rev-parse", "HEAD"): (0, "head")})
        output = io.StringIO()
        with redirect_stdout(output):
            result = gates.main(["sanity", "--bead", "bead", "--commit", "head", "--worktree", "/wt"], runner)
        self.assertEqual(result, 0)
        self.assertEqual(output.getvalue(), "READY\n")

    def test_sanity_cli_still_requires_bead(self):
        with self.assertRaises(SystemExit):
            gates.main(["sanity", "--commit", "head"], sanity_runner())

    def test_other_cli_kinds_still_require_pr_target(self):
        for kind in ("dev", "qa", "stack-top"):
            with self.subTest(kind=kind):
                with self.assertRaises(SystemExit):
                    gates.main([kind, "--root", "root", "--bead", "bead"], FakeRunner({}))

    def test_qa_cli_still_requires_root(self):
        with self.assertRaises(SystemExit):
            gates.main(["qa", "--bead", "bead", "--pr-target", "target"], qa_runner())

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
