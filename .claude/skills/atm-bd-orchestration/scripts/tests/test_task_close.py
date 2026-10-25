from __future__ import annotations

from contextlib import redirect_stderr, redirect_stdout
from collections import Counter
import copy
import importlib.machinery
import importlib.util
import inspect
import io
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import xml.etree.ElementTree as ET

import pytest
from unittest.mock import patch


from task_close_world import DERIVED, EXAMPLES, SCRIPTS, World, cache_call, example_values, hook

@pytest.fixture
def world(tmp_path, monkeypatch):
    return World(tmp_path, monkeypatch)


def test_assignment_details_uses_latest_assignment_and_prior_completions():
    latest = {"event": "reopened", "seq": 4}
    events = [{"event": "assigned", "message_id": "first", "seq": 1},
              {"event": "completed"}, {"event": "reassigned", "message_id": "second", "seq": 3},
              latest, {"event": "completed"}]
    assert hook.assignment_details(events, "task") == (latest, 4, 2)
    latest["message_id"] = "latest"
    assert hook.assignment_details(events, "task") == (latest, "latest", 2)


@pytest.mark.parametrize(("events", "terminal"), [
    ([], False),
    ([{"to_state": "assigned"}], False),
    ([{"to_state": "complete"}], True),
    ([{"to_state": "complete"}, {"to_state": "assigned"}], False),
])
def test_is_terminal_reads_only_the_latest_event(events, terminal):
    assert hook.is_terminal(events) is terminal


def test_assignment_details_requires_an_assignment():
    with pytest.raises(hook.CloseError) as error:
        hook.assignment_details([{"event": "completed"}], "missing-task")
    assert error.value.code == "REPLAY.CONFLICT"
    assert str(error.value) == "missing-task has no assignment event"
    assert error.value.fix == "Assign the tracked task before closing it."


@pytest.mark.parametrize("template", ["dev-template", "fix-assignment"])
@pytest.mark.parametrize("layers,expected", [
    ([{"branch": "older", "head": "a" * 40}, {"branch": "ledger-top", "head": "b" * 40}], "ledger-top"),
    ([], "lower-target"),
])
def test_template_rebase_and_hook_ancestry_use_the_same_stack_top(world, template, layers, expected):
    spec = importlib.util.spec_from_file_location("assignment_gates_for_close", SCRIPTS / "assignment-gates.py")
    assert spec is not None and spec.loader is not None
    gates = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(gates)
    world.beads["atm-phase-bj.stack"]["metadata"]["layers"] = layers
    world.values["pr_target"] = "lower-target"
    values = json.loads((EXAMPLES / f"{template}-vars.json").read_text())
    values.update(primary_checkout=str(world.root), worktree_path=str(world.root),
                  phase_root="atm-phase-bj", pr_target="lower-target")
    rendered = hook.sc_render.render(SCRIPTS.parent / "templates" / f"{template}.xml.j2", values)
    match = re.search(r"`([^`]*assignment-gates\.py stack-top [^`]*)`", rendered)
    assert match
    argv = shlex.split(match.group(1))[1:]

    def gate_runner(command, **kwargs):
        if command[:2] == ["git", "-C"]:
            command = ["git", *command[3:]]
        return world(command, **kwargs)

    output = io.StringIO()
    with redirect_stdout(output):
        assert gates.main(argv, gate_runner) == 0
    assert output.getvalue().strip() == expected
    assert "ok" in world.run()
    assert any(argv == ["git", "merge-base", "--is-ancestor", f"origin/{expected}", world.values["commit"]]
               for argv, _kwargs in world.calls)


def test_dev_handoff_is_immediate_and_closes_the_dev_task_as_the_dev(world, monkeypatch):
    before = dict(hook.os.environ)
    assert "ok" in world.run()
    assert dict(hook.os.environ) == before
    calls = [argv for argv, _kwargs in world.calls if not cache_call(argv)]
    close_index = next(index for index, argv in enumerate(calls) if argv[:2] == ["bd", "close"])
    # The sanity assignment (and its beads_dir) is built and size-checked before the bead closes.
    assert ["bd", "where", "--json"] in calls[:close_index]
    assert calls[close_index + 1][:2] == ["atm", "send"]
    assert calls[close_index + 2][:3] == ["atm", "task", "assign"]
    # The dev is free for its next queued task once the hook returns; sanity runs on its own task.
    dev_task = world.tasks[world.values["dev_task_id"]]
    assert dev_task[-1]["to_state"] == "complete" and dev_task[-1]["close_outcome"] == "completed"
    assert world.tasks[world.values["sanity_task_id"]][-1]["to_state"] == "assigned"
    assert calls[close_index + 3] == ["atm", "task", "close", world.values["dev_task_id"], "completed",
                                      f"dev complete at {world.values['commit']}"]
    for argv, sender in world.writes:
        assert sender == ("phase-lead" if argv[:3] == ["atm", "task", "assign"] else "caller")
        if argv[0] == "bd":
            assert argv[-2:] == ["--actor", "caller"]
        if "--vars" in argv:
            # The hook sends the completion file it wrote, never the lead's dispatch file.
            assert Path(argv[argv.index("--vars") + 1]).name == "completion-vars.json"
    for argv, kwargs in world.calls:
        if argv[:3] in (["atm", "task", "assign"], ["atm", "hook", "list"]):   # the lead's team: the sanity dispatch
            assert kwargs["env"]["ATM_TEAM"] == "test-team"
        else:
            assert kwargs["env"].get("ATM_TEAM") == before.get("ATM_TEAM")


def test_dev_rerun_after_the_task_is_closed_does_not_close_it_again(world):
    assert "ok" in world.run()
    assert "ok" in world.run()
    closes = [argv for argv, _sender in world.writes if argv[:4] == ["atm", "task", "close", world.values["dev_task_id"]]]
    assert len(closes) == 1


def test_not_reproducible_fix_closes_its_task_with_the_evidence_reason(world):
    world.beads[world.values["checked_bead"]]["metadata"]["role"] = "fix"
    assert "ok" in world.run("fix", outcome="not_reproducible")
    close = next(argv for argv, _sender in world.writes if argv[:4] == ["atm", "task", "close", world.values["dev_task_id"]])
    assert close[4:] == ["completed", f"not reproducible: {world.values['evidence']}"]


def test_preview_and_native_send_use_identical_completion_vars(world, monkeypatch):
    world.values.pop("carry_forward")
    original_render = hook.sc_render.render
    inputs = []

    def render(template, values):
        inputs.append(copy.deepcopy(values))
        return original_render(template, values)

    monkeypatch.setattr(hook.sc_render, "render", render)
    assert "ok" in world.run()
    assert inputs and world.messages
    assert all(any(message["values"] == rendered for rendered in inputs) for message in world.messages)


@pytest.mark.parametrize("change,code", [({"commit": ""}, "VARS.EMPTY"), ({"extra": 1}, "VARS.UNKNOWN"),
                                      ({"verdict": "rebase_required"}, "VARS.INVALID")])
def test_invalid_vars_have_no_commands(world, change, code):
    assert world.run("sanity", **change)["err"]["code"] == code
    assert world.calls == []


def test_shared_contract_strict_render_precedes_task_event_reads(world):
    world.values.pop("base")
    world.beads[world.values["sanity_bead"]]["metadata"]["sanity_round_key"] = [
        world.values["sanity_task_id"], "assignment-1", "FAIL", "old-head"]
    result = world.run("sanity")
    assert result["err"]["code"] == "VARS.MISSING"
    assert "base" in result["err"]["message"]
    assert world.calls == []


@pytest.mark.parametrize("missing", [False, True])
def test_vars_validation_runs_once_before_live_commands(world, monkeypatch, missing):
    original_validate = hook.validate
    calls = []

    def validate(values, role):
        calls.append(role)
        return original_validate(values, role)

    monkeypatch.setattr(hook, "validate", validate)
    if missing:
        world.values.pop("base")
        assert world.run()["err"]["code"] == "VARS.MISSING"
        assert not world.calls
    else:
        assert "ok" in world.run()
    assert calls == ["dev"]


@pytest.mark.parametrize("argv", [[], ["--role", "other", "--vars", "/absent"], ["--vars", "/absent"]])
def test_argument_errors_use_typed_json(argv):
    stdout, stderr = io.StringIO(), io.StringIO()
    with redirect_stdout(stdout), redirect_stderr(stderr):
        assert hook.main(argv, runner=lambda *_args, **_kwargs: pytest.fail("unexpected command")) == 1
    assert json.loads(stdout.getvalue())["err"]["code"] == "ARGS.INVALID"
    assert stdout.getvalue() == stderr.getvalue()


@pytest.mark.parametrize("role", ["dev", "fix", "sanity", "qa"])
def test_every_role_required_field_is_validated_before_commands(world, role):
    original = example_values(role)
    original.update(primary_checkout=str(world.root), worktree_path=str(world.root))
    contract = hook.header(hook.TEMPLATES / "task-close-vars.j2")
    required = contract["required_variables_by_role"][role]
    results = contract.get("result_variables_by_role", {}).get(role, [])
    assert set(original) == set(required) | set(results)
    for field in [*required, *results]:
        world.values = dict(original)
        del world.values[field]
        assert world.run(role)["err"]["code"] == "VARS.MISSING", (role, field)
    assert world.calls == []


def test_not_reproducible_missing_pr_identity_is_fix_only(world):
    for field in ("branch", "commit", "pr_number"):
        world.values.pop(field, None)
    assert world.run("fix", outcome="not_reproducible")["ok"]
    world.calls.clear()
    world.values.update(outcome="not_reproducible")
    assert world.run("sanity")["err"]["code"] == "VARS.MISSING"
    assert world.calls == []


@pytest.mark.parametrize("failure", [True, "raise"])
def test_github_retries_and_falls_back_without_requiring_pr_base(world, failure):
    world.github_failure = failure
    assert world.run()["ok"]["github"] == "unavailable"
    assert len([argv for argv, _kwargs in world.calls if argv[0] == "gh"]) == 3


@pytest.mark.parametrize("command", [["gh", "api"], ["git", "ls-remote"], ["git", "fetch"]])
def test_network_timeouts_retry_then_fall_back(world, command, monkeypatch):
    monkeypatch.setattr(hook, "COMMAND_TIMEOUT", 7)
    world.timeout_command = command
    assert world.run()["ok"]["github"] == "unavailable"
    calls = [(argv, kwargs) for argv, kwargs in world.calls if argv[:len(command)] == command]
    assert len(calls) == 3
    assert all(kwargs["timeout"] == 7 for _, kwargs in world.calls)


def test_deadline_injection_convention_matches_assignment_gates(world, monkeypatch):
    spec = importlib.util.spec_from_file_location("assignment_gates_deadline_contract", SCRIPTS / "assignment-gates.py")
    assert spec is not None and spec.loader is not None
    gates = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(gates)
    assert hook.COMMAND_TIMEOUT == gates.COMMAND_TIMEOUT == hook.command_timeout(os.environ)
    assert gates.DEFAULT_COMMAND_TIMEOUT == 180
    for entry_point in (hook.main, hook.Hook.__init__, gates.main):
        assert "command_timeout" not in inspect.signature(entry_point).parameters
    instance = hook.Hook("dev", world.values, world.path, world, lambda: 42)
    command = ["bd", "show", world.values["checked_bead"], "--json"]
    for deadline in (7, 11):
        monkeypatch.setattr(hook, "COMMAND_TIMEOUT", deadline)
        monkeypatch.setattr(gates, "COMMAND_TIMEOUT", deadline)
        world.calls.clear()
        gates.invoke(world, command, capture_output=True, text=True)
        instance.run(command)
        assert [kwargs["timeout"] for _, kwargs in world.calls] == [deadline, deadline]


def test_local_timeout_is_typed_and_names_command_and_deadline(world, monkeypatch):
    monkeypatch.setattr(hook, "COMMAND_TIMEOUT", 7)
    world.timeout_command = ["git", "rev-parse"]
    error = world.run()["err"]
    assert error["code"] == "COMMAND.FAILED"
    assert "git" in error["message"] and "rev-parse" in error["message"] and "7s" in error["message"]
    assert error["fix"].startswith("Retry once, or raise ATM_BD_GATE_TIMEOUT_S")


@pytest.mark.parametrize("bad", ["abc", "0", "-3"])
def test_invalid_timeout_env_is_a_typed_config_error_not_a_traceback(world, bad):
    with patch.dict(os.environ, {"ATM_BD_GATE_TIMEOUT_S": bad}):
        result = world.run()
    error = result["err"]
    assert error["code"] == "CONFIG.INVALID"
    assert f"ATM_BD_GATE_TIMEOUT_S={bad!r}" in error["message"]
    assert "ATM_BD_GATE_TIMEOUT_S=180" in error["fix"]
    assert world.calls == []


@pytest.mark.parametrize("failed_command", ["ls-remote", "fetch"])
def test_remote_git_failures_retry_then_use_cached_local_checks(world, failed_command):
    world.git_network_failure = failed_command
    result = world.run()
    assert result["ok"]["github"] == "unavailable"
    assert result["ok"]["assigned"] == [world.values["sanity_bead"]]
    assert len([argv for argv, _kwargs in world.calls if argv[:2] == ["git", failed_command]]) == 3
    assert not any(argv[:2] == ["gh", "api"] for argv, _kwargs in world.calls)
    assert ["git", "merge-base", "--is-ancestor", "origin/integrate/phase-bj", world.values["commit"]] in [
        argv for argv, _kwargs in world.calls
    ]


@pytest.mark.parametrize("failed_command", ["ls-remote", "fetch"])
def test_remote_git_fallback_names_the_failed_command_and_stderr(world, failed_command):
    world.git_network_failure = failed_command
    expected_command = {
        "ls-remote": f"git ls-remote origin refs/heads/{world.values['branch']}",
        "fetch": "git fetch origin",
    }[failed_command]
    result = world.run()["ok"]
    assert result["github"] == "unavailable"
    assert result["github_error"] == f"{expected_command}: remote unavailable"


@pytest.mark.parametrize(("failure", "stderr"), [(True, "HTTP 429"), ("raise", "GitHub unavailable")])
def test_github_fallback_names_the_failed_api_call_and_error(world, failure, stderr):
    world.github_failure = failure
    result = world.run()["ok"]
    assert result["github"] == "unavailable"
    assert result["github_error"].startswith("gh api repos/example/repo/pulls?head=example:")
    assert result["github_error"].endswith(f": {stderr}")


def test_github_ok_result_carries_no_github_error(world):
    result = world.run()["ok"]
    assert result["github"] == "ok"
    assert "github_error" not in result


def test_ledger_top_overrides_static_target(world):
    metadata = world.beads["atm-phase-bj.stack"]["metadata"]
    metadata["layers"] = [
        {"branch": "irrelevant-broken-lower-layer", "pr": 350, "head": "0" * 40,
         "base": metadata["trunk"], "linked": True},
        {"branch": "new-top", "pr": 351, "head": "b" * 40,
         "base": "irrelevant-broken-lower-layer", "linked": True},
    ]
    assert len(metadata["layers"]) == 2
    assert metadata["layers"][-1]["branch"] == "new-top"
    assert metadata["layers"][-1]["head"] == "b" * 40
    assert "ok" in world.run()
    calls = [argv for argv, _kwargs in world.calls]
    assert ["git", "merge-base", "--is-ancestor", "origin/new-top", world.values["commit"]] in calls
    assert not any("irrelevant-broken-lower-layer" in str(argv) for argv in calls)


def test_stack_bead_id_comes_from_dispatch_vars(world):
    original = "atm-phase-bj.stack"
    selected = "an-unrelated-ledger-name"
    world.beads[selected] = world.beads.pop(original)
    world.beads[selected]["id"] = selected
    assert "ok" in world.run(stack_bead=selected)
    assert any(argv[:3] == ["bd", "show", selected] for argv, _ in world.calls)
    assert not any(argv[:3] == ["bd", "show", original] for argv, _ in world.calls)


def test_qa_does_not_require_stack_bead(world):
    world.values.pop("stack_bead")
    assert "ok" in world.run("qa")


def test_dev_skip_checks_does_not_read_stack_bead(world):
    del world.beads[world.values["stack_bead"]]
    assert "ok" in world.run("dev", skip_checks=True)
    assert not any(argv[:3] == ["bd", "show", world.values["stack_bead"]] for argv, _ in world.calls)


def test_unrelated_bead_layers_are_not_stack_schema_validated(world):
    world.beads[world.values["checked_bead"]]["metadata"]["layers"] = "unrelated data"
    assert "ok" in world.run()


def test_sanity_lookup_fallback_and_actionable_missing_bead(world):
    del world.beads[world.values["checked_bead"]]["metadata"]["sanity_bead"]
    assert "ok" in world.run()
    world.beads[world.values["sanity_bead"]]["metadata"] = {}
    world.writes.clear()
    output = world.run()
    assert output["err"]["code"] == "BEAD.NO_SANITY"
    assert "phase-lead" in output["err"]["fix"]
    assert world.writes == []


@pytest.mark.parametrize("argv,response,code", [
    (["git", "rev-parse", "HEAD"], (0, "wrong"), "HEAD.NOT_PUSHED"),
    (["git", "status", "--porcelain"], (0, " M source.py"), "HEAD.DIRTY"),
    (["git", "ls-remote", "origin", "refs/heads/sprint/bj-1-task-close-hook"], (0, "wrong"), "HEAD.NOT_PUSHED"),
    (["git", "merge-base", "--is-ancestor", "origin/integrate/phase-bj", "a" * 40], (1, ""), "BASE.NOT_ANCESTOR"),
    (["gh", "api", "repos/example/repo/pulls?head=example:sprint/bj-1-task-close-hook&state=open"], (0, "[]"), "PR.NOT_FOUND"),
])
def test_dev_preconditions_do_not_write(world, argv, response, code):
    world.overrides[tuple(argv)] = response
    assert world.run()["err"]["code"] == code
    assert world.writes == []


def test_not_reproducible_assigns_runtime_top_without_rewriting_input(world):
    for field in ("branch", "commit", "pr_number"):
        del world.values[field]
    result = world.run("fix", outcome="not_reproducible")
    assert "ok" in result, result
    contract = hook.header(hook.TEMPLATES / "task-close-vars.j2")
    results = set(contract["result_variables_by_role"]["fix"])
    assert json.loads(world.path.read_text()) == {name: value for name, value in world.values.items()
                                                 if name not in results and name not in DERIVED}
    assignment = next(message for message in world.messages if message["type"] == "dev-sanity-task")
    assert assignment["values"]["branch"] == "integrate/phase-bj"
    assert assignment["values"]["commit"] == "b" * 40
    assert assignment["values"]["beads_dir"] == str(world.root / ".beads")
    assert any(argv == ["bd", "where", "--json"] for argv, _kwargs in world.calls)
    assert set(assignment["values"]) == set(world.values) | {"branch", "commit", *contract["derived_variables"],
                                                          "close_context", "beads_dir", "sanity_close_mode"}
    assert not any(argv[0] == "gh" for argv, _kwargs in world.calls)


def test_not_reproducible_fix_without_stack_bead_uses_pr_target(world):
    world.values.pop("stack_bead")
    for field in ("branch", "commit", "pr_number"):
        world.values.pop(field, None)
    result = world.run("fix", outcome="not_reproducible")
    assert "ok" in result, result
    assert any(argv == ["git", "rev-parse", "origin/integrate/phase-bj"] for argv, _kwargs in world.calls)
    assert not any(argv[:2] == ["bd", "show"] and argv[2] == "atm-phase-bj.stack" for argv, _kwargs in world.calls)
    assignment = next(message for message in world.messages if message["type"] == "dev-sanity-task")
    assert "stack_bead" not in assignment["values"]


def test_not_reproducible_hook_verifies_the_cited_file_and_line(world):
    result = world.run("fix", outcome="not_reproducible", evidence="missing.py:1")
    assert result["err"]["code"] == "EVIDENCE.INVALID"
    assert "missing.py:1" in result["err"]["message"]
    assert world.writes == []


def test_sanity_pass_leaves_the_already_closed_dev_task_alone_then_assigns_qa(world):
    world.tasks[world.values["dev_task_id"]].append(dict(event="completed", to_state="complete", close_outcome="completed"))
    assert "ok" in world.run("sanity")
    assert world.beads[world.values["sanity_bead"]]["status"] == "closed"
    assert not any(argv[:4] == ["atm", "task", "close", world.values["dev_task_id"]] for argv, _sender in world.writes)
    assert world.tasks[world.values["sanity_bead"]][-1]["close_outcome"] == "completed"
    assert world.messages[-1]["recipient"] == "quality"


def test_sanity_pass_closes_without_waiting_for_stack(world):
    assert "ok" in world.run("sanity", stacked=None)
    assert world.beads[world.values["sanity_bead"]]["status"] == "closed"
    calls = [argv for argv, _kwargs in world.calls]
    close = next(index for index, argv in enumerate(calls) if argv[:3] == ["bd", "close", world.values["sanity_bead"]])
    assert not any(argv[:3] == ["bd", "update", world.values["stack_bead"]] for argv in calls)
    assert close < next(index for index, argv in enumerate(calls)
                        if argv[:4] == ["atm", "task", "close", world.values["sanity_task_id"]])


@pytest.mark.parametrize("configured_cap", [None, 2])
def test_failure_cap_exceeds_limit_and_counts_distinct_assignments(world, configured_cap):
    config = world.root / hook.repo_config.RELATIVE_PATH
    if configured_cap is not None:
        config.write_text(config.read_text() + f"max_sanity_rounds: {configured_cap}\n")
    cap = configured_cap if configured_cap is not None else 5
    bead = world.values["sanity_bead"]
    checked = world.values["checked_bead"]
    for number in range(1, cap + 2):
        world.tasks[bead] = [world.assignment(number)]
        world.beads[bead].update(status="in_progress", assignee="sanity")
        world.beads[checked]["status"] = "closed"
        output = world.run("sanity", verdict="FAIL", undone=[{"number": 2, "reason": "verbatim missing implementation"}])
        assert "ok" in output, output
        assert world.beads[bead]["metadata"]["sanity_round"] == number
        if number <= cap:
            assert world.beads[checked]["status"] == "open"
            assert world.beads[bead]["assignee"] == ""
        else:
            assert output["ok"]["escalated"] == "SANITY.ROUND_CAP"
            assert world.beads[checked]["status"] == "closed"
            assert world.beads[bead]["status"] == "in_progress"
    cap_messages = [message for message in world.messages if "SANITY.ROUND_CAP" in message["body"]]
    assert [(message["type"], message["recipient"]) for message in cap_messages] == [(None, "team-lead")]


def test_sanity_fail_message_contains_exact_undone_deliverables(world):
    undone = [{"number": 2, "reason": "missing source evidence"},
              {"number": 5, "reason": "line 5 needs review"}]
    assert "ok" in world.run("sanity", verdict="FAIL", undone=undone)
    messages = [message for message in world.messages if message["type"] == "sanity-fail"]
    assert len(messages) == 1
    root = ET.fromstring(messages[0]["body"])
    assert root.attrib == {"bead": world.values["checked_bead"], "sprint": world.values["sprint"]}
    description = root.find("description")
    assert description is not None and description.text is not None
    lines = description.text.splitlines()
    assert lines == [
        f"{world.values['checked_title']}: sanity round {world.values['round']} of {world.values['max_sanity_rounds']}.",
        f"Branch {world.values['branch']}, PR #{world.values['pr_number']}, commit {world.values['commit']}.",
        "2. NOT complete: missing source evidence", "5. NOT complete: line 5 needs review",
    ]


def test_metadata_update_preserves_concurrent_keys_and_writes_fail_counter_atomically(world):
    bead = world.values["sanity_bead"]
    world.concurrent_metadata = {"concurrent": "preserved"}
    result = world.run("sanity", verdict="FAIL", undone=[{"number": 1, "reason": "missing"}])
    assert "ok" in result
    assert world.beads[bead]["metadata"]["concurrent"] == "preserved"
    update = next(argv for argv, _ in world.writes if argv[:3] == ["bd", "update", bead])
    entries = [update[i + 1].split("=", 1)[0] for i, flag in enumerate(update) if flag == "--set-metadata"]
    assert {"sanity_round", "sanity_round_key"} <= set(entries)
    assert "--metadata" not in update


@pytest.mark.parametrize("role,verdict", [("dev", "PASS"), ("fix", "PASS"), ("sanity", "PASS"),
                                          ("sanity", "FAIL"), ("sanity", "CANNOT_RUN"), ("qa", "PASS")])
def test_replay_state_is_snapshotted_before_writes(world, role, verdict):
    assert "ok" in world.run(role, verdict=verdict, undone=[{"number": 1, "reason": "missing"}])
    calls = [argv for argv, _ in world.calls if not cache_call(argv)]  # the drain's own reads apply the write
    first_write = next(index for index, argv in enumerate(calls) if argv[:2] in (["bd", "update"], ["bd", "close"], ["atm", "send"]) or argv[:3] in (["atm", "task", "assign"], ["atm", "task", "close"]))
    reads = [argv for argv in calls[first_write + 1:] if argv[:2] in (["bd", "show"], ["bd", "list"]) or argv[:3] == ["atm", "task", "events"]]
    expected = [["atm", "task", "events", world.values["sanity_task_id"], "--all", "--json"]] if role == "sanity" and verdict == "FAIL" else []
    assert reads == expected


@pytest.mark.parametrize("invalid", ["[invalid", "{}", "null", 3, ["too", "short"], ["task", True, "FAIL", "sha"]])
def test_invalid_sanity_round_key_is_a_typed_error_before_writes(world, invalid):
    bead = world.values["sanity_bead"]
    world.beads[bead]["metadata"]["sanity_round_key"] = invalid
    result = world.run("sanity", verdict="FAIL", undone=[{"number": 1, "reason": "missing"}])
    assert result["err"]["code"] == "BEAD.INVALID"
    assert bead in result["err"]["fix"]
    assert world.writes == []


def test_metadata_comparison_decodes_key_and_bool_is_json(world):
    bead = world.values["sanity_bead"]
    key = [bead, "assignment-1", "FAIL", world.values["commit"]]
    world.beads[bead]["metadata"]["sanity_round_key"] = json.dumps(key, separators=(",", ":"))
    world.tasks[bead].append(world.assignment(1))
    world.values["verdict"] = "FAIL"
    instance = hook.Hook("sanity", world.values, world.path, world, lambda: 42)
    instance.snapshot()
    instance.metadata(bead, {"sanity_round_key": key})
    assert world.writes == []
    instance.update(bead, metadata={"enabled": True})
    assert "enabled=true" in world.writes[-1][0]
    assert world.beads[bead]["metadata"]["enabled"] is True


def test_cannot_run_resets_sanity_and_refuses_without_notice(world):
    assert "ok" in world.run("sanity", verdict="CANNOT_RUN")
    assert world.beads[world.values["sanity_bead"]]["status"] == "open"
    assert world.tasks[world.values["sanity_bead"]][-1]["to_state"] == "complete"
    assert world.tasks[world.values["sanity_bead"]][-1]["close_outcome"] == "refused"
    assert world.messages == []


@pytest.mark.parametrize("verdict", ["PASS", "FAIL"])
def test_qa_closes_bead_before_task_and_sends_lead_status(world, verdict):
    assert "ok" in world.run("qa", verdict=verdict)
    assert [argv[:2] for argv, _sender in world.writes] == [["bd", "close"], ["atm", "send"], ["atm", "task"]]
    assert world.messages[0]["recipient"] == "phase-lead"
    assert world.messages[0]["type"] is None
    assert world.messages[0]["body"] == f"QA {verdict} {world.values['qa_bead']} commit={world.values['commit']} round=1"
    assert "--template" not in world.writes[1][0]


def test_close_key_is_triple_and_only_fail_counter_key_adds_assignment(world):
    assert "ok" in world.run("sanity", verdict="FAIL", undone=[{"number": 1, "reason": "not implemented"}])
    bead = world.values["sanity_bead"]
    metadata = world.beads[bead]["metadata"]
    assert "task_close_key" not in metadata and "task_close_assignment" not in metadata
    assert json.loads(metadata["sanity_round_key"]) == [bead, "assignment-1", "FAIL", world.values["commit"]]
    world.tasks[bead] = [world.assignment(2)]
    assert "ok" in world.run("sanity", verdict="PASS")
    assert world.beads[bead]["close_reason"] == f"PASS at {world.values['commit']}"


def test_old_sanity_fail_replay_after_dev_reclose_does_not_touch_new_round(world):
    old_commit = world.values["commit"]
    assert "ok" in world.run("dev")
    assert "ok" in world.run("sanity", verdict="FAIL", round=1,
                             undone=[{"number": 2, "reason": "verbatim missing implementation"}])

    new_commit = "c" * 40
    assert "ok" in world.run("dev", commit=new_commit)
    checked = world.values["checked_bead"]
    sanity = world.values["sanity_bead"]
    assert world.beads[checked]["status"] == "closed"
    assert world.beads[checked]["metadata"]["commit"] == new_commit
    writes_before = copy.deepcopy(world.writes)
    messages_before = copy.deepcopy(world.messages)
    task_events_before = copy.deepcopy(world.tasks[sanity])
    replay = world.run("sanity", commit=old_commit, verdict="FAIL", round=1,
                       undone=[{"number": 2, "reason": "verbatim missing implementation"}])

    assert "ok" in replay
    assert world.writes == writes_before
    assert world.messages == messages_before
    assert world.tasks[sanity] == task_events_before
    assert world.beads[checked]["status"] == "closed"
    assert world.beads[checked]["metadata"]["commit"] == new_commit


def test_new_assignment_between_fail_writes_stops_stale_hook(world):
    world.reassign_after_write = 1
    assert "ok" in world.run("sanity", verdict="FAIL", undone=[{"number": 1, "reason": "missing"}])
    assert len(world.writes) == 1
    assert world.beads[world.values["sanity_bead"]]["metadata"]["sanity_round"] == 1
    assert world.beads[world.values["checked_bead"]]["status"] == "in_progress"


def test_same_head_failures_on_distinct_assignments_increment_twice(world):
    bead = world.values["sanity_bead"]
    assert "ok" in world.run("sanity", verdict="FAIL", round=1,
                             undone=[{"number": 2, "reason": "verbatim missing implementation"}])
    assert world.beads[bead]["metadata"]["sanity_round"] == 1

    world.tasks[bead].append(dict(event="assigned", message_id="assignment-2", seq=2,
                                  at="2026-10-19T00:02:00Z", to_state="assigned"))
    second = world.run("sanity", verdict="FAIL", round=2,
                       undone=[{"number": 2, "reason": "verbatim missing implementation"}])
    assert "ok" in second
    assert world.beads[bead]["metadata"]["sanity_round"] == 2


def test_existing_four_part_state_allows_a_new_assignment(world):
    bead = world.values["sanity_bead"]
    world.beads[bead]["metadata"]["sanity_round_key"] = [bead, "assignment-0", "FAIL", "old-head"]
    assert "ok" in world.run("sanity")
    assert world.beads[bead]["close_reason"] == f"PASS at {world.values['commit']}"


@pytest.mark.parametrize("role,verdict", [("dev", "PASS"), ("fix", "PASS"), ("sanity", "PASS"), ("sanity", "FAIL"), ("sanity", "CANNOT_RUN"), ("qa", "PASS"), ("qa", "FAIL")])
def test_replay_after_every_committed_write_converges(tmp_path, monkeypatch, role, verdict):
    baseline_root = tmp_path / "baseline"
    baseline_root.mkdir()
    baseline = World(baseline_root, monkeypatch)
    assert "ok" in baseline.run(role, verdict=verdict, undone=[{"number": 1, "reason": "not implemented"}])
    # each baseline send, keyed by the boundary that commits it: the next write is its gating write
    sends = [index for index, (argv, _) in enumerate(baseline.writes) if argv[:2] == ["atm", "send"]]
    gated = {index + 1: index for index in sends if index + 1 < len(baseline.writes)}
    # the baseline messages come from the writes that send or assign, in write order
    notices = [index for index, (argv, _) in enumerate(baseline.writes) if argv[:2] == ["atm", "send"] or argv[:3] == ["atm", "task", "assign"]]
    notice_of = {write: (item["type"], item["recipient"]) for write, item in zip(notices, baseline.messages)}
    for boundary in range(1, len(baseline.writes) + 1):
        root = tmp_path / f"boundary-{boundary}"
        root.mkdir()
        world = World(root, monkeypatch)
        world.fail_after = boundary
        assert "err" in world.run(role, verdict=verdict, undone=[{"number": 1, "reason": "not implemented"}])
        assert world.writes[-1][0][:3] == baseline.writes[boundary - 1][0][:3], boundary
        world.fail_after = None
        result = world.run(role)
        assert "ok" in result, (boundary, result)
        assert world.beads == baseline.beads
        for task in baseline.tasks:
            if role in ("dev", "fix") and task == world.values["sanity_bead"]:
                assert world.tasks[task][-1]["to_state"] == "assigned"
            else:
                assert world.tasks[task] == baseline.tasks[task]
        expected = Counter((item["type"], item["recipient"]) for item in baseline.messages)
        actual = Counter((item["type"], item["recipient"]) for item in world.messages)
        if boundary in gated:
            # a crash after a send and before its gating write: the replay sends that notice again
            gate = baseline.writes[boundary][0]
            assert gate[:3] in (["atm", "task", "close"], ["atm", "task", "assign"]), (boundary, gate[:4])
            expected[notice_of[gated[boundary]]] += 1
        assert actual == expected, (boundary, world.writes[-1][0][:4])
        assert not any(argv[:2] == ["atm", "search"] for argv, _ in world.calls)


@pytest.mark.parametrize("role,verdict", [("dev", "PASS"), ("fix", "PASS"), ("sanity", "PASS"), ("sanity", "FAIL"), ("sanity", "CANNOT_RUN"), ("qa", "PASS"), ("qa", "FAIL")])
def test_second_run_after_success_sends_nothing(tmp_path, monkeypatch, role, verdict):
    root = tmp_path / "world"
    root.mkdir()
    world = World(root, monkeypatch)
    assert "ok" in world.run(role, verdict=verdict, undone=[{"number": 1, "reason": "not implemented"}])
    sent = len(world.messages)
    assert "ok" in world.run(role)
    assert len(world.messages) == sent


def test_changed_sha_rerun_of_open_sanity_reassigns_without_a_second_report(world):
    assert "ok" in world.run()
    assert "ok" in world.run(commit="c" * 40)
    assert [item["type"] for item in world.messages] == ["dev-complete", "dev-sanity-task", "dev-sanity-task"]


def test_replay_with_changed_verdict_or_head_is_conflict(world):
    assert "ok" in world.run("sanity")
    assert world.run("sanity", verdict="FAIL", undone=[{"number": 1, "reason": "different"}])["err"]["code"] == "REPLAY.CONFLICT"
    assert world.run("sanity", verdict="PASS", commit="c" * 40)["err"]["code"] == "REPLAY.CONFLICT"


def test_fix_pass_requires_its_own_carry_forward(world):
    world.beads[world.values["checked_bead"]]["labels"] = ["stage:fix"]
    assert world.run("sanity")["err"]["code"] == "VARS.INVALID"
    assert world.writes == []
    assert "ok" in world.run("sanity", carry_forward=world.values["checked_bead"])
    assert world.messages[-1]["values"]["carry_forward"] == world.values["checked_bead"]


def test_cannot_run_terminal_refusal_is_a_noop_until_new_assignment(world):
    assert "ok" in world.run("sanity", verdict="CANNOT_RUN")
    writes = list(world.writes)
    assert "ok" in world.run("sanity", verdict="CANNOT_RUN")
    for verdict in ("PASS", "FAIL"):
        error = world.run("sanity", verdict=verdict, commit="c" * 40,
                          undone=[{"number": 1, "reason": "missing"}])["err"]
        assert error["code"] == "REPLAY.CONFLICT"
        assert "new same-id assignment" in error["fix"]
    assert world.writes == writes
    world.tasks[world.values["sanity_bead"]].append(world.assignment(2))
    assert "ok" in world.run("sanity", verdict="PASS")


def test_round_cap_replay_sends_one_notice_and_does_not_increment_counter(world):
    bead = world.values["sanity_bead"]
    world.beads[bead]["metadata"]["sanity_round"] = 5
    world.beads[world.values["checked_bead"]]["status"] = "closed"
    result = world.run("sanity", verdict="FAIL", undone=[{"number": 7, "reason": "not implemented"}])
    assert result["ok"]["escalated"] == "SANITY.ROUND_CAP"
    assert world.run("sanity")["ok"]["escalated"] == "SANITY.ROUND_CAP"
    assert len(world.messages) == 1
    assert world.beads[bead]["metadata"]["sanity_round"] == 6


def test_same_head_dev_retry_reassigns_sanity_after_fail(world):
    assert "ok" in world.run()
    assert "ok" in world.run("sanity", verdict="FAIL", undone=[{"number": 1, "reason": "missing evidence"}])
    assert "ok" in world.run("dev", verdict="PASS", undone=[])
    sanity = world.values["sanity_bead"]
    assert world.tasks[sanity][-1]["to_state"] == "assigned"
    assert len([item for item in world.messages if item["type"] == "dev-sanity-task"]) == 2
    assert "ok" in world.run("dev")
    assert len([item for item in world.messages if item["type"] == "dev-sanity-task"]) == 2


@pytest.mark.parametrize("role", ["dev", "fix"])
def test_rebase_required_rerun_updates_same_open_sanity_task(world, role):
    assert "ok" in world.run(role)
    sanity = world.values["sanity_bead"]
    old = world.values["commit"]
    original_close_reason = world.beads[world.values["checked_bead"]]["close_reason"]
    world.messages.append(dict(type="rebase-required", recipient="developer", sender="phase-lead",
                               body="rebase and rerun", values={}, at="2026-10-19T23:00:01Z"))
    new = "c" * 40
    assert "ok" in world.run(role, commit=new)
    assignments = [message for message in world.messages if message["type"] == "dev-sanity-task"]
    assert [message["values"]["commit"] for message in assignments] == [old, new]
    assert world.tasks[sanity][-1]["to_state"] == "assigned"
    assert world.tasks[sanity][-1]["event"] == "reassigned"
    assert world.beads[world.values["checked_bead"]]["metadata"]["commit"] == new
    assert world.beads[world.values["checked_bead"]]["close_reason"] == original_close_reason
    assert len([task for task in world.tasks if task == sanity]) == 1


def test_dev_replay_does_not_bypass_round_cap(world):
    assert "ok" in world.run()
    sanity = world.values["sanity_bead"]
    world.beads[sanity]["metadata"]["sanity_round"] = 5
    assert world.run("sanity", verdict="FAIL", undone=[{"number": 1, "reason": "not implemented"}])["ok"]["escalated"] == "SANITY.ROUND_CAP"
    assert "ok" in world.run("dev", verdict="PASS", undone=[])
    assert world.tasks[sanity][-1]["to_state"] == "complete"
    assert len([item for item in world.messages if item["type"] == "dev-sanity-task"]) == 1


def test_dev_changed_sha_after_completed_sanity_is_conflict(world):
    assert "ok" in world.run("dev")
    assert "ok" in world.run("sanity")
    writes = list(world.writes)
    assert world.run("dev", commit="c" * 40)["err"]["code"] == "REPLAY.CONFLICT"
    assert world.writes == writes


@pytest.mark.parametrize("boundary", [1, 2, 3, 4])
def test_not_reproducible_retry_uses_current_stack_top(world, boundary):
    for field in ("branch", "commit", "pr_number"):
        del world.values[field]
    world.fail_after = boundary
    assert "err" in world.run("fix", outcome="not_reproducible")
    world.values["pr_target"] = "moved-top"
    world.overrides[("git", "rev-parse", "origin/moved-top")] = (0, "c" * 40)
    world.fail_after = None
    assert "ok" in world.run("fix")
    assignments = [item for item in world.messages if item["type"] == "dev-sanity-task"]
    assert len(assignments) == (2 if boundary == 4 else 1)  # a replay after a moved top re-assigns the new sha
    assert assignments[-1]["values"]["branch"] == "moved-top"
    assert assignments[-1]["values"]["commit"] == "c" * 40
    # a crash after the report (boundary 3) makes the replay report again; after the assign (boundary 4)
    # the sanity task is open, so the replay assigns the moved sha without a second report
    assert len([item for item in world.messages if item["type"] == "fix-complete"]) == (2 if boundary == 3 else 1)


def test_shared_contract_covers_all_role_templates_and_ids(world):
    contract = hook.header(hook.TEMPLATES / "task-close-vars.j2")
    declared = set(contract["required_variables"])
    assert declared == set(contract["derived_variables"]).union(
        *map(set, contract["required_variables_by_role"].values()),
        *map(set, contract["result_variables_by_role"].values()))
    names = ("dev-complete.md.j2", "fix-complete.md.j2", "dev-sanity-template.xml.j2", "sanity-fail.xml.j2",
             "dev-sanity-complete.md.j2", "qa-template.xml.j2", "qa-complete.md.j2")
    for name in names:
        dispatch_only = {"beads_dir", "close_context"} if name == "dev-sanity-template.xml.j2" else set()
        assert set(hook.header(hook.TEMPLATES / name)["required_variables"]) <= declared | dispatch_only, name
        values = {**world.values, "beads_dir": str(world.root / ".beads"),
                  "close_context": {**world.values, "phase_lead": "phase-lead"}}
        body = hook.sc_render.render(hook.TEMPLATES / name, values)
        role_id = "sanity_task_id" if name.startswith("dev-sanity") else "qa_task_id" if name.startswith("qa-") else "dev_task_id"
        if name != "sanity-fail.xml.j2":
            assert world.values[role_id] in body
    rebase = ET.fromstring(hook.sc_render.render(hook.TEMPLATES / "rebase-required.xml.j2", world.values))
    assert rebase.attrib["bead"] == world.values["checked_bead"]
    description_node = rebase.find("description")
    assert description_node is not None and description_node.text is not None
    description = description_node.text
    assert world.values["branch"] in description
    assert world.values["stacked"]["top"]["branch"] in description
    assert world.values["stacked"]["top"]["head"] in description
    assert "push" in description.lower()


@pytest.mark.parametrize("role", ["dev", "fix", "sanity", "qa"])
def test_each_role_example_renders_every_shared_template(world, role):
    values = example_values(role)
    hook.validate(values, role)
    if role in ("dev", "fix"):
        values = {**world.values, **values}
    values.update(beads_dir=str(world.root / ".beads"),
                  close_context={**values, "phase_lead": "phase-lead"})
    common = ("dev-sanity-complete.md.j2", "sanity-fail.xml.j2", "qa-template.xml.j2", "qa-complete.md.j2")
    names = {
        "dev": ("dev-complete.md.j2", "dev-sanity-template.xml.j2", *common, "rebase-required.xml.j2"),
        "fix": ("fix-complete.md.j2", "dev-sanity-template.xml.j2", *common, "rebase-required.xml.j2"),
        "sanity": common,
        "qa": ("qa-complete.md.j2",),
    }[role]
    for name in names:
        hook.sc_render.render(hook.TEMPLATES / name, values)


@pytest.mark.parametrize("role", ["dev", "fix", "sanity", "qa"])
def test_each_role_example_executes_its_close_path(world, role):
    world.values = example_values(role)
    world.values.update(primary_checkout=str(world.root), worktree_path=str(world.root))
    assert "ok" in world.run(role)


@pytest.mark.parametrize("role, expected_writes", [
    ("dev", [["bd", "update"], ["bd", "close"], ["atm", "send"], ["atm", "task", "assign"], ["atm", "task", "close"]]),
    ("fix", [["bd", "update"], ["bd", "close"], ["atm", "send"], ["atm", "task", "assign"], ["atm", "task", "close"]]),
    ("sanity", [["bd", "close"], ["atm", "task", "close"], ["atm", "task", "assign"]]),
    ("qa", [["bd", "close"], ["atm", "send"], ["atm", "task", "close"]]),
])
def test_each_role_command_order_and_identity(world, role, expected_writes):
    if role == "fix":
        world.beads[world.values["checked_bead"]]["metadata"]["role"] = "fix"
    assert "ok" in world.run(role)
    assert [[argv[0], *argv[1:2]] for argv, _sender in world.writes] == [item[:2] for item in expected_writes]
    for argv, sender in world.writes:
        expected_sender = "phase-lead" if argv[:3] == ["atm", "task", "assign"] else "caller"
        assert sender == expected_sender, argv
    for argv, kwargs in world.calls:
        expected_identity = "phase-lead" if argv[:3] in (["atm", "task", "assign"], ["atm", "hook", "list"]) else "caller"
        assert kwargs["env"]["ATM_IDENTITY"] == expected_identity, argv


def test_help_describes_the_permission_only_override(capsys):
    with pytest.raises(SystemExit) as exit_info:
        hook.main(["--help"])
    assert exit_info.value.code == 0
    assert " ".join("Do NOT use without permission from the phase lead.".split()) in " ".join(capsys.readouterr().out.split())


def d1_run_path(world, path):
    stdout, stderr = io.StringIO(), io.StringIO()
    with redirect_stdout(stdout), redirect_stderr(stderr):
        result = hook.main(["--role", "dev", "--vars", str(path)], runner=world, clock=lambda: 42)
    assert result == 1
    assert stdout.getvalue() == stderr.getvalue()
    return json.loads(stdout.getvalue())["err"]


@pytest.mark.parametrize("contents", [None, '{"broken":', '[]', 'null'])
def test_d1_vars_read_errors_name_exact_path_and_correction(world, contents):
    path = world.root / "bad-input.json"
    if contents is not None:
        path.write_text(contents)
    error = d1_run_path(world, path)
    assert error["code"] == "VARS.INVALID"
    assert str(path) in error["message"]
    assert str(path) in error["fix"]
    assert world.calls == []


@pytest.mark.parametrize("relative,field,contents", [
    (".atm-bd/phase-bj.toml", "lead", None),
    (".atm-bd/phase-bj.toml", "lead", "lead = ["),
    (".atm-bd/phase-bj.toml", "lead", "other = 'value'\n"),
    (".atm-bd/phase-bj.toml", "lead", "lead = 5\n"),
    (".atm.toml", "atm.default_team", None),
    (".atm.toml", "atm.default_team", "[atm\n"),
    (".atm.toml", "atm.default_team", "[atm]\n"),
    (".atm.toml", "atm.default_team", "[atm]\ndefault_team = ''\n"),
])
def test_d1_toml_errors_name_file_and_field_before_commands(world, relative, field, contents):
    path = world.root / relative
    if contents is None:
        path.unlink()
    else:
        path.write_text(contents)
    error = world.run()["err"]
    assert error["code"] == "CONFIG.INVALID"
    assert str(path) in error["message"] and field in error["message"]
    assert str(path) in error["fix"] and field in error["fix"]
    assert world.calls == []


@pytest.mark.parametrize("contents", [None, "invalid: [", "[]", "max_sanity_rounds: zero\n"])
def test_d1_repository_config_errors_name_generated_path(world, contents):
    path = world.root / hook.repo_config.RELATIVE_PATH
    if contents is None:
        path.unlink()
    else:
        path.write_text(contents)
    error = world.run()["err"]
    assert error["code"] == "CONFIG.INVALID"
    assert str(path) in error["message"] and str(path) in error["fix"]
    assert world.calls == []


@pytest.mark.parametrize("field", ["dev_sanity_member"])
def test_d1_missing_repository_field_names_correction(world, field):
    path = world.root / hook.repo_config.RELATIVE_PATH
    values = hook.yaml.safe_load(path.read_text())
    del values[field]
    path.write_text(hook.yaml.safe_dump(values))
    error = world.run()["err"]
    assert error["code"] == "CONFIG.INVALID"
    assert field in error["message"] and field in error["fix"]
    assert str(path) in error["fix"]
    assert world.calls == []


def test_sanity_fail_without_team_lead_member_names_correction(world):
    path = world.root / hook.repo_config.RELATIVE_PATH
    values = hook.yaml.safe_load(path.read_text())
    del values["team_lead_member"]
    path.write_text(hook.yaml.safe_dump(values))
    error = world.run("sanity", verdict="FAIL", undone=[{"number": 1, "reason": "missing"}])["err"]
    assert error["code"] == "CONFIG.INVALID"
    assert "team_lead_member" in error["message"] and "team_lead_member" in error["fix"]
    assert world.calls == []


@pytest.mark.parametrize("count", ["not-an-integer", "2", -1, True, None, 1.5, [], {}])
def test_d1_invalid_sanity_counter_names_bead_metadata_and_fix(world, count):
    bead = world.values["sanity_bead"]
    world.beads[bead]["metadata"]["sanity_round"] = count
    error = world.run("sanity", verdict="FAIL", undone=[{"number": 1, "reason": "missing"}])["err"]
    assert error["code"] == "BEAD.INVALID"
    assert bead in error["message"] and "metadata.sanity_round" in error["message"]
    assert bead in error["fix"] and "metadata.sanity_round" in error["fix"]
    assert world.writes == []


@pytest.mark.parametrize("field", ["primary_checkout", "worktree_path", "phase"])
def test_d1_invalid_path_fields_name_the_required_string(world, field):
    error = world.run(**{field: 42})["err"]
    assert error["code"] == "VARS.INVALID"
    assert field in error["message"] and field in error["fix"]
    assert world.calls == []


@pytest.mark.parametrize("metadata,field", [
    ([], "$[0].metadata"),
    ({"layers": {}}, "metadata.layers"),
    ({"layers": ["invalid"]}, "last stack layer"),
    ({"layers": [{"branch": "top"}]}, "last stack layer"),
])
def test_d1_malformed_bead_shapes_name_producer_and_field(world, metadata, field):
    bead = "atm-phase-bj.stack"
    world.beads[bead]["metadata"] = metadata
    error = world.run()["err"]
    producer = f"bd show {bead} --json"
    assert error["code"] == "COMMAND.INVALID"
    assert producer in error["message"] and field in error["message"]
    assert producer in error["fix"] and field in error["fix"]
    assert "vars file and" not in error["fix"]
    assert world.writes == []


@pytest.mark.parametrize("payload,field", [
    ([], "$"),
    ({"events": {}}, "$.events"),
    ({"events": [42]}, "$.events[0]"),
    ({"events": [{"event": "assigned", "seq": 1}]}, "$.events[0].at"),
    ({"events": [{"event": "assigned", "at": "2026-10-19T00:00:00Z"}]}, "$.events[0].message_id/seq"),
])
def test_d1_malformed_atm_events_name_producer_and_field(world, payload, field):
    argv = ("atm", "task", "events", world.values["dev_task_id"], "--all", "--json")
    world.overrides[argv] = (0, json.dumps(payload))
    error = world.run()["err"]
    producer = " ".join(argv)
    assert error["code"] == "COMMAND.INVALID"
    assert producer in error["message"] and field in error["message"]
    assert producer in error["fix"] and field in error["fix"]
    assert world.writes == []


def close_from_files(world, role, dispatch, result=None):
    """Run the hook exactly as the lead and fixer do: the dispatch file, then the optional result file."""
    world.path.write_text(json.dumps(dispatch))
    argv = ["--role", role, "--vars", str(world.path)]
    if result is not None:
        world.result_path.write_text(json.dumps(result))
        argv += ["--result", str(world.result_path)]
    stdout, stderr = io.StringIO(), io.StringIO()
    with redirect_stdout(stdout), redirect_stderr(stderr):
        status = hook.main(argv, runner=world, clock=lambda: 42)
    output = json.loads(stdout.getvalue())
    assert status == int("err" in output)
    return output


@pytest.mark.parametrize("role", ["dev", "fix"])
def test_close_from_documented_dispatch_and_result_files_derives_handoff(world, role):
    dispatch = json.loads((EXAMPLES / f"task-close-vars-{role}.json").read_text())
    dispatch.update(primary_checkout=str(world.root), worktree_path=str(world.root))
    result = json.loads((EXAMPLES / f"task-close-result-{role}.json").read_text())
    metadata = world.beads[dispatch["checked_bead"]]["metadata"]
    metadata.pop("pr_number")
    metadata.pop("pr_url")
    output = close_from_files(world, role, dispatch, result)
    assert "ok" in output, output
    assert output["ok"]["commit"] == world.head
    assert output["ok"]["pr_number"] == str(world.pr_number)
    assignment = next(message for message in world.messages if message["type"] == "dev-sanity-task")
    derived = {"commit": world.head, "pr_number": str(world.pr_number),
               "pr_url": f"https://github.com/example/repo/pull/{world.pr_number}",
               "rebased_onto": "b" * 40}
    assert set(assignment["values"]) == set(dispatch) | set(result) | set(DERIVED) | {"close_context", "beads_dir",
                                                                                         "sanity_close_mode"}
    assert {name: assignment["values"][name] for name in derived} == derived
    assert re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", assignment["values"]["generated_at"])
    update = next(argv for argv, _ in world.writes if argv[:3] == ["bd", "update", dispatch["checked_bead"]])
    written = dict(update[index + 1].split("=", 1) for index, flag in enumerate(update) if flag == "--set-metadata")
    assert written == {"branch": dispatch["branch"], "pr_base": "deliberately-different",
                       **{name: derived[name] for name in ("commit", "pr_number", "pr_url")}}
    assert json.loads(world.path.read_text()) == dispatch
    assert json.loads(world.result_path.read_text()) == result


@pytest.mark.parametrize("role", ["dev", "fix"])
def test_fixer_fields_in_the_dispatch_file_are_refused_before_commands(world, role):
    contract = hook.header(hook.TEMPLATES / "task-close-vars.j2")
    example = example_values(role)
    example.update(primary_checkout=str(world.root), worktree_path=str(world.root))
    dispatch = {name: example[name] for name in contract["required_variables_by_role"][role]}
    dispatch.update(primary_checkout=str(world.root), worktree_path=str(world.root), outcome="fixed")
    result = {name: example[name] for name in contract["result_variables_by_role"][role]}
    output = close_from_files(world, role, dispatch, result)
    assert output["err"]["code"] == "VARS.INVALID" and "outcome" in output["err"]["message"]
    assert world.calls == []


def test_result_file_rejects_fields_that_are_not_its_role_fields(world):
    contract = hook.header(hook.TEMPLATES / "task-close-vars.j2")
    example = example_values("dev")
    example.update(primary_checkout=str(world.root), worktree_path=str(world.root))
    dispatch = {name: example[name] for name in contract["required_variables_by_role"]["dev"]}
    dispatch.update(primary_checkout=str(world.root), worktree_path=str(world.root))
    result = {name: example[name] for name in contract["result_variables_by_role"]["dev"]}
    result["resolution_md"] = "fix-only field"
    output = close_from_files(world, "dev", dispatch, result)
    assert output["err"]["code"] == "VARS.UNKNOWN" and "resolution_md" in output["err"]["message"]
    assert world.calls == []


def test_dev_and_fix_close_require_a_result_file(world):
    example = json.loads((EXAMPLES / "task-close-vars-fix.json").read_text())
    example.update(primary_checkout=str(world.root), worktree_path=str(world.root))
    output = close_from_files(world, "fix", example, None)
    assert output["err"]["code"] == "ARGS.INVALID"
    assert world.calls == []


@pytest.mark.parametrize("role,allowed", [("dev", "fixed"), ("fix", "fixed or not_reproducible")])
def test_bad_result_outcome_names_field_value_and_allowed_values_before_commands(world, role, allowed):
    output = world.run(role, outcome="done")
    assert output["err"] == {"code": "VARS.INVALID", "message": f'{role} outcome must be {allowed}, got "done"',
                             "fix": f"Set outcome to {allowed} in {world.result_path}, then rerun the hook."}
    assert world.calls == []


@pytest.mark.parametrize("role", ["dev", "fix"])
def test_completed_result_outcome_closes_as_fixed(world, role):
    assert "ok" in world.run(role, outcome="completed")
    assert hook.validate_outcome({"outcome": "completed"}, role, world.result_path) == {"outcome": "fixed"}


@pytest.mark.parametrize("role", ["dev", "fix"])
@pytest.mark.parametrize("skip", [False, True])
def test_empty_derived_commit_never_writes_even_when_checks_are_skipped(world, role, skip):
    world.overrides[("git", "rev-parse", "HEAD")] = (0, "")
    output = world.run(role, skip_checks=skip)
    assert output["err"]["code"] == "VARS.EMPTY"
    assert "commit" in output["err"]["message"]
    assert world.writes == []


@pytest.mark.parametrize("role", ["dev", "fix"])
@pytest.mark.parametrize("skip", [False, True])
@pytest.mark.parametrize("missing", ["pr_number", "pr_url"])
def test_offline_or_skipped_checks_require_recorded_pr_identity(world, role, skip, missing):
    world.github_failure = True
    world.beads[world.values["checked_bead"]]["metadata"][missing] = None
    output = world.run(role, skip_checks=skip)
    assert output["err"]["code"] == "VARS.EMPTY"
    assert missing in output["err"]["message"]
    assert "record" in output["err"]["fix"]
    assert world.writes == []


@pytest.mark.parametrize("role", ["dev", "fix"])
def test_skipped_checks_preserve_recorded_pr_identity(world, role):
    output = world.run(role, skip_checks=True)
    assert "ok" in output, output
    assert output["ok"]["pr_number"] == str(world.pr_number)
    assert not any(argv[0] == "gh" for argv, _ in world.calls)
    assignment = next(message for message in world.messages if message["type"] == "dev-sanity-task")
    assert assignment["values"]["pr_url"] == world.values["pr_url"]
    assert assignment["values"]["rebased_onto"] == "b" * 40



def checked_metadata_write(world):
    update = next(argv for argv, _ in world.writes if argv[:3] == ["bd", "update", world.values["checked_bead"]])
    return dict(update[index + 1].split("=", 1) for index, flag in enumerate(update) if flag == "--set-metadata")


@pytest.mark.parametrize("role", ["dev", "fix"])
def test_close_records_the_pr_base_from_the_same_pr_lookup(world, role):
    # QA reviews against the PR's actual base (the layer below), not the poured pr_target.
    output = world.run(role)
    assert "ok" in output, output
    assert checked_metadata_write(world)["pr_base"] == "deliberately-different"
    assert len([argv for argv, _ in world.calls if argv[0] == "gh"]) == 1


@pytest.mark.parametrize("role", ["dev", "fix"])
def test_offline_close_keeps_the_recorded_pr_base(world, role):
    world.github_failure = True
    world.beads[world.values["checked_bead"]]["metadata"]["pr_base"] = "fix/recorded-layer"
    output = world.run(role)
    assert "ok" in output, output
    assert "pr_base" not in checked_metadata_write(world)
    assert world.beads[world.values["checked_bead"]]["metadata"]["pr_base"] == "fix/recorded-layer"

def test_multiple_open_prs_at_head_are_refused_before_writes(world):
    prs = [{"number": number, "head": {"sha": world.head}} for number in (351, 352)]
    command = ("gh", "api", f"repos/example/repo/pulls?head=example:{world.values['branch']}&state=open")
    world.overrides[command] = (0, json.dumps(prs))
    output = world.run()
    assert output["err"]["code"] == "PR.AMBIGUOUS"
    assert "duplicate" in output["err"]["fix"]
    assert world.writes == []


@pytest.mark.parametrize("number", [None, "", 0])
def test_empty_discovered_pr_identity_is_refused_before_writes(world, number):
    command = ("gh", "api", f"repos/example/repo/pulls?head=example:{world.values['branch']}&state=open")
    world.overrides[command] = (0, json.dumps([{"number": number, "head": {"sha": world.head}}]))
    output = world.run()
    assert output["err"]["code"] == "VARS.EMPTY"
    assert "pr_number" in output["err"]["message"]
    assert world.writes == []


@pytest.mark.parametrize("role", ["dev", "fix", "sanity", "qa"])
def test_shared_render_uses_exact_role_fields_without_union_placeholders(world, monkeypatch, role):
    contract = hook.header(hook.TEMPLATES / "task-close-vars.j2")
    expected = set(contract["required_variables_by_role"][role]) | set(contract["result_variables_by_role"].get(role, []))
    assert set(contract["render_variables_by_role"][role]) == expected
    original = hook.sc_render.render
    rendered = []

    def render(template, values):
        if Path(template).name == "task-close-vars.j2":
            assert set(hook.header(Path(template))["required_variables"]) == expected
            assert set(values) == expected
            assert values == {name: world.values[name] for name in expected}
            rendered.append(True)
        return original(template, values)

    monkeypatch.setattr(hook.sc_render, "render", render)
    assert "ok" in world.run(role)
    assert rendered == [True]


@pytest.mark.parametrize("role", ["dev", "fix"])
def test_assignment_consumes_completion_vars_before_the_temporary_file_is_removed(world, role):
    assert "ok" in world.run(role)
    command = next(argv for argv, _ in world.calls if argv[:3] == ["atm", "task", "assign"])
    consumed = Path(command[command.index("--vars") + 1])
    assert consumed.name == "completion-vars.json"
    assert not consumed.exists()
    assignment = next(message for message in world.messages if message["type"] == "dev-sanity-task")
    assert assignment["values"]["commit"] == world.head
    assert world.values["checked_bead"] in assignment["body"]
    assert json.loads(world.path.read_text())


# The dev/fix fields sanity-close-vars and the sanity close read back from close_context (#557).
SANITY_READ = {"base", "carry_forward", "checked_title", "evidence", "layer", "layer_prs", "max_sanity_rounds", "outcome",
               "phase", "phase_feature", "policy_path", "pr_number", "pr_url", "primary_checkout", "qa_bead", "qa_task_id",
               "reviewers_round1", "round", "sprint_bead", "stack", "stack_bead"}


def sanity_close_vars():
    loader = importlib.machinery.SourceFileLoader("sanity_close_vars_for_close", str(SCRIPTS / "sanity-close-vars"))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


@pytest.mark.parametrize("role,outcome", [("dev", "fixed"), ("fix", "fixed"), ("fix", "not_reproducible")])
def test_sanity_close_context_carries_only_the_fields_the_sanity_close_reads(world, role, outcome):
    if outcome == "not_reproducible":
        world.beads[world.values["checked_bead"]]["metadata"]["role"] = "fix"
    assert "ok" in world.run(role, outcome=outcome)
    assignment = next(message for message in world.messages if message["type"] == "dev-sanity-task")["values"]
    context = assignment["close_context"]
    assert set(context) == (SANITY_READ & set(assignment)) | {"phase_lead", "budget"}
    assert context == {**{name: assignment[name] for name in SANITY_READ & set(assignment)},
                       "phase_lead": "phase-lead", "budget": "full"}
    # No other top-level field is written a second time inside close_context, and nothing nests.
    assert not (set(assignment) - SANITY_READ) & set(context)
    assert "close_context" not in context
    for name in ("stack_view", "validation_md", "deliverables_md", "lint_command", "pr_target", "rebased_onto"):
        assert name not in context


def test_sanity_close_context_subset_builds_the_same_sanity_close(world):
    assert "ok" in world.run()
    assignment = next(message for message in world.messages if message["type"] == "dev-sanity-task")["values"]
    close = sanity_close_vars()
    full = {**{name: value for name, value in assignment.items() if name not in ("close_context", "beads_dir")},
            "phase_lead": "phase-lead", "budget": "full"}

    def built(context, tmp):
        manifest = {"run_id": "run", "sha": assignment["commit"], "sanity_bead": assignment["sanity_bead"],
                    "dev_bead": assignment["checked_bead"], "branch": assignment["branch"],
                    "worktree_path": assignment["worktree_path"], "deliverables_total": 1, "close_context": context,
                    "checked_bead_data": {"title": "Checked", "labels": []}, "assignments": [{"number": 1}]}
        selected = {"run_id": "run", "reviewer": "sanity-selected", "commit": assignment["commit"],
                    "task_id": assignment["sanity_bead"], "checked_bead": assignment["checked_bead"],
                    "sprint": assignment["sprint"], "completed_at": "2026-10-24T00:00:00Z", "verdict": "PASS",
                    "findings_count": 0, "findings": [], "findings_md": "D1: done"}
        values = close.build(manifest, selected)
        stack_input = world.root / f"stack-input-{tmp}.json"
        assert close.write_stack_input(stack_input, values, manifest)
        return values, json.loads(stack_input.read_text())

    subset_values, subset_stack = built(assignment["close_context"], "subset")
    full_values, full_stack = built(full, "full")
    assert subset_stack == full_stack
    read = set(hook.header(hook.TEMPLATES / "task-close-vars.j2")["render_variables_by_role"]["sanity"]) | {"carry_forward"}
    for name in ("dev-sanity-complete.md.j2", "sanity-fail.xml.j2", "qa-template.xml.j2"):
        header = hook.header(hook.TEMPLATES / name)
        read |= set(header["required_variables"]) | set(header.get("defaults") or {})
    assert {name: subset_values.get(name) for name in read} == {name: full_values.get(name) for name in read}
    # The only differences are fields the sanity close never reads, which fall back to their defaults.
    differing = {name for name in full_values if subset_values.get(name) != full_values[name]}
    assert differing and not differing & read


def test_the_incident_stack_view_fits_in_one_sanity_assignment(world):
    # 785,090 bytes: the stack view whose sanity assignment, sent twice over, exceeded ATM's limit (#557).
    assert "ok" in world.run(stack_view="x" * 785_090)
    command = next(argv for argv, _ in world.calls if argv[:3] == ["atm", "task", "assign"])
    assignment = next(message for message in world.messages if message["type"] == "dev-sanity-task")["values"]
    assert assignment["stack_view"] == "x" * 785_090
    assert len(json.dumps(assignment).encode()) + len(Path(command[command.index("--template") + 1]).read_bytes()) <= 1_114_112


@pytest.mark.parametrize("role", ["dev", "fix"])
def test_oversized_sanity_assignment_is_refused_before_the_bead_closes(world, role):
    output = world.run(role, stack_view="x" * 1_200_000)
    assert output["err"]["code"] == "ASSIGNMENT.TOO_LARGE", output
    assert "1114112" in output["err"]["message"]
    assert "stack_view (1200002 bytes)" in output["err"]["message"]
    assert "--compact-json" in output["err"]["fix"]
    assert not any(argv[:2] == ["bd", "close"] for argv, _ in world.calls)
    assert world.writes == []


class ScriptedRunner:
    """Fake runner: the first `drops` calls fail like a dropped Dolt connection, or time out, then succeed."""

    def __init__(self, drops, *, timeouts=False):
        self.drops, self.timeouts, self.calls = drops, timeouts, 0

    def __call__(self, argv, **kwargs):
        self.calls += 1
        if self.calls <= self.drops:
            if self.timeouts:
                raise subprocess.TimeoutExpired(argv, kwargs["timeout"])
            return subprocess.CompletedProcess(argv, 1, "", "driver: invalid connection")
        return subprocess.CompletedProcess(argv, 0, "[]", "")


def retry_hook(world, runner):
    ticks, sleeps = iter(range(0, 10_000, 5)), []
    loads = iter(range(1, 100))
    sample = lambda: {"load_avg": [float(next(loads))] * 3, "cpu_pct": 50.0}
    return hook.Hook("dev", world.values, world.path, runner, lambda: next(ticks), sleeps.append, sample), sleeps


@pytest.mark.parametrize("drops", [0, 1, 3])
def test_dropped_bd_calls_are_retried_with_logged_attempts(world, drops):
    runner = ScriptedRunner(drops)
    instance, sleeps = retry_hook(world, runner)
    assert instance.run(["bd", "show", "x", "--json"]).returncode == 0
    assert runner.calls == drops + 1
    assert sleeps == [hook.DOLT_RETRY_SLEEP_S] * drops
    assert [a["outcome"] for a in instance.attempts] == ["dolt_drop_retry"] * drops + (["ok"] if drops else [])
    assert [a["attempt"] for a in instance.attempts] == list(range(1, len(instance.attempts) + 1))
    for record in instance.attempts:
        assert record["timeout_s"] == hook.COMMAND_TIMEOUT and record["elapsed_s"] == 5
        assert record["cpu_pct"] == 50.0 and len(record["load_avg"]) == 3
    if drops:
        assert instance.attempts[-1]["retries_until_now"] == drops


def test_bd_drop_beyond_the_retry_budget_fails_after_four_attempts(world):
    runner = ScriptedRunner(99)
    instance, sleeps = retry_hook(world, runner)
    with pytest.raises(hook.CloseError) as error:
        instance.run(["bd", "show", "x", "--json"])
    assert error.value.code == "COMMAND.FAILED" and "invalid connection" in str(error.value)
    assert runner.calls == hook.DOLT_RETRIES + 1 and len(sleeps) == hook.DOLT_RETRIES
    assert [a["outcome"] for a in instance.attempts] == ["dolt_drop_retry"] * hook.DOLT_RETRIES + ["failed"]


def test_timeouts_and_non_bd_failures_are_not_retried_but_are_logged(world):
    runner = ScriptedRunner(1, timeouts=True)
    instance, sleeps = retry_hook(world, runner)
    with pytest.raises(hook.CloseError, match="timed out"):
        instance.run(["bd", "show", "x", "--json"])
    assert runner.calls == 1 and sleeps == []
    assert [a["outcome"] for a in instance.attempts] == ["timeout"]
    runner = ScriptedRunner(99)
    instance, sleeps = retry_hook(world, runner)
    with pytest.raises(hook.CloseError):
        instance.run(["git", "status"])
    assert runner.calls == 1 and sleeps == []


def test_attempts_reach_the_hook_output(world):
    world.timeout_command = ["git", "rev-parse"]
    output = world.run()
    assert output["attempts"][0]["outcome"] == "timeout" and output["attempts"][0]["attempt"] == 1


def test_load_sample_is_portable(monkeypatch):
    monkeypatch.setattr(hook.os, "getloadavg", lambda: (4.0, 2.0, 1.0))
    monkeypatch.setattr(hook.os, "cpu_count", lambda: 8)
    assert hook.load_sample() == {"load_avg": [4.0, 2.0, 1.0], "cpu_pct": 50.0}
    def unavailable():
        raise OSError
    monkeypatch.setattr(hook.os, "getloadavg", unavailable)
    assert hook.load_sample() == {"load_avg": None, "cpu_pct": None}


# -- bead writes go through the bd-cache (#557) ----------------------------------------------------

def test_dev_close_leaves_its_recorded_fields_in_the_cache(world):
    assert "ok" in world.run()
    cache = hook.bd_cache.BdCache(world.root, runner=world)
    checked = cache.cached(world.values["checked_bead"])
    assert (checked["status"], checked["metadata"]["commit"], checked["metadata"]["pr_number"]) == (
        "closed", world.values["commit"], int(world.pr_number))
    journal = cache.status()["journal"]
    assert journal and all(state.endswith(":applied") for state in journal), journal


def test_a_write_bd_does_not_take_stays_journaled_and_applies_once_on_rerun(world):
    world.timeout_command = ["bd", "close"]
    result = world.run()
    assert result["err"]["code"] == "BEADS.WRITE_PENDING" and "journaled" in result["err"]["message"]
    world.timeout_command = None
    assert "ok" in world.run()
    closes = [argv for argv, _ in world.writes if argv[:3] == ["bd", "close", world.values["checked_bead"]]]
    assert len(closes) == 1 and world.beads[world.values["checked_bead"]]["status"] == "closed"


def test_a_write_bd_refuses_is_escalated_not_retried(world):
    reason = f"dev complete at {world.values['commit']}"
    world.overrides[("bd", "close", world.values["checked_bead"], "--reason", reason, "--actor", "caller")] = (13, "")
    result = world.run()
    assert result["err"]["code"] == "BEADS.WRITE_ESCALATED" and "bd_cache.py resolve" in result["err"]["fix"]
    assert not any(argv[:3] == ["atm", "task", "assign"] for argv, _ in world.writes)  # nothing is handed off


def test_a_busy_drain_is_waited_for_then_reported(world):
    cache = hook.bd_cache.BdCache(world.root, runner=world)
    with cache.drain_lock() as held:
        assert held
        result = world.run()
    assert result["err"]["code"] == "BEADS.WRITE_PENDING" and "busy" in result["err"]["message"]
    assert world.sleeps.count(hook.DRAIN_BUSY_SLEEP_S) == hook.DRAIN_TRIES
    assert "ok" in world.run()


def sanity_hook_row(**changes):
    """One `atm hook list --json` row for the phase's sanity task.close registration (SANITY-CLOSE-HOOK.md)."""
    return {"team": "test-team", "phase": "bj", "hook": "task.close", "active": True, "cwd": "/repo",
            "module_path": "/repo/.claude/skills/atm-bd-orchestration/scripts/sanity_task_close_hook.py",
            "custom": {"task_prefix": "atm-bj-", "sanity_agents": ["sanity"]}, "timeout_ms": 2500,
            "registered_by": "phase-lead", "registered_at": "2026-10-19T00:00:00Z", "disabled_at": None, **changes}


def sanity_assignment(world):
    return next(message for message in world.messages if message["type"] == "dev-sanity-task")


def test_sanity_dispatch_is_daemon_mode_when_the_phase_registers_the_sanity_close_hook(world):
    world.hooks = [sanity_hook_row()]
    assert "ok" in world.run()
    assignment = sanity_assignment(world)
    assert assignment["values"]["sanity_close_mode"] == "daemon"
    assert 'close-mode="daemon"' in assignment["body"]
    assert "sanity-task-close.md.j2" in assignment["body"]
    listed = [kwargs for argv, kwargs in world.calls if argv == ["atm", "hook", "list", "--phase", "bj", "--json"]]
    assert len(listed) == 1 and listed[0]["env"]["ATM_IDENTITY"] == "phase-lead"


@pytest.mark.parametrize("rows", [
    [],
    [sanity_hook_row(active=False, disabled_at="2026-10-19T01:00:00Z")],
    [sanity_hook_row(custom={"task_prefix": "atm-bj-", "sanity_agents": ["someone-else"]})],
    [sanity_hook_row(module_path="/repo/hooks/other_close_hook.py")],
    [sanity_hook_row(hook="task.start")],
    [sanity_hook_row(phase="bk")],
], ids=["none", "disabled", "other-agent", "other-module", "other-hook", "other-phase"])
def test_sanity_dispatch_stays_legacy_without_an_active_sanity_close_hook(world, rows):
    world.hooks = rows
    assert "ok" in world.run()
    assignment = sanity_assignment(world)
    assert assignment["values"]["sanity_close_mode"] == "legacy"
    assert 'close-mode="legacy"' in assignment["body"]


def test_sanity_dispatch_refuses_before_closing_when_hook_list_fails(world):
    world.overrides[("atm", "hook", "list", "--phase", "bj", "--json")] = (1, "")
    output = world.run()
    assert output["err"]["code"] == "COMMAND.FAILED"
    assert "atm hook list" in output["err"]["fix"]
    assert world.beads[world.values["checked_bead"]]["status"] == "in_progress"
    assert not [argv for argv, _identity in world.writes if argv[0] == "atm"]
