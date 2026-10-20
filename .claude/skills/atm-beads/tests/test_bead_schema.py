"""Phase-wide bead validation and stack metadata contracts."""
import copy
import importlib
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest
from pydantic import ValidationError

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
bead_schema = importlib.import_module("bead_schema")


FAMILY = {"branch": "sprint/bi-7", "worktree": "/work/bi-7", "pr_target": "main", "difficulty": "normal"}
SCHEMA_CLI = Path(__file__).resolve().parents[1] / "scripts" / "bead_schema.py"


@pytest.mark.parametrize("missing_branch, bd_exit, expected_code", [
    (False, 0, 0),
    (True, 0, 5),
    (False, 7, 2),
])
def test_validate_phase_cli_with_controlled_bd(tmp_path, missing_branch, bd_exit, expected_code):
    stub = tmp_path / "bd"
    stub.write_text(
        '#!/bin/sh\nprintf "%s\\n" "$@" > "$BD_ARGS"\n'
        'printf "%s" "$BD_OUTPUT"\nprintf "%s" "$BD_ERROR" >&2\nexit "$BD_EXIT"\n',
        encoding="utf-8",
    )
    stub.chmod(0o755)
    args_file = tmp_path / "bd-args"
    metadata = {**FAMILY, "checked_bead": "atm-bi-7.group-dev", "round": 1}
    if missing_branch:
        del metadata["branch"]
    bead = {"id": "atm-bi-7.group-qa", "labels": ["phase-bi", "stage:qa"], "metadata": metadata}
    env = {**os.environ, "PATH": f"{tmp_path}{os.pathsep}{os.environ.get('PATH', '')}",
           "BD_ARGS": str(args_file), "BD_OUTPUT": json.dumps([bead]),
           "BD_ERROR": "database unavailable" if bd_exit else "", "BD_EXIT": str(bd_exit)}
    result = subprocess.run([sys.executable, str(SCHEMA_CLI), "validate", "--phase", "bi"],
                            env=env, capture_output=True, text=True, timeout=60, check=False)
    assert args_file.read_text(encoding="utf-8").splitlines() == [
        "list", "-l", "phase-bi", "--status", "open,in_progress,blocked,deferred,closed",
        "--include-gates", "-n", "0", "--json",
    ]
    assert result.returncode == expected_code
    if missing_branch:
        assert result.stdout.startswith("atm-bi-7.group-qa: branch:")
    else:
        assert result.stdout == ""
    if bd_exit:
        assert "bd list failed: database unavailable" in result.stderr
        assert "Traceback" not in result.stderr
    else:
        assert result.stderr == ""


def test_validate_phase_cli_rejects_invalid_phase_before_bd(tmp_path):
    result = subprocess.run([sys.executable, str(SCHEMA_CLI), "validate", "--phase", "../bad"],
                            env={**os.environ, "PATH": str(tmp_path)},
                            capture_output=True, text=True, timeout=60, check=False)
    assert result.returncode == 2
    assert "invalid phase" in result.stderr
    assert "Traceback" not in result.stderr


@pytest.mark.parametrize("edge", ["blocks", "validates"])
def test_sanity_accepts_one_dev_link(edge):
    bead = {"id": "atm-bi-7.group-sanity", "metadata": {**FAMILY, "dev_bead": "atm-bi-7.group-dev"},
            "dependencies": [{"type": edge, "depends_on_id": "atm-bi-7.group-dev"}]}
    assert bead_schema.problems(bead, bead_schema.SanityBead) == []


def test_sanity_rejects_absent_or_duplicate_dev_links():
    bead = {"id": "atm-phase-bi.15-sanity", "metadata": {**FAMILY, "dev_bead": "atm-phase-bi.15"}}
    assert bead_schema.problems(bead, bead_schema.SanityBead)
    bead["dependencies"] = [{"type": edge, "depends_on_id": "atm-phase-bi.15"} for edge in ("blocks", "validates")]
    assert bead_schema.problems(bead, bead_schema.SanityBead)


def test_stack_layers_have_all_fields_and_preserve_order():
    layer = {"branch": "first", "pr": 1, "head": "abc", "base": "trunk", "linked": True}
    second = {**layer, "branch": "second", "pr": 2}
    stack = {"trunk": "main", "stack_number": None, "layers": [layer, second]}
    assert bead_schema.StackMetadata.model_validate(stack).layers[-1].branch == "second"
    for key in layer:
        incomplete = copy.deepcopy(stack)
        del incomplete["layers"][0][key]
        with pytest.raises(ValidationError):
            bead_schema.StackMetadata.model_validate(incomplete)


def test_validate_phase_includes_qa_and_unknown_stage():
    qa = {"id": "atm-phase-bi.18-qa", "labels": ["stage:qa"],
          "metadata": {**FAMILY, "checked_bead": "atm-phase-bi.18", "round": 1}}
    assert bead_schema.validate_phase([qa]) == []
    del qa["metadata"]["branch"]
    errors = bead_schema.validate_phase([qa, {"id": "atm-bi-unexpected", "labels": ["stage:unexpected"]}])
    assert any(error.startswith("atm-phase-bi.18-qa: branch") for error in errors)
    assert "atm-bi-unexpected: no model" in errors


@pytest.mark.parametrize("stage, bead_id, metadata", [
    ("dev", "atm-bi-7.group-dev", {"role": "dev", "phase": "bi", "sprint": "bi-7", "sprint_bead": "atm-bi-7", "stack": "phase-bi", "layer": 7}),
    ("qa", "atm-bi-7.group-qa", {"role": "qa", "phase": "bi", "sprint": "bi-7", "sprint_bead": "atm-bi-7", "stack": "phase-bi", "checked_bead": "atm-bi-7.group-dev", "round": 1}),
    ("qa", "atm-phase-bi.18-qa", {"checked_bead": "atm-phase-bi.18", "phase": "bi", "sprint": "bi", "sprint_bead": "atm-phase-bi", "stack": "phase-bi", "round": 1, "layer": 40, "quick_fix": True}),
    ("fix", "atm-bi-7.qa1-f1-r1-fix", {"finding_ref": "qa1-f1", "severity": "blocking", "reviewer": "qa", "found_at_commit": "abc1234"}),
])
def test_family_models_require_each_cut_field(stage, bead_id, metadata):
    bead = {"id": bead_id, "labels": ["phase-bi", f"stage:{stage}"], "metadata": {**metadata, **FAMILY}}
    assert bead_schema.validate_phase([bead]) == []
    for field in FAMILY:
        incomplete = copy.deepcopy(bead)
        del incomplete["metadata"][field]
        assert any(problem.startswith(f"{bead_id}: {field}") for problem in bead_schema.validate_phase([incomplete]))


def test_sprint_finding_root_and_gate_models():
    rows = [
        {"id": "atm-bi-7", "labels": ["phase-bi", "stage:sprint"], "description": "## Deliverables\n1. Implement the work",
         "acceptance_criteria": "- [ ] done", "metadata": {**FAMILY, "requirements": ["NONE"], "adrs": ["NONE"]}},
        {"id": "atm-phase-bi.15", "labels": ["phase-bi", "stage:finding"], "metadata": {"severity": "minor", "reviewer": "qa", "found_at_commit": "abc1234"}},
        {"id": "atm-phase-bi", "metadata": {"integration_branch": "integrate/phase-bi"}},
        {"id": "atm-bi-gate", "issue_type": "gate"},
        {"id": "atm-phase-bi.stack", "labels": ["stage:stack"], "metadata": {"trunk": "integrate/phase-bi", "stack_number": None, "layers": []}},
    ]
    assert bead_schema.validate_phase(rows) == []
    for bead, key in ((rows[0], "requirements"), (rows[1], "reviewer"), (rows[2], "integration_branch"), (rows[4], "trunk")):
        incomplete = copy.deepcopy(bead)
        del incomplete["metadata"][key]
        assert any(key in problem for problem in bead_schema.validate_phase([incomplete]))
