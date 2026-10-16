"""Unit tests for the graph-orchestration dependency gate."""

from __future__ import annotations

import importlib.metadata
import importlib.util
import json
import os
import subprocess
import sys
from pathlib import Path



SCRIPTS = Path(__file__).parent
REPO_ROOT = SCRIPTS.resolve().parents[3]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))


def _module():
    spec = importlib.util.spec_from_file_location("graph_preflight", SCRIPTS / "preflight.py")
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def test_version_floor_is_semver_and_inclusive():
    preflight = _module()
    assert preflight._version("sc-compose 1.2.0") == (1, 2, 0)
    assert preflight._version("sc-compose 1.7.1") > preflight.MIN_SC_COMPOSE
    assert preflight._version("sc-compose 1.2.9") < preflight.MIN_SC_COMPOSE
    assert preflight.is_supported_sc_compose_version(preflight.MIN_SC_COMPOSE)
    assert preflight.is_supported_sc_compose_version((1, 8, 0))
    assert not preflight.is_supported_sc_compose_version((1, 6, 9))
    assert not preflight.is_supported_sc_compose_version((2, 0, 0))
    assert preflight._version("unknown") is None


def test_sc_compose_rejects_old_cli(monkeypatch):
    preflight = _module()
    monkeypatch.setattr(preflight, "_find_command", lambda *_args: "/fake/sc-compose")
    monkeypatch.setattr(
        preflight,
        "_run",
        lambda *_args, **_kwargs: (0, "sc-compose 1.1.9", ""),
    )
    result = preflight.check_sc_compose()
    assert not result.ok
    assert f"required {preflight.MIN_SC_COMPOSE_TEXT}" in result.detail


def test_sc_compose_rejects_next_major_cli(monkeypatch):
    preflight = _module()
    monkeypatch.setattr(preflight, "_find_command", lambda *_args: "/fake/sc-compose")
    monkeypatch.setattr(preflight, "_run", lambda *_args, **_kwargs: (0, "sc-compose 2.0.0", ""))
    result = preflight.check_sc_compose()
    assert not result.ok
    assert f"required {preflight.MIN_SC_COMPOSE_TEXT}" in result.detail


def test_binding_rejects_old_python_wheel(monkeypatch):
    preflight = _module()
    monkeypatch.setattr(preflight, "_python_candidates", lambda: ["/fake/python3"])
    monkeypatch.setattr(
        preflight,
        "_run",
        lambda *_args, **_kwargs: (0, "1.1.0", ""),
    )
    result = preflight.check_sc_compose_binding()
    assert not result.ok
    assert f"required {preflight.MIN_SC_COMPOSE_BINDING_TEXT}" in result.detail


def test_binding_rejects_next_major_python_wheel(monkeypatch):
    preflight = _module()
    monkeypatch.setattr(preflight, "_python_candidates", lambda: ["/fake/python3"])
    monkeypatch.setattr(preflight, "_run", lambda *_args, **_kwargs: (0, "2.0.0", ""))
    result = preflight.check_sc_compose_binding()
    assert not result.ok
    assert f"required {preflight.MIN_SC_COMPOSE_BINDING_TEXT}" in result.detail


def test_binding_failure_has_actionable_install_hint(monkeypatch):
    preflight = _module()
    monkeypatch.setattr(preflight, "_python_candidates", lambda: ["/fake/python3"])
    monkeypatch.setattr(
        preflight,
        "_run",
        lambda *_args, **_kwargs: (1, "", "ModuleNotFoundError: sc_compose"),
    )
    result = preflight.check_sc_compose_binding()
    assert not result.ok
    assert "python3 -m pip install --user --break-system-packages" in result.detail


def test_missing_rdflib_is_a_structured_failure(monkeypatch):
    preflight = _module()
    monkeypatch.setattr(preflight, "_python_candidates", lambda: ["/fake/python3"])
    monkeypatch.setattr(
        preflight,
        "_run",
        lambda *_args, **_kwargs: (1, "", "ModuleNotFoundError: rdflib"),
    )
    result = preflight.check_rdflib()
    assert not result.ok
    assert "rdflib" in result.detail


def test_run_preflight_success_and_failure_envelopes():
    preflight = _module()

    def good():
        return preflight.Check("fake", True, "ok")

    def bad():
        return preflight.Check("fake", False, "missing")

    success = preflight.run_preflight(checks=[good])
    failure = preflight.run_preflight(checks=[bad])
    assert success == {
        "success": True,
        "data": {"checks": [{"name": "fake", "ok": True, "detail": "ok"}], "required_sc_compose": preflight.MIN_SC_COMPOSE_TEXT, "for_tests": False},
        "error": None,
    }
    assert failure["success"] is False
    assert failure["error"]["code"] == "DEPENDENCY.PREFLIGHT_FAILED"


def test_cli_returns_exit_two_and_json_on_forced_bad_python():
    script = SCRIPTS / "preflight.py"
    env = os.environ.copy()
    env["GRAPH_ORCH_PYTHON"] = str(SCRIPTS / "does-not-exist")
    result = subprocess.run(
        [sys.executable, str(script)],
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 2
    payload = json.loads(result.stdout)
    assert payload["success"] is False
    assert payload["error"]["code"] == "DEPENDENCY.PREFLIGHT_FAILED"


def test_python_binding_render_integration_when_1_6_1_or_newer_is_installed():
    """Exercise the required maturin binding; missing setup is a test failure."""

    try:
        version = importlib.metadata.version("sc-compose")
        import sc_compose
    except (importlib.metadata.PackageNotFoundError, ImportError) as exc:
        raise AssertionError(
            "sc-compose Python bindings not installed. Run: "
            f"{_module().SC_COMPOSE_BINDING_INSTALL} before running binding integration tests: "
            f"{exc}"
        ) from exc
    if _module()._version(version) < _module().MIN_SC_COMPOSE_BINDING:
        raise AssertionError(
            f"sc-compose Python binding {version} is below "
            f"{_module().MIN_SC_COMPOSE_BINDING_TEXT}"
        )
    assert sc_compose.render_template("Hello {{ name }}", {"name": "graph"}) == "Hello graph"


def test_wheel_renders_graph_dev_template(tmp_path):
    variables = {
        "task_id": "GO-TEST",
        "sprint": "AICH-S1",
        "node_id": "AICH-S1",
        "node_order": "1",
        "criteria_doc": "docs/plans/phase-ai/sprint-ai-21-pre.md",
        "worktree_path": "/tmp/worktree",
        "branch": "feature/test",
        "pr_target": "integrate/phase-AI",
        "phase_local": "AICH",
        "ttl_dir": ".sprints/AICH",
        "finding_ids": "",
    }
    output_path = tmp_path / "task.xml"
    from scripts.sc_compose_render import render_file

    rendered = render_file(
        REPO_ROOT / ".claude/skills/graph-orchestration/dev-task.xml.j2",
        variables,
        root=REPO_ROOT,
        strict=False,
    )
    output_path.write_text(rendered, encoding="utf-8")
    assert '<atm-task id="GO-TEST" sprint="AICH-S1"' in rendered
