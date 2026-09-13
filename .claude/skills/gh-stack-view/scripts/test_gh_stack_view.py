"""Regression tests for gh_stack_view.build_rows against gh-stack JSON shapes.

gh-stack v0.1.0 omits ``head``/``base`` for a layer whose branch is not present
locally (viewed from a sibling worktree before fetch).  The report must degrade
to ❓/notes for that layer, never raise.  Runs under the repo pytests lint task
(.just/run_pytests.py) and standalone with ``python3 -m unittest``.
"""
from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import unittest
from unittest import mock

SCRIPT = Path(__file__).with_name("gh_stack_view.py")
spec = importlib.util.spec_from_file_location("gh_stack_view_under_test", SCRIPT)
assert spec is not None and spec.loader is not None
gsv = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = gsv
spec.loader.exec_module(gsv)

TRUNK = "integrate/phase-ax"
T0 = "0" * 40
A1 = "a" * 40
B2 = "b" * 40
C3 = "c" * 40
OLD = "d" * 40


def layer(name: str, pr: int, *, head: str | None, base: str | None, needs_rebase: bool = False) -> dict:
    entry = {"name": name, "isCurrent": False, "isMerged": False, "isQueued": False,
             "needsRebase": needs_rebase, "pr": {"number": pr, "state": "OPEN"}}
    if head is not None:
        entry["head"] = head
    if base is not None:
        entry["base"] = base
    return entry


def pr(head: str, base: str, *, mergeable: str = "MERGEABLE", state: str = "CLEAN") -> dict:
    return {"headRefOid": head, "baseRefName": base, "mergeable": mergeable,
            "mergeStateStatus": state, "isDraft": False, "ci": "SUCCESS"}


class BuildRowsShapeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.origins = {TRUNK: T0, "fix/bottom": A1, "fix/middle": B2, "docs/top": C3}
        patches = [
            mock.patch.object(gsv, "origin_sha", side_effect=lambda ref: self.origins.get(ref)),
            mock.patch.object(gsv, "is_ancestor", return_value=False),
        ]
        for p in patches:
            p.start()
            self.addCleanup(p.stop)

    def coherent_stack(self) -> tuple[dict, dict]:
        stack = {"trunk": TRUNK, "currentBranch": "fix/bottom", "branches": [
            layer("fix/bottom", 1, head=A1, base=T0),
            layer("fix/middle", 2, head=B2, base=A1),
            layer("docs/top", 3, head=C3, base=B2),
        ]}
        prs = {1: pr(A1, TRUNK), 2: pr(B2, "fix/bottom"), 3: pr(C3, "fix/middle")}
        return stack, prs

    def test_full_shape_is_coherent(self) -> None:
        stack, prs = self.coherent_stack()
        rows, problems, notes = gsv.build_rows(stack, prs, fetched=True)
        self.assertEqual(problems, [])
        self.assertEqual(notes, [])
        self.assertEqual([gsv.sync_icon(r) for r in rows], [gsv.ICON_SYNC["ok"]] * 3)

    def test_missing_head_and_base_keys_do_not_crash(self) -> None:
        # Regression: gh stack view --json returned layers without head/base;
        # build_rows raised KeyError('head') while formatting the origin problem.
        stack, prs = self.coherent_stack()
        stack["branches"][1] = layer("fix/middle", 2, head=None, base=None)
        stack["branches"][2] = layer("docs/top", 3, head=None, base=None)
        rows, problems, notes = gsv.build_rows(stack, prs, fetched=True)
        self.assertEqual(problems, [], "remote side (origin == PR head) is coherent, so no problems")
        self.assertTrue(any("no local head" in n and "fix/middle" in n for n in notes))
        self.assertTrue(any("no base SHA" in n and "docs/top" in n for n in notes))
        self.assertIsNone(rows[1]["head"])
        self.assertIsNone(rows[1]["base"])
        self.assertIsNone(rows[1]["base_ok"])
        self.assertEqual(gsv.sync_icon(rows[1]), gsv.ICON_SYNC["unknown"])

    def test_missing_head_with_stale_pr_is_reported_not_raised(self) -> None:
        stack, prs = self.coherent_stack()
        stack["branches"][2] = layer("docs/top", 3, head=None, base=B2)
        prs[3] = pr(OLD, "fix/middle")  # PR head differs from origin -> stale push
        rows, problems, _ = gsv.build_rows(stack, prs, fetched=True)
        self.assertEqual(len(problems), 1)
        self.assertIn("docs/top", problems[0])
        self.assertIn("local - / origin ccccccccc / PR ddddddddd differ", problems[0])
        self.assertEqual(gsv.sync_icon(rows[2]), gsv.ICON_SYNC["stale"])

    def test_missing_head_falls_back_to_pr_head_for_next_layer_base(self) -> None:
        stack, prs = self.coherent_stack()
        stack["branches"][0] = layer("fix/bottom", 1, head=None, base=T0)
        self.origins["fix/bottom"] = None  # not fetched either
        rows, problems, _ = gsv.build_rows(stack, prs, fetched=True)
        self.assertEqual(problems, [])
        self.assertEqual(rows[1]["expected_base"], A1)

    def test_no_fetch_marks_unknown_without_raising(self) -> None:
        stack, prs = self.coherent_stack()
        stack["branches"][2] = layer("docs/top", 3, head=None, base=None)
        rows, problems, _ = gsv.build_rows(stack, prs, fetched=False)
        self.assertEqual(problems, [])
        self.assertEqual(gsv.sync_icon(rows[2]), gsv.ICON_SYNC["unknown"])

    def test_render_table_handles_missing_head(self) -> None:
        stack, prs = self.coherent_stack()
        stack["branches"][2] = layer("docs/top", 3, head=None, base=None)
        rows, problems, notes = gsv.build_rows(stack, prs, fetched=True)
        landing = {"landable": None, "reason": "no open layer or trunk not fetched"}
        text = gsv.render_table(stack, rows, problems, notes, landing, trunk_origin=T0)
        self.assertIn("VERDICT: ✅ COHERENT", text)
        self.assertIn("note: L3 docs/top: gh stack reported no local head", text)

    def test_needs_rebase_and_base_mismatch_still_flagged(self) -> None:
        stack, prs = self.coherent_stack()
        stack["branches"][1] = layer("fix/middle", 2, head=B2, base=OLD, needs_rebase=True)
        _, problems, _ = gsv.build_rows(stack, prs, fetched=True)
        self.assertTrue(any("base ddddddddd != parent head aaaaaaaaa" in p for p in problems))
        self.assertTrue(any("gh stack reports needsRebase" in p for p in problems))



T1 = "1" * 40  # trunk head after the bottom layer merged


class MergedBottomLayerTests(unittest.TestCase):
    """Regression: after ``gh stack merge``/merge-async lands the bottom layer,
    GitHub retargets the next layer onto the trunk.  The report used to demand
    that layer be based on the MERGED branch (PR base and base SHA), producing
    two false problems on a stack gh-stack itself reported as needsRebase=false.
    Seen twice on the Phase AX evidence stack (#1266 merged under #1262)."""

    def setUp(self) -> None:
        # Trunk moved from T0 to T1 (the merge commit of the bottom layer).
        self.origins = {TRUNK: T1, "fix/bottom": A1, "fix/middle": B2, "docs/top": C3}
        self.ancestors = {(T0, T1), (A1, T1)}
        patches = [
            mock.patch.object(gsv, "origin_sha", side_effect=lambda ref: self.origins.get(ref)),
            mock.patch.object(gsv, "is_ancestor", side_effect=lambda a, b: (a, b) in self.ancestors),
        ]
        for p in patches:
            p.start()
            self.addCleanup(p.stop)

    def merged_bottom_stack(self, *, middle_base: str, middle_pr_base: str = TRUNK) -> tuple[dict, dict]:
        bottom = layer("fix/bottom", 1, head=A1, base=T0)
        bottom["isMerged"] = True
        bottom["pr"]["state"] = "MERGED"
        stack = {"trunk": TRUNK, "currentBranch": "docs/top", "branches": [
            bottom,
            layer("fix/middle", 2, head=B2, base=middle_base),
            layer("docs/top", 3, head=C3, base=B2),
        ]}
        prs = {1: pr(A1, TRUNK, state="MERGED"), 2: pr(B2, middle_pr_base), 3: pr(C3, "fix/middle")}
        return stack, prs

    def test_layer_above_merged_bottom_is_judged_against_trunk(self) -> None:
        # gh stack reports the retargeted layer's base as the pre-merge trunk
        # commit (T0), an ancestor of the new trunk head: behind trunk, not a rebase.
        stack, prs = self.merged_bottom_stack(middle_base=T0)
        rows, problems, notes = gsv.build_rows(stack, prs, fetched=True)
        self.assertEqual(problems, [], problems)
        self.assertEqual(len(notes), 1)
        self.assertIn("L2 fix/middle: behind trunk", notes[0])
        self.assertEqual(gsv.sync_icon(rows[0]), gsv.ICON_MERGE["MERGED"])
        self.assertEqual(gsv.sync_icon(rows[1]), gsv.ICON_SYNC["rebase"], "behind trunk still shows the rebase icon")
        self.assertEqual(gsv.sync_icon(rows[2]), gsv.ICON_SYNC["ok"])
        self.assertEqual(rows[1]["expected_base"], T1, "expected base is the trunk head, not the merged layer's head")

    def test_layer_above_merged_bottom_on_trunk_head_is_clean(self) -> None:
        stack, prs = self.merged_bottom_stack(middle_base=T1)
        rows, problems, notes = gsv.build_rows(stack, prs, fetched=True)
        self.assertEqual(problems, [])
        self.assertEqual(notes, [])
        self.assertEqual([gsv.sync_icon(r) for r in rows[1:]], [gsv.ICON_SYNC["ok"]] * 2)

    def test_layer_above_merged_bottom_still_targeting_merged_branch_is_flagged(self) -> None:
        # GitHub has not retargeted the PR yet: that IS a problem the owner must fix.
        stack, prs = self.merged_bottom_stack(middle_base=T1, middle_pr_base="fix/bottom")
        _rows, problems, _notes = gsv.build_rows(stack, prs, fetched=True)
        self.assertEqual(len(problems), 1)
        self.assertIn("PR #2 base is fix/bottom, expected integrate/phase-ax", problems[0])

    def test_open_parent_mismatch_is_still_a_problem(self) -> None:
        # Above an OPEN layer the ancestor leniency must not apply.
        stack, prs = self.merged_bottom_stack(middle_base=T1)
        stack["branches"][2]["base"] = OLD
        self.ancestors.add((OLD, B2))
        _rows, problems, _notes = gsv.build_rows(stack, prs, fetched=True)
        self.assertEqual(len(problems), 1)
        self.assertIn("L3 docs/top: base ddddddddd != parent head bbbbbbbbb -> needs rebase", problems[0])

if __name__ == "__main__":
    unittest.main()
