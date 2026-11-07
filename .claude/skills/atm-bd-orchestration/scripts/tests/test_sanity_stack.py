from __future__ import annotations

from contextlib import redirect_stderr, redirect_stdout
import copy
import importlib.machinery
import importlib.util
import io
import json
import os
import shlex
from pathlib import Path
import subprocess
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "atm-beads/scripts"))

from stack_top import StackTop, select_confirmed_stack_top
from task_close_world import World as HookWorld, hook as close_hook

SCRIPTS = Path(__file__).resolve().parents[1]
GIT_TIMEOUT_SECONDS = 15
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
        self.git_env = {
            key: value for key, value in os.environ.items() if not key.startswith("GIT_")
        }
        self.git_env.update(
            {
                "GIT_CONFIG_GLOBAL": str(tmp_path / "empty-gitconfig"),
                "GIT_CONFIG_NOSYSTEM": "1",
                "GIT_CONFIG_COUNT": "1",
                "GIT_CONFIG_KEY_0": "core.hooksPath",
                "GIT_CONFIG_VALUE_0": str(tmp_path / "empty-hooks"),
            }
        )
        (tmp_path / "empty-gitconfig").touch()
        (tmp_path / "empty-hooks").mkdir()
        subprocess.run(
            ["git", "init", "--bare", str(origin)],
            check=True,
            capture_output=True,
            env=self.git_env,
            timeout=GIT_TIMEOUT_SECONDS,
        )
        subprocess.run(
            ["git", "init", "-b", "trunk", str(self.root)],
            check=True,
            capture_output=True,
            env=self.git_env,
            timeout=GIT_TIMEOUT_SECONDS,
        )
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
        return subprocess.run(
            ["git", *args],
            cwd=self.root,
            check=True,
            capture_output=True,
            text=True,
            env=self.git_env,
            timeout=GIT_TIMEOUT_SECONDS,
        ).stdout.strip()

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
            elif argv[3] == "--base" and len(argv) < 7:
                status, error = 1, "accepts between 2 and 255 arg(s), received 1"   # gh: a stack needs two PRs
            elif argv[3] == "--base":
                self.stacks[:] = [{"number": 7, "base": argv[4], "prs": [int(value) for value in argv[5:]]}]
            else:
                stack = next(item for item in self.stacks if item["number"] == int(argv[3]))
                stack["prs"].append(int(argv[4]))
        elif argv[0] == "git":
            return subprocess.run(
                argv,
                **{
                    "cwd": self.root,
                    "env": self.git_env,
                    "timeout": GIT_TIMEOUT_SECONDS,
                    **kwargs,
                },
            )
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


def test_world_git_runner_ignores_host_hooks(tmp_path, monkeypatch):
    marker = tmp_path / "host-hook-ran"
    hooks = tmp_path / "host-hooks"
    hooks.mkdir()
    hook = hooks / "pre-commit"
    hook.write_text(f"#!/bin/sh\nprintf ran > {shlex.quote(str(marker))}\n")
    hook.chmod(0o755)
    config = tmp_path / "host-gitconfig"
    config.write_text(f"[core]\n\thooksPath = {hooks}\n")
    monkeypatch.setenv("GIT_CONFIG_GLOBAL", str(config))
    monkeypatch.delenv("GIT_CONFIG_NOSYSTEM", raising=False)

    fixture_root = tmp_path / "fixture"
    fixture_root.mkdir()
    World(fixture_root, monkeypatch)

    assert not marker.exists()


def test_equal_sha_open_parents_are_refused_as_ambiguous(world):
    parent = world.branch('parent', 'trunk', 1)
    world.git('branch', 'twin', parent['commit'])
    world.git('push', 'origin', 'twin')
    world.pulls.append({'number': 2, 'base': 'trunk', 'head': 'twin', 'sha': parent['commit'], 'draft': False})
    child = world.branch('child', 'parent', 3)
    before = copy.deepcopy(world.bead['metadata'])

    result = world.link(child)

    assert result['result'] == 'stack_failed'
    assert 'multiple incomparable open pull request parents' in result['evidence']
    assert world.bead['metadata'] == before
    assert not any(call[:3] == ['gh', 'stack', 'link'] for call in world.calls)


def test_open_pr_already_in_trunk_is_not_a_pending_parent(world):
    trunk = world.git('rev-parse', 'origin/trunk')
    world.git('branch', 'unrelated', trunk)
    world.git('push', 'origin', 'unrelated')
    world.pulls.append({'number': 9, 'base': 'trunk', 'head': 'unrelated', 'sha': trunk, 'draft': False})
    first = world.branch('first', 'trunk', 1)

    result = world.link(first)

    assert result['result'] == 'stacked'
    assert world.bead['metadata']['layers'][0]['branch'] == 'first'
    assert 'held_on' not in world.bead['metadata']['layers'][0]


def test_ready_child_of_draft_parent_is_held_and_draft_candidate_is_refused(world):
    parent = world.branch('parent', 'trunk', 1)
    child = world.branch('child', 'parent', 2)
    world.pulls[0]['draft'] = True

    refused = world.link(parent)
    assert refused['result'] == 'stack_failed'
    assert 'draft' in refused['evidence']
    held = world.link(child)
    assert held['result'] == 'held'
    assert held['held_on'] == {'pr': 1, 'branch': 'parent'}


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


@pytest.mark.parametrize('legacy', [False, True])
def test_advanced_trunk_keeps_frozen_bottom_and_links_merged_top(world, legacy):
    cut = world.git('rev-parse', 'origin/trunk')
    first = world.branch('first', 'trunk', 1)
    second = world.branch('second', 'first', 2)
    assert world.link(first)['result'] == 'stacked'
    assert world.link(second)['result'] == 'stacked'
    if legacy:
        world.bead['metadata']['layers'][0].pop('base_head', None)
    world.git('checkout', 'trunk')
    (world.root / 'trunk.txt').write_text('new trunk work\n')
    world.git('add', 'trunk.txt')
    world.git('commit', '-m', 'advance trunk after bottom froze')
    world.git('push', 'origin', 'trunk')
    third = world.branch('third', 'second', 3)
    world.git('merge', '--no-edit', 'origin/trunk')
    third['commit'] = world.git('rev-parse', 'HEAD')
    world.pulls[-1]['sha'] = third['commit']
    world.git('push', 'origin', 'third')

    result = world.link(third)

    assert result['result'] == 'stacked'
    assert world.stacks[0]['prs'] == [1, 2, 3]
    assert world.git('rev-parse', 'origin/first') == first['commit']
    bottom = world.bead['metadata']['layers'][0]
    assert bottom.get('base_head') == (None if legacy else cut)
    assert world.link(first)['result'] == 'stacked'


def test_unavailable_recorded_bottom_cut_fails_without_writes(world):
    first = world.branch('first', 'trunk', 1)
    second = world.branch('second', 'first', 2)
    assert world.link(first)['result'] == 'stacked'
    assert world.link(second)['result'] == 'stacked'
    world.bead['metadata']['layers'][0]['base_head'] = 'f' * 40
    before = copy.deepcopy(world.bead['metadata'])
    world.calls.clear()

    result = world.link(second)

    assert result['result'] == 'stack_failed'
    assert 'cut ancestry' in result['evidence']
    assert world.bead['metadata'] == before
    assert not any(call[:3] == ['gh', 'stack', 'link'] or '--metadata' in call for call in world.calls)


def test_legacy_bottom_rejects_unrelated_trunk_history(world):
    first = world.branch('first', 'trunk', 1)
    second = world.branch('second', 'first', 2)
    assert world.link(first)['result'] == 'stacked'
    assert world.link(second)['result'] == 'stacked'
    world.bead['metadata']['layers'][0].pop('base_head')
    world.git('checkout', '--orphan', 'replacement')
    world.git('commit', '--allow-empty', '-m', 'unrelated trunk')
    world.git('push', '--force', 'origin', 'replacement:trunk')
    before = copy.deepcopy(world.bead['metadata'])
    world.calls.clear()

    result = world.link(second)

    assert result['result'] == 'stack_failed'
    assert 'cut ancestry' in result['evidence']
    assert world.bead['metadata'] == before
    assert not any(call[:3] == ['gh', 'stack', 'link'] or '--metadata' in call for call in world.calls)


def test_hosted_interlayer_divergence_remains_an_error(world):
    first = world.branch('first', 'trunk', 1)
    second = world.branch('second', 'first', 2)
    assert world.link(first)['result'] == 'stacked'
    assert world.link(second)['result'] == 'stacked'
    rival = world.branch('rival', 'trunk', 3)
    world.git('push', '--force', 'origin', 'rival:second')
    world.pulls[1]['sha'] = rival['commit']
    world.bead['metadata']['layers'][1]['head'] = rival['commit']
    before = copy.deepcopy(world.bead['metadata'])
    world.calls.clear()

    result = world.link(first)

    assert result['result'] == 'stack_failed'
    assert 'conflicting git ancestry' in result['evidence']
    assert world.bead['metadata'] == before
    assert not any(call[:3] == ['gh', 'stack', 'link'] or '--metadata' in call for call in world.calls)


def test_trunk_conflict_fails_before_appending_or_writing(world):
    first = world.branch('first', 'trunk', 1)
    second = world.branch('second', 'first', 2)
    assert world.link(first)['result'] == 'stacked'
    assert world.link(second)['result'] == 'stacked'
    world.git('checkout', 'trunk')
    (world.root / 'conflict.txt').write_text('trunk\n')
    world.git('add', 'conflict.txt')
    world.git('commit', '-m', 'trunk change')
    world.git('push', 'origin', 'trunk')
    third = world.branch('third', 'second', 3)
    (world.root / 'conflict.txt').write_text('layer\n')
    world.git('add', 'conflict.txt')
    world.git('commit', '-m', 'conflicting layer change')
    third['commit'] = world.git('rev-parse', 'HEAD')
    world.pulls[-1]['sha'] = third['commit']
    world.git('push', 'origin', 'third')
    before = copy.deepcopy(world.bead['metadata'])
    world.calls.clear()

    result = world.link(third)

    assert result['result'] == 'stack_failed'
    assert 'merge cleanly into current trunk' in result['evidence']
    assert world.bead['metadata'] == before
    assert world.stacks[0]['prs'] == [1, 2]
    assert not any(call[:3] == ['gh', 'stack', 'link'] or '--metadata' in call for call in world.calls)


def test_unlinked_recorded_layer_is_not_reported_as_already_stacked(world):
    values = world.branch('first', 'trunk', 1)
    world.bead['metadata']['layers'].append({
        'branch': values['branch'], 'pr': values['pr_number'], 'head': values['commit'],
        'base': 'trunk', 'linked': False,
    })

    result = world.link(values)

    # Layer 0 alone waits unlinked: no one-PR `gh stack link`, which gh refuses (phase bg, PR 2251).
    assert result['result'] == 'stacked' and result['stack_number'] is None
    assert world.bead['metadata']['layers'] == [{
        'branch': values['branch'], 'pr': values['pr_number'], 'head': values['commit'], 'base': 'trunk', 'linked': False}]
    assert not any(call[:3] == ['gh', 'stack', 'link'] for call in world.calls)
    second = world.branch('second', 'first', 2)
    assert world.link(second)['result'] == 'stacked'
    assert [call for call in world.calls if call[:3] == ['gh', 'stack', 'link']] == [
        ['gh', 'stack', 'link', '--base', 'trunk', '1', '2'],
    ]
    assert [layer['linked'] for layer in world.bead['metadata']['layers']] == [True, True]


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

    def capture(layers, hosted=None):
        top = select_confirmed_stack_top(layers, hosted)
        selected.append((copy.deepcopy(layers), hosted, top))
        return top

    monkeypatch.setattr(LINK, 'select_confirmed_stack_top', capture)
    first = world.branch('first', 'trunk', 1)
    second = world.branch('second', 'first', 2)
    assert world.link(first)['result'] == 'stacked'
    assert world.link(second)['result'] == 'stacked'
    assert selected[0] == ([], None, StackTop(None, None, None))
    assert any(layers and layers[-1]['branch'] == 'first' for layers, _, _ in selected)


@pytest.mark.parametrize('replay', [False, True])
def test_recorded_head_check_uses_selected_head(world, monkeypatch, replay):
    first = world.branch('first', 'trunk', 1)
    second = world.branch('second', 'first', 2)
    third = world.branch('third', 'second', 3)
    assert world.link(first)['result'] == 'stacked'
    assert world.link(second)['result'] == 'stacked'
    before = copy.deepcopy(world.bead['metadata'])
    monkeypatch.setattr(LINK, 'select_confirmed_stack_top', lambda layers, hosted=None: StackTop(2, 'second', 'wrong-head'))
    result = world.link(first if replay else third)
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


def test_rebase_required_names_rebase_options(world):
    first = world.branch("first", "trunk", 1)
    rival = world.branch("rival", "trunk", 2)
    assert world.link(first)["result"] == "stacked"
    result = world.link(rival)
    assert result["result"] == "rebase_required"
    assert [step["option"] for step in result["next_steps"]] == ["rebase", "dev_rebase"]
    assert "git rebase origin/first" in result["next_steps"][0]["steps"][0]
    assert "dev-sanity-bead.json.j2" in result["next_steps"][0]["steps"][1]


def test_rebase_required_inside_top_history_names_relink(world):
    first = world.branch("first", "trunk", 1)
    lower = world.branch("lower", "first", 2)
    upper = world.branch("upper", "lower", 3)
    world.pulls[2].update(base="first")
    assert world.link(first)["result"] == "stacked"
    # Historical stack created before held-parent enforcement linked upper first.
    world.bead["metadata"]["layers"].append({"pr": 3, "branch": "upper", "head": upper["commit"], "base": "first", "linked": True})
    world.bead["metadata"]["layers"][0]["linked"] = True
    world.bead["metadata"]["stack_number"] = 7
    world.stacks = [{"number": 7, "base": "trunk", "prs": [1, 3]}]
    result = world.link(lower)
    assert result["result"] == "rebase_required"
    assert [step["option"] for step in result["next_steps"]] == ["relink_in_git_order"]
    assert result["next_steps"][0]["steps"][0] == "do not rebase lower"


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
    first_layer = {"branch": "first", "pr": 1, "head": first["commit"], "base": "trunk", "linked": False,
                   "base_head": world.git('rev-parse', 'origin/trunk')}
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
    # Layer 0 alone stays unlinked (gh stack link needs two PRs); the retry rewrites the same ledger.
    assert world.bead["metadata"] == before
    assert world.bead["metadata"]["layers"][0]["linked"] is False
    assert world.bead["metadata"]["stack_number"] is None
    assert not any(call[:3] == ["gh", "stack", "link"] for call in world.calls)
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
    hook.beads[hook.values["checked_bead"]]["metadata"].update(
        branch="rival", commit=rival["commit"], pr_number=2, pr_base="first")
    hook.beads[hook.values["qa_bead"]]["metadata"].update(branch="rival", pr_number=2)
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


def test_self_lead_notification_is_skipped(world):
    values = world.branch("first", "trunk", 1)
    values["lead"] = "sanity"
    world.pulls[0]["draft"] = True
    result = world.link(values)
    assert result["result"] == "stack_failed"
    assert result["notified"] == "skipped (self)"
    assert not any(call[:2] == ["atm", "send"] for call in world.calls)


def test_duplicate_layer_refusal_has_repair(world):
    values = world.branch("first", "trunk", 1)
    assert world.link(values)["result"] == "stacked"
    layer = copy.deepcopy(world.bead["metadata"]["layers"][0])
    layer.update(branch="duplicate", base="first")
    world.bead["metadata"]["layers"].append(layer)
    result = world.link(values)
    assert result["result"] == "stack_failed"
    assert all(text in result["evidence"] for text in ("PR 1", "duplicate", layer["head"], "duplicate pr"))
    assert "bd update phase.stack --metadata" in result["fix"]
    command = shlex.split(result["fix"])
    assert command[:4] == ["bd", "update", "phase.stack", "--metadata"]
    assert len(json.loads(command[4])["layers"]) == 1
    assert "--set-metadata stores a string" in result["evidence"]


def test_ghost_linked_layer_refusal_has_repair(world):
    values = world.branch("first", "trunk", 1)
    assert world.link(values)["result"] == "stacked"
    world.bead["metadata"]["layers"][0]["linked"] = True
    result = world.link(values)
    assert result["result"] == "stack_failed"
    assert all(text in result["evidence"] for text in ("PR 1", "first", values["commit"], "absent from the hosted stack"))
    assert "bd update phase.stack --metadata" in result["fix"]
    assert '"linked":false' in result["fix"]


def test_out_of_order_parent_holds_then_links_in_git_order(world):
    f1 = world.branch("f1", "trunk", 1)
    f2 = world.branch("f2", "f1", 2)
    f3 = world.branch("f3", "f2", 3)
    f4 = world.branch("f4", "f3", 4)
    assert world.link(f1)["result"] == "stacked"
    assert world.link(f2)["result"] == "stacked"
    world.calls.clear()
    held = world.link(f4)
    assert held["result"] == "held"
    assert held["held_on"] == {"pr": 3, "branch": "f3"}
    assert not any(call[:3] == ["gh", "stack", "link"] for call in world.calls)
    assert world.bead["metadata"]["layers"][-1]["held_on"] == held["held_on"]
    from bead_schema import StackMetadata
    assert StackMetadata.model_validate(world.bead["metadata"]).layers[-1].base == "f3"
    assert world.link(f1)["top"]["pr"] == 2
    world.calls.clear()
    released = world.link(f3)
    assert released["result"] == "stacked"
    assert released["top"]["pr"] == 4
    assert [item["pr"] for item in world.bead["metadata"]["layers"]] == [1, 2, 3, 4]
    assert all(item["linked"] for item in world.bead["metadata"]["layers"])
    assert [call for call in world.calls if call[:3] == ["gh", "stack", "link"]] == [["gh", "stack", "link", "--base", "trunk", "1", "2", "3", "4"]]


def test_open_linked_parent_does_not_hold_child(world):
    f1 = world.branch("f1", "trunk", 1)
    f2 = world.branch("f2", "f1", 2)
    f3 = world.branch("f3", "f2", 3)
    f4 = world.branch("f4", "f3", 4)
    for layer in (f1, f2, f3):
        assert world.link(layer)["result"] == "stacked"
    assert world.link(f4)["result"] == "stacked"
    assert world.stacks[0]["prs"] == [1, 2, 3, 4]


def test_hosted_ahead_without_ancestry_refuses_with_both_tops(world):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    rival = world.branch("rival", "trunk", 3)
    candidate = world.branch("candidate", "second", 4)
    assert world.link(first)["result"] == "stacked"
    assert world.link(second)["result"] == "stacked"
    world.pulls[2]["base"] = "second"
    world.stacks[0]["prs"].append(3)
    before = copy.deepcopy(world.bead["metadata"])
    result = world.link(candidate)
    assert result["result"] == "stack_failed"
    assert "stack top mismatch" in result["evidence"]
    assert "second" in result["evidence"] and "hosted 3" in result["evidence"]
    assert world.bead["metadata"] == before


def test_first_parent_pass_releases_held_successor(world):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    assert world.link(second)["result"] == "held"
    assert world.link(first)["result"] == "stacked"
    assert [layer["pr"] for layer in world.bead["metadata"]["layers"]] == [1, 2]
    assert world.stacks[0]["prs"] == [1, 2]


def test_moved_held_child_cannot_be_linked(world):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    assert world.link(second)["result"] == "held"
    world.git("commit", "--allow-empty", "-m", "moved held child")
    world.git("push", "origin", "second")
    world.pulls[1]["sha"] = world.git("rev-parse", "HEAD")
    result = world.link(first)
    assert result["result"] == "stack_failed"
    assert "pinned commit" in result["evidence"]
    assert not world.stacks


def test_torn_link_recovers_held_children_in_order(world):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    third = world.branch("third", "second", 3)
    fourth = world.branch("fourth", "third", 4)
    assert world.link(first)["result"] == "stacked"
    assert world.link(second)["result"] == "stacked"
    assert world.link(fourth)["result"] == "held"
    world.fail_verification = True
    assert world.link(third)["result"] == "stack_failed"
    world.fail_verification = False
    assert world.link(third)["result"] == "stacked"
    assert [(layer["pr"], layer["linked"]) for layer in world.bead["metadata"]["layers"]] == [(1, True), (2, True), (3, True), (4, True)]


def test_invalid_layer_includes_copy_paste_repair(world):
    values = world.branch("first", "trunk", 1)
    world.bead["metadata"]["layers"] = [{"pr": 1, "branch": "first", "head": values["commit"], "base": "wrong", "linked": False}]
    result = world.link(values)
    assert result["result"] == "stack_failed"
    assert all(text in result["evidence"] for text in ("PR 1", "first", values["commit"], "base ancestry"))
    assert "bd update phase.stack --metadata" in result["fix"]


def test_self_lead_success_returns_link_result_without_send(world):
    values = world.branch("first", "trunk", 1)
    values["lead"] = "sanity"
    result = world.link(values)
    assert result["result"] == "stacked"
    assert result["notified"] == "skipped (self)"
    assert not any(call[:2] == ["atm", "send"] for call in world.calls)


def test_malformed_held_parent_is_a_repairable_refusal(world):
    values = world.branch("first", "trunk", 1)
    world.bead["metadata"]["layers"] = [{"pr": 1, "branch": "first", "head": values["commit"], "base": "trunk", "linked": False, "held_on": None}]
    result = world.link(values)
    assert result["result"] == "stack_failed"
    assert "invalid held_on" in result["evidence"]
    assert "bd update phase.stack --metadata" in result["fix"]


def test_unrelated_held_branch_is_not_released(world):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    parent_a = world.branch("parent-a", "second", 3)
    child_a = world.branch("child-a", "parent-a", 4)
    parent_b = world.branch("parent-b", "second", 5)
    child_b = world.branch("child-b", "parent-b", 6)
    assert world.link(first)["result"] == "stacked"
    assert world.link(second)["result"] == "stacked"
    assert world.link(child_a)["result"] == "held"
    assert world.link(child_b)["result"] == "held"
    result = world.link(parent_a)
    assert result["result"] == "stacked"
    assert world.stacks[0]["prs"] == [1, 2, 3, 4]
    remaining = world.bead["metadata"]["layers"][-1]
    assert remaining["pr"] == 6 and remaining["linked"] is False
    assert remaining["held_on"] == {"pr": 5, "branch": "parent-b"}


def test_lean_parent_keeps_held_child_until_full_retry(world):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    assert world.link(second)["result"] == "held"
    first["budget"] = "lean"
    assert world.link(first)["result"] == "stacked"
    assert not world.stacks
    assert world.bead["metadata"]["layers"][-1]["held_on"] == {"pr": 1, "branch": "first"}
    first["budget"] = "full"
    assert world.link(first)["result"] == "stacked"
    assert world.stacks[0]["prs"] == [1, 2]


def test_held_chain_is_released_in_git_order(world):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    third = world.branch("third", "second", 3)
    assert world.link(third)["held_on"] == {"pr": 2, "branch": "second"}
    assert world.link(second)["held_on"] == {"pr": 1, "branch": "first"}
    result = world.link(first)
    assert result["result"] == "stacked"
    assert result["top"]["pr"] == 3
    assert [(layer["pr"], layer["linked"]) for layer in world.bead["metadata"]["layers"]] == [(1, True), (2, True), (3, True)]
    assert world.stacks[0]["prs"] == [1, 2, 3]


def test_empty_ledger_with_recorded_hosted_stack_is_not_silently_selected(world):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    world.bead["metadata"]["stack_number"] = 7
    world.stacks = [{"number": 7, "base": "trunk", "prs": [1]}]
    result = world.link(second)
    assert result["result"] == "stack_failed"
    assert "stack top mismatch" in result["evidence"]
    assert world.bead["metadata"]["layers"] == []


def test_hosted_ahead_must_include_the_unlinked_ledger_anchor(world):
    first = world.branch("first", "trunk", 1)
    second = world.branch("second", "first", 2)
    rival = world.branch("rival", "trunk", 3)
    rival_child = world.branch("rival-child", "rival", 4)
    assert world.link(first)["result"] == "stacked"
    world.bead["metadata"]["stack_number"] = 7
    world.stacks = [{"number": 7, "base": "trunk", "prs": [3, 4]}]
    result = world.link(second)
    assert result["result"] == "stack_failed"
    assert "stack top mismatch" in result["evidence"]
    assert [layer["pr"] for layer in world.bead["metadata"]["layers"]] == [1]
