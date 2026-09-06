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
        text = gsv.render_table(stack, rows, problems, notes, trunk_origin=T0)
        self.assertIn("VERDICT: ✅ COHERENT", text)
        self.assertIn("note: L3 docs/top: gh stack reported no local head", text)

    def test_needs_rebase_and_base_mismatch_still_flagged(self) -> None:
        stack, prs = self.coherent_stack()
        stack["branches"][1] = layer("fix/middle", 2, head=B2, base=OLD, needs_rebase=True)
        _, problems, _ = gsv.build_rows(stack, prs, fetched=True)
        self.assertTrue(any("base ddddddddd != parent head aaaaaaaaa" in p for p in problems))
        self.assertTrue(any("gh stack reports needsRebase" in p for p in problems))


if __name__ == "__main__":
    unittest.main()
