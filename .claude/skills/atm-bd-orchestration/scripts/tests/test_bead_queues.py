"""bead-queues against a real bd workspace (the pour tests' proxied server), with `atm` and `gh` stubbed.

The beads are built in the package's shape: sprint groups poured by bead-groups
under a container that is a child of the phase root, finding beads and fix
groups as quality-mgr files them. Only `atm` and `gh` are fake executables on
PATH printing fixed JSON from files the test writes. Each test uses its own phase.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_bead_pour_mock import MISSING, SKIP_REASON, Workspace, findings_file, group, init_workspace, stop_server  # noqa: E402

FAKE_ATM = """#!/usr/bin/env python3
import os, sys
sys.stdout.write(open(os.environ["FAKE_ATM_TASKS"]).read())
"""
FAKE_GH = """#!/usr/bin/env python3
import os, sys
if os.environ.get("FAKE_GH_FAIL"):
    sys.exit("gh: HTTP 502")
key = "FAKE_GH_STACKS" if "api" in sys.argv[1:] else "FAKE_GH_PRS"
sys.stdout.write(open(os.environ[key]).read())
"""


class Queues:
    """One phase: its root, phase file, fake atm/gh data and a bead-queues runner."""

    def __init__(self, ws: Workspace, phase: str):
        self.ws, self.phase, self.root = ws, phase, f"t-phase-{phase}"
        self.dir = ws.root / f"q-{phase}"
        self.dir.mkdir()
        self.trunk = f"integrate/phase-{phase}"
        ws.bd("create", f"phase {phase}", "--id", self.root, "--type", "epic",
              "--metadata", json.dumps({"integration_branch": self.trunk}), "--silent")
        toml = ws.root / ".atm-bd" / f"phase-{phase}.toml"
        toml.parent.mkdir(exist_ok=True)
        toml.write_text(f'plan = "docs/plans/phase-{phase}.jsonl"\nroot = "{self.root}"\nintegration_branch = "{self.trunk}"\n')
        self.tasks([])
        self.github([], [])
        self.state = self.dir / "state.json"

    def sprint(self, n: int) -> str:
        bead = self.ws.sprint(self.phase, n)
        self.ws.bd("update", bead, "--parent", self.root)
        self.ws.groups("--sprint", bead)
        return bead

    def tasks(self, task_ids: list[str], reminders: int = 0) -> None:
        rows = [{"task_id": t, "assignee": "dev1", "state": "active", "assigned_at": "2026-01-01T00:00:00Z",
                 "reminder_count": reminders} for t in task_ids]
        (self.dir / "tasks.json").write_text(json.dumps(rows))

    def github(self, stacks: list[dict], prs: list[dict]) -> None:
        (self.dir / "stacks.json").write_text(json.dumps([stacks]))   # --paginate --slurp: a list of pages
        (self.dir / "prs.json").write_text(json.dumps(prs))

    def run(self, *args: str, env: dict | None = None) -> subprocess.CompletedProcess:
        bin_dir = self.ws.root / "fake-bin"
        e = {**self.ws.env, "PATH": f"{bin_dir}{os.pathsep}{self.ws.env['PATH']}", "ATM_IDENTITY": "lead1",
             "FAKE_ATM_TASKS": str(self.dir / "tasks.json"), "FAKE_GH_STACKS": str(self.dir / "stacks.json"),
             "FAKE_GH_PRS": str(self.dir / "prs.json"), **(env or {})}
        return subprocess.run([sys.executable, str(self.ws.scripts / "bead-queues"), "--phase", self.phase, *args],
                              cwd=self.ws.root, env=e, text=True, capture_output=True)

    def queues(self) -> dict[str, set[str]]:
        """Queue -> ids, read from the cron JSON with a fresh state and no age window."""
        fresh = self.dir / "fresh.json"
        fresh.unlink(missing_ok=True)
        proc = self.run("--json", "--min-age", "0", "--state-file", str(fresh))
        assert proc.returncode == 0, proc.stderr
        out: dict[str, set[str]] = {}
        for row in json.loads(proc.stdout)["rows"] if proc.stdout else []:
            out.setdefault(row["queue"], set()).add(row["id"])
        return out

    def cron(self, min_age: int = 0) -> list[tuple[str, str]]:
        proc = self.run("--json", "--min-age", str(min_age), "--state-file", str(self.state))
        assert proc.returncode == 0, proc.stderr
        return [] if proc.stdout == "" else [(r["queue"], r["id"]) for r in json.loads(proc.stdout)["rows"]]


def child(ws: Workspace, bead: str, parent: str, *args: str) -> None:
    """bd refuses --id with --parent: create, then attach."""
    ws.bd("create", bead, "--id", bead, *args, "--silent")
    ws.bd("update", bead, "--parent", parent)


def pr(number: int, base: str, head: str) -> dict:
    return {"number": number, "baseRefName": base, "headRefName": head, "isDraft": False,
            "createdAt": "2026-01-01T00:00:00Z", "title": f"pr {number}"}


def stack(number: int, base: str, *prs_: dict) -> dict:
    return {"number": number, "open": True, "base": {"ref": base}, "created_at": "2026-01-01T00:00:00Z",
            "pull_requests": [{"number": p["number"], "state": "open", "head": {"ref": p["headRefName"]}} for p in prs_]}


@unittest.skipUnless(not MISSING, SKIP_REASON)
class BeadQueuesTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        root = Path(tempfile.mkdtemp(prefix="bead-queues-")).resolve()
        cls.ws = Workspace(root)
        cls.addClassCleanup(shutil.rmtree, root, ignore_errors=True)
        cls.addClassCleanup(stop_server, root)
        init_workspace(cls.ws)
        bin_dir = root / "fake-bin"
        bin_dir.mkdir()
        for name, body in (("atm", FAKE_ATM), ("gh", FAKE_GH)):
            (bin_dir / name).write_text(body)
            (bin_dir / name).chmod(0o755)

    def test_sprint_group_walks_through_its_queues(self):
        q = Queues(self.ws, "qa")
        plan_qa = f"{q.root}-plan-qa"
        sprint = q.sprint(1)
        dev, sanity, qa = group(sprint)
        child(self.ws, plan_qa, q.root, "-l", "phase-qa,stage:plan-review")
        self.ws.bd("dep", "add", sprint, plan_qa)
        got = q.queues()
        assert got.get("needs_plan_review") == {plan_qa}
        assert dev not in got.get("needs_dev", set())               # prerequisite: the plan review

        self.ws.close(plan_qa, "PASS")
        assert q.queues().get("needs_dev") == {dev}
        q.tasks([dev])                                               # a live ATM task suppresses the row
        assert dev not in str(q.queues())
        human = q.run().stdout
        assert "In flight (ATM task open)" in human and dev in human
        q.tasks([dev], reminders=2)                                  # under the stall threshold: still quiet
        assert dev not in str(q.queues())
        q.tasks([dev], reminders=10)                                 # active, reminders unanswered: the lead re-engages
        assert q.queues() == {"task_stalled": {dev}}

        q.tasks([])
        self.ws.bd("update", dev, "--claim", "--actor", "dev1")
        assert q.queues().get("claimed_no_task") == {dev}

        self.ws.close(dev, "dev complete at abc1234")
        got = q.queues()
        assert got.get("dev_no_pr") == {dev} and got.get("needs_sanity") == {sanity}
        assert qa not in str(got)                                    # QA waits on the sanity PASS
        self.ws.bd("update", dev, "--append-notes", "layer PR #7 https://example/pull/7")
        assert "dev_no_pr" not in q.queues()

        q.tasks([dev])                                               # the dev never closed its ATM task
        assert q.queues().get("task_bead_closed") == {dev}
        q.tasks([])
        self.ws.close(sanity, "PASS at abc1234")
        assert q.queues() == {"needs_qa": {qa}}

        # QA FAIL: a blocking fix group, an important and a minor finding bead
        path = findings_file(self.ws, sprint, qa, 1, {"ref": "qa1-f1"})
        self.ws.groups("--findings", str(path))
        meta = {"severity": "important", "reviewer": "rbp"}
        for ref, sev in (("imp1", "important"), ("min1", "minor")):
            child(self.ws, f"{sprint}.{ref}", sprint, "--type", "bug", "-l", "phase-qa,stage:finding",
                  "--deps", f"discovered-from:{qa}", "--metadata", json.dumps({**meta, "severity": sev}))
        self.ws.close(qa, "FAIL: 1 blocking")
        got = q.queues()
        assert got.get("needs_fix") == {f"{sprint}.qa1-f1-r1-fix"}
        assert got.get("needs_finding_fix") == {f"{sprint}.imp1"} and got.get("minor_backlog") == {f"{sprint}.min1"}
        assert f"{sprint}.qa1-f1-r1-sanity" not in str(got)

        # an important finding fixed with no sanity bead imported for it
        self.ws.bd("update", f"{sprint}.imp1", "--assignee", "dev1")
        self.ws.close(f"{sprint}.imp1", "fixed at def5678 (PR #8)")
        assert q.queues().get("finding_no_sanity") == {f"{sprint}.imp1"}

    def test_sanity_fail_children_and_closable_container(self):
        q = Queues(self.ws, "sf")
        sprint = q.sprint(1)
        dev, sanity, qa = group(sprint)
        self.ws.close(dev, "dev complete (PR #3)")
        # sanity FAIL: dev-sanity files a child finding under the checked bead and reopens it
        child(self.ws, f"{dev}.1", dev, "--type", "bug", "-l", "phase-sf,stage:finding",
              "--metadata", json.dumps({"sanity_finding": True, "severity": "blocking"}))
        self.ws.bd("update", dev, "--status", "open")
        got = q.queues()
        assert got.get("needs_dev_fix") == {dev}
        assert f"{dev}.1" not in str(got)                            # dispatched with its checked bead, never alone

        for bead, reason in ((f"{dev}.1", "fixed"), (dev, "dev complete (PR #4)"), (sanity, "PASS at 1234567"), (qa, "PASS")):
            self.ws.close(bead, reason)
        assert q.queues() == {"container_closable": {sprint}}

    def test_sanity_pass_with_no_qa_bead(self):
        q = Queues(self.ws, "nq")
        sprint = q.sprint(1)
        dev, sanity, qa = group(sprint)
        self.ws.bd("delete", qa, "--force")
        self.ws.close(dev, "dev complete (PR #5)")
        self.ws.close(sanity, "PASS at 1234567")
        assert q.queues().get("no_qa_record") == {sanity}

    def test_stack_rows(self):
        q = Queues(self.ws, "st")
        t = q.trunk
        p1, p2, p3, p4 = pr(1, t, "a"), pr(2, "a", "b"), pr(3, "x", "c"), pr(4, "b", "d")
        q.github([stack(10, t, p1, p2, p3)], [p1, p2, p3, p4, pr(9, "develop", "z")])
        got = q.queues()
        assert got == {"stack_incoherent": {"stack#10"}, "pr_unlinked": {"pr#4"}}
        q.github([stack(10, t, p1, p2), stack(11, t, p4)], [p1, p2, p4])
        assert q.queues() == {"stack_incoherent": {"stack#10", "stack#11"}}   # two stacks: not one merge
        q.github([], [p1])                                           # layer 0 waits unlinked on the trunk
        assert q.queues() == {}
        q.github([], [p1, p2])
        assert q.queues() == {"pr_unlinked": {"pr#1", "pr#2"}}

    def test_cron_contract(self):
        q = Queues(self.ws, "cr")
        sprint = q.sprint(1)
        dev = group(sprint)[0]
        assert q.cron(min_age=10) == []                              # just poured: held back by the window
        assert not q.state.exists() or json.loads(q.state.read_text())["reported"] == []
        assert q.cron() == [("needs_dev", dev)]
        assert q.run("--json", "--min-age", "0", "--state-file", str(q.state)).stdout == ""   # reported once
        q.tasks([dev])
        assert q.cron() == []                                        # left the queue
        q.tasks([])
        assert q.cron() == [("needs_dev", dev)]                      # re-entered: reported again
        assert json.loads(q.state.read_text())["reported"] == [f"needs_dev:{dev}"]
        default = q.run("--json", "--min-age", "0")
        assert default.returncode == 0 and (self.ws.root / ".atm-bd/bead-queues" / f"{q.root}.json").is_file()
        assert (self.ws.root / ".atm-bd/bead-queues/.gitignore").read_text() == "*\n"

    def test_errors_exit_nonzero_with_empty_stdout(self):
        q = Queues(self.ws, "er")
        proc = q.run("--json", env={"FAKE_GH_FAIL": "1"})
        assert proc.returncode == 2 and proc.stdout == "" and "gh" in proc.stderr
        proc = q.run("--json", env={"ATM_IDENTITY": ""})
        assert proc.returncode == 3 and proc.stdout == ""
        self.ws.bd("delete", q.root, "--force")
        proc = q.run("--json")
        assert proc.returncode == 2 and proc.stdout == "" and q.root in proc.stderr


if __name__ == "__main__":
    unittest.main()
