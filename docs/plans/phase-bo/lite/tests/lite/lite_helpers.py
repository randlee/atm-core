"""Shared helpers for running the lite hook cases against the installed atm_bd_types wheel."""
from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path
from types import ModuleType

import atm_bd_types as T
import pytest
from atm_bd_types._core import patch_json
from atm_bd_types._native import _bead, _layer, _phase_file, _stack

HERE = Path(__file__).parent
sys.path.insert(0, str(HERE))
SCRIPTS = HERE.parent.parent / "skills" / "atm-bd-lite" / "scripts"
LEAD, DEV, QA = "lead", "dev", "qa"
SHA = "0123456789abcdef0123456789abcdef01234567"


def layer(branch: str = "work", head: str = SHA, pr: int = 7, base: str = "main"):
    return _layer(branch, head, pr, base)


def bead(
    bead_id: str,
    status: T.Status = T.Status.OPEN,
    assignee: str | None = None,
    *,
    stage: T.Stage | str = T.Stage.DEV,
    open_blockers: list[str] | None = None,
    blocks_ids: list[str] | None = None,
    branch: str = "",
    stack_bead: str = "",
    sprint: str = "",
    phase: str = "",
    layers: dict[str, object] | None = None,
    checked_bead: str | None = None,
):
    if isinstance(stage, str):
        stage = getattr(T.Stage, stage.upper(), T.Stage.OTHER)
    ledger = _stack(list(layers.values())) if layers is not None else None
    return _bead(bead_id, status, stage, assignee, open_blockers=open_blockers or [], blocks_ids=blocks_ids or [],
                 branch=branch, stack_bead=stack_bead, sprint=sprint, phase=phase, stack=ledger, checked_bead=checked_bead)


def phase_file(profile: str = "lite", lead: str = LEAD, repo: str | None = "example/repo"):
    root = "/lite/templates"
    templates = [(f"{root}/{name}.md", root, b"template") for name in
                 ("refuse", "dev-close", "qa-pass", "qa-findings", "land-close")]
    return _phase_file(profile, lead, templates, [] if repo is None else [("repo", repo)])


def hook_request(
    *,
    caller: str = DEV,
    task_id: str = "bo-10",
    phase: str = "bo",
    phase_task: bool = True,
    assignee: str | None = None,
    outcome: str | None = None,
    close_report: str | None = None,
    close_reason: str | None = None,
    verdict: str | None = None,
    verdict_sha: str = "",
    findings: list | None = None,
    hook: str | None = None,
):
    if hook is None:
        hook = "task.close" if outcome is not None else ("task.assign" if assignee is not None else "task.start")
    phase = phase if phase_task else ""
    if verdict is not None:
        close_report = 'done\n```json\n' + json.dumps({
            "verdict": verdict,
            "sha": verdict_sha,
            "findings": [({"id": item.id, "blocking": item.blocking} if not isinstance(item, dict) else item)
                         for item in (findings or [])],
        }) + '\n```\n'
    operation = None
    if hook == "task.start":
        operation = {"op": "start"}
    elif hook == "task.close":
        operation = None if outcome is None else {"op": "close", "outcome": outcome,
                                                   **({"reason": close_reason} if close_reason is not None else {})}
    body = {
        "home_dir": "/h", "current_dir": "/r", "caller_team": "t",
        "to": None if assignee is None else {"agent": assignee},
        "message_source": {"Inline": close_report or ""}, "task_id": task_id,
        "task_op": operation, "dry_run": False,
    }
    envelope = {"schema_version": "1.0.0", "hook": hook, "team": "t", "phase": phase,
                "caller": {"identity": caller, "team": "t", "host": None}, "custom": {}, "request": body}
    request = T.HookRequest.parse(json.dumps(envelope))
    return request


def reply_data(reply) -> dict:
    return json.loads(reply.to_json())


def reply_actions(reply) -> list[dict]:
    return reply_data(reply).get("atm-action", [])


def action_template(action: dict) -> str:
    source = action["request"]["message_source"]["Template"]
    return source["canonical_template_path"]


def action_vars(action: dict) -> dict:
    source = action["request"]["message_source"]["Template"]
    return source["var_file_values"]


def load_hook(name: str) -> ModuleType:
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), SCRIPTS / f"{name}.py")
    assert spec and spec.loader
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


class FakeStore:
    """In-memory BeadStore adapter returning real wheel records and recording serialized guarded writes."""

    def __init__(self, *beads: T.Bead, target: T.Layer | T.Refusal | None = None, race: bool = False) -> None:
        self.beads = {item.id: item for item in beads}
        self.target, self.race = target, race
        self.writes: list[tuple[str, str, object, str]] = []
        self.refreshes: list[list[str]] = []

    def refresh(self, bead_ids: list[str]) -> T.Refusal | None:
        self.refreshes.append(bead_ids)
        return None

    def show(self, bead_id: str) -> T.Bead | T.Refusal:
        return self.beads.get(bead_id) or T.Refusal(T.ErrorId.BEAD_NOT_FOUND, bead=bead_id)

    def target_layer(self, qa_bead: T.Bead) -> T.Layer | T.Refusal:
        return self.target if self.target is not None else T.Refusal(T.ErrorId.QA_NOTHING_TO_REVIEW, bead=qa_bead.id)

    def update(self, patch: T.BeadPatch, actor: str) -> T.Refusal | None:
        data = json.loads(patch_json(patch))
        operation, value = data["operation"], data["value"]
        argument = value.get("agent") if operation == "assign" else (None if operation in ("start", "reopen") else value)
        self.writes.append((operation, data["id"], argument, actor))
        return T.Refusal(T.ErrorId.BEAD_RACED, bead=data["id"]) if self.race else None


class FakePhases:
    def __init__(self, lead: str = LEAD, profile: str = "lite", readable: bool = True) -> None:
        self.lead, self.profile, self.readable = lead, profile, readable
        self.file = phase_file(profile, lead)

    def load(self, path: str, profile: str) -> T.PhaseFile | T.Refusal:
        if not self.readable:
            return T.Refusal(T.ErrorId.PHASE_UNREADABLE, path=path)
        if profile != self.profile:
            return T.Refusal(T.ErrorId.PROFILE_MISMATCH, path=path, profile=self.profile)
        return self.file


@pytest.fixture(scope="session")
def hooks() -> dict[str, ModuleType]:
    return {n: load_hook(f"atm-bd-task-{n}") for n in ("assign-hook", "start-hook", "hook")}


def expect_refused(reply, code: T.ErrorId, store: FakeStore, writes: int = 0) -> None:
    data = reply_data(reply)
    assert data["result"] == "refused", data
    assert data["message"].startswith(code.name), data
    assert len(store.writes) == writes, store.writes
