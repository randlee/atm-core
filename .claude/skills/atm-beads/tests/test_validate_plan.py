"""validate-plan on a two-sprint plan (t-2 depends on t-1), the phase file, and the published bead schemas."""
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
PLAN = [{"sprint": "t-1"}, {"sprint": "t-2", "depends_on": ["t-1"]}]
TOML = 'plan = "plans/phase-t.jsonl"\nroot = "x-phase-t"\nintegration_branch = "integrate/phase-t"\n'


def container(bid: str) -> dict:
    return {"id": bid, "parent": "x-phase-t", "issue_type": "task", "status": "open", "labels": ["phase-t", "stage:sprint"],
            "acceptance_criteria": "- [ ] #1: done", "description": "Goal.\n\n## Deliverables\n1. the thing\n",
            "created_at": "2026-09-26T00:00:00Z",
            "metadata": {"requirements": ["NONE"], "adrs": ["ADR-1"], "worktree": "wt", "branch": f"sprint/{bid}",
                         "pr_target": "integrate/phase-t", "difficulty": "normal"}}


def poured(container_id: str, step: str, blocks: list[str]) -> dict:
    bead = {"id": f"{container_id}.group-{step}", "issue_type": "task", "status": "open", "labels": ["phase-t", f"stage:{step}"],
            "created_at": "2026-09-26T00:00:00Z",
            "dependencies": [{"dependency_type": "parent-child", "id": container_id}] + [{"dependency_type": "blocks", "id": b} for b in blocks],
            "metadata": {"difficulty": "normal"}}
    if step == "sanity":
        bead["metadata"]["dev_bead"] = f"{container_id}.group-dev"
    return bead


ROOT_EPIC = {"id": "x-phase-t", "issue_type": "epic", "status": "open", "created_at": "2026-09-25T00:00:00Z",
             "metadata": {"phase": "t", "integration_branch": "integrate/phase-t"}}
VALID = [ROOT_EPIC,
         container("x-t-1"), poured("x-t-1", "dev", []), poured("x-t-1", "sanity", ["x-t-1.group-dev"]),
         container("x-t-2"), poured("x-t-2", "dev", ["x-t-1.group-sanity"]), poured("x-t-2", "sanity", ["x-t-2.group-dev"])]


def git(cwd: Path, *args: str) -> str:
    return subprocess.run(["git", "-c", "user.email=t@t", "-c", "user.name=t", *args], cwd=cwd, check=True,
                          capture_output=True, text=True).stdout


def lines(plan: list) -> str:
    return "".join((r if isinstance(r, str) else json.dumps(r)) + "\n" for r in plan)


class Repo(unittest.TestCase):
    """A repository holding the phase file; the plan file is passed with --index unless a test pushes it."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.base = Path(self.tmp.name)
        self.repo = self.base / "repo"
        subprocess.run(["git", "init", "-q", "-b", "main", str(self.repo)], check=True)
        config = self.repo / ".claude/project/atm-bd-orchestration.yaml"
        config.parent.mkdir(parents=True)
        config.write_text("plans_dir: plans\nbead_prefix: x\n")
        (self.repo / ".atm-bd").mkdir()
        (self.repo / ".atm-bd/phase-t.toml").write_text(TOML)

    def tearDown(self):
        self.tmp.cleanup()

    def run_vp(self, *args: str, env: dict | None = None) -> subprocess.CompletedProcess:
        return subprocess.run([str(SCRIPT), *args], cwd=self.repo, capture_output=True, text=True, env=env)

    def run_beads(self, beads: list[dict], plan: list = PLAN, *extra: str) -> subprocess.CompletedProcess:
        (self.base / "plan.jsonl").write_text(lines(plan))
        (self.base / "beads.json").write_text(json.dumps(beads))
        return self.run_vp("--root", "x-phase-t", "--index", str(self.base / "plan.jsonl"),
                           "--beads", str(self.base / "beads.json"), "--no-doctor", *extra)


def broken(bid: str, change) -> list[dict]:
    beads = copy.deepcopy(VALID)
    change(next(b for b in beads if b["id"] == bid))
    return beads


class ValidatePlan(Repo):
    def test_valid(self):
        out = self.run_beads(VALID)
        self.assertEqual((out.returncode, out.stdout), (0, "plan valid: 2 sprints\n"), out.stderr)
        self.assertEqual(out.stderr, "")
        out = self.run_beads(VALID, PLAN, "--phase", "t")
        self.assertEqual(out.returncode, 0, out.stderr)

    def test_each_problem(self):
        cases = {
            "x-t-2.group-dev: missing blocks edge to x-t-1.group-sanity or x-t-1": (VALID, PLAN, "x-t-2.group-dev", lambda b: b.update(dependencies=[])),
            "x-t-1: metadata.requirements: ": (VALID, PLAN, "x-t-1", lambda b: b["metadata"].update(requirements=[])),
            "x-t-1: metadata.adrs: mixes NONE with ids": (VALID, PLAN, "x-t-1", lambda b: b["metadata"].update(adrs=["NONE", "ADR-1"])),
            "x-t-1: metadata.worktree: ": (VALID, PLAN, "x-t-1", lambda b: b["metadata"].pop("worktree")),
            "x-t-1: description: ": (VALID, PLAN, "x-t-1", lambda b: b.update(description="none")),
            "x-t-1: metadata.difficulty: ": (VALID, PLAN, "x-t-1", lambda b: b["metadata"].update(difficulty="medium")),
            "x-t-1.group-sanity: missing blocks edge to its sprint x-t-1.group-dev": (VALID, PLAN, "x-t-1.group-sanity", lambda b: b.update(dependencies=[])),
            "x-phase-t: metadata.integration_branch is \"integrate/other\"": (VALID, PLAN, "x-phase-t", lambda b: b["metadata"].update(integration_branch="integrate/other")),
        }
        for want, (beads, plan, bid, change) in cases.items():
            with self.subTest(problem=want):
                out = self.run_beads(broken(bid, change) if bid else beads, plan)
                got = out.stdout.splitlines()
                self.assertEqual(out.returncode, 5, out.stderr)
                self.assertEqual(len(got), 1, got)
                self.assertTrue(got[0].startswith(want), got)

    def test_plan_file_schema(self):
        cases = {
            ":1: each line is": ['["t-1", "x-t-1-sanity", []]', {"sprint": "t-2"}],
            ":2: each line is": [{"sprint": "t-1"}, {"sprint": "t-2", "after": ["t-1"]}],
            "t-2 depends_on unknown sprint(s): t-9": [{"sprint": "t-1"}, {"sprint": "t-2", "depends_on": ["t-9"]}],
            ":2: sprint t-1 is listed twice": [{"sprint": "t-1"}, {"sprint": "t-1"}, {"sprint": "t-2", "depends_on": ["t-1"]}],
        }
        for want, plan in cases.items():
            with self.subTest(problem=want):
                out = self.run_beads(VALID, plan)
                self.assertEqual(out.returncode, 5, out.stderr)
                self.assertTrue(any(want in line for line in out.stdout.splitlines()), out.stdout)

    def test_missing_and_extra_sprint_beads(self):
        out = self.run_beads([b for b in VALID if not b["id"].startswith("x-t-2")])
        self.assertEqual((out.returncode, out.stdout), (5, "x-t-2: sprint t-2 is in the plan file but has no sprint bead\n"))
        out = self.run_beads(VALID + [container("x-t-3")])
        self.assertEqual((out.returncode, out.stdout), (5, "x-t-3: stage:sprint bead of phase-t that the plan file does not list\n"))

    def test_an_edge_to_the_predecessor_sprint_bead_also_satisfies_depends_on(self):
        beads = broken("x-t-2.group-dev", lambda b: b.update(dependencies=[{"dependency_type": "blocks", "id": "x-t-1"}]))
        self.assertEqual(self.run_beads(beads).returncode, 0)

    def test_everything_else_warns(self):
        stray = {"id": "x-stray", "issue_type": "task", "status": "open", "created_at": "2026-09-26T00:00:00Z"}
        out = self.run_beads(VALID + [stray])
        self.assertEqual((out.returncode, out.stdout), (0, "plan valid: 2 sprints\n"))
        self.assertIn("warning: x-stray: task at the top level", out.stderr)
        beads = broken("x-t-1.group-sanity", lambda b: b.update(metadata={"dev_bead": "x-t-2.group-dev"},
                                                                dependencies=[{"dependency_type": "blocks", "id": "x-t-2.group-dev"}]))
        out = self.run_beads(beads)
        self.assertEqual(out.returncode, 0)
        self.assertIn("warning: x-t-1.group-sanity: metadata.dev_bead is not x-t-1.group-dev", out.stderr)

    def test_cannot_run(self):
        out = self.run_beads(VALID, PLAN)
        self.assertEqual(out.returncode, 0)
        (self.base / "beads.json").write_text(json.dumps(VALID))
        out = self.run_vp("--root", "y-phase-t", "--index", str(self.base / "plan.jsonl"), "--beads", str(self.base / "beads.json"), "--no-doctor")
        self.assertEqual(out.returncode, 2)
        self.assertIn("phase-t.toml names x-phase-t", out.stderr)
        (self.repo / ".atm-bd/phase-t.toml").unlink()
        out = self.run_beads(VALID)
        self.assertEqual(out.returncode, 2)
        self.assertIn(".atm-bd/phase-t.toml does not exist", out.stderr)

    def test_published_schemas_are_exported_from_the_models(self):
        with tempfile.TemporaryDirectory() as d:
            subprocess.run(["python3", str(ROOT / "scripts" / "bead_schema.py"), "export", d], check=True)
            for exported in sorted(Path(d).iterdir()):
                with self.subTest(schema=exported.name):
                    self.assertEqual((ROOT / "schemas" / exported.name).read_text(), exported.read_text(),
                                     "run: scripts/bead_schema.py export schemas")


class CiCheck(Repo):
    """--ci: every tracked .atm-bd/phase-*.toml loads and its plan file parses; bd is never run."""

    def setUp(self):
        super().setUp()
        bin_dir = self.base / "bin"
        bin_dir.mkdir()
        self.bd_ran = self.base / "bd-ran"
        (bin_dir / "bd").write_text(f"#!/bin/sh\ntouch {self.bd_ran}\nexit 1\n")
        (bin_dir / "bd").chmod(0o755)
        # CI runners have no pydantic: make it unimportable so --ci must not need it.
        blocked = self.base / "no-pydantic/pydantic"
        blocked.mkdir(parents=True)
        (blocked / "__init__.py").write_text("raise ImportError('pydantic is not installed')\n")
        self.env = {**os.environ, "PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}",
                    "PYTHONPATH": str(blocked.parent)}
        (self.repo / "plans").mkdir()
        (self.repo / "plans/phase-t.jsonl").write_text(lines(PLAN))

    def ci(self, *tracked: str) -> subprocess.CompletedProcess:
        if tracked:
            git(self.repo, "add", "--", *tracked)
        out = self.run_vp("--ci", env=self.env)
        self.assertFalse(self.bd_ran.exists(), "--ci ran bd")
        return out

    def test_valid_and_untracked_files_are_ignored(self):
        (self.repo / ".atm-bd/phase-u.toml").write_text("not toml")
        out = self.ci(".atm-bd/phase-t.toml", "plans/phase-t.jsonl")
        self.assertEqual((out.returncode, out.stdout), (0, "plan valid: 1 tracked phase file(s)\n"), out.stderr)

    def test_no_tracked_phase_file_is_a_note(self):
        out = self.ci()
        self.assertEqual((out.returncode, out.stdout), (0, "plan valid: no tracked .atm-bd/phase-*.toml to check\n"), out.stderr)

    def test_each_problem_names_its_file(self):
        cases = {
            "malformed toml": ("plan = \n", None, ".atm-bd/phase-t.toml: "),
            "missing key": (TOML.replace('integration_branch = "integrate/phase-t"\n', ""), None,
                            ".atm-bd/phase-t.toml: "),
            "bad plan row": (TOML, [{"sprint": "t-1"}, {"sprint": "t-2", "after": ["t-1"]}], "plans/phase-t.jsonl:2: each line is"),
            "missing plan file": (TOML.replace("phase-t.jsonl", "phase-gone.jsonl"), None, ".atm-bd/phase-t.toml: "),
        }
        for name, (toml, plan, want) in cases.items():
            with self.subTest(problem=name):
                (self.repo / ".atm-bd/phase-t.toml").write_text(toml)
                (self.repo / "plans/phase-t.jsonl").write_text(lines(plan or PLAN))
                out = self.ci(".atm-bd/phase-t.toml", "plans/phase-t.jsonl")
                self.assertEqual(out.returncode, 5, out.stdout + out.stderr)
                got = out.stdout.splitlines()
                self.assertEqual(len(got), 1, got)
                self.assertTrue(got[0].startswith(want), got)

    def test_every_tracked_phase_file_is_checked(self):
        (self.repo / ".atm-bd/phase-u.toml").write_text("not toml")
        (self.repo / "plans/phase-t.jsonl").write_text(lines([{"sprint": "t-1", "depends_on": ["t-9"]}]))
        out = self.ci(".atm-bd/phase-t.toml", ".atm-bd/phase-u.toml", "plans/phase-t.jsonl")
        self.assertEqual(out.returncode, 5, out.stderr)
        got = out.stdout.splitlines()
        self.assertEqual(len(got), 2, got)
        self.assertIn("plans/phase-t.jsonl: t-1 depends_on unknown sprint(s): t-9", got[0])
        self.assertTrue(got[1].startswith(".atm-bd/phase-u.toml: "), got)

    def test_ci_takes_no_other_option(self):
        out = self.run_vp("--ci", "--phase", "t", env=self.env)
        self.assertEqual(out.returncode, 2)
        self.assertFalse(self.bd_ran.exists())


FAKE_BD = """#!/usr/bin/env python3
import json, os, sys
beads = json.load(open(os.environ["FAKE_BD_BEADS"]))
args = sys.argv[1:]
if args[:1] == ["show"]:
    ids = [a for a in args[1:] if not a.startswith("--")]
    found = [b for b in beads if b["id"] in ids]
    if not found:
        sys.exit("Error: no issue found matching " + " ".join(ids))
    print(json.dumps(found))
elif args[:1] == ["list"]:
    labels = [args[i + 1] for i, a in enumerate(args) if a == "-l"]
    print(json.dumps([b for b in beads if set(labels) <= set(b.get("labels") or [])]))
elif args[:1] == ["doctor"]:
    if os.environ.get("FAKE_BD_DOCTOR_ERR"):
        sys.stderr.write(os.environ["FAKE_BD_DOCTOR_ERR"] + "\\n")
        sys.exit(1)
    print(json.dumps({"checks": [{"name": "dolt", "status": os.environ.get("FAKE_BD_DOCTOR_STATUS", "ok"), "message": "m"}]}))
else:
    sys.exit("fake bd: unsupported " + " ".join(args))
"""


class LivePlan(Repo):
    """Without --index the plan file comes from the phase file's integration branch, never from a fixed base."""

    def setUp(self):
        super().setUp()
        origin, bin_dir = self.base / "origin.git", self.base / "bin"
        subprocess.run(["git", "init", "-q", "--bare", "-b", "main", str(origin)], check=True)
        git(self.repo, "remote", "add", "origin", str(origin))
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-qm", "config")
        git(self.repo, "push", "-q", "origin", "main")
        git(self.repo, "checkout", "-qb", "integrate/phase-t")
        plan = self.repo / "plans/phase-t.jsonl"
        plan.parent.mkdir(parents=True)
        plan.write_text(lines(PLAN))
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-qm", "plan")
        git(self.repo, "push", "-q", "origin", "integrate/phase-t")
        git(self.repo, "checkout", "-q", "main")
        bin_dir.mkdir()
        (bin_dir / "bd").write_text(FAKE_BD)
        (bin_dir / "bd").chmod(0o755)
        self.beads = self.base / "live.json"
        self.env = {**os.environ, "PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}", "FAKE_BD_BEADS": str(self.beads)}

    def live(self, beads: list[dict] = VALID, *args: str, **env: str) -> subprocess.CompletedProcess:
        self.beads.write_text(json.dumps(beads))
        return self.run_vp(*(args or ("--phase", "t")), env={**self.env, **env})

    def test_reads_the_plan_from_the_integration_branch(self):
        out = self.live()
        self.assertEqual((out.returncode, out.stdout), (0, "plan valid: 2 sprints\n"), out.stderr)
        self.assertEqual(self.live(VALID, "--root", "x-phase-t").returncode, 0)

    def test_plan_missing_on_the_branch_cannot_run(self):
        (self.repo / ".atm-bd/phase-t.toml").write_text(TOML.replace("integrate/phase-t", "integrate/phase-u"))
        git(self.repo, "push", "-q", "origin", "main:refs/heads/integrate/phase-u")
        out = self.live()
        self.assertEqual(out.returncode, 2)
        self.assertIn("origin/integrate/phase-u:plans/phase-t.jsonl is missing", out.stderr)

    def test_doctor_problems_are_warnings(self):
        why = "proxy.doctor.unsupported: doctor is not supported in proxied-server mode"
        out = self.live(VALID, FAKE_BD_DOCTOR_ERR=why)
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertIn("bd doctor produced no JSON", out.stderr)
        self.assertIn(why, out.stderr)
        out = self.live(VALID, FAKE_BD_DOCTOR_STATUS="error")
        self.assertEqual(out.returncode, 0)
        self.assertIn("warning: bd doctor: dolt: m", out.stderr)

    def test_a_missing_live_edge_is_a_problem(self):
        beads = broken("x-t-2.group-dev", lambda b: b.update(dependencies=[]))
        out = self.live(beads)
        self.assertEqual(out.returncode, 5)
        self.assertEqual(out.stdout.splitlines(), ["x-t-2.group-dev: missing blocks edge to x-t-1.group-sanity or x-t-1 (plan file: t-2 depends on t-1)"])


class FilePlan(LivePlan):
    """--file before import: the containers are rendered, their groups not yet poured."""

    def run_file(self, plan_beads: list[dict], live: list[dict]) -> subprocess.CompletedProcess:
        self.beads.write_text(json.dumps(live))
        (self.base / "beads.jsonl").write_text("".join(json.dumps(b) + "\n" for b in plan_beads))
        return self.run_vp("--phase", "t", "--file", str(self.base / "beads.jsonl"), "--no-doctor", env=self.env)

    def test_a_new_plan_passes_before_its_groups_are_poured(self):
        out = self.run_file([ROOT_EPIC, container("x-t-1"), container("x-t-2")], [])
        self.assertEqual((out.returncode, out.stdout), (0, "plan valid: 2 sprints\n"), out.stderr)
        self.assertIn("warning: x-t-2: not poured yet", out.stderr)

    def test_a_plan_bead_with_an_assignee_or_no_parent_is_a_warning(self):
        orphan = {**{k: v for k, v in container("x-t-2").items() if k != "parent"}, "assignee": "my-dev"}
        out = self.run_file([ROOT_EPIC, container("x-t-1"), orphan], [])
        self.assertEqual(out.returncode, 0, out.stdout)
        self.assertIn("warning: x-t-2: assignee set at plan time", out.stderr)
        self.assertIn("warning: x-t-2: task at the top level", out.stderr)

    def test_an_import_into_a_running_phase_reads_the_rest_live(self):
        out = self.run_file([container("x-t-2")], [b for b in VALID if not b["id"].startswith("x-t-2")])
        self.assertEqual(out.returncode, 0, out.stdout + out.stderr)


if __name__ == "__main__":
    unittest.main()
