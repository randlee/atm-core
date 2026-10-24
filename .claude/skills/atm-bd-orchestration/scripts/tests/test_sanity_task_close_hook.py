from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys

import pytest

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import bd_cache
import sanity_task_close_hook as hook
from test_bd_cache import FakeBd, workspace


class World:
    """Real hook and real bd-cache over a deterministic bd; ATM calls are recorded."""

    def __init__(self, tmp_path, monkeypatch):
        self.root = workspace(tmp_path)
        monkeypatch.chdir(tmp_path)
        monkeypatch.delenv("BEADS_DIR", raising=False)
        monkeypatch.setenv("ATM_IDENTITY", "test-sanity")
        (tmp_path / ".atm-bd").mkdir()
        (tmp_path / ".atm-bd/phase-test.toml").write_text('lead = "test-lead"\n')
        self.task = "atm-test-1.sanity"
        self.dev = "atm-test-1.dev"
        self.sha = "a" * 40
        self.bd = FakeBd()
        self.bd.add(self.task, status="in_progress", assignee="test-sanity", labels=["phase-test", "stage:dev-sanity"],
                    metadata={"dev_bead": self.dev, "commit": self.sha})
        self.bd.edge(self.task, self.dev, "blocks")
        self.bd.add(self.dev, status="closed", assignee="test-dev", labels=["phase-test", "stage:dev"],
                    metadata={"commit": self.sha})
        self.histories = {self.task: [{"event": "assigned", "assignee": "test-sanity", "message_id": "assign-s", "to_state": "assigned"}],
                          self.dev: [{"event": "assigned", "assignee": "test-dev", "message_id": "assign-d", "to_state": "assigned"}]}
        self.values = {"task_id": self.task, "checked_bead": self.dev, "commit": self.sha,
                       "reviewer": "sanity-selected", "run_id": "run-1", "verdict": "PASS", "findings": []}
        self.request = {"schema_version": "1.0.0", "hook": "task.close", "team": "test-team", "phase": "test",
                        "caller": {"identity": "test-sanity"}, "custom": {},
                        "request": {"task_id": self.task, "task_op": {"op": "close", "outcome": "completed"},
                                    "message_source": {"Template": {"var_file_values": self.values}}}}
        self.commands, self.spawned = [], []
        self.fail = None
        self.spawn_error = None

    @property
    def beads(self):
        return self.bd.rows

    def run(self, command, **kwargs):
        self.commands.append(command)
        if self.fail and self.fail(command):
            return subprocess.CompletedProcess(command, 1, "", "Dolt unavailable")
        if command[0] == "bd":
            return self.bd(command, **kwargs)
        if command[:3] == ["atm", "task", "events"]:
            output = {"events": self.histories[command[3]]}
        elif command[:2] == ["atm", "send"] or command[:3] == ["atm", "task", "assign"]:
            output = {"message_id": "test-message"}
        else:
            raise AssertionError(command)
        return subprocess.CompletedProcess(command, 0, json.dumps(output), "")

    def spawn(self, command, **kwargs):
        if self.spawn_error:
            raise self.spawn_error
        assert kwargs["stderr"].name.endswith(".bd-cache.drain.log") and kwargs["start_new_session"]
        self.spawned.append(command)

    def invoke(self, runner=None, clock=lambda: 0):
        return hook.hook(self.request, runner=runner or self.run, clock=clock, spawn=self.spawn)

    def cache(self):
        return bd_cache.BdCache(self.root, runner=self.bd)

    def journal(self):
        return [(row["bead"], row["op"], json.loads(row["args_json"]), row["state"])
                for row in self.cache().db.execute("SELECT * FROM journal ORDER BY seq")]

    def bd_writes(self):
        return [c for c in self.commands if c[0] == "bd" and c[1] in ("close", "update", "dep")]

    def warm(self):
        cache = self.cache()
        cache.get(self.task)
        cache.get(self.dev)


@pytest.fixture
def world(tmp_path, monkeypatch):
    return World(tmp_path, monkeypatch)


def test_pass_journals_the_close_starts_a_drain_and_returns_dev_and_sanity_closes(world):
    result = world.invoke()
    assert world.journal() == [(world.task, "close", {"reason": f"PASS at {world.sha}"}, "pending")]
    assert not world.bd_writes()  # the hook never writes bd itself
    assert world.spawned and world.spawned[0][-1] == "drain" and world.spawned[0][1].endswith("bd_cache.py")
    assert [(a["command"], a["request"]["task_id"]) for a in result["atm-action"]] == [
        ("task.close", world.dev), ("task.close", world.task)]
    assert result["atm-action"][0]["as"] == "test-dev"
    assert not any(c[:3] == ["atm", "task", "close"] for c in world.commands)
    assert world.cache().drain()["applied"] == [1]
    assert world.beads[world.task]["status"] == "closed"
    assert world.beads[world.task]["close_reason"] == f"PASS at {world.sha}"


def test_reads_are_one_batched_list_and_no_show(world):
    world.invoke()
    reads = [c for c in world.commands if c[0] == "bd"]
    assert [c[1] for c in reads] == ["list", "list"]  # sanity miss, then the batched refresh
    assert reads[1][reads[1].index("--id") + 1] == f"{world.task},{world.dev}"


def test_warm_cache_routes_without_bd_and_refreshes_once(world):
    world.warm()
    world.invoke()
    assert [c[1] for c in world.commands if c[0] == "bd"] == ["list"]


def test_pass_skips_already_completed_dev_task(world):
    world.histories[world.dev].append({"to_state": "complete"})
    assert len(world.invoke()["atm-action"]) == 1


def test_fail_reopens_dev_leaves_sanity_blocked_and_notifies_dev_before_close(world):
    world.values.update(verdict="FAIL", findings=[{"deliverable": 2, "issue": "missing behavior"}])
    before = list(world.beads[world.task]["dependencies"])
    actions = world.invoke()["atm-action"]
    assert world.journal() == [(world.dev, "update", {"status": "open"}, "pending"),
                               (world.task, "update", {"status": "open", "assignee": ""}, "pending")]
    view = world.cache()
    assert view.get(world.dev)["status"] == "open" and view.get(world.task)["assignee"] == ""
    assert [a["command"] for a in actions] == ["send", "task.close"]
    assert actions[0]["request"]["to"] == {"agent": "test-dev", "team": "test-team"}
    assert actions[0]["request"]["message_source"]["Template"]["var_file_values"]["findings"] == world.values["findings"]
    assert world.cache().drain()["applied"] == [1, 2]
    assert world.beads[world.dev]["status"] == "open"
    assert (world.beads[world.task]["status"], world.beads[world.task]["assignee"]) == ("open", "")
    assert world.beads[world.task]["dependencies"] == before


def test_reassigned_dev_before_drain_escalates_instead_of_reopening(world):
    world.values.update(verdict="FAIL", findings=[{"deliverable": 1, "issue": "missing"}])
    world.invoke()
    world.beads[world.dev]["assignee"] = "another-dev"
    world.bd.touch(world.dev)
    report = world.cache().drain()
    assert [e["seq"] for e in report["escalated"]] == [1]
    assert world.beads[world.dev]["status"] == "closed"


@pytest.mark.parametrize("role", ["dev", "fix", "qa"])
def test_other_roles_pass_through(world, role):
    world.beads[world.task]["labels"] = ["phase-test", f"stage:{role}"]
    assert world.invoke() == {"result": "pass-through"}
    assert not world.journal()


@pytest.mark.parametrize("field,value", [("task_id", "other"), ("checked_bead", "other"),
                                        ("reviewer", "sanity-llm"), ("run_id", ""), ("verdict", "oops"),
                                        ("findings", "bad")])
def test_invalid_results_refuse_without_writes_and_escalate_once(world, field, value):
    world.values[field] = value
    for _ in range(2):
        reply = world.invoke()
        assert reply["result"] == "refused"
        assert field in reply["message"]
    assert not world.journal() and not world.spawned
    assert len([c for c in world.commands if c[:3] == ["atm", "task", "assign"]]) == 1


def test_missing_dependency_is_owned_by_lead(world):
    world.beads[world.task]["dependencies"] = []
    reply = world.invoke()
    assert json.loads(reply["message"])["owner"] == "phase-lead"
    assert not world.journal()


def test_legacy_cannot_run_escalates_without_transition(world):
    world.values["verdict"] = "CANNOT_RUN"
    reply = world.invoke()
    error = json.loads(reply["message"])
    assert (reply["result"], error["code"], error["owner"]) == ("refused", "SANITY_CLOSE.LEGACY_VERDICT", "phase-lead")
    assert not world.journal()
    assert "atm-action" not in reply
    assert any(command[:3] == ["atm", "task", "assign"] for command in world.commands)


def test_bd_down_with_warm_cache_decides_on_cached_rows_and_says_so(world, capsys):
    world.warm()
    world.fail = lambda c: c[0] == "bd"
    assert world.invoke()["result"] == "handled"
    assert world.journal() == [(world.task, "close", {"reason": f"PASS at {world.sha}"}, "pending")]
    assert "BD_CACHE.BD_LIST_FAILED" in capsys.readouterr().err


def test_bd_down_with_cold_cache_refuses_and_escalates(world):
    world.fail = lambda c: c[0] == "bd"
    reply = world.invoke()
    assert reply["result"] == "refused"
    assert json.loads(reply["message"])["code"] == "BD_CACHE.BD_LIST_FAILED"
    assert not world.journal()
    assert any(c[:4] == ["atm", "task", "assign", "test-lead"] for c in world.commands)


def test_refusal_on_cached_rows_names_the_failed_refresh(world):
    world.warm()
    world.beads[world.dev]["status"] = "open"  # bd has it reopened, cache does not: still closed
    world.values.update(verdict="oops")  # refused after the refresh
    world.fail = lambda c: c[0] == "bd"
    error = json.loads(world.invoke()["message"])
    assert error["read_fallback"].startswith("BD_CACHE.BD_LIST_FAILED")


def test_fresh_rows_win_over_a_stale_cache(world):
    world.warm()
    world.beads[world.dev]["status"] = "open"
    world.bd.touch(world.dev)
    assert world.invoke()["result"] == "refused"
    assert not world.journal()


def test_repeat_close_does_not_journal_twice(world):
    world.invoke()
    world.invoke()
    assert len(world.journal()) == 1


def test_drain_spawn_failure_is_logged_and_the_close_still_handled(world, capsys):
    world.spawn_error = OSError("no fork")
    assert world.invoke()["result"] == "handled"
    assert "no fork" in capsys.readouterr().err
    assert world.cache().drain()["applied"] == [1]  # the cron drain applies it


def test_journal_conflict_at_write_refuses_and_escalates(world, monkeypatch):
    def conflict(self, *args, **kwargs):
        raise bd_cache.CacheError("BD_CACHE.PRECONDITION", "assignee changed", "re-read and decide again")
    monkeypatch.setattr(bd_cache.BdCache, "write", conflict)
    reply = world.invoke()
    assert json.loads(reply["message"])["code"] == "BD_CACHE.PRECONDITION"
    assert not world.spawned


def test_panic_after_partial_journal_reports_progress(world, monkeypatch):
    world.values.update(verdict="FAIL", findings=[{"deliverable": 1, "issue": "missing"}])
    real = bd_cache.BdCache.write
    def second_panics(self, bead, *args, **kwargs):
        if bead == world.task:
            raise RuntimeError("controlled panic")
        return real(self, bead, *args, **kwargs)
    monkeypatch.setattr(bd_cache.BdCache, "write", second_panics)
    error = json.loads(world.invoke()["message"])
    assert error["code"] == "SANITY_CLOSE.INTERNAL"
    assert error["progress"] == {"journaled": [1], "last_attempted_write": [world.task, "update"]}
    assert error["notified"]


def test_dry_run_does_not_journal_or_send(world):
    world.request["request"]["dry_run"] = True
    assert world.invoke()["result"] == "handled"
    assert not world.journal() and not world.spawned
    assert not any(c[:2] == ["atm", "send"] for c in world.commands)


def test_failed_escalation_is_retained_for_doctor(world):
    world.values["verdict"] = "bad"
    world.fail = lambda c: c[:3] == ["atm", "task", "assign"]
    assert world.invoke()["result"] == "refused"
    assert hook.doctor(world.request)["findings"][0]["code"] == "SANITY_CLOSE.ESCALATION_PENDING"


def test_task_lock_contention_refuses_without_writes(world):
    with hook.task_lock(world.root, "test-team", world.task):
        result = world.invoke()
    assert result["result"] == "refused" and "BUSY" in result["message"]
    assert not world.journal()


def test_nonphase_and_nonclose_calls_do_not_query_beads(world):
    world.request["request"]["task_id"] = "outside-task"
    assert world.invoke()["result"] == "not-applicable"
    world.request["hook"] = "task.start"
    assert world.invoke()["result"] == "not-applicable"
    assert not world.commands


def test_conflicting_repeat_verdict_rejected(world):
    world.invoke()
    world.values.update(verdict="FAIL", findings=[{"deliverable": 1, "issue": "changed verdict"}])
    assert world.invoke()["result"] == "refused"
    assert len(world.journal()) == 1


def test_new_templates_render_with_real_composer(world):
    sys.path.insert(0, str(SCRIPTS.parents[1] / "atm-beads/scripts"))
    import sc_render
    for name in ("sanity-task-close.md.j2", "sanity-failed-template.md.j2", "sanity-dev-close.md.j2"):
        assert sc_render.render(hook.TEMPLATES / name, world.values)


def test_rollout_other_agents_never_touch_dolt(world):
    world.request["custom"]["sanity_agents"] = ["test-sanity"]
    world.request["caller"]["identity"] = "test-dev"
    world.fail = lambda _: True
    assert world.invoke() == {"result": "pass-through"}
    assert world.commands == []


def test_pass_on_reopened_dev_is_validation_failure(world):
    world.beads[world.dev]["status"] = "open"
    assert world.invoke()["result"] == "refused"
    assert not world.journal()


def test_captured_var_precedence_matches_daemon(world):
    source = world.request["request"]["message_source"]["Template"]
    source["environment_values"] = {"commit": "stale-env-sha"}
    assert hook.selected_values(world.request["request"])["commit"] == world.sha
    source["explicit_values"] = {"commit": "explicit"}
    assert hook.selected_values(world.request["request"])["commit"] == "explicit"


def test_direct_finding_fix_requires_matching_sanity_link(world):
    world.beads[world.dev]["labels"] = ["phase-test", "stage:finding"]
    assert world.invoke()["result"] == "refused"
    assert not world.journal()
    world.beads[world.dev]["metadata"]["sanity_bead"] = world.task
    world.bd.touch(world.dev)
    assert world.invoke()["result"] == "handled"


@pytest.mark.parametrize("owner", ["sanity", "phase-lead", "hook-maintainer"])
def test_all_escalations_route_to_phase_lead(world, owner):
    rt = hook.Runtime(world.request, runner=world.run, clock=lambda: 0)
    reply = rt.validation_error(hook.Failure("TEST.FAILURE", "controlled failure", owner))
    assert json.loads(reply["message"])["escalation_to"] == "test-lead"
    send = next(command for command in world.commands if command[:3] == ["atm", "task", "assign"])
    assert send[3] == "test-lead"


def test_commit_metadata_is_not_a_pin(world):
    world.beads[world.task]["metadata"].pop("commit")
    world.beads[world.dev]["metadata"]["commit"] = "implementation-tweaked"
    world.values.pop("commit")
    assert world.invoke()["result"] == "handled"


def test_escalation_assignment_renders_and_is_sent_once(world):
    sys.path.insert(0, str(SCRIPTS.parents[1] / "atm-beads/scripts"))
    import sc_render
    assigned = []
    def capture(command, **kwargs):
        if command[:3] == ["atm", "task", "assign"]:
            values = json.loads(Path(command[command.index("--vars") + 1]).read_text())
            assert sc_render.render(Path(command[command.index("--template") + 1]), values)
            assert values["original_task"] == world.task
            assert values["task_id"].startswith(world.task + ".close-error-")
            assigned.append(values["task_id"])
        return world.run(command, **kwargs)
    world.values["verdict"] = "bad"
    for _ in range(2):
        assert world.invoke(runner=capture)["result"] == "refused"
    assert len(assigned) == 1


def test_dev_relationship_resolves_without_copied_result_field(world):
    world.values.pop("checked_bead")
    assert world.invoke()["result"] == "handled"


def test_exhausted_work_budget_retains_bounded_escalation_window(world):
    now = [0.0]
    def expire(command, **kwargs):
        if command[0] == "bd":
            now[0] = 1.8
            raise subprocess.TimeoutExpired(command, kwargs["timeout"])
        assert command[:3] == ["atm", "task", "assign"]
        assert 0 < kwargs["timeout"] <= 0.5
        return world.run(command, **kwargs)
    result = world.invoke(runner=expire, clock=lambda: now[0])
    assert json.loads(result["message"])["notified"]


def test_bd_reads_use_the_remaining_work_deadline(world):
    budgets = []
    def record(command, **kwargs):
        if command[0] == "bd":
            budgets.append(kwargs["timeout"])
        return world.run(command, **kwargs)
    assert world.invoke(runner=record, clock=lambda: 0.0)["result"] == "handled"
    assert budgets == [pytest.approx(1.8), pytest.approx(1.3)]  # miss read, refresh leaving reply time


def test_reassigned_sanity_before_drain_escalates_instead_of_reopening(world):
    world.values.update(verdict="FAIL", findings=[{"deliverable": 1, "issue": "missing"}])
    world.invoke()
    world.beads[world.task]["assignee"] = "other-sanity"
    world.bd.touch(world.task)
    report = world.cache().drain()
    assert [e["seq"] for e in report["escalated"]] == [2]
    assert world.beads[world.task]["assignee"] == "other-sanity"


def test_retried_close_does_not_requeue_an_escalated_write(world):
    world.invoke()
    world.bd.fail = [(lambda c: c[1] == "close", 1)] * bd_cache.MAX_ATTEMPTS
    for _ in range(bd_cache.MAX_ATTEMPTS):
        world.cache().drain()
    assert world.journal()[0][3] == "escalated"
    world.invoke()
    assert len(world.journal()) == 1


def test_dry_run_refusal_writes_no_journal_record_and_sends_nothing(world):
    world.request["request"]["dry_run"] = True
    world.values["verdict"] = "oops"
    assert world.invoke()["result"] == "refused"
    assert not (world.root / ".atm-bd/sanity-close/journal.sqlite3").exists()
    assert not any(c[:3] == ["atm", "task", "assign"] for c in world.commands)
