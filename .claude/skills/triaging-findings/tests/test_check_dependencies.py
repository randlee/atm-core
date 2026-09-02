import importlib.util
import json
import subprocess
import sys
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "scripts" / "check_dependencies.py"
spec = importlib.util.spec_from_file_location("check_dependencies", SCRIPT)
check_dependencies = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(check_dependencies)


def test_version_parser():
    assert check_dependencies._version("sc-compose 1.2.0") == (1, 2, 0)
    assert check_dependencies._version("oxigraph 0.5.7") == (0, 5, 7)
    assert check_dependencies._version("missing") is None


def test_preflight_passes_in_current_environment(monkeypatch):
    monkeypatch.setattr(check_dependencies, "_find", lambda name: Path("/bin/true"))
    monkeypatch.setattr(check_dependencies, "_run_version", lambda path: ("tool 1.6.1", "tool 1.6.1"))
    monkeypatch.setattr(check_dependencies, "_python_binding_entry", lambda: {"name": "sc_compose", "ok": True})
    result = check_dependencies.run()
    assert result["success"] is True
    assert result["error"] is None


def test_old_sc_compose_is_a_structured_failure(monkeypatch):
    monkeypatch.setattr(check_dependencies, "_find", lambda name: Path("/bin/true"))
    monkeypatch.setattr(check_dependencies, "_run_version", lambda path: ("sc-compose 1.0.1", "sc-compose 1.0.1"))
    monkeypatch.setattr(check_dependencies, "_python_binding_entry", lambda: {"name": "sc_compose", "ok": True})
    result = check_dependencies.run()
    assert result["success"] is False
    assert result["error"]["code"] == "EXECUTION.DEPENDENCY"
    assert any(item["name"] == "sc-compose" for item in result["error"]["failures"])


def test_missing_python_binding_has_actionable_install_hint(monkeypatch):
    def missing(_name):
        raise check_dependencies.importlib.metadata.PackageNotFoundError("sc-compose")

    monkeypatch.setattr(check_dependencies.importlib.metadata, "version", missing)
    result = check_dependencies._python_binding_entry()
    assert result["ok"] is False
    assert "python3 -m pip install --user --break-system-packages" in result["error"]


def test_cli_emits_json_success_contract():
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        capture_output=True,
        text=True,
        check=False,
    )
    payload = json.loads(result.stdout)
    assert payload["success"] is True
    assert payload["error"] is None
