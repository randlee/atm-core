from __future__ import annotations

import importlib.machinery
import importlib.util
import json
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
LOADER = importlib.machinery.SourceFileLoader("sanity_close_vars", str(ROOT / "scripts/sanity-close-vars"))
SPEC = importlib.util.spec_from_loader(LOADER.name, LOADER)
assert SPEC is not None
CLOSE = importlib.util.module_from_spec(SPEC)
LOADER.exec_module(CLOSE)


@pytest.fixture
def inputs():
    context = json.loads((ROOT / "examples/task-close-vars-sanity.json").read_text())
    context.update(budget="full")
    manifest = {"run_id": "run", "sha": context["commit"], "sanity_bead": context["sanity_bead"],
                "dev_bead": context["checked_bead"], "branch": context["branch"],
                "worktree_path": context["worktree_path"], "deliverables_total": 2,
                "close_context": context, "checked_bead_data": {"title": "Checked title", "labels": ["stage:fix"]},
                "assignments": [{"number": 1}, {"number": 2}]}
    selected = {"run_id": "run", "reviewer": "sanity-selected", "commit": context["commit"],
                "task_id": context["sanity_bead"], "checked_bead": context["checked_bead"],
                "sprint": context["sprint"], "completed_at": "2026-10-20T00:00:00Z",
                "verdict": "PASS", "findings_count": 0, "findings": [], "findings_md": "D1: done\nD2: done"}
    return manifest, selected


def test_pass_builds_full_close_input_without_waiting_for_stack(inputs):
    manifest, selected = inputs
    result = CLOSE.build(manifest, selected)
    assert result["stacked"] is None
    assert result["verdict"] == "PASS" and result["deliverables_complete"] == 2
    assert result["undone"] == []
    assert result["commit"] == manifest["sha"]
    assert result["carry_forward"] == manifest["dev_bead"]
    assert result["run_id"] == "run" and result["reviewer"] == "sanity-selected"
    assert "stacked" in manifest["close_context"]


def test_fail_retains_verbatim_deliverable_reason_and_stack_result(inputs):
    manifest, selected = inputs
    selected.update(verdict="FAIL", findings_count=1,
                    findings=[{"deliverable": 2, "issue": "missing required implementation"}])
    stack = {"result": "lock_timeout", "holder": "other"}
    result = CLOSE.build(manifest, selected, stack)
    assert result["stacked"] is stack
    assert result["undone"] == [{"number": 2, "reason": "missing required implementation"}]
    assert result["deliverables_complete"] == 1


@pytest.mark.parametrize("field", ["run_id", "commit", "task_id", "checked_bead"])
def test_pinned_identity_mismatch_is_rejected(inputs, field):
    manifest, selected = inputs
    selected[field] = "wrong"
    with pytest.raises(ValueError, match=field):
        CLOSE.build(manifest, selected)


def test_lint_only_failure_marks_numbered_deliverables_undone(inputs):
    manifest, selected = inputs
    selected.update(verdict="FAIL", findings_count=1, lint_md="lint exited 1")
    result = CLOSE.build(manifest, selected)
    assert result["undone"] == [{"number": 1, "reason": "lint exited 1"}, {"number": 2, "reason": "lint exited 1"}]


def test_minimal_documented_dev_dispatch_needs_no_hand_built_fields(inputs):
    manifest, selected = inputs
    manifest["close_context"] = json.loads((ROOT / "examples/task-close-vars-dev.json").read_text())
    result = CLOSE.build(manifest, selected)
    assert result["evidence"] == "" and result["outcome"] == "fixed"
    assert result["round"] == 1 and result["findings_blocking"] == 0


@pytest.mark.parametrize(("outcome", "omit_stack"), [("fixed", False), ("not_reproducible", True)])
def test_optional_stack_input_does_not_block_close_vars(inputs, tmp_path, outcome, omit_stack):
    manifest, selected = inputs
    manifest["close_context"]["pr_number"] = ""
    manifest["close_context"]["outcome"] = outcome
    if omit_stack:
        manifest["close_context"].pop("stack_bead", None)
    values = CLOSE.build(manifest, selected)
    stack_file = tmp_path / "stack-input.json"
    stack_file.write_text("stale")

    assert not CLOSE.write_stack_input(stack_file, values, manifest)
    assert not stack_file.exists()


def test_stack_input_carries_assignment_budget_and_worktree_and_only_passes(inputs, tmp_path):
    manifest, selected = inputs
    manifest["close_context"].update(budget="lean")
    values = CLOSE.build(manifest, selected)
    stack_file = tmp_path / "stack-input.json"

    assert CLOSE.write_stack_input(stack_file, values, manifest)
    payload = json.loads(stack_file.read_text())
    assert payload["budget"] == "lean" and "timeout_s" not in payload
    assert payload["worktree_path"] == manifest["worktree_path"]

    selected["verdict"] = "FAIL"
    values = CLOSE.build(manifest, selected)
    assert not CLOSE.write_stack_input(stack_file, values, manifest)
    assert not stack_file.exists()


def test_stack_input_requires_assignment_budget(inputs, tmp_path):
    manifest, selected = inputs
    manifest["close_context"].update(budget="full")
    values = CLOSE.build(manifest, selected)
    stack_file = tmp_path / "stack-input.json"

    assert CLOSE.write_stack_input(stack_file, values, manifest)
    manifest["close_context"].pop("budget")
    with pytest.raises(ValueError, match="budget"):
        CLOSE.write_stack_input(stack_file, values, manifest)
