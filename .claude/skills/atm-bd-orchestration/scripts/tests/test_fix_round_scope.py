from __future__ import annotations

import importlib.machinery
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).parents[1] / "fix-round-scope"
LOADER = importlib.machinery.SourceFileLoader("fix_round_scope", str(Path(__file__).parents[1] / "fix-round-scope"))
scope = importlib.util.module_from_spec(importlib.util.spec_from_loader("fix_round_scope", LOADER))
LOADER.exec_module(scope)

CARRIED = [
    {"id": "x-d-4-qa1-f1", "metadata": {"reviewer": "ruthless-boundary-qa", "finding_ref": "RBQA-F001"}},
    {"id": "x-d-4-qa1-f2", "metadata": {"reviewer": "arch-qa", "finding_ref": "ARCH-F002"}},
]


class FixVerificationScopeTests(unittest.TestCase):
    """A fix is verified only by the agent that filed it, locked to that finding's own id."""

    def test_dispatch_set_is_the_filing_reviewers(self):
        self.assertEqual(scope.owned(CARRIED), {"ruthless-boundary-qa": ["RBQA-F001"], "arch-qa": ["ARCH-F002"]})
        self.assertEqual(scope.owned([CARRIED[1]]), {"arch-qa": ["ARCH-F002"]})  # no automatic panel

    def test_carried_finding_without_reviewer_or_ref_cannot_be_locked(self):
        for meta in ({"finding_ref": "R-1"}, {"reviewer": "req-qa"}, {"reviewer": "", "finding_ref": "R-1"}):
            with self.subTest(meta=meta), self.assertRaises(scope.ScopeError):
                scope.owned([{"id": "x-f9", "metadata": meta}])

    def test_check_rejects_a_panel_and_a_missing_verifier(self):
        self.assertEqual(scope.check(CARRIED, ["ruthless-boundary-qa", "arch-qa"]), ([], []))
        self.assertEqual(scope.check([CARRIED[1]], ["arch-qa", "req-qa", "rust-qa-agent"]), (["req-qa", "rust-qa-agent"], []))
        self.assertEqual(scope.check(CARRIED, ["arch-qa"]), ([], ["ruthless-boundary-qa"]))

    def test_out_of_scope_findings_never_survive(self):
        result = {"data": {"findings": [
            {"id": "RBQA-F001", "evidence": "still leaks"},
            {"id": "RBQA-F002", "evidence": "brand new"},
            {"id": "RBQA-F001", "evidence": "id collision, second copy"}]}}
        out = scope.filter_result("ruthless-boundary-qa", CARRIED, result)
        self.assertEqual(out["new_findings"], [])
        self.assertEqual([d["bead"] for d in out["dispositions"]], ["x-d-4-qa1-f1"])
        self.assertEqual(out["dispositions"][0]["disposition"], "open")
        self.assertEqual(len(out["dropped_out_of_scope"]), 2)

    def test_unreported_carried_finding_stays_open(self):
        for result in ({"data": {"findings": []}}, {}, {"data": {"findings": [{"id": "ARCH-F099", "disposition": "fixed"}]}}):
            with self.subTest(result=result):  # empty, error-shaped, renumbered
                out = scope.filter_result("arch-qa", CARRIED, result)
                self.assertEqual(out["dispositions"], [{"bead": "x-d-4-qa1-f2", "finding_ref": "ARCH-F002",
                                                        "disposition": "open", "evidence": ""}])

    def test_only_an_explicit_fixed_disposition_passes(self):
        out = scope.filter_result("arch-qa", CARRIED, {"data": {"findings": [
            {"id": "ARCH-F002", "disposition": "fixed", "evidence": "a.rs:3 now bounded"}]}})
        self.assertEqual(out["dispositions"], [{"bead": "x-d-4-qa1-f2", "finding_ref": "ARCH-F002",
                                                "disposition": "fixed", "evidence": "a.rs:3 now bounded"}])

    def test_another_reviewers_ids_are_not_in_scope(self):
        out = scope.filter_result("arch-qa", CARRIED, {"data": {"findings": [{"id": "RBQA-F001"}]}})
        self.assertEqual([d["finding_ref"] for d in out["dispositions"]], ["ARCH-F002"])
        self.assertEqual(len(out["dropped_out_of_scope"]), 1)


PLAN = """- x-d-4 blocking validate-plan requirements: empty; list the governing REQ ids or ["NONE"]
- x-d-5 blocking arch-qa adrs: ADR-020 is not in docs/architecture.md
- x-d-6 important ruthless-boundary-qa design: the exporter reaches into the config crate
"""


class PlanFixRoundTests(unittest.TestCase):
    """Plan findings are report lines; the reviewer named on each line is its filing reviewer."""

    def test_lines_name_their_filing_reviewer(self):
        carried = scope.plan_carried(PLAN)
        self.assertEqual(scope.owned(carried), {"validate-plan": ["x-d-4 requirements"], "arch-qa": ["x-d-5 adrs"],
                                                "ruthless-boundary-qa": ["x-d-6 design"]})

    def test_validate_plan_is_never_dispatched(self):
        carried = scope.plan_carried(PLAN)
        self.assertEqual(scope.check(carried, ["arch-qa", "ruthless-boundary-qa"]), ([], []))
        self.assertEqual(scope.check(carried, ["arch-qa", "ruthless-boundary-qa", "req-qa", "plan-scope-reviewer"]),
                         (["plan-scope-reviewer", "req-qa"], []))
        self.assertEqual(scope.check(carried, ["validate-plan", "arch-qa", "ruthless-boundary-qa"]), (["validate-plan"], []))

    def test_a_line_without_a_reviewer_is_an_error(self):
        with self.assertRaises(scope.ScopeError):
            scope.plan_carried("- x-d-4 blocking requirements: empty\n")


class CliTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        (self.dir / "carried.json").write_text(json.dumps(CARRIED))

    def tearDown(self):
        self.tmp.cleanup()

    def run_cli(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run([sys.executable, str(SCRIPT), *args], cwd=self.dir, capture_output=True, text=True)

    def test_owned_needs_no_repository_configuration(self):
        out = self.run_cli("owned", "--carried", "carried.json")
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertEqual(json.loads(out.stdout), {"ruthless-boundary-qa": ["RBQA-F001"], "arch-qa": ["ARCH-F002"]})

    def test_check_exit_codes(self):
        ok = self.run_cli("check", "--carried", "carried.json", "--dispatch", "arch-qa,ruthless-boundary-qa")
        self.assertEqual(ok.returncode, 0, ok.stderr)
        bad = self.run_cli("check", "--carried", "carried.json", "--dispatch", "arch-qa,req-qa")
        self.assertEqual(bad.returncode, 5)
        self.assertIn("FIX_ROUND_DISPATCH_MISMATCH", bad.stderr)
        self.assertIn("req-qa", bad.stderr)
        self.assertIn("ruthless-boundary-qa", bad.stderr)

    def test_unlockable_carried_finding_is_a_named_error(self):
        (self.dir / "carried.json").write_text(json.dumps([{"id": "x-f9", "metadata": {}}]))
        out = self.run_cli("owned", "--carried", "carried.json")
        self.assertEqual(out.returncode, 2)
        self.assertIn("x-f9", out.stderr)

    def test_plan_mode(self):
        (self.dir / "plan.txt").write_text(PLAN)
        out = self.run_cli("check", "--plan", "--carried", "plan.txt", "--dispatch", "arch-qa,req-qa")
        self.assertEqual(out.returncode, 5)
        self.assertIn("req-qa", out.stderr)
        self.assertIn("ruthless-boundary-qa", out.stderr)
        (self.dir / "bad.txt").write_text("x-d-4 blocking requirements: empty\n")
        self.assertEqual(self.run_cli("owned", "--plan", "--carried", "bad.txt").returncode, 2)

    def test_filter_refuses_a_reviewer_that_filed_nothing_carried(self):
        (self.dir / "r.json").write_text(json.dumps({"data": {"findings": []}}))
        out = self.run_cli("filter", "--carried", "carried.json", "--reviewer", "req-qa", "--result", "r.json")
        self.assertEqual(out.returncode, 2)


if __name__ == "__main__":
    unittest.main()
