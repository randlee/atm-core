from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
from typing import Any
import subprocess
import sys

import pytest

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
import sanity_close_legacy as hook

SPEC = importlib.util.spec_from_file_location("sanity_close_repair", SCRIPTS / "sanity-close-repair.py")
assert SPEC is not None and SPEC.loader is not None
repair = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(repair)


class World:
    def __init__(self, tmp_path, monkeypatch):
        self.root = tmp_path
        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("ATM_IDENTITY", "test-sanity")
        (tmp_path / ".atm-bd").mkdir()
        (tmp_path / ".atm-bd/phase-test.toml").write_text('lead = "test-lead"\n')
        self.task = "atm-test-1.sanity"
        self.dev = "atm-test-1.dev"
        self.sha = "a" * 40
        self.beads = {
            self.task: {"id": self.task, "status": "in_progress", "assignee": "test-sanity",
                        "labels": ["stage:dev-sanity"], "metadata": {"dev_bead": self.dev, "commit": self.sha},
                        "dependencies": [{"id": self.dev, "dependency_type": "blocks"}]},
            self.dev: {"id": self.dev, "status": "closed", "assignee": "test-dev",
                       "labels": ["stage:dev"], "metadata": {"commit": self.sha}},
        }
        self.histories = {self.task: [{"event": "assigned", "assignee": "test-sanity", "message_id": "assign-s", "to_state": "assigned"}],
                          self.dev: [{"event": "assigned", "assignee": "test-dev", "message_id": "assign-d", "to_state": "assigned"}]}
        self.values = {"task_id": self.task, "checked_bead": self.dev, "commit": self.sha,
                       "reviewer": "sanity-selected", "run_id": "run-1", "verdict": "PASS", "findings": []}
        self.request = {"schema_version": "1.0.0", "hook": "task.close", "team": "test-team", "phase": "test",
                        "caller": {"identity": "test-sanity"}, "custom": {},
                        "request": {"task_id": self.task, "task_op": {"op": "close", "outcome": "completed"},
                                    "message_source": {"Template": {"var_file_values": self.values}}}}
        self.commands = []
        self.fail = None

    def run(self, command, **kwargs):
        self.commands.append(command)
        if self.fail and self.fail(command):
            return subprocess.CompletedProcess(command, 1, "", "Dolt unavailable")
        if command[:2] == ["bd", "show"]:
            output = [self.beads[command[2]]]
        elif command[:3] == ["atm", "task", "events"]:
            output = {"events": self.histories[command[3]]}
        elif command[:2] == ["bd", "close"]:
            self.beads[command[2]]["status"] = "closed"
            output = {}
        elif command[:2] == ["bd", "update"]:
            bead = self.beads[command[2]]
            bead["status"] = command[command.index("--status") + 1]
            if "--assignee" in command:
                bead["assignee"] = command[command.index("--assignee") + 1]
            output = {}
        elif command[:2] == ["atm", "send"] or command[:3] == ["atm", "task", "assign"]:
            output = {"message_id": "test-message"}
        elif command[:3] == ["atm", "task", "close"]:
            self.histories[command[3]].append({"event": "completed", "to_state": "complete"})
            output = {}
        else:
            raise AssertionError(command)
        return subprocess.CompletedProcess(command, 0, json.dumps(output), "")

    def invoke(self):
        return hook.hook(self.request, runner=self.run, clock=lambda: 0)

    def writes(self):
        return [command for command in self.commands if command[:2] in (["bd", "close"], ["bd", "update"])]


@pytest.fixture
def world(tmp_path, monkeypatch):
    return World(tmp_path, monkeypatch)


def test_pass_closes_bead_then_returns_unhooked_dev_and_sanity_closes(world):
    result = world.invoke()
    assert world.beads[world.task]["status"] == "closed"
    assert [(a["command"], a["request"]["task_id"]) for a in result["atm-action"]] == [
        ("task.close", world.dev), ("task.close", world.task)]
    assert result["atm-action"][0]["as"] == "test-dev"
    assert not any(c[:3] == ["atm", "task", "close"] for c in world.commands)


def test_pass_skips_already_completed_dev_task(world):
    world.histories[world.dev].append({"to_state": "complete"})
    assert len(world.invoke()["atm-action"]) == 1


def test_fail_reopens_dev_leaves_sanity_blocked_and_notifies_dev_before_close(world):
    world.values.update(verdict="FAIL", findings=[{"deliverable": 2, "issue": "missing behavior"}])
    before = copy.deepcopy(world.beads[world.task]["dependencies"])
    result = world.invoke()
    assert world.beads[world.dev]["status"] == "open"
    assert world.beads[world.task]["status"] == "open"
    assert world.beads[world.task]["assignee"] == ""
    assert world.beads[world.task]["dependencies"] == before
    actions = result["atm-action"]
    assert [a["command"] for a in actions] == ["send", "task.close"]
    assert actions[0]["request"]["to"] == {"agent": "test-dev", "team": "test-team"}
    assert actions[0]["request"]["message_source"]["Template"]["var_file_values"]["findings"] == world.values["findings"]


@pytest.mark.parametrize("role", ["dev", "fix", "qa"])
def test_other_roles_pass_through(world, role):
    world.beads[world.task]["labels"] = [f"stage:{role}"]
    assert world.invoke() == {"result": "pass-through"}
    assert not world.writes()


@pytest.mark.parametrize("field,value", [("task_id", "other"), ("checked_bead", "other"),
                                        ("reviewer", "sanity-llm"), ("run_id", ""), ("verdict", "oops"),
                                        ("findings", "bad")])
def test_invalid_results_refuse_without_mutations_and_escalate_once(world, field, value):
    world.values[field] = value
    for _ in range(2):
        reply = world.invoke()
        assert reply["result"] == "refused"
        assert field in reply["message"]
        assert not world.writes()
    assert len([c for c in world.commands if c[:3] == ["atm", "task", "assign"]]) == 1


def test_missing_dependency_is_owned_by_lead(world):
    world.beads[world.task]["dependencies"] = []
    reply = world.invoke()
    assert json.loads(reply["message"])["owner"] == "phase-lead"
    assert not world.writes()


def test_legacy_cannot_run_escalates_without_transition(world):
    world.values["verdict"] = "CANNOT_RUN"
    reply = world.invoke()
    assert reply["result"] == "refused"
    error = json.loads(reply["message"])
    assert error["code"] == "SANITY_CLOSE.LEGACY_VERDICT"
    assert error["owner"] == "phase-lead"
    assert not world.writes()
    assert "atm-action" not in reply
    assert any(command[:3] == ["atm", "task", "assign"] for command in world.commands)


def test_bd_close_failure_assigns_repair_then_closes_original(world):
    world.fail = lambda c: c[:2] == ["bd", "close"]
    reply = world.invoke()
    actions = reply["atm-action"]
    assert [a["command"] for a in actions] == ["task.assign", "task.close"]
    assert actions[0]["request"]["to"] == {"agent": "test-sanity", "team": "test-team"}
    assert actions[0]["as"] == "test-lead"
    assert actions[1]["request"]["task_id"] == world.task
    assert "deferred" in actions[1]["request"]["task_op"]["reason"]
    assert len(world.writes()) == 1  # no hook retry loop
    variables = actions[0]["request"]["message_source"]["Template"]["var_file_values"]
    assert "Dolt unavailable" in str(variables["failure"])
    world.fail = None
    repaired = repair.repair(Path(variables["journal"]), variables["key"], runner=world.run)
    assert repaired["repaired"]
    assert world.beads[world.task]["status"] == "closed"
    assert world.histories[world.dev][-1]["to_state"] == "complete"
    count = len(world.commands)
    repair.repair(Path(variables["journal"]), variables["key"], runner=world.run)
    assert len(world.commands) == count


def test_configurable_repair_agent(world):
    world.request["custom"]["repair_agent"] = "test-bd-close"
    world.fail = lambda c: c[:2] == ["bd", "close"]
    assert world.invoke()["atm-action"][0]["request"]["to"] == {"agent": "test-bd-close", "team": "test-team"}


def test_partial_failure_does_not_lose_successful_dev_reopen(world):
    world.values.update(verdict="FAIL", findings=[{"deliverable": 0, "issue": "lint failed"}])
    world.fail = lambda c: c[:3] == ["bd", "update", world.task]
    actions = world.invoke()["atm-action"]
    assert world.beads[world.dev]["status"] == "open"
    variables = actions[0]["request"]["message_source"]["Template"]["var_file_values"]
    world.fail = None
    world.commands.clear()
    repair.repair(Path(variables["journal"]), variables["key"], runner=world.run)
    assert not any(c[:3] == ["bd", "update", world.dev] for c in world.commands)
    assert any(c[:2] == ["atm", "send"] for c in world.commands)


def test_dry_run_does_not_mutate_beads_or_send(world):
    world.request["request"]["dry_run"] = True
    assert world.invoke()["result"] == "handled"
    assert not world.writes()
    assert not any(c[:2] == ["atm", "send"] for c in world.commands)


def test_failed_escalation_is_retained_for_doctor(world):
    world.values["verdict"] = "bad"
    world.fail = lambda c: c[:3] == ["atm", "task", "assign"]
    assert world.invoke()["result"] == "refused"
    findings = hook.doctor(world.request)["findings"]
    assert findings[0]["code"] == "SANITY_CLOSE.ESCALATION_PENDING"


def test_timeout_after_write_transfers_uncertain_state_and_repair_reads_it(world):
    original_run = world.run
    def run(command, **kwargs):
        result = original_run(command, **kwargs)
        if command[:2] == ["bd", "close"]:
            raise subprocess.TimeoutExpired(command, kwargs["timeout"])
        return result
    reply: Any = hook.hook(world.request, runner=run, clock=lambda: 0)
    assert world.beads[world.task]["status"] == "closed"
    variables = reply["atm-action"][0]["request"]["message_source"]["Template"]["var_file_values"]
    world.commands.clear()
    repair.repair(Path(variables["journal"]), variables["key"], runner=world.run)
    assert not any(c[:2] == ["bd", "close"] for c in world.commands)


def test_repair_rejects_new_assignment_without_writes(world):
    world.fail = lambda c: c[:2] == ["bd", "close"]
    variables = world.invoke()["atm-action"][0]["request"]["message_source"]["Template"]["var_file_values"]
    world.histories[world.task].append({"event": "reassigned", "assignee": "test-sanity", "message_id": "new"})
    world.fail = None
    world.commands.clear()
    with pytest.raises(hook.Failure, match="reassigned"):
        repair.repair(Path(variables["journal"]), variables["key"], runner=world.run)
    assert not world.writes()


def test_task_lock_contention_refuses_without_mutation(world):
    with hook.task_lock(world.root, "test-team", world.task):
        result = world.invoke()
    assert result["result"] == "refused"
    assert "BUSY" in result["message"]
    assert not world.writes()


def test_nonphase_and_nonclose_calls_do_not_query_beads(world):
    world.request["request"]["task_id"] = "outside-task"
    assert world.invoke()["result"] == "not-applicable"
    world.request["hook"] = "task.start"
    assert world.invoke()["result"] == "not-applicable"
    assert not world.commands


def test_conflicting_repeat_verdict_rejected(world):
    world.invoke()
    world.commands.clear()
    world.values.update(verdict="FAIL", findings=[{"deliverable": 1, "issue": "changed verdict"}])
    assert world.invoke()["result"] == "refused"
    assert not world.writes()


def test_new_templates_render_with_real_composer(world):
    # Verify actual daemon-compatible template contracts, not a second renderer.
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
    assert not world.writes()


def test_captured_var_precedence_matches_daemon(world):
    source = world.request["request"]["message_source"]["Template"]
    source["environment_values"] = {"commit": "stale-env-sha"}
    assert hook.selected_values(world.request["request"])["commit"] == world.sha
    source["explicit_values"] = {"commit": "explicit"}
    assert hook.selected_values(world.request["request"])["commit"] == "explicit"


def test_direct_finding_fix_requires_matching_sanity_link(world):
    world.beads[world.dev]["labels"] = ["stage:finding"]
    assert world.invoke()["result"] == "refused"
    assert not world.writes()
    world.beads[world.dev]["metadata"]["sanity_bead"] = world.task
    assert world.invoke()["result"] == "handled"


@pytest.mark.parametrize("repair_owner", ["sanity", "phase-lead", "hook-maintainer"])
def test_all_escalations_route_to_phase_lead(world, repair_owner):
    rt = hook.Runtime(world.request, runner=world.run, clock=lambda: 0)
    reply = rt.validation_error(hook.Failure("TEST.FAILURE", "controlled failure", repair_owner))
    assert json.loads(reply["message"])["escalation_to"] == "test-lead"
    send = next(command for command in world.commands if command[:3] == ["atm", "task", "assign"])
    assert send[3] == "test-lead"


def test_repair_reads_use_repair_timeout_not_hook_timeout(world):
    world.fail = lambda c: c[:2] == ["bd", "close"]
    reply = world.invoke()
    variables = reply["atm-action"][0]["request"]["message_source"]["Template"]["var_file_values"]
    world.fail = None
    reads = []
    def runner(command, **kwargs):
        if command[:2] == ["bd", "show"] or command[:3] == ["atm", "task", "events"]:
            reads.append(command)
            if kwargs["timeout"] <= 0.5:
                raise subprocess.TimeoutExpired(command, kwargs["timeout"])
            assert kwargs["timeout"] <= 30
        return world.run(command, **kwargs)
    result = repair.repair(Path(variables["journal"]), variables["key"], runner=runner)
    assert reads
    assert result["repaired"]
    assert world.beads[world.task]["status"] == "closed"
    assert hook.Runtime(world.request).command_timeout is None


def test_commit_metadata_is_not_a_pin(world):
    world.beads[world.task]["metadata"].pop("commit")
    world.beads[world.dev]["metadata"]["commit"] = "implementation-tweaked"
    world.values.pop("commit")
    assert world.invoke()["result"] == "handled"


def deferred(world):
    world.fail = lambda c: c[:2] == ["bd", "close"]
    variables = world.invoke()["atm-action"][0]["request"]["message_source"]["Template"]["var_file_values"]
    world.fail = None
    return variables


def test_repair_close_refused_until_successful(world):
    variables = deferred(world)
    original = world.request["request"]["task_id"]
    world.request["request"]["task_id"] = variables["task_id"]
    assert world.invoke()["result"] == "refused"
    world.beads[original]["metadata"].pop("commit")
    world.beads[world.dev]["metadata"]["commit"] = "tweak"
    repair.repair(Path(variables["journal"]), variables["key"], runner=world.run)
    assert world.invoke()["result"] == "pass-through"


def test_repair_failure_automatically_escalates_and_retains_obligation(world):
    variables = deferred(world)
    world.commands.clear()
    world.fail = lambda c: c[:2] == ["bd", "close"]
    with pytest.raises(hook.Failure):
        repair.repair(Path(variables["journal"]), variables["key"], runner=world.run)
    assert any(c[:4] == ["atm", "task", "assign", "test-lead"] for c in world.commands)
    state = hook.Journal(Path(variables["journal"]))
    assert not state.get(variables["key"]).get("repaired")
    errors = [json.loads(raw) for key, raw in state.db.execute("SELECT key,value FROM records") if key.startswith("error:")]
    assert errors[-1]["progress"]["last_attempted_command"][:2] == ["bd", "close"]
    assert "Keep the repair task open" in errors[-1]["recovery"]


def test_panic_after_partial_write_reports_uncertainty(world):
    world.values.update(verdict="FAIL", findings=[{"deliverable": 1, "issue": "missing"}])
    original = world.run
    def panic(command, **kwargs):
        if command[:3] == ["bd", "update", world.task]:
            raise RuntimeError("controlled panic")
        return original(command, **kwargs)
    result = hook.hook(world.request, runner=panic, clock=lambda: 0)
    error = json.loads(result["message"])
    assert error["code"] == "SANITY_CLOSE.INTERNAL"
    assert error["progress"]["completed_commands"][0][2] == world.dev
    assert error["progress"]["last_attempted_command"][2] == world.task
    assert error["notified"]
    assert world.beads[world.dev]["status"] == "open"


def test_escalation_assignment_renders_and_retains_same_obligation(world):
    sys.path.insert(0, str(SCRIPTS.parents[1] / "atm-beads/scripts"))
    import sc_render
    assigned = []
    original = world.run
    def capture(command, **kwargs):
        if command[:3] == ["atm", "task", "assign"]:
            values = json.loads(Path(command[command.index("--vars") + 1]).read_text())
            assert sc_render.render(Path(command[command.index("--template") + 1]), values)
            assert values["original_task"] == world.task
            assert values["task_id"].startswith(world.task + ".close-error-")
            assigned.append(values["task_id"])
        return original(command, **kwargs)
    world.values["verdict"] = "bad"
    for _ in range(2):
        assert hook.hook(world.request, runner=capture, clock=lambda: 0)["result"] == "refused"
    assert len(assigned) == 1


def test_repair_assignment_resolves_installed_runner(world):
    sys.path.insert(0, str(SCRIPTS.parents[1] / "atm-beads/scripts"))
    import sc_render
    values = deferred(world)
    assert Path(values["repair_script"]) == SCRIPTS / "sanity-close-repair.py"
    assert sc_render.render(hook.TEMPLATES / "sanity-bead-repair.xml.j2", values)


def test_changed_developer_assignment_blocks_repair_and_escalates(world):
    values = deferred(world)
    world.histories[world.dev].append({"event": "reassigned", "assignee": "another-dev"})
    world.commands.clear()
    with pytest.raises(hook.Failure, match="ownership changed"):
        repair.repair(Path(values["journal"]), values["key"], runner=world.run)
    assert not world.writes()
    assert any(c[:4] == ["atm", "task", "assign", "test-lead"] for c in world.commands)


def test_dev_relationship_resolves_without_copied_result_field(world):
    world.values.pop("checked_bead")
    assert world.invoke()["result"] == "handled"


def test_exhausted_work_budget_retains_bounded_escalation_window(world):
    now = [0.0]
    original = world.run
    def expire(command, **kwargs):
        if command[:2] == ["bd", "show"]:
            now[0] = 1.8
            raise subprocess.TimeoutExpired(command, kwargs["timeout"])
        assert command[:3] == ["atm", "task", "assign"]
        assert 0 < kwargs["timeout"] <= 0.5
        return original(command, **kwargs)
    result = hook.hook(world.request, runner=expire, clock=lambda: now[0])
    assert json.loads(result["message"])["notified"]


def test_repair_cannot_claim_success_when_dev_close_is_unconfirmed(world):
    values = deferred(world)
    original = world.run
    def lost_close(command, **kwargs):
        if command[:3] == ["atm", "task", "close"]:
            return subprocess.CompletedProcess(command, 0, "{}", "")
        return original(command, **kwargs)
    with pytest.raises(hook.Failure, match="close not confirmed") as raised:
        repair.repair(Path(values["journal"]), values["key"], runner=lost_close)
    assert raised.value.command == ["atm", "task", "events", world.dev, "--json"]
    assert hook.Journal(Path(values["journal"])).get(values["key"])["last_attempted_command"][:3] == ["atm", "task", "close"]
    assert not hook.Journal(Path(values["journal"])).get(values["key"]).get("repaired")


@pytest.mark.parametrize("corrupt", [False, True])
def test_missing_or_corrupt_repair_journal_escalates_from_invocation_context(world, corrupt):
    path = world.root / ".atm-bd/sanity-close/journal.sqlite3"
    path.parent.mkdir(parents=True, exist_ok=True)
    if corrupt:
        path.write_bytes(b"deliberately corrupt sqlite database")
    context = {"team": "test-team", "phase": "test", "task_id": world.task, "lead": "test-lead"}
    with pytest.raises(hook.Failure):
        repair.repair(path, "missing-record", runner=world.run, context=context)
    assert not world.writes()
    assert any(c[:4] == ["atm", "task", "assign", "test-lead"] for c in world.commands)
    error_path = path.with_name("journal-escalations.sqlite3") if corrupt else path
    rows = hook.Journal(error_path).db.execute("SELECT value FROM records").fetchall()
    assert any(json.loads(row[0]).get("notified") for row in rows)
    if corrupt:
        assert any(f["code"] == "SANITY_CLOSE.JOURNAL_UNREADABLE" for f in hook.doctor(world.request)["findings"])


def test_corrupt_journal_failed_escalation_remains_visible(world):
    path = world.root / ".atm-bd/sanity-close/journal.sqlite3"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b"corrupt SQLite")
    world.fail = lambda c: c[:3] == ["atm", "task", "assign"]
    context = {"team": "test-team", "phase": "test", "task_id": world.task, "lead": "test-lead"}
    with pytest.raises(hook.Failure):
        repair.repair(path, "missing", runner=world.run, context=context)
    codes = {f["code"] for f in hook.doctor(world.request)["findings"]}
    assert codes == {"SANITY_CLOSE.JOURNAL_UNREADABLE", "SANITY_CLOSE.ESCALATION_PENDING"}


def test_slow_read_uses_remaining_work_deadline(world):
    # bd under host load routinely exceeds 0.5s; a read may use the whole work deadline.
    budgets = []
    original = world.run
    def slow(command, **kwargs):
        if command[:2] == ["bd", "show"]:
            budgets.append(kwargs["timeout"])
            if kwargs["timeout"] <= 0.5:
                raise subprocess.TimeoutExpired(command, kwargs["timeout"])
        return original(command, **kwargs)
    result = hook.hook(world.request, runner=slow, clock=lambda: 0.0)
    assert result["result"] == "handled"
    assert budgets and min(budgets) == pytest.approx(1.8)


def test_fast_read_failure_is_retried_once(world):
    calls = []
    original = world.run
    def flaky(command, **kwargs):
        if command[:2] == ["bd", "show"] and command[2] == world.task:
            calls.append(command)
            if len(calls) == 1:
                raise OSError("dolt connection reset")
        return original(command, **kwargs)
    assert hook.hook(world.request, runner=flaky, clock=lambda: 0.0)["result"] == "handled"
    assert len(calls) >= 2
