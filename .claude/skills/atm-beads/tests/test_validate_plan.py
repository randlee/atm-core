"""validate-plan on a two-sprint plan (t-2 depends on t-1), the phase file, and the published bead schemas."""
from __future__ import annotations

import copy
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

from phase_fixtures import phase_data, phase_toml, run_subprocess, write_phase

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "validate-plan"
PLAN = [{"sprint": "t-1"}, {"sprint": "t-2", "depends_on": ["t-1"]}]
TOML = phase_toml(phase_data())


def container(bid: str) -> dict:
    return {"id": bid, "parent": "x-phase-t", "issue_type": "task", "status": "open", "labels": ["phase-t", "stage:sprint"],
            "acceptance_criteria": "- [ ] #1: done", "description": "Goal.\n\n## Deliverables\n1. the thing\n",
            "created_at": "2026-09-26T00:00:00Z",
            "metadata": {"requirements": ["NONE"], "adrs": ["ADR-1"], "worktree": "wt", "branch": f"sprint/{bid}",
                         "pr_target": "integrate/phase-t", "difficulty": "normal"}}


def close_fields(container_id: str, step: str) -> dict:
    """The close fields a pour stamps on each step (close_fields.POURED, less branch/worktree/pr_target)."""
    group = f"{container_id}.group"
    fields = {"role": {"sanity": "sanity"}.get(step, step), "phase": "t", "sprint": container_id[2:],
              "sprint_bead": container_id, "stack": "phase-t", "stack_bead": "x-phase-t.stack", "base": "integrate/phase-t",
              "checked_bead": f"{group}-dev", "checked_title": "the sprint", "sanity_bead": f"{group}-sanity",
              "qa_bead": f"{group}-qa", "phase_feature": "phase-t feature", "lint_command": "just lint",
              "policy_path": "policy.md", "max_sanity_rounds": 5}
    return fields


def poured(container_id: str, step: str, blocks: list[str]) -> dict:
    stage = "dev-sanity" if step == "sanity" else step
    bead = {"id": f"{container_id}.group-{step}", "issue_type": "task", "status": "open", "labels": ["phase-t", f"stage:{stage}"],
            "created_at": "2026-09-26T00:00:00Z",
            "dependencies": [{"dependency_type": "parent-child", "id": container_id}] + [{"dependency_type": "blocks", "id": b} for b in blocks],
            "metadata": {"difficulty": "normal", "branch": "sprint/test", "worktree": "wt", "pr_target": "integrate/phase-t",
                         **close_fields(container_id, step)}}
    if step == "sanity":
        bead["metadata"]["dev_bead"] = f"{container_id}.group-dev"
    return bead


ROOT_EPIC = {"id": "x-phase-t", "issue_type": "epic", "status": "open", "created_at": "2026-09-25T00:00:00Z",
             "metadata": {"phase": "t", "integration_branch": "integrate/phase-t"}}
STACK = {"id": "x-phase-t.stack", "parent": "x-phase-t", "issue_type": "task", "status": "open",
         "labels": ["phase-t", "stage:stack"], "metadata": {"phase": "t", "trunk": "integrate/phase-t",
                                                           "stack_number": None, "layers": []}}
VALID = [ROOT_EPIC, STACK,
         container("x-t-1"), poured("x-t-1", "dev", []), poured("x-t-1", "sanity", ["x-t-1.group-dev"]),
         container("x-t-2"), poured("x-t-2", "dev", ["x-t-1.group-sanity"]), poured("x-t-2", "sanity", ["x-t-2.group-dev"])]


def git(cwd: Path, *args: str) -> str:
    return run_subprocess(["git", "-c", "user.email=t@t", "-c", "user.name=t", *args], cwd=cwd, check=True,
                          capture_output=True, text=True).stdout


def lines(plan: list) -> str:
    return "".join((r if isinstance(r, str) else json.dumps(r)) + "\n" for r in plan)


class Repo(unittest.TestCase):
    """A repository holding the phase file; the plan file is passed with --index unless a test pushes it."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.base = Path(self.tmp.name)
        self.repo = self.base / "repo"
        run_subprocess(["git", "init", "-q", "-b", "main", str(self.repo)], check=True)
        config = self.repo / ".claude/project/atm-bd-orchestration.yaml"
        config.parent.mkdir(parents=True)
        config.write_text("plans_dir: plans\nbead_prefix: x\n")
        registry = self.repo / ".claude/agents/registry.yaml"
        registry.parent.mkdir(parents=True)
        registry.write_text("agents:\n  req-qa: {}\n  arch-qa: {}\n")
        write_phase(self.repo)

    def tearDown(self):
        self.tmp.cleanup()

    def run_vp(self, *args: str, env: dict | None = None) -> subprocess.CompletedProcess:
        return run_subprocess([str(SCRIPT), *args], cwd=self.repo, capture_output=True, text=True, env=env)

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
    def test_qa_reviewers_are_required_at_planning_not_inferred_from_config(self):
        qa = poured("x-t-1", "qa", ["x-t-1.group-sanity"])
        qa["metadata"].update(checked_bead="x-t-1.group-dev", round=1)
        config = self.repo / ".claude/project/atm-bd-orchestration.yaml"
        with config.open("a") as stream:
            stream.write('reviewers_round1: ["req-qa"]\n')
        for value in (None, "", "req-qa", [], [""], [" "], [7], ["req-qa", "req-qa"], ["unknown-qa"]):
            with self.subTest(reviewers=value):
                candidate = copy.deepcopy(qa)
                if value is not None:
                    candidate["metadata"]["reviewers_round1"] = value
                out = self.run_beads(VALID + [candidate])
                self.assertEqual(out.returncode, 5, out.stdout + out.stderr)
                self.assertIn("x-t-1.group-qa: reviewers_round1", out.stdout)
                self.assertNotIn("plan valid:", out.stdout)
        qa["metadata"]["reviewers_round1"] = ["req-qa", "arch-qa"]
        out = self.run_beads(VALID + [qa])
        self.assertEqual(out.returncode, 0, out.stdout + out.stderr)

    def test_qa_reviewer_registry_must_be_readable_and_valid(self):
        qa = poured("x-t-1", "qa", ["x-t-1.group-sanity"])
        qa["metadata"].update(checked_bead="x-t-1.group-dev", round=1, reviewers_round1=["req-qa"])
        registry = self.repo / ".claude/agents/registry.yaml"
        for content in (None, "agents: [", "agents: []", "agents: {}"):
            with self.subTest(registry=content):
                registry.unlink(missing_ok=True)
                if content is not None:
                    registry.write_text(content)
                out = self.run_beads(VALID + [qa])
                self.assertEqual(out.returncode, 5, out.stdout + out.stderr)
                self.assertIn("cannot validate reviewers_round1", out.stdout)

    def test_closed_qa_history_does_not_require_new_reviewer_metadata(self):
        qa = poured("x-t-1", "qa", [])
        qa.update(status="closed")
        qa["metadata"].update(checked_bead="x-t-1.group-dev", round=1)
        out = self.run_beads(VALID + [qa])
        self.assertEqual(out.returncode, 0, out.stdout + out.stderr)

    def test_stack_is_required_only_for_open_phase(self):
        without_stack = [bead for bead in VALID if bead["id"] != STACK["id"]]
        result = self.run_beads(without_stack)
        self.assertEqual(result.returncode, 5)
        self.assertIn("x-phase-t.stack", result.stdout)
        closed = copy.deepcopy(without_stack)
        closed[0]["status"] = "closed"
        self.assertEqual(self.run_beads(closed).returncode, 0)
        wrong_parent = broken("x-phase-t.stack", lambda bead: bead.update(parent="wrong"))
        self.assertEqual(self.run_beads(wrong_parent).returncode, 5)
        wrong_trunk = broken("x-phase-t.stack", lambda bead: bead["metadata"].update(trunk="wrong"))
        self.assertEqual(self.run_beads(wrong_trunk).returncode, 5)

    def test_valid(self):
        out = self.run_beads(VALID)
        self.assertEqual((out.returncode, out.stdout), (0, "plan valid: 2 sprints\n"), out.stderr)
        self.assertEqual(out.stderr, "")
        out = self.run_beads(VALID, PLAN, "--phase", "t")
        self.assertEqual(out.returncode, 0, out.stderr)

    def test_each_problem(self):
        cases = {
            "x-t-2.group-dev: missing blocks edge to x-t-1.group-sanity or x-t-1": (VALID, PLAN, "x-t-2.group-dev", lambda b: b.update(dependencies=[])),
            "x-t-1: requirements: ": (VALID, PLAN, "x-t-1", lambda b: b["metadata"].update(requirements=[])),
            "x-t-1: adrs: mixes NONE with ids": (VALID, PLAN, "x-t-1", lambda b: b["metadata"].update(adrs=["NONE", "ADR-1"])),
            "x-t-1: worktree: ": (VALID, PLAN, "x-t-1", lambda b: b["metadata"].pop("worktree")),
            "x-t-1: description: ": (VALID, PLAN, "x-t-1", lambda b: b.update(description="none")),
            "x-t-1: difficulty: ": (VALID, PLAN, "x-t-1", lambda b: b["metadata"].update(difficulty="medium")),
            "x-t-1.group-sanity: missing blocks edge to its sprint x-t-1.group-dev": (VALID, PLAN, "x-t-1.group-sanity", lambda b: b.update(dependencies=[])),
            "x-phase-t: metadata.integration_branch is \"integrate/other\"": (VALID, PLAN, "x-phase-t", lambda b: b["metadata"].update(integration_branch="integrate/other")),
        }
        for want, (beads, plan, bid, change) in cases.items():
            with self.subTest(problem=want):
                out = self.run_beads(broken(bid, change) if bid else beads, plan)
                got = out.stdout.splitlines()
                self.assertEqual(out.returncode, 5, out.stderr)
                if bid == "x-phase-t":
                    self.assertEqual(len(got), 2, got)
                    self.assertIn("x-phase-t.stack: missing stack bead", got[1])
                else:
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
        beads = broken("x-t-1.group-sanity", lambda b: b.update(metadata={**b["metadata"], "dev_bead": "x-t-2.group-dev"},
                                                                dependencies=[{"dependency_type": "blocks", "id": "x-t-2.group-dev"}]))
        out = self.run_beads(beads)
        self.assertEqual(out.returncode, 0)
        self.assertIn("warning: x-t-1.group-sanity: metadata.dev_bead is not x-t-1.group-dev", out.stderr)

    def test_open_finding_missing_required_key_is_a_problem_but_closed_history_is_ignored(self):
        finding = {"id": "x-t-1.finding", "parent": "x-t-1", "status": "open", "issue_type": "task",
                   "labels": ["phase-t", "stage:finding"], "metadata": {"severity": "important", "reviewer": "qa"}}
        out = self.run_beads(VALID + [finding])
        self.assertEqual((out.returncode, out.stdout), (5, "x-t-1.finding: found_at_commit: Field required\n"))
        finding["status"] = "closed"
        out = self.run_beads(VALID + [finding])
        self.assertEqual((out.returncode, out.stdout), (0, "plan valid: 2 sprints\n"), out.stderr)

    def test_open_phase_bead_without_a_stage_has_no_model(self):
        unknown = {"id": "x-t-1.unknown", "parent": "x-t-1", "status": "open", "labels": ["phase-t"]}
        out = self.run_beads(VALID + [unknown])
        self.assertEqual((out.returncode, out.stdout), (5, "x-t-1.unknown: no model\n"))

    def test_open_anchor_without_a_stage_is_not_a_workflow_bead(self):
        anchor = {"id": "x-t-1.anchor", "parent": "x-phase-t", "status": "open",
                  "labels": ["phase-t", "anchor"]}
        out = self.run_beads(VALID + [anchor])
        self.assertEqual((out.returncode, out.stdout), (0, "plan valid: 2 sprints\n"), out.stderr)

    def test_closed_sprint_and_sanity_are_not_schema_validated(self):
        beads = broken("x-t-1", lambda bead: (bead.update(status="closed"), bead["metadata"].pop("worktree")))
        sanity = next(bead for bead in beads if bead["id"] == "x-t-1.group-sanity")
        sanity["status"] = "closed"
        sanity["metadata"].pop("branch")
        out = self.run_beads(beads)
        self.assertEqual((out.returncode, out.stdout), (0, "plan valid: 2 sprints\n"), out.stderr)

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
            run_subprocess(["python3", str(ROOT / "scripts" / "bead_schema.py"), "export", d], check=True)
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

    def test_tracked_plan_without_its_phase_file_fails(self):
        out = self.ci("plans/phase-t.jsonl")
        self.assertEqual(out.returncode, 5, out.stdout + out.stderr)
        self.assertEqual(out.stdout.splitlines(),
                         ["plans/phase-t.jsonl: no tracked .atm-bd/phase-t.toml; the planner commits it with the plan"])

    def test_tracked_plan_with_its_phase_file_passes(self):
        out = self.ci("plans/phase-t.jsonl", ".atm-bd/phase-t.toml")
        self.assertEqual((out.returncode, out.stdout), (0, "plan valid: 1 tracked phase file(s)\n"), out.stderr)

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

    def test_missing_phase_lead_and_each_template_are_named(self):
        for key in ("lead", *phase_data()["templates"]):
            with self.subTest(key=key):
                data = phase_data()
                target = data if key == "lead" else data["templates"]
                target.pop(key)
                (self.repo / ".atm-bd/phase-t.toml").write_text(phase_toml(data))
                out = self.ci(".atm-bd/phase-t.toml", "plans/phase-t.jsonl")
                self.assertEqual(out.returncode, 5, out.stdout + out.stderr)
                self.assertIn(".atm-bd/phase-t.toml", out.stdout)
                self.assertIn(f"{key}: required", out.stdout)

    def test_each_dangling_template_is_named_before_bead_queries(self):
        for key in phase_data()["templates"]:
            with self.subTest(key=key):
                data = phase_data()
                data["templates"][key] = f".claude/skills/atm-bd-orchestration/templates/missing-{key}.xml.j2"
                (self.repo / ".atm-bd/phase-t.toml").write_text(phase_toml(data))
                out = self.ci(".atm-bd/phase-t.toml", "plans/phase-t.jsonl")
                self.assertEqual(out.returncode, 5, out.stdout + out.stderr)
                self.assertIn(f"templates.{key}", out.stdout)
                self.assertIn(f"missing-{key}.xml.j2", out.stdout)

    def test_ci_takes_no_other_option(self):
        out = self.run_vp("--ci", "--phase", "t", env=self.env)
        self.assertEqual(out.returncode, 2)
        self.assertFalse(self.bd_ran.exists())


FAKE_BD = """#!/usr/bin/env python3
import json, os, sys
beads = json.load(open(os.environ["FAKE_BD_BEADS"]))
args = sys.argv[1:]
if os.environ.get("FAKE_BD_LOG"):
    with open(os.environ["FAKE_BD_LOG"], "a") as log:
        log.write(json.dumps(args) + "\\n")
if args[:1] == ["show"]:
    ids = [a for a in args[1:] if not a.startswith("--")]
    found = [b for b in beads if b["id"] in ids]
    if not found:
        sys.exit("Error: no issue found matching " + " ".join(ids))
    print(json.dumps(found))
elif args[:1] == ["list"]:
    labels = [args[i + 1] for i, a in enumerate(args) if a == "-l"]
    print(json.dumps([b for b in beads if set(labels) <= set(b.get("labels") or [])
                      and b.get("status") not in ("closed", "tombstone")]))
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
        run_subprocess(["git", "init", "-q", "--bare", "-b", "main", str(origin)], check=True)
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

    def test_live_qa_reviewer_data_is_a_hard_gate(self):
        qa = poured("x-t-1", "qa", ["x-t-1.group-sanity"])
        qa["metadata"].update(checked_bead="x-t-1.group-dev", round=1, reviewers_round1=None)
        out = self.live(VALID + [qa])
        self.assertEqual(out.returncode, 5, out.stdout + out.stderr)
        self.assertIn("x-t-1.group-qa: reviewers_round1", out.stdout)
        qa["metadata"]["reviewers_round1"] = ["req-qa"]
        out = self.live(VALID + [qa])
        self.assertEqual(out.returncode, 0, out.stdout + out.stderr)

    def test_phase_walk_uses_one_list_includes_gates_and_excludes_other_phases(self):
        log = self.base / "bd.log"
        rows = VALID + [{"id": "other", "status": "open", "labels": ["phase-u"]},
                        {"id": "gate", "status": "open", "issue_type": "gate", "labels": ["phase-t"]}]
        out = self.live(rows, "--phase", "t", "--no-doctor", FAKE_BD_LOG=str(log))
        self.assertEqual((out.returncode, out.stdout), (0, "plan valid: 2 sprints\n"), out.stderr)
        lists = [json.loads(line) for line in log.read_text().splitlines() if json.loads(line)[0] == "list"]
        self.assertEqual(len(lists), 1)
        self.assertEqual(lists[0], ["list", "-l", "phase-t", "--include-gates", "-n", "0", "--json"])
        self.assertFalse(any("--all" in json.loads(line) for line in log.read_text().splitlines()))

    def test_closed_planned_ids_are_read_by_show_not_an_unscoped_list(self):
        beads = copy.deepcopy(VALID)
        closed_ids = {"x-t-1", "x-t-1.group-dev", "x-t-1.group-sanity"}
        for bead in beads:
            if bead["id"] in closed_ids:
                bead["status"] = "closed"
        beads.append({"id": "x-t-1.anchor", "parent": "x-phase-t", "status": "open", "labels": ["phase-t", "anchor"]})
        log = self.base / "closed-reads.jsonl"
        out = self.live(beads, "--phase", "t", "--no-doctor", FAKE_BD_LOG=str(log))
        self.assertEqual((out.returncode, out.stdout), (0, "plan valid: 2 sprints\n"), out.stderr)
        calls = [json.loads(line) for line in log.read_text().splitlines()]
        self.assertTrue(closed_ids <= {call[1] for call in calls if call[0] == "show"})
        self.assertEqual([call for call in calls if call[0] == "list"],
                         [["list", "-l", "phase-t", "--include-gates", "-n", "0", "--json"]])
        self.assertFalse(any("--all" in call for call in calls))

    def test_unlabelled_root_read_by_show_is_still_schema_validated(self):
        beads = broken("x-phase-t", lambda bead: bead["metadata"].pop("integration_branch"))
        out = self.live(beads, "--phase", "t", "--no-doctor")
        self.assertEqual(out.returncode, 5, out.stderr)
        self.assertIn("x-phase-t: integration_branch: Field required", out.stdout)

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
        out = self.run_file([ROOT_EPIC, STACK, container("x-t-1"), container("x-t-2")], [])
        self.assertEqual((out.returncode, out.stdout), (0, "plan valid: 2 sprints\n"), out.stderr)
        self.assertIn("warning: x-t-2: not poured yet", out.stderr)

    def test_import_rejects_qa_without_planned_reviewers(self):
        qa = poured("x-t-1", "qa", ["x-t-1.group-sanity"])
        qa["metadata"].update(checked_bead="x-t-1.group-dev", round=1)
        out = self.run_file([qa], VALID)
        self.assertEqual(out.returncode, 5, out.stdout + out.stderr)
        self.assertIn("x-t-1.group-qa: reviewers_round1", out.stdout)
        qa["metadata"]["reviewers_round1"] = ["req-qa"]
        out = self.run_file([qa], VALID)
        self.assertEqual(out.returncode, 0, out.stdout + out.stderr)

    def test_an_open_phase_requires_its_stack_bead_in_the_file(self):
        out = self.run_file([ROOT_EPIC, container("x-t-1"), container("x-t-2")], [])
        self.assertEqual(out.returncode, 5, out.stderr)
        self.assertIn("x-phase-t.stack: missing stack bead", out.stdout)

    def test_a_plan_bead_with_an_assignee_or_no_parent_is_a_warning(self):
        orphan = {**{k: v for k, v in container("x-t-2").items() if k != "parent"}, "assignee": "my-dev"}
        out = self.run_file([ROOT_EPIC, STACK, container("x-t-1"), orphan], [])
        self.assertEqual(out.returncode, 0, out.stdout)
        self.assertIn("warning: x-t-2: assignee set at plan time", out.stderr)
        self.assertIn("warning: x-t-2: task at the top level", out.stderr)

    def test_an_import_into_a_running_phase_reads_the_rest_live(self):
        out = self.run_file([container("x-t-2")], [b for b in VALID if not b["id"].startswith("x-t-2")])
        self.assertEqual(out.returncode, 0, out.stdout + out.stderr)

    def test_file_overlays_live_and_validates_other_live_phase_beads(self):
        invalid = {"id": "finding", "parent": "x-t-1", "status": "open", "labels": ["phase-t", "stage:finding"],
                   "metadata": {"severity": "important", "reviewer": "qa"}}
        out = self.run_file([container("x-t-2")], VALID + [invalid])
        self.assertEqual((out.returncode, out.stdout), (5, "finding: found_at_commit: Field required\n"))
        fixed = {**invalid, "metadata": {**invalid["metadata"], "found_at_commit": "abc"}}
        out = self.run_file([container("x-t-2"), fixed], VALID + [invalid])
        self.assertEqual((out.returncode, out.stdout), (0, "plan valid: 2 sprints\n"), out.stderr)


if __name__ == "__main__":
    unittest.main()
