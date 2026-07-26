"""Focused render and Turtle-parse tests for the canonical triage record."""

from __future__ import annotations

import json
import os
import subprocess
from pathlib import Path

import pytest


REPO_ROOT = Path(__file__).resolve().parents[4]
TEMPLATE = ".claude/skills/triaging-findings/triage-record.ttl.j2"


def _vars() -> dict:
    return {
        "finding_id": "FTQ-001",
        "title": "Process-global shutdown state in tests",
        "description": "Global state leaks across test cases.",
        "phase_id": "phase-R",
        "triage_mode": "initial_pass",
        "category": "FTQ",
        "severity": "important",
        "repeatable": True,
        "sweep_scope": "crate",
        "status": "open",
        "dispatch_ready": True,
        "triaged_at": "2026-07-25T16:30:00Z",
        "found_in": "AICH-S7",
        "found_at": "2026-07-25T16:26:33Z",
        # sc-compose var-files intentionally accept scalar arrays only. The
        # parallel arrays below are joined by index in the Turtle template.
        "occurrences": ["R17-1"],
        "occurrence_files": ["crates/atm-daemon/src/tests.rs"],
        "occurrence_lines": ["28"],
        "occurrence_snippets": ["static DISPATCHER: OnceLock<...>"],
        "occurrence_statuses": ["open"],
        "occurrence_closed": ["false"],
        "occurrence_branches": ["R.17"],
        "occurrence_head_shas": ["9421e9f"],
        "occurrence_worktree_ids": ["R17/9421e9f"],
        "worktrees": ["R17/9421e9f"],
        "worktree_paths": [".worktrees/R17"],
        "worktree_branches": ["R.17"],
        "worktree_head_shas": ["9421e9f"],
        "worktree_order_indices": ["17"],
    }


def _render(tmp_path: Path, variables: dict) -> subprocess.CompletedProcess[str]:
    vars_path = tmp_path / "vars.json"
    output_path = tmp_path / "FTQ-001.ttl"
    vars_path.write_text(json.dumps(variables), encoding="utf-8")
    return subprocess.run(
        [
            "sc-compose",
            "render",
            "--root",
            str(REPO_ROOT),
            "--file",
            TEMPLATE,
            "--var-file",
            str(vars_path),
            "--output",
            str(output_path),
        ],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        env={**os.environ, "NO_COLOR": "1"},
    )


def _parse_turtle(
    path: Path, tmp_path: Path
) -> subprocess.CompletedProcess[str]:
    converted = tmp_path / "parsed.ttl"
    return subprocess.run(
        [
            "oxigraph",
            "convert",
            "--from-file",
            str(path),
            "--from-format",
            "ttl",
            "--to-file",
            str(converted),
            "--to-format",
            "ttl",
        ],
        text=True,
        capture_output=True,
    )


def test_render_includes_found_provenance_and_parses_as_turtle(tmp_path: Path) -> None:
    result = _render(tmp_path, _vars())
    assert result.returncode == 0, result.stderr or result.stdout

    output = tmp_path / "FTQ-001.ttl"
    rendered = output.read_text(encoding="utf-8")
    assert "triage:foundIn triage:AICH-S7" in rendered
    assert 'triage:foundAt "2026-07-25T16:26:33Z"^^xsd:dateTime' in rendered
    assert "triage:hasOccurrence" in rendered
    assert "a triage:WorktreeSnapshot" in rendered
    assert 'triage:path ".worktrees/R17"' in rendered

    parsed = _parse_turtle(output, tmp_path)
    assert parsed.returncode == 0, parsed.stderr or parsed.stdout


def test_python_binding_renders_canonical_template() -> None:
    """The pip/maturin binding must render the same template tokens as the CLI."""
    try:
        import importlib.metadata
        import sc_compose
    except (ImportError, importlib.metadata.PackageNotFoundError) as exc:  # pragma: no cover - dependency preflight owns setup
        pytest.fail(
            "sc-compose Python bindings not installed. Run: "
            "python3 -m pip install --user --break-system-packages "
            "'sc-compose>=1.2.0' before running binding tests: "
            f"{exc}"
        )
    assert tuple(int(part) for part in importlib.metadata.version("sc-compose").split(".")[:3]) >= (1, 2, 0)
    rendered = sc_compose.render_template(
        (REPO_ROOT / TEMPLATE).read_text(encoding="utf-8"), _vars()
    )
    assert "triage:foundIn triage:AICH-S7" in rendered
    assert 'triage:foundAt "2026-07-25T16:26:33Z"^^xsd:dateTime' in rendered


@pytest.mark.parametrize("missing", ["found_in", "found_at", "worktree_paths"])
def test_render_rejects_missing_provenance_variable(
    tmp_path: Path, missing: str
) -> None:
    variables = _vars()
    del variables[missing]

    result = _render(tmp_path, variables)

    assert result.returncode != 0
    diagnostic = f"{result.stdout}\n{result.stderr}".lower()
    assert missing in diagnostic


@pytest.mark.parametrize(
    ("variable", "path_value"),
    [
        (
            "occurrence_files",
            "/abs/integrate-phase-R/crates/atm-daemon/src/tests.rs",
        ),
        ("occurrence_files", "../outside-repository.rs"),
        ("occurrence_files", "crates/../outside-repository.rs"),
        ("occurrence_files", r"C:\\checkout\\crates\\atm-daemon\\src\\tests.rs"),
        ("worktree_paths", "/abs/integrate-phase-R"),
        ("worktree_paths", "../outside-repository"),
        ("worktree_paths", "worktrees/../outside-repository"),
        ("worktree_paths", r"C:\\checkout\\integrate-phase-R"),
        ("worktree_paths", r"\\server\\share\\integrate-phase-R"),
    ],
)
def test_render_rejects_non_repository_relative_persisted_paths(
    tmp_path: Path, variable: str, path_value: str
) -> None:
    variables = _vars()
    variables[variable] = [path_value]

    result = _render(tmp_path, variables)
    assert result.returncode == 0, result.stderr or result.stdout

    output = tmp_path / "FTQ-001.ttl"
    rendered = output.read_text(encoding="utf-8")
    marker = (
        "__ERROR_REPOSITORY_RELATIVE_OCCURRENCE_PATH_REQUIRED__"
        if variable == "occurrence_files"
        else "__ERROR_REPOSITORY_RELATIVE_WORKTREE_PATH_REQUIRED__"
    )
    assert marker in rendered

    parsed = _parse_turtle(output, tmp_path)
    assert parsed.returncode != 0
