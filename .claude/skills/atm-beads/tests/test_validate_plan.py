"""validate-plan's checks on a two-sprint plan (t-2 depends on t-1), and the published bead schemas."""
from __future__ import annotations

import copy
import json
import os
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


FAKE_BD = """#!/usr/bin/env python3
import json, os, sys
beads = json.load(open(os.environ["FAKE_BD_BEADS"]))
args = sys.argv[1:]
if args[:1] == ["show"]:
    ids = [a for a in args[1:] if not a.startswith("--")]
    print(json.dumps([b for b in beads if b["id"] in ids]))
elif args[:1] == ["list"]:
    print(json.dumps(beads))
elif args[:1] == ["doctor"]:
    if os.environ.get("FAKE_BD_DOCTOR_ERR"):
        sys.stderr.write(os.environ["FAKE_BD_DOCTOR_ERR"] + "\\n")
        sys.exit(1)
    print(json.dumps({"checks": [{"name": "ok", "status": "ok"}]}))
else:
    sys.exit("fake bd: unsupported " + " ".join(args))
"""


def git(cwd: Path, *args: str) -> str:
    return subprocess.run(["git", "-c", "user.email=t@t", "-c", "user.name=t", *args], cwd=cwd, check=True,
                          capture_output=True, text=True).stdout


class LivePlan(unittest.TestCase):
    """Without --index the plan comes from the root bead's integration branch, never from a fixed base."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        base = Path(self.tmp.name)
        origin, self.repo, bin_dir = base / "origin.git", base / "repo", base / "bin"
        subprocess.run(["git", "init", "-q", "--bare", "-b", "main", str(origin)], check=True)
        subprocess.run(["git", "init", "-q", "-b", "main", str(self.repo)], check=True)
        git(self.repo, "remote", "add", "origin", str(origin))
        config = self.repo / ".claude/project/atm-bd-orchestration.yaml"
        config.parent.mkdir(parents=True)
        config.write_text("plans_dir: plans\n")
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-qm", "config")
        git(self.repo, "push", "-q", "origin", "main")
        # the plan exists only on the integration branch; there is no develop branch anywhere
        git(self.repo, "checkout", "-qb", "integrate/phase-t")
        plan = self.repo / "plans/phase-t/sprints.jsonl"
        plan.parent.mkdir(parents=True)
        plan.write_text("".join(json.dumps(r) + "\n" for r in PLAN))
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-qm", "plan")
        git(self.repo, "push", "-q", "origin", "integrate/phase-t")
        git(self.repo, "checkout", "-q", "main")
        bin_dir.mkdir()
        (bin_dir / "bd").write_text(FAKE_BD)
        (bin_dir / "bd").chmod(0o755)
        self.beads = base / "beads.json"
        self.env = {**os.environ, "PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}", "FAKE_BD_BEADS": str(self.beads)}

    def tearDown(self):
        self.tmp.cleanup()

    def run_live(self, root_metadata: dict, **env: str) -> subprocess.CompletedProcess:
        beads = copy.deepcopy(VALID)
        beads[0]["metadata"] = root_metadata
        self.beads.write_text(json.dumps(beads))
        return subprocess.run([str(SCRIPT), "--root", "x-phase-t"], cwd=self.repo, env={**self.env, **env},
                              capture_output=True, text=True)

    def test_reads_the_plan_from_the_root_integration_branch(self):
        out = self.run_live({"phase": "t", "integration_branch": "integrate/phase-t"})
        self.assertEqual((out.returncode, out.stdout), (0, "plan valid: 2 sprints\n"), out.stderr)
        self.assertNotIn("develop", out.stderr)

    def test_root_without_integration_branch_cannot_run(self):
        out = self.run_live({"phase": "t"})
        self.assertEqual(out.returncode, 2)
        self.assertIn("x-phase-t has no metadata.integration_branch", out.stderr)

    def test_plan_missing_on_the_branch_is_a_problem(self):
        git(self.repo, "push", "-q", "origin", "main:refs/heads/integrate/phase-u")
        out = self.run_live({"phase": "t", "integration_branch": "integrate/phase-u"})
        self.assertEqual(out.returncode, 5)
        self.assertIn("origin/integrate/phase-u:plans/phase-t/sprints.jsonl is missing", out.stdout)

    def test_doctor_stderr_is_surfaced(self):
        why = "proxy.doctor.unsupported: doctor is not supported in proxied-server mode"
        out = self.run_live({"phase": "t", "integration_branch": "integrate/phase-t"}, FAKE_BD_DOCTOR_ERR=why)
        self.assertEqual(out.returncode, 2)
        self.assertIn("bd doctor produced no JSON", out.stderr)
        self.assertIn(why, out.stderr)


if __name__ == "__main__":
    unittest.main()
