"""bead-groups and sc-compose-pour-mock against a real bd 1.3.0 database.

The module initializes one throwaway proxied-server workspace
(`bd init --prefix t --proxied-server`; the embedded engine is not in this bd
build) laid out like an installed repository, so these tests need `bd`,
`dolt` (the proxy's backend) and `sc-compose` (the mock renders with the real
one). Nothing here mocks bd. Each test uses its own phase.
"""
from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SKILL = Path(__file__).resolve().parents[2]
ATM_BEADS = SKILL.parent / "atm-beads"

MISSING = [tool for tool in ("bd", "dolt", "sc-compose") if shutil.which(tool) is None]
SKIP_REASON = f"{', '.join(MISSING)} not on PATH: real-bd pour tests not run"

# Counters bd derives per bead; they change when another bead gains an edge to it, which is not an edit of the bead.
DERIVED = ("dependent_count", "dependency_count")


def clean_env() -> dict:
    env = {k: v for k, v in os.environ.items() if not k.startswith(("BEADS_", "BD_"))}
    env.update({"BD_NON_INTERACTIVE": "1", "BEADS_ACTOR": "pour-test"})
    return env


class Workspace:
    def __init__(self, root: Path):
        self.root = root
        self.env = clean_env()
        self.scripts = root / ".claude/skills/atm-bd-orchestration/scripts"

    def run(self, *argv, check=True) -> subprocess.CompletedProcess:
        proc = subprocess.run([str(a) for a in argv], cwd=self.root, env=self.env, text=True, capture_output=True)
        if check and proc.returncode:
            raise AssertionError(f"{argv} exited {proc.returncode}\nstdout: {proc.stdout}\nstderr: {proc.stderr}{self.server_logs()}")
        return proc

    def server_logs(self) -> str:
        """The proxied server's log tails, so a dropped connection carries its cause."""
        out = ""
        for name in ("proxy.log", "server.log"):
            log = self.root / ".beads" / "dolt" / name
            if log.is_file():
                out += f"\n--- {name} (last 40 lines) ---\n" + "\n".join(log.read_text(errors="replace").splitlines()[-40:])
        return out

    def bd(self, *args, check=True):
        return self.run("bd", *args, check=check)

    def show(self, bead: str) -> dict | None:
        proc = self.bd("show", bead, "--json", check=False)
        return json.loads(proc.stdout)[0] if proc.returncode == 0 else None

    def snapshot(self, *beads: str) -> dict:
        """The beads' own fields. Edges are kept as sorted (id, type) pairs: `bd show` embeds the whole
        row of each related bead, and those rows change with the other bead, not with this one."""
        out = {}
        for bead in beads:
            row = {k: v for k, v in (self.show(bead) or {}).items() if k not in DERIVED}
            for key in ("dependencies", "dependents"):
                if key in row:
                    row[key] = sorted((d["id"], d.get("dependency_type")) for d in row[key])
            out[bead] = row
        return out

    def unchanged(self, before: dict) -> None:
        after = self.snapshot(*before)
        changed = {b: {k: (before[b].get(k), after[b].get(k)) for k in set(before[b]) | set(after[b])
                       if before[b].get(k) != after[b].get(k)} for b in before}
        assert not any(changed.values()), changed

    def deps(self, bead: str) -> dict[str, str]:
        return {row["id"]: row["dependency_type"] for row in json.loads(self.bd("dep", "list", bead, "--json").stdout or "[]")}

    def ready(self) -> set[str]:
        return {row["id"] for row in json.loads(self.bd("ready", "--json", "-n", "0").stdout or "[]")}

    def close(self, bead: str, reason: str, *, check=True):
        """Close as the bead's assignee (bd refuses a close by anyone else)."""
        actor = (self.show(bead) or {}).get("assignee") or self.env["BEADS_ACTOR"]
        return self.bd("close", bead, "--reason", reason, "--actor", actor, check=check)

    def sprint(self, phase: str, n: int, deps: tuple[int, ...] = ()) -> str:
        """Stage 3 (plan import): a sprint container, with no assignee, its plan file line and the phase file."""
        name = f"{phase}-{n}"
        bead = f"t-{name}"
        meta = {"phase": phase, "sprint": name, "stack": f"phase-{phase}", "layer": n, "difficulty": "normal",
                "requirements": ["REQ-1"], "adrs": ["NONE"]}
        self.bd("create", f"{name}: sprint", "--id", bead, "--type", "feature",
                "--metadata", json.dumps(meta), "--silent")
        plan = self.root / "docs/plans" / f"phase-{phase}.jsonl"
        plan.parent.mkdir(parents=True, exist_ok=True)
        with plan.open("a") as fh:
            fh.write(json.dumps({"sprint": name, **({"depends_on": [f"{phase}-{d}" for d in deps]} if deps else {})}) + "\n")
        toml = self.root / ".atm-bd" / f"phase-{phase}.toml"
        toml.parent.mkdir(exist_ok=True)
        toml.write_text(f'plan = "docs/plans/phase-{phase}.jsonl"\nroot = "t-phase-{phase}"\nintegration_branch = "integrate/phase-{phase}"\n')
        return bead

    def groups(self, *args, expect: int = 0) -> dict:
        proc = self.run(sys.executable, self.scripts / "bead-groups", "--json", *args, check=False)
        assert proc.returncode == expect, f"exit {proc.returncode}\n{proc.stdout}\n{proc.stderr}{self.server_logs()}"
        return json.loads(proc.stdout)


def stop_server(root: Path) -> None:
    subprocess.run(["bd", "dolt", "stop"], cwd=root, env=clean_env(), capture_output=True)
    marker = str(root / ".beads" / "dolt")
    for line in subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True, text=True).stdout.splitlines():
        pid, _, command = line.strip().partition(" ")
        if marker in command:
            try:
                os.kill(int(pid), signal.SIGTERM)
            except (ProcessLookupError, ValueError):
                pass


def init_workspace(w: Workspace) -> None:
    root = w.root
    subprocess.run(["git", "init", "-q"], cwd=root, check=True)
    w.bd("init", "--prefix", "t", "--proxied-server", "--non-interactive", "--quiet", "--skip-agents", "--skip-hooks")
    ignore = shutil.ignore_patterns("__pycache__", "tests")
    shutil.copytree(SKILL, root / ".claude/skills/atm-bd-orchestration", ignore=ignore)
    shutil.copytree(ATM_BEADS, root / ".claude/skills/atm-beads", ignore=ignore)
    config = root / ".claude/project/atm-bd-orchestration.yaml"
    config.parent.mkdir(parents=True)
    config.write_text("bead_prefix: t\ndev_sanity_member: dev-sanity\nqa_member: quality-mgr\nplans_dir: docs/plans\n")


def group(sprint: str) -> tuple[str, str, str]:
    return f"{sprint}.group-dev", f"{sprint}.group-sanity", f"{sprint}.group-qa"


def target(result: dict, bead: str) -> dict:
    return next(t for t in result["targets"] if t["id"] == bead)


def findings_file(ws: Workspace, sprint: str, filed_by: str, round_: int, *findings: dict) -> Path:
    """What quality-mgr writes for one QA round: the blocking findings it raised on one sprint."""
    rows = [{"severity": "blocking", "reviewer": "rbp", "title": "unchecked error", "remedy": "return the typed error",
             "priority": 1, **f} for f in findings]
    path = ws.root / f"findings-{sprint}-r{round_}-{len(list(ws.root.glob('findings-*')))}.json"
    path.write_text(json.dumps({"sprint": sprint, "round": round_, "filed_by": filed_by, "found_at_commit": "abc1234", "findings": rows}))
    return path


def fix_group(sprint: str, ref: str, round_: int = 1) -> tuple[str, str, str]:
    return tuple(f"{sprint}.{ref}-r{round_}-{step}" for step in ("fix", "sanity", "qa"))


@unittest.skipUnless(not MISSING, SKIP_REASON)
class BeadPourMockTests(unittest.TestCase):
    """All tests share one workspace, created once for the class; each test uses its own phase."""

    @classmethod
    def setUpClass(cls):
        root = Path(tempfile.mkdtemp(prefix="pour-mock-")).resolve()
        cls.ws = Workspace(root)
        # Registered before init so a failed init still stops the server and removes the directory.
        cls.addClassCleanup(shutil.rmtree, root, ignore_errors=True)
        cls.addClassCleanup(stop_server, root)
        init_workspace(cls.ws)

    def test_validate_reports_then_pour_creates_the_sprint_group(self):
        ws = self.ws
        sprint = ws.sprint("a", 1)
        dev, sanity, qa = group(sprint)

        report = ws.groups("--validate", "--sprint", sprint, expect=5)
        assert {f"missing bead {b}" for b in (dev, sanity, qa)} <= set(target(report, sprint)["problems"])
        assert all(ws.show(b) is None for b in (dev, sanity, qa))

        entry = target(ws.groups("--sprint", sprint), sprint)
        assert entry["ids"] == {"dev": dev, "sanity": sanity, "qa": qa}
        assert [n["action"] for n in entry["nodes"]] == ["created"] * 3
        # each formula is rendered beside its request and receipt; nothing is written into bd's registry
        pour = ws.root / ".atm-bd" / "pour"
        assert sorted(f.name for f in pour.glob(f"{sprint}.*.formula.toml"))
        assert not (ws.root / ".beads" / "formulas").exists()
        assert [(e["from"], e["to"], e["type"], e["action"]) for e in entry["edges"]] == [(qa, dev, "validates", "added")]
        assert ws.deps(dev) == {sprint: "parent-child"}
        assert ws.deps(sanity) == {sprint: "parent-child", dev: "blocks"}
        assert ws.deps(qa) == {sprint: "parent-child", sanity: "blocks", dev: "validates"}
        assert set(ws.show(sanity)["metadata"]) == {"dev_bead", "difficulty", "sc_compose_attach"}   # N9: minimal sanity bead
        assert ws.show(sanity)["metadata"]["dev_bead"] == dev
        assert ws.show(qa)["metadata"]["checked_bead"] == dev
        assert ws.show(dev)["metadata"]["layer"] == 1
        # no assignment in advance: every poured bead carries difficulty, the lead picks the agent at dispatch
        assert [ws.show(b).get("assignee") for b in (dev, sanity, qa)] == [None] * 3
        assert [ws.show(b)["metadata"]["difficulty"] for b in (dev, sanity, qa)] == ["normal"] * 3
        # stage labels route each poured bead in the lead's Loop
        assert [sorted(l for l in ws.show(b)["labels"] if l.startswith("stage:")) for b in (dev, sanity, qa)] == [
            ["stage:dev"], ["stage:dev-sanity"], ["stage:qa"]]
        assert target(ws.groups("--validate", "--sprint", sprint), sprint)["problems"] == []   # SanityBead schema included

        ready = ws.ready()
        assert dev in ready and sanity not in ready and qa not in ready
        ws.close(dev, "done")
        ready = ws.ready()
        assert sanity in ready and qa not in ready
        ws.close(sanity, "PASS")
        assert qa in ws.ready()


    def test_rerun_changes_nothing_and_never_reopens(self):
        ws = self.ws
        sprint = ws.sprint("b", 1)
        dev, sanity, qa = group(sprint)
        ws.groups("--sprint", sprint)
        ws.bd("update", dev, "--notes", "dev evidence: commit abc123")
        ws.close(dev, "done at abc123")
        ws.bd("update", sanity, "--status", "in_progress", "--notes", "checking abc123", "--actor", "dev-sanity")
        before = ws.snapshot(sprint, dev, sanity, qa)

        entry = target(ws.groups("--sprint", sprint), sprint)
        assert [n["action"] for n in entry["nodes"]] == ["existing"] * 3
        assert {e["action"] for e in entry["edges"]} == {"existing"}
        after = ws.snapshot(sprint, dev, sanity, qa)
        ws.unchanged(before)
        assert after[dev]["status"] == "closed" and after[dev]["notes"] == "dev evidence: commit abc123"
        assert after[sanity]["status"] == "in_progress"


    def test_phase_target_fills_in_only_new_sprints_and_gates_dependents(self):
        ws = self.ws
        first = ws.sprint("c", 1)
        second = ws.sprint("c", 2, (1,))
        f_dev, f_sanity, f_qa = group(first)
        ws.groups("--phase", "c")
        # the dependent's dev bead, not its sprint container, blocks on the predecessor's initial sanity bead
        assert ws.deps(f"{second}.group-dev")[f_sanity] == "blocks"
        assert f_sanity not in ws.deps(second)
        before = ws.snapshot(*group(first), *group(second), first, second)

        third = ws.sprint("c", 3, (1,))     # added to the plan after the first run
        result = ws.groups("--phase", "c")
        assert {t["id"]: {n["action"] for n in t["nodes"]} for t in result["targets"]} == {
            first: {"existing"}, second: {"existing"}, third: {"created"}}
        ws.unchanged(before)
        assert ws.deps(f"{third}.group-dev")[f_sanity] == "blocks"
        assert first not in ws.deps(third) and first not in ws.deps(f"{third}.group-dev")   # no sprint edge unless the user asks
        assert ws.groups("--validate", "--sprint", f"{first},{second}", "--sprint", third)["outcome"] == "succeeded"

        ready = ws.ready()
        assert f_dev in ready and f"{second}.group-dev" not in ready and f"{third}.group-dev" not in ready
        ws.close(f_dev, "done")
        ws.close(f_sanity, "PASS")
        ready = ws.ready()
        assert {f"{second}.group-dev", f"{third}.group-dev"} <= ready   # released by the predecessor's sanity, not its QA
        assert f_qa in ready


    def test_an_interrupted_pour_resumes_by_creating_only_what_is_missing(self):
        ws = self.ws
        sprint = ws.sprint("d", 1)
        dev, sanity, qa = group(sprint)
        ws.groups("--sprint", sprint)
        ws.close(dev, "done")
        ws.bd("delete", qa, "--force")
        before = ws.snapshot(dev, sanity)
        assert f"missing bead {qa}" in target(ws.groups("--validate", "--sprint", sprint, expect=5), sprint)["problems"]

        entry = target(ws.groups("--sprint", sprint), sprint)
        assert {n["id"]: n["action"] for n in entry["nodes"]} == {dev: "existing", sanity: "existing", qa: "created"}
        ws.unchanged(before)
        assert ws.deps(qa) == {sprint: "parent-child", sanity: "blocks", dev: "validates"}


    def test_conflicts_are_refused_before_any_write(self):
        ws = self.ws
        # A bead at a poured id that this formula did not pour.
        handmade = ws.sprint("e", 1)
        dev, sanity, qa = group(handmade)
        ws.bd("create", "hand made", "--id", dev, "--silent")
        assert target(ws.groups("--sprint", handmade, expect=2), handmade)["error"]["code"] == "BEADS_ATTACH_CONFLICT"
        assert ws.show(sanity) is None and ws.show(qa) is None

        # A second dependency type on a pair the post-pour step needs: refused, and its other edges are not written.
        sprint = ws.sprint("e", 2)
        dependent = ws.sprint("e", 3, (2,))
        dev, sanity, qa = group(sprint)
        d_dev, _, d_qa = group(dependent)
        ws.groups("--sprint", sprint, "--sprint", dependent)
        ws.bd("dep", "remove", d_qa, d_dev)
        ws.bd("dep", "remove", d_dev, sanity)
        ws.bd("dep", "add", d_qa, d_dev, "--type", "related")
        entry = target(ws.groups("--sprint", dependent, expect=2), dependent)
        assert entry["error"]["code"] == "BEAD_GROUPS_EDGE_REFUSED"
        assert sanity not in ws.deps(d_dev)           # the cross-sprint edge of the same target was not added
        assert ws.deps(d_qa)[d_dev] == "related"
        problems = target(ws.groups("--validate", "--sprint", dependent, expect=5), dependent)["problems"]
        assert any("exists as related" in p for p in problems)
        assert any(p == f"missing edge {d_dev} -blocks-> {sanity}" for p in problems)

        # The same sprint poured again from a different formula revision (its priority changed).
        ws.bd("update", sprint, "--priority", "3")
        assert target(ws.groups("--sprint", sprint, expect=2), sprint)["error"]["code"] == "BEADS_ATTACH_CONFLICT"
        assert ws.show(dev)["priority"] == 2


    def test_the_mock_refuses_unauthorized_and_mismatched_requests(self):
        ws = self.ws
        sprint = ws.sprint("h", 1)
        ws.groups("--validate", "--sprint", sprint, expect=5)   # writes the request file
        request = json.loads((ws.root / ".atm-bd/pour" / f"{sprint}.group.sprint-group.request.json").read_text())
        path = ws.root / "h-request.json"
        mock = ws.scripts / "sc-compose-pour-mock"

        path.write_text(json.dumps({**request, "pour_authorization": None}))
        proc = ws.run(sys.executable, mock, "bead", "pour", "--request", path, "--json", check=False)
        assert proc.returncode == 3 and json.loads(proc.stdout)["payload"]["error"]["code"] == "BEADS_POUR_AUTH_REQUIRED"

        path.write_text(json.dumps({**request, "pour_authorization": "CreatePersistentBeads",
                                    "bead_variables": {"parent": "t-e-2", "ref": "group"}}))
        proc = ws.run(sys.executable, mock, "bead", "pour", "--request", path, "--json", check=False)
        assert proc.returncode == 2
        assert json.loads(proc.stdout)["payload"]["outcome"] == {"refused": {"code": "BEADS_ATTACH_SCOPE_MISMATCH"}}
        assert ws.show(f"{sprint}.group-dev") is None


    def test_option_b_pours_no_sanity_blocks_and_adds_validates(self):
        ws = self.ws
        sprint = ws.sprint("f", 1)
        dev, sanity, qa = group(sprint)
        ws.groups("--option", "B", "--sprint", sprint)
        assert ws.deps(sanity) == {sprint: "parent-child", dev: "validates"}
        assert ws.deps(qa) == {sprint: "parent-child", sanity: "blocks", dev: "validates"}
        # The option is baked into the render, so the same sprint under C is another formula revision.
        assert target(ws.groups("--validate", "--option", "C", "--sprint", sprint, expect=2), sprint)["error"]["code"] == "BEADS_ATTACH_CONFLICT"
        ws.bd("dep", "remove", sanity, dev)
        problems = target(ws.groups("--validate", "--option", "B", "--sprint", sprint, expect=5), sprint)["problems"]
        assert f"missing edge {sanity} -validates-> {dev}" in problems

        # Under C the pour's own sanity -> dev blocks edge is checked too.
        other = ws.sprint("f", 2)
        o_dev, o_sanity, _ = group(other)
        ws.groups("--sprint", other)
        ws.bd("dep", "remove", o_sanity, o_dev)
        problems = target(ws.groups("--validate", "--sprint", other, expect=5), other)["problems"]
        assert any(p.startswith(f"option C expects {o_sanity} -> {o_dev} as blocks") for p in problems)


    def test_a_repository_override_formula_wins(self):
        ws = self.ws
        override = ws.root / ".atm-bd/formula"
        override.mkdir(parents=True, exist_ok=True)
        source = (ws.root / ".claude/skills/atm-bd-orchestration/formulas/sprint-group.formula.toml.j2").read_text()
        marker = "description = \"Dev work for sprint"
        assert marker in source
        (override / "sprint-group.formula.toml.j2").write_text(source.replace(marker, "description = \"OVERRIDE. Dev work for sprint", 1))
        try:
            sprint = ws.sprint("i", 1)
            ws.groups("--sprint", sprint)
            assert ws.show(f"{sprint}.group-dev")["description"].startswith("OVERRIDE.")
        finally:
            shutil.rmtree(override)


    def test_blocking_findings_pour_flat_fix_groups_under_the_sprint(self):
        ws = self.ws
        sprint = ws.sprint("g", 1)
        dev, sanity, qa = group(sprint)
        ws.groups("--sprint", sprint)
        ws.close(dev, "done")
        ws.close(sanity, "PASS")
        path = findings_file(ws, sprint, qa, 1, {"ref": "qa1-f1"}, {"ref": "qa1-f2", "remedy": "add the missing test"})

        assert target(ws.groups("--validate", "--findings", path, expect=5), f"{sprint}.qa1-f1-r1")["problems"]
        result = ws.groups("--findings", path)
        fix, fix_sanity, fix_qa = fix_group(sprint, "qa1-f1")
        entry = target(result, f"{sprint}.qa1-f1-r1")
        assert entry["ids"] == {"fix": fix, "sanity": fix_sanity, "qa": fix_qa}
        assert [n["action"] for n in entry["nodes"]] == ["created"] * 3
        assert {t["id"] for t in result["targets"]} == {f"{sprint}.qa1-f1-r1", f"{sprint}.qa1-f2-r1"}

        # Flat: siblings of dev, sanity and qa under the sprint container, the same shape as the sprint group.
        assert ws.deps(fix) == {sprint: "parent-child", qa: "discovered-from"}
        assert ws.deps(fix_sanity) == {sprint: "parent-child", fix: "blocks"}
        assert ws.deps(fix_qa) == {sprint: "parent-child", fix_sanity: "blocks", fix: "validates"}
        meta = ws.show(fix)["metadata"]
        assert {k: meta[k] for k in ("role", "finding_ref", "severity", "reviewer", "remedy", "filed_by", "round", "sprint_bead",
                                     "found_at_commit", "requirements", "adrs", "difficulty")} == {
            "role": "fix", "finding_ref": "qa1-f1", "severity": "blocking", "reviewer": "rbp",
            "remedy": "return the typed error", "filed_by": qa, "round": 1, "sprint_bead": sprint,
            "found_at_commit": "abc1234", "requirements": ["REQ-1"], "adrs": ["NONE"], "difficulty": "normal"}
        assert "Remedy: return the typed error" in ws.show(fix)["description"]
        assert set(ws.show(fix_sanity)["metadata"]) == {"dev_bead", "difficulty", "sc_compose_attach"}
        assert ws.show(fix_sanity)["metadata"]["dev_bead"] == fix
        assert ws.show(fix_qa)["metadata"]["checked_bead"] == fix
        assert [ws.show(b).get("assignee") for b in (fix, fix_sanity, fix_qa)] == [None] * 3
        assert [sorted(l for l in ws.show(b)["labels"] if l.startswith("stage:")) for b in (fix, fix_sanity, fix_qa)] == [
            ["stage:fix"], ["stage:dev-sanity"], ["stage:qa"]]
        assert ws.groups("--validate", "--findings", path)["outcome"] == "succeeded"

        # Idempotent: the same file again creates and changes nothing.
        before = ws.snapshot(*fix_group(sprint, "qa1-f1"), *fix_group(sprint, "qa1-f2"), sprint)
        again = ws.groups("--findings", path)
        assert {n["action"] for t in again["targets"] for n in t["nodes"]} == {"existing"}
        ws.unchanged(before)

        # quality-mgr closes its QA bead only after pouring; a pour after the close is refused.
        ws.close(qa, "FAIL: 2 blocking findings poured")
        late = findings_file(ws, sprint, qa, 1, {"ref": "qa1-f3"})
        assert target(ws.groups("--findings", late, expect=2), f"{sprint}.qa1-f3-r1")["error"]["code"] == "BEAD_GROUPS_QA_CLOSED"
        assert ws.show(fix_group(sprint, "qa1-f3")[0]) is None
        ws.groups("--findings", path)        # re-running an already poured file is still a no-op

        # Ready order fix -> sanity -> qa.
        ready = ws.ready()
        assert fix in ready and fix_sanity not in ready and fix_qa not in ready
        ws.close(fix, "fixed at def456")
        ready = ws.ready()
        assert fix_sanity in ready and fix_qa not in ready

        # Sanity FAIL: dev-sanity reopens the fix and leaves sanity open; sanity is blocked again.
        ws.bd("reopen", fix, "--reason", "sanity FAIL: lint fails", "--actor", "dev-sanity")
        assert ws.show(fix)["status"] == "open" and ws.show(fix_sanity)["status"] == "open"
        ready = ws.ready()
        assert fix in ready and fix_sanity not in ready and fix_qa not in ready
        refused = ws.close(fix_sanity, "PASS", check=False)              # bd refuses to close a bead with an open blocker
        assert refused.returncode != 0 and "blocked" in refused.stderr.lower(), refused.stderr
        assert ws.show(fix_sanity)["status"] == "open"

        # The sprint cannot close while any poured group bead is open.
        assert "open child" in ws.close(sprint, "early", check=False).stderr
        ws.close(fix, "fixed at 789abc")
        ws.close(fix_sanity, "PASS")
        assert fix_qa in ws.ready()
        ws.close(fix_qa, "verified by rbp")
        assert "open child" in ws.close(sprint, "early", check=False).stderr      # qa1-f2's group is still open
        for bead in fix_group(sprint, "qa1-f2"):
            ws.close(bead, "done")
        ws.close(sprint, "all fix groups closed")
        assert ws.show(sprint)["status"] == "closed"


    def test_sanity_split_reads_the_plan_of_poured_dev_and_fix_beads(self):
        ws = self.ws
        sprint = ws.sprint("s", 1)
        ws.bd("update", sprint, "--description", "## Goal\nRetry.\n\n## Deliverables\n1. Add retry\n2. Test 429\n\n## Does Not Close\n- none",
              "--metadata", json.dumps({**ws.show(sprint)["metadata"], "owned_paths": ["crates/types/**"]}))
        dev, _, qa = group(sprint)
        ws.groups("--sprint", sprint)
        fix = fix_group(sprint, "qa1-f1")[0]
        ws.groups("--findings", findings_file(ws, sprint, qa, 1, {"ref": "qa1-f1", "remedy": "return the typed error"}))

        def split(bead):
            proc = ws.run(ws.scripts / "sanity-split", "--task", f"{bead}-sanity", "--bead", bead, "--worktree", ws.root,
                          "--branch", "b", "--commit", "abc1234", "--base", "develop", "--lint-command", "true",
                          "--scratch", ws.root / "scratch", "--split-only")
            manifest = json.loads(proc.stdout)
            return [a["assignment"]["deliverable"]["text"] for a in manifest["assignments"]], manifest["assignments"][0]["assignment"]["owned_paths"]

        assert "## Deliverables" not in (ws.show(dev)["description"] or "")
        assert split(dev) == (["Add retry", "Test 429"], ["crates/types/**"])
        assert split(fix) == (["return the typed error"], ["crates/types/**"])

    def test_pours_before_dispatch_and_a_finding_overrides_difficulty_and_ids(self):
        ws = self.ws
        sprint = ws.sprint("k", 1)      # no assignee anywhere: pouring happens before plan review
        _, _, qa = group(sprint)
        ws.groups("--sprint", sprint)
        path = findings_file(ws, sprint, qa, 1, {"ref": "qa1-f1", "difficulty": "hard", "requirements": ["REQ-9"], "adrs": ["ADR-2"]},
                             {"ref": "qa1-f2", "requirements": []})
        result = ws.groups("--findings", path, expect=2)
        assert target(result, f"{sprint}.qa1-f2-r1")["error"]["code"] == "BEAD_GROUPS_FINDINGS_INVALID"
        fix, fix_sanity, fix_qa = fix_group(sprint, "qa1-f1")
        assert {k: ws.show(fix)["metadata"][k] for k in ("difficulty", "requirements", "adrs")} == {
            "difficulty": "hard", "requirements": ["REQ-9"], "adrs": ["ADR-2"]}
        assert ws.show(fix_qa)["metadata"]["difficulty"] == "hard"
        bad = ws.root / "k-no-commit.json"
        bad.write_text(json.dumps({"sprint": sprint, "round": 1, "filed_by": qa, "findings": []}))
        assert ws.groups("--findings", bad, expect=3)["error"]["code"] == "BEAD_GROUPS_FINDINGS_INVALID"


    def test_only_blocking_findings_are_poured(self):
        ws = self.ws
        sprint = ws.sprint("j", 1)
        _, _, qa = group(sprint)
        ws.groups("--sprint", sprint)
        path = findings_file(ws, sprint, qa, 1, {"ref": "qa1-f1", "severity": "minor"}, {"ref": "QA1_F2"}, {"ref": "qa1-f3"})
        result = ws.groups("--findings", path, expect=2)
        assert target(result, f"{sprint}.qa1-f1-r1")["error"]["code"] == "BEAD_GROUPS_NOT_BLOCKING"
        assert target(result, f"{sprint}.QA1_F2-r1")["error"]["code"] == "BEAD_GROUPS_FINDINGS_INVALID"
        assert target(result, f"{sprint}.qa1-f3-r1")["nodes"][0]["action"] == "created"   # the valid finding still pours
        assert ws.show(fix_group(sprint, "qa1-f1")[0]) is None

    def render(self, template: Path, values: dict) -> dict:
        path = self.ws.root / f"vars-{values['id']}.json"
        path.write_text(json.dumps(values))
        out = self.ws.run("sc-compose", "render", "--file", template, "--var-file", path, "--strict").stdout
        return json.loads(out)

    def test_an_assigned_important_finding_gets_its_sanity_and_qa_beads(self):
        """F1: assigning an important finding creates its sanity and QA beads in one import, finding <- sanity <- qa."""
        ws = self.ws
        sprint = ws.sprint("m", 1)
        _, _, sprint_qa = group(sprint)
        ws.groups("--sprint", sprint)
        ws.bd("create", "phase m", "--id", "t-phase-m", "--type", "epic", "--silent")
        templates = ws.root / ".claude/skills/atm-bd-orchestration/templates"
        example = json.loads((templates.parent / "examples/finding-bead-vars.json").read_text())
        finding = "t-m-1-qa1-f1"
        rows = [self.render(templates / "finding-bead.json.j2", {
            **example, "id": finding, "qa_bead": sprint_qa, "phase": "m", "sprint": "m-1", "stack": "phase-m",
            "parent": "t-phase-m", "sprint_bead": sprint})]
        found = ws.root / "finding.jsonl"
        found.write_text("".join(json.dumps(r) + "\n" for r in rows))
        ws.bd("import", "-i", found)
        # The assignment: one import of both beads.
        sanity, qa = f"{finding}-sanity", f"{finding}-qa"
        rows = [self.render(ws.root / ".claude/skills/atm-beads/templates/dev-sanity-bead.json.j2", {
                    "id": sanity, "parent": "t-phase-m", "dev_bead": finding, "phase": "m", "sprint": "m-1", "stack": "phase-m"}),
                self.render(templates / "qa-bead.json.j2", {
                    "id": qa, "checked_bead": finding, "parent": "t-phase-m", "blocked_by": sanity, "phase": "m", "sprint": "m-1",
                    "stack": "phase-m", "round": 1, "qa_member": "quality-mgr"})]
        assigned = ws.root / "assigned.jsonl"
        assigned.write_text("".join(json.dumps(r) + "\n" for r in rows))
        ws.bd("import", "-i", assigned)
        # Its parent is the finding's parent: bd keeps one edge type per pair, so a child of the finding would lose `blocks`.
        assert ws.deps(sanity) == {"t-phase-m": "parent-child", finding: "blocks"}, ws.deps(sanity)
        assert ws.deps(qa) == {"t-phase-m": "parent-child", sanity: "blocks"}, ws.deps(qa)
        assert ws.show(qa)["metadata"]["checked_bead"] == finding
        ws.bd("update", finding, "--status", "in_progress", "--assignee", "pour-test")
        assert not {sanity, qa} & ws.ready()
        ws.close(finding, "fixed at abc1234")
        assert sanity in ws.ready() and qa not in ws.ready()
        ws.close(sanity, "PASS at abc1234")
        assert qa in ws.ready()


if __name__ == "__main__":
    unittest.main()
