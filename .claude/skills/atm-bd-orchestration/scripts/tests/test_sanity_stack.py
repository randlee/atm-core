from __future__ import annotations

from contextlib import redirect_stderr, redirect_stdout
import copy
import importlib.machinery
import importlib.util
import io
import json
from pathlib import Path
import subprocess

import pytest

from stack_top import select_stack_top
from task_close_world import World as HookWorld, hook as close_hook

SCRIPTS = Path(__file__).resolve().parents[1]
LOADER = importlib.machinery.SourceFileLoader("sanity_stack_link", str(SCRIPTS / "sanity-stack-link"))
SPEC = importlib.util.spec_from_loader(LOADER.name, LOADER)
assert SPEC is not None
LINK = importlib.util.module_from_spec(SPEC)
LOADER.exec_module(LINK)


class Clock:
    def __init__(self):
        self.now = 1800000000.0

    def __call__(self):
        return self.now

class World:
    def __init__(self, tmp_path, monkeypatch):
        monkeypatch.setenv("ATM_IDENTITY", "sanity")
        self.root = tmp_path / "repo"
        origin = tmp_path / "origin.git"
        subprocess.run(["git", "init", "--bare", str(origin)], check=True, capture_output=True)
        subprocess.run(["git", "init", "-b", "trunk", str(self.root)], check=True, capture_output=True)
        self.git("config", "user.email", "test@example.com")
        self.git("config", "user.name", "Test")
        self.git("remote", "add", "origin", str(origin))
        self.git("commit", "--allow-empty", "-m", "trunk")
        self.git("push", "origin", "trunk")
        self.clock = Clock()
        self.bead = {"id": "phase.stack", "assignee": "", "metadata": {"trunk": "trunk", "stack_number": None, "layers": []}}
        self.beads = {self.bead["id"]: self.bead}
        self.pulls = []
        self.stacks = []
        self.calls = []
        self.cwds = []
        self.unavailable = False
        self.quota_failure = None
        self.fail_link = False
        self.cas_fail = False
        self.fail_verification = False
        self.release_failure = None
        self.advance_on_pulls = 0
        self.hook = None

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True, text=True).stdout.strip()

    def branch(self, name, base, number):
        self.git("checkout", "-b", name, base)
        self.git("commit", "--allow-empty", "-m", name)
        sha = self.git("rev-parse", "HEAD")
        self.git("push", "origin", name)
        self.pulls.append({"number": number, "base": base, "head": name, "sha": sha, "draft": False})
        return {"checked_bead": "dev", "branch": name, "pr_number": number, "commit": sha,
                "stack_bead": "phase.stack", "budget": "full"}

    def __call__(self, argv, **kwargs):
        self.calls.append(list(argv))
        self.cwds.append((argv[0], kwargs.get("cwd")))
        if self.hook is not None:
            return self.hook(argv, **kwargs)
        output, error, status = "", "", 0
        if argv[:2] == ["bd", "show"]:
            output = json.dumps([self.beads[argv[2]]])
        elif argv[:2] == ["bd", "heartbeat"]:
            assert self.bead["assignee"] == "sanity"
        elif argv[:2] == ["bd", "update"]:
            if "--claim" in argv:
                if self.bead["assignee"]:
                    status, error = 1, "held"
                else:
                    self.bead["assignee"] = "sanity"
            elif self.bead["assignee"] != "sanity" or (self.cas_fail and "--metadata" in argv):
                status, error = 13, "assignee changed"
                if self.cas_fail:
                    self.bead["assignee"] = ""
            elif "--metadata" in argv:
                self.bead["metadata"] = json.loads(Path(argv[argv.index("--metadata") + 1][1:]).read_text())
            elif self.release_failure == "exit13":
                status, error = 13, "injected release failure"
            elif self.release_failure == "transport":
                raise OSError("injected release transport failure")
            else:
                self.bead["assignee"] = ""
        elif argv[:3] == ["gh", "api", "rate_limit"]:
            if self.quota_failure == "timeout":
                raise subprocess.TimeoutExpired(argv, kwargs["timeout"])
            if self.quota_failure == "oserror":
                raise OSError("quota endpoint unavailable")
            if self.quota_failure == "nonzero":
                status, error = 1, "quota call failed"
            elif self.quota_failure == "invalid-json":
                output = "not JSON"
            else:
                output = json.dumps({"remaining": 0 if self.unavailable else 500})
        elif argv[:3] == ["gh", "api", "repos/{owner}/{repo}/pulls?state=open&per_page=100"]:
            self.clock.now += self.advance_on_pulls
            self.advance_on_pulls = 0
            output = json.dumps(self.pulls)
        elif argv[:3] == ["gh", "api", "repos/{owner}/{repo}/stacks"]:
            if self.fail_verification and self.stacks:
                status, error = 1, "injected hosted verification failure"
            else:
                output = json.dumps(self.stacks)
        elif argv[:3] == ["gh", "stack", "link"]:
            if self.fail_link:
                status, error = 1, "injected link failure"
            elif argv[3] == "--base":
                self.stacks.append({"number": 7, "base": argv[4], "prs": [int(value) for value in argv[5:]]})
            else:
                stack = next(item for item in self.stacks if item["number"] == int(argv[3]))
                stack["prs"].append(int(argv[4]))
        elif argv[0] == "git":
            return subprocess.run(argv, **{"cwd": self.root, **kwargs})
        else:
            raise AssertionError(argv)
        return subprocess.CompletedProcess(argv, status, output, error)

    def link(self, values):
        path = self.root / "input.json"
        path.write_text(json.dumps(values))
        output = io.StringIO()
        with redirect_stdout(output):
            status = LINK.main(["--input", str(path)], runner=self, clock=self.clock)
        result = json.loads(output.getvalue())
        assert status == (result["result"] == "stack_failed")
        return result


@pytest.fixture
def world(tmp_path, monkeypatch):
    return World(tmp_path, monkeypatch)


def test_already_linked_lower_layer_at_pinned_commit_is_stacked(world):
    first = world.branch('first', 'trunk', 1)
    second = world.branch('second', 'first', 2)
    assert world.link(first)['result'] == 'stacked'
    assert world.link(second)['result'] == 'stacked'
    before = copy.deepcopy(world.bead['metadata'])
    world.calls.clear()
    replay = world.link(first)
    assert replay['result'] == 'stacked'
    assert replay['top']['branch'] == 'second'
    assert world.bead['metadata'] == before
    assert not any(call[:3] == ['gh', 'stack', 'link'] for call in world.calls)


def test_unlinked_recorded_layer_is_not_reported_as_already_stacked(world):
    values = world.branch('first', 'trunk', 1)
    world.bead['metadata']['layers'].append({
        'branch': values['branch'], 'pr': values['pr_number'], 'head': values['commit'],
        'base': 'trunk', 'linked': False,
    })

    result = world.link(values)

    assert result['result'] == 'stacked'
    assert world.bead['metadata']['layers'][-1]['linked'] is True
    assert [call for call in world.calls if call[:3] == ['gh', 'stack', 'link']] == [
        ['gh', 'stack', 'link', '--base', 'trunk', '1'],
    ]


def test_failed_post_close_handoff_notifies_lead_without_task_writes(world):
    values = world.branch('first', 'trunk', 1)
    values['lead'] = 'phase-lead'
    world.pulls.clear()
    notices = []

    def run(argv, **kwargs):
        if argv[:2] == ['atm', 'send']:
            notices.append(argv)
            return subprocess.CompletedProcess(argv, 0, '', '')
        return world(argv, **kwargs)

    path = world.root / 'post-close.json'
    path.write_text(json.dumps(values))
    output = io.StringIO()
    with redirect_stdout(output):
        status = LINK.main(['--input', str(path)], runner=run, clock=world.clock)
    assert status == 1
    assert json.loads(output.getvalue())['result'] == 'stack_failed'
    assert len(notices) == 1 and notices[0][2] == 'phase-lead'
    assert 'already closed' in notices[0][3]
    assert not any(call[:2] == ['atm', 'task'] for call in world.calls)


def test_stack_top_selector_drives_empty_and_populated_ledgers(world, monkeypatch):
    selected = []

    def capture(bead, fallback, resolve_head):
        top = select_stack_top(bead, fallback, resolve_head)
        selected.append((copy.deepcopy(bead['metadata']['layers']), fallback, top))
        return top

    monkeypatch.setattr(LINK, 'select_stack_top', capture)
    first = world.branch('first', 'trunk', 1)
    second = world.branch('second', 'first', 2)
    assert world.link(first)['result'] == 'stacked'
    assert world.link(second)['result'] == 'stacked'
    assert selected[0] == ([], 'trunk', ('trunk', world.git('rev-parse', 'origin/trunk')))
    assert selected[1][0][-1]['branch'] == 'first'
    assert selected[1][1:] == ('trunk', ('first', first['commit']))


@pytest.mark.parametrize('replay', [False, True])
def test_recorded_head_check_uses_selected_head(world, monkeypatch, replay):
    first = world.branch('first', 'trunk', 1)
    second = world.branch('second', 'first', 2)
    assert world.link(first)['result'] == 'stacked'
    before = copy.deepcopy(world.bead['metadata'])
    monkeypatch.setattr(LINK, 'select_stack_top', lambda bead, fallback, resolve: ('first', 'wrong-head'))
    result = world.link(first if replay else second)
    assert result['result'] == 'stack_failed'
    assert ('recorded stack top' if replay else 'recorded head') in result['evidence']
    assert world.bead['metadata'] == before


def test_three_passes_link_in_verdict_order(world):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    third = world.branch("third", "second", 3)
    assert [world.link(item)["result"] for item in (first, second, third)] == ["stacked"] * 3
    metadata = world.bead["metadata"]
    assert [item["branch"] for item in metadata["layers"]] == ["first", "second", "third"]
    assert all(item["linked"] for item in metadata["layers"])
    assert metadata["stack_number"] == 7
    assert world.bead["assignee"] == ""
    assert not any(call[0] == "python3" for call in world.calls)


@pytest.mark.parametrize("existing_stack", [False, True])
def test_full_mode_verifies_hosted_stack_without_view_helper(world, monkeypatch, tmp_path, existing_stack):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    assert world.link(first)["result"] == "stacked"
    if existing_stack:
        assert world.link(second)["result"] == "stacked"
        second = world.branch("third", "second", 3)
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.delenv("ATM_STACK_VIEW_SCRIPT", raising=False)
    assert not (tmp_path / ".claude/scripts/gh_stack_view.py").exists()
    world.calls.clear()
    result = world.link(second)
    assert result["result"] == "stacked"
    assert result["mode"] == "full"
    assert result["lean_reason"] is None
    assert result["top"]["linked"] is True
    assert [call for call in world.calls if call[:3] == ["gh", "api", "repos/{owner}/{repo}/stacks"]]
    assert not any(call[0] == "python3" for call in world.calls)
    assert world.stacks == [{"number": 7, "base": "trunk", "prs": [1, 2, 3] if existing_stack else [1, 2]}]
    assert world.bead["assignee"] == ""


@pytest.mark.parametrize("budget,failure,unavailable,mode,reason", [
    ("lean", None, False, "lean", "budget_lean"),
    ("full", None, True, "lean", "graphql_remaining_0"),
    ("full", "nonzero", False, "lean", "rate_limit_call_failed"),
    ("full", "timeout", False, "lean", "rate_limit_call_failed"),
    ("full", "oserror", False, "lean", "rate_limit_call_failed"),
    ("full", "invalid-json", False, "lean", "rate_limit_response_invalid"),
    ("full", None, False, "full", None),
])
def test_mode_and_lean_cause_are_reported(world, capsys, budget, failure, unavailable, mode, reason):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    assert world.link(first)["result"] == "stacked"
    second["budget"] = budget
    world.quota_failure = failure
    world.unavailable = unavailable
    world.calls.clear()
    capsys.readouterr()
    result = world.link(second)
    assert result["result"] == "stacked"
    assert (result["mode"], result["lean_reason"]) == (mode, reason)
    assert result["top"]["linked"] is (mode == "full")
    assert capsys.readouterr().err.splitlines() == ([f"sanity-stack-link: lean mode: {reason}"] if reason else [])
    if mode == "lean":
        assert not any(call[:3] in (["gh", "api", "repos/{owner}/{repo}/stacks"], ["gh", "stack", "link"])
                       for call in world.calls)


def test_lean_run_does_not_require_view_tool(world, monkeypatch, tmp_path):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    assert world.link(first)["result"] == "stacked"
    monkeypatch.setenv("ATM_STACK_VIEW_SCRIPT", str(tmp_path / "missing-view.py"))
    second["budget"] = "lean"
    world.calls.clear()
    assert world.link(second)["result"] == "stacked"
    assert not any(call[:3] == ["gh", "stack", "link"] or call[0] == "python3" for call in world.calls)


def test_divergence_never_links_or_writes(world):
    first = world.branch("first", "trunk", 1)
    rival = world.branch("rival", "trunk", 2)
    assert world.link(first)["result"] == "stacked"
    before = copy.deepcopy(world.bead["metadata"])
    world.calls.clear()
    assert world.link(rival)["result"] == "rebase_required"
    assert world.bead["metadata"] == before
    assert not any(call[:3] == ["gh", "stack", "link"] or "--metadata" in call for call in world.calls)


def test_moved_ledger_top_fails_closed(world):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    assert world.link(first)["result"] == "stacked"
    before = copy.deepcopy(world.bead["metadata"])
    world.git("checkout", "first")
    world.git("commit", "--allow-empty", "-m", "moved")
    world.git("push", "origin", "first")
    result = world.link(second)
    assert result["result"] == "stack_failed"
    assert "recorded head" in result["evidence"]
    assert world.bead["metadata"] == before


def test_replaying_same_pr_with_changed_head_cannot_claim_stack_success(world):
    values = world.branch("first", "trunk", 1)
    assert world.link(values)["result"] == "stacked"
    before = copy.deepcopy(world.bead["metadata"])
    world.git("commit", "--allow-empty", "--message", "new head")
    world.git("push", "origin", "first")
    values["commit"] = world.git("rev-parse", "HEAD")
    world.pulls[0]["sha"] = values["commit"]
    assert world.link(values)["result"] == "stack_failed"
    assert world.bead["metadata"] == before


@pytest.mark.parametrize("budget,unavailable", [("lean", False), ("full", True)])
def test_lean_uses_only_rest_and_defers_link(world, budget, unavailable):
    first = world.branch("first", "trunk", 1)
    assert world.link(first)["top"]["linked"] is False
    first_layer = {"branch": "first", "pr": 1, "head": first["commit"], "base": "trunk", "linked": False}
    assert world.bead["metadata"]["layers"] == [first_layer]
    assert world.bead["metadata"]["stack_number"] is None

    second = world.branch("second", "first", 2)
    assert world.link(second)["result"] == "stacked"
    assert all(layer["linked"] for layer in world.bead["metadata"]["layers"])
    assert world.bead["metadata"]["stack_number"] == 7
    assert [call for call in world.calls if call[:3] == ["gh", "stack", "link"]] == [
        ["gh", "stack", "link", "--base", "trunk", "1", "2"]]

    world.calls.clear()
    deferred = world.branch("deferred", "second", 3)
    deferred["budget"] = budget
    world.unavailable = unavailable
    result = world.link(deferred)
    assert result["result"] == "stacked"
    assert result["top"]["linked"] is False
    assert world.bead["metadata"]["layers"][-1]["linked"] is False
    assert len([call for call in world.calls if len(call) > 2 and "/pulls?" in call[2]]) == 1
    assert not any(call[:3] == ["gh", "api", "repos/{owner}/{repo}/stacks"] for call in world.calls)
    assert not any(call[:2] == ["gh", "api"] and len(call) > 2 and "graphql" in call[2].lower()
                   for call in world.calls)
    assert not any(call[:3] == ["gh", "stack", "link"] for call in world.calls)
    assert not any(call[0] == "python3" and any("gh_stack_view.py" in arg for arg in call)
                   for call in world.calls)
    rate_limit_calls = [call for call in world.calls if call[:3] == ["gh", "api", "rate_limit"]]
    assert bool(rate_limit_calls) is (budget == "full")

    world.unavailable = False
    fourth = world.branch("fourth", "deferred", 4)
    world.calls.clear()
    assert world.link(fourth)["result"] == "stacked"
    assert len([call for call in world.calls if call[:3] == ["gh", "api", "repos/{owner}/{repo}/stacks"]]) == 2
    assert all(layer["linked"] for layer in world.bead["metadata"]["layers"])
    assert [call for call in world.calls if call[:3] == ["gh", "stack", "link"]] == [
        ["gh", "stack", "link", "7", "3"],
        ["gh", "stack", "link", "7", "4"]]


def test_lock_timeout_returns_after_one_claim_attempt(world):
    values = world.branch("first", "trunk", 1)
    world.bead.update(assignee="other", started_at="2027-01-01T00:00:00Z")
    start = world.clock()
    result = world.link(values)
    assert result == {"result": "lock_timeout", "holder": "other", "claimed_at": "2027-01-01T00:00:00Z"}
    assert world.clock() == start
    claim_calls = [call for call in world.calls if call[:2] == ["bd", "update"] and "--claim" in call]
    assert len(claim_calls) == 1
    assert not any(call[:2] == ["bd", "reclaim"] for call in world.calls)


def test_heartbeat_uses_controlled_clock(world):
    values = world.branch("first", "trunk", 1)
    world.advance_on_pulls = 120
    assert world.link(values)["result"] == "stacked"
    assert len([call for call in world.calls if call[:2] == ["bd", "heartbeat"]]) == 1


@pytest.mark.parametrize("failure", ["exit13", "transport"])
def test_release_failure_after_write_keeps_stacked_result(world, failure):
    values = world.branch("first", "trunk", 1)
    world.release_failure = failure
    result = world.link(values)
    assert result["result"] == "stacked"
    assert result["top"] == world.bead["metadata"]["layers"][-1]
    assert result["top"]["head"] == values["commit"]
    assert "injected release" in result["release_warning"]
    assert world.bead["assignee"] == "sanity"


def test_release_failure_after_recorded_unlinked_layer_is_retried(world):
    values = world.branch("first", "trunk", 1)
    assert world.link(values)["result"] == "stacked"
    before = copy.deepcopy(world.bead["metadata"])
    world.release_failure = "exit13"
    world.calls.clear()
    result = world.link(values)
    assert result["result"] == "stacked"
    assert "injected release failure" in result["release_warning"]
    assert world.bead["metadata"] != before
    assert world.bead["metadata"]["layers"][0]["linked"] is True
    assert world.bead["metadata"]["stack_number"] == 7
    assert any("--metadata" in call for call in world.calls)


def test_precommit_failure_with_release_failure_preserves_both_errors(world):
    values = world.branch("first", "trunk", 1)
    before = copy.deepcopy(world.bead["metadata"])
    world.pulls[0]["draft"] = True
    world.release_failure = "exit13"
    result = world.link(values)
    assert result["result"] == "stack_failed"
    assert "missing, draft" in result["evidence"]
    assert "injected release failure" in result["evidence"]
    assert world.bead["metadata"] == before


def test_release_failure_after_rebase_required_is_stack_failed(world):
    first = world.branch("first", "trunk", 1)
    rival = world.branch("rival", "trunk", 2)
    assert world.link(first)["result"] == "stacked"
    before = copy.deepcopy(world.bead["metadata"])
    world.release_failure = "exit13"
    result = world.link(rival)
    assert result["result"] == "stack_failed"
    assert "injected release failure" in result["evidence"]
    assert world.bead["metadata"] == before


@pytest.mark.parametrize("failure", ["link", "cas", "ambiguous"])
def test_failures_leave_ledger_and_release(world, failure):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    assert world.link(first)["result"] == "stacked"
    before = copy.deepcopy(world.bead["metadata"])
    if failure == "link":
        world.fail_link = True
    elif failure == "cas":
        world.cas_fail = True
    else:
        world.stacks = [{"number": 7, "base": "trunk", "prs": [1]}, {"number": 8, "base": "trunk", "prs": [1]}]
    assert world.link(second)["result"] == "stack_failed"
    assert world.bead["metadata"] == before
    assert world.bead["assignee"] == ""


def test_lean_preserves_ledger_without_reconciling_torn_hosted_link(world):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    third = world.branch("third", "second", 3)
    first["budget"] = second["budget"] = "lean"
    assert world.link(first)["result"] == "stacked"
    assert world.link(second)["result"] == "stacked"
    world.stacks = [{"number": 7, "base": "trunk", "prs": [1]}]
    third["budget"] = "lean"
    assert world.link(third)["result"] == "stacked"
    assert [item["linked"] for item in world.bead["metadata"]["layers"]] == [False, False, False]
    assert world.bead["metadata"]["stack_number"] is None


def test_retry_recovers_hosted_only_torn_link(world):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    assert world.link(first)["result"] == "stacked"
    before = copy.deepcopy(world.bead["metadata"])
    world.fail_verification = True
    assert world.link(second)["result"] == "stack_failed"
    assert world.bead["metadata"] == before
    world.fail_verification = False
    world.calls.clear()
    assert world.link(second)["result"] == "stacked"
    assert [item["pr"] for item in world.bead["metadata"]["layers"]] == [1, 2]
    assert not any(call[:3] == ["gh", "stack", "link"] for call in world.calls)


def test_next_verdict_recovers_prior_hosted_only_torn_link(world):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    third = world.branch("third", "second", 3)
    assert world.link(first)["result"] == "stacked"
    world.fail_verification = True
    assert world.link(second)["result"] == "stack_failed"
    world.fail_verification = False
    assert world.link(third)["result"] == "stacked"
    assert [(item["pr"], item["linked"]) for item in world.bead["metadata"]["layers"]] == [(1, True), (2, True), (3, True)]


def test_rebase_then_real_sanity_hook_preserves_round_and_hands_off(world, tmp_path, monkeypatch):
    first = world.branch("first", "trunk", 1)
    rival = world.branch("rival", "trunk", 2)
    assert world.link(first)["result"] == "stacked"
    assert world.link(rival)["result"] == "rebase_required"
    world.git("checkout", "rival")
    world.git("rebase", "first")
    world.git("push", "--force", "origin", "rival")
    rival["commit"] = world.git("rev-parse", "HEAD")
    world.pulls[1].update(sha=rival["commit"], base="first")
    result = world.link(rival)
    assert result["result"] == "stacked"

    hook_root = tmp_path / "hook"
    hook_root.mkdir()
    hook = HookWorld(hook_root, monkeypatch)
    hook.beads["phase.stack"] = world.bead
    hook.beads[hook.values["sanity_bead"]]["metadata"]["sanity_round"] = 2
    hook.values.update(stack_bead="phase.stack", branch="rival", commit=rival["commit"],
                       pr_number=2, base="first", stacked=result)
    hook.tasks[hook.values["sanity_task_id"]].append(hook.assignment(1))
    hook.path.write_text(json.dumps(hook.values))
    world.hook = hook
    output, errors = io.StringIO(), io.StringIO()
    with redirect_stdout(output), redirect_stderr(errors):
        code = close_hook.main(["--role", "sanity", "--vars", str(hook.path)], runner=world, clock=world.clock)
    assert code == 0, errors.getvalue()
    outcome = json.loads(output.getvalue())
    assert "ok" in outcome
    sanity = hook.beads[hook.values["sanity_bead"]]
    assert sanity["close_reason"] == "PASS at " + rival["commit"]
    assert sanity["metadata"]["sanity_round"] == 2
    assert not any(command[:4] == ["atm", "task", "close", hook.values["dev_task_id"]] for command, _actor in hook.writes)
    qa = next(message for message in hook.messages if message["type"] == "qa-task")
    assert (qa["sender"], qa["recipient"], qa["values"]["base"]) == ("phase-lead", "quality", "first")


def test_git_and_gh_run_in_the_checked_layers_worktree_and_bd_does_not(world, tmp_path):
    values = world.branch("layer", "trunk", 11)
    values["worktree_path"] = str(tmp_path / "other-repo-worktree")
    seen = {"git": set(), "gh": set(), "bd": set()}

    def runner(argv, **kwargs):
        seen[argv[0]].add(kwargs.get("cwd"))
        return subprocess.CompletedProcess(argv, 0, "", "")

    linker = LINK.Linker(values, runner, world.clock, "sanity")
    for argv in (["git", "fetch", "origin"], ["gh", "api", "rate_limit"], ["bd", "show", "x"]):
        linker.command(argv)
    assert seen == {"git": {values["worktree_path"]}, "gh": {values["worktree_path"]}, "bd": {None}}


def test_without_worktree_path_commands_keep_the_callers_cwd(world):
    values = world.branch("layer", "trunk", 11)
    linker = LINK.Linker(values, lambda argv, **kw: subprocess.CompletedProcess(argv, 0, "", "") if "cwd" not in kw
                         else (_ for _ in ()).throw(AssertionError("cwd passed")), world.clock, "sanity")
    linker.command(["git", "fetch", "origin"])
    linker.command(["gh", "api", "rate_limit"])


def test_full_flow_passes_the_worktree_cwd_to_every_git_and_gh_call(world):
    values = world.branch("layer", "trunk", 11)
    values["worktree_path"] = str(world.root)
    result = world.link(values)
    assert result["result"] == "stacked"
    git_gh = [cwd for kind, cwd in world.cwds if kind in ("git", "gh")]
    assert git_gh and set(git_gh) == {str(world.root)}
    assert {cwd for kind, cwd in world.cwds if kind == "bd"} == {None}


def test_invalid_worktree_path_is_stack_failed(world):
    values = world.branch("layer", "trunk", 11)
    values["worktree_path"] = 5
    assert world.link(values)["result"] == "stack_failed"


def test_missing_input_key_is_stack_failed_and_releases(world, monkeypatch):
    values = world.branch("layer", "trunk", 11)

    def boom(self):
        return self.values["absent_key"]

    monkeypatch.setattr(LINK.Linker, "execute", boom)
    result = world.link(values)
    assert result["result"] == "stack_failed" and "absent_key" in result["evidence"]
