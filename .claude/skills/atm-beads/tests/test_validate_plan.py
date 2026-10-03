"""validate-plan's four checks on a two-sprint plan (t-2 depends on t-1), and the published bead schemas."""
from __future__ import annotations

import copy
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "validate-plan"
PLAN = [["t-1", "x-t-1-sanity", []], ["t-2", "x-t-2-sanity", ["t-1"]]]


def sprint(bid: str, blocks: list[str]) -> dict:
    return {"id": bid, "parent": "x-phase-t", "assignee": "dev", "acceptance_criteria": "- [ ] #1: done",
            "description": "Goal.\n\n## Deliverables\n1. the thing\n",
            "dependencies": [{"type": "blocks", "depends_on_id": b} for b in blocks],
            "metadata": {"requirements": ["NONE"], "adrs": ["ADR-1"], "worktree": "wt", "branch": f"sprint/{bid}",
                         "pr_target": "integrate/phase-t", "difficulty": "normal"}}


def sanity(bid: str, dev: str) -> dict:
    return {"id": bid, "parent": "x-phase-t", "assignee": "sanity", "metadata": {"dev_bead": dev},
            "dependencies": [{"dependency_type": "blocks", "id": dev}]}


ROOT_EPIC = {"id": "x-phase-t", "issue_type": "epic", "status": "open", "created_at": "2026-09-25T00:00:00Z"}
VALID = [ROOT_EPIC, sprint("x-t-1", []), sanity("x-t-1-sanity", "x-t-1"),
         sprint("x-t-2", ["x-t-1-sanity"]), sanity("x-t-2-sanity", "x-t-2")]


def run(beads: list[dict]) -> tuple[int, list[str]]:
    with tempfile.TemporaryDirectory() as d:
        index, data = Path(d) / "sprints.jsonl", Path(d) / "beads.json"
        index.write_text("".join(json.dumps(r) + "\n" for r in PLAN))
        data.write_text(json.dumps(beads))
        out = subprocess.run([str(SCRIPT), "--root", "x-phase-t", "--index", str(index), "--beads", str(data), "--no-doctor"],
                             capture_output=True, text=True)
    return out.returncode, out.stdout.splitlines()


def broken(bid: str, change) -> list[dict]:
    beads = copy.deepcopy(VALID)
    change(next(b for b in beads if b["id"] == bid))
    return beads


class ValidatePlan(unittest.TestCase):
    def test_valid(self):
        self.assertEqual(run(VALID), (0, ["plan valid: 2 sprints"]))

    def test_each_problem(self):
        cases = {
            "x-t-2: missing blocks edge to x-t-1-sanity": ("x-t-2", lambda b: b.update(dependencies=[])),
            "x-t-1: metadata.requirements: ": ("x-t-1", lambda b: b["metadata"].update(requirements=[])),
            "x-t-1: metadata.adrs: mixes NONE with ids": ("x-t-1", lambda b: b["metadata"].update(adrs=["NONE", "ADR-1"])),
            "x-t-1: metadata.worktree: ": ("x-t-1", lambda b: b["metadata"].pop("worktree")),
            "x-t-1: metadata.branch: ": ("x-t-1", lambda b: b["metadata"].update(branch="")),
            "x-t-1: description: ": ("x-t-1", lambda b: b.update(description="none")),
            "x-t-1: acceptance_criteria: ": ("x-t-1", lambda b: b.update(acceptance_criteria="")),
            "x-t-1: metadata.difficulty: ": ("x-t-1", lambda b: b["metadata"].update(difficulty="medium")),
            "x-t-1-sanity: assignee: ": ("x-t-1-sanity", lambda b: b.update(assignee="")),
            "x-t-1-sanity: metadata.dev_bead is \"x-t-2\", not x-t-1": ("x-t-1-sanity", lambda b: b.update(metadata={"dev_bead": "x-t-2"}, dependencies=[{"type": "blocks", "depends_on_id": "x-t-2"}])),
            "x-t-1-sanity: missing blocks edge to its sprint x-t-1": ("x-t-1-sanity", lambda b: b.update(dependencies=[])),
        }
        for want, (bid, change) in cases.items():
            with self.subTest(problem=want):
                rc, lines = run(broken(bid, change))
                self.assertEqual(rc, 5)
                self.assertEqual(len(lines), 1, lines)
                self.assertTrue(lines[0].startswith(want), lines)

    def test_missing_bead(self):
        rc, lines = run(VALID[:4])
        self.assertEqual((rc, lines), (5, ["x-t-2-sanity: sanity bead of x-t-2 is not in beads"]))


    def test_only_epics_at_the_top_level(self):
        stray = {"id": "x-stray", "issue_type": "task", "status": "open", "created_at": "2026-09-26T00:00:00Z"}
        rc, lines = run(VALID + [stray])
        self.assertEqual((rc, lines), (5, ["x-stray: task at the top level; only epics live at the top level, parent it under its epic"]))
        for ok in ({**stray, "status": "closed"}, {**stray, "created_at": "2026-09-24T00:00:00Z"}, {**stray, "issue_type": "epic"}):
            with self.subTest(bead=ok):
                self.assertEqual(run(VALID + [ok])[0], 0)

    def test_published_schemas_are_exported_from_the_models(self):
        with tempfile.TemporaryDirectory() as d:
            subprocess.run(["python3", str(ROOT / "scripts" / "bead_schema.py"), "export", d], check=True)
            for exported in sorted(Path(d).iterdir()):
                with self.subTest(schema=exported.name):
                    self.assertEqual((ROOT / "schemas" / exported.name).read_text(), exported.read_text(),
                                     "run: scripts/bead_schema.py export schemas")


if __name__ == "__main__":
    unittest.main()
