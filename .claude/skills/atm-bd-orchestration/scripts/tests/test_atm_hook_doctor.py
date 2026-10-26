"""atm-hook-doctor.py against copied install folders with controlled file contents.

Every run is a real `python3 atm-hook-doctor.py` process. The checks use an interpreter whose environment holds only
the declared sc-sha test dependency; `-I` isolates repository imports, and the sc_sha-absent cases run with
site-packages disabled.
"""
from __future__ import annotations

import ast
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parents[1] / "atm-hook-doctor.py"
SC_SHA = "sc-sha>=1.7.0,<2"
REQUEST = {"schema_version": "1.0.0", "hook": "doctor", "team": "my-team", "phase": "bj", "custom": {}}
TIMEOUT = 120
SKILL_FILES = {
    ".claude/skills/atm-bd-orchestration/SKILL.md": "# orchestration\nteam: my-team\n",
    ".claude/skills/atm-bd-orchestration/scripts/tool.py": "print('tool')\n",
    ".claude/skills/atm-beads/SKILL.md": "# beads\n",
    ".claude/agents/dev-sanity.md": "---\nname: dev-sanity\n---\n",
}


def _run(argv, **kwargs) -> subprocess.CompletedProcess:
    return subprocess.run([str(a) for a in argv], capture_output=True, text=True, timeout=TIMEOUT, check=False, **kwargs)


@pytest.fixture(scope="module")
def sc_sha_python() -> Path:
    """Use the test interpreter; sc-sha is a declared package-test dependency, never installed by a test."""
    python = Path(sys.executable)
    probe = _run([python, "-I", "-c", "import sc_sha; print(sc_sha.__file__)"])
    if probe.returncode:
        pytest.fail(f"{SC_SHA} must be installed in the package-test environment: {probe.stderr.strip()}")
    assert probe.stdout.strip(), probe.stdout
    return python


def _record(python: Path, repo: Path, paths: list[str]) -> dict:
    code = ("import json, sys, sc_sha; from pathlib import Path; repo = Path(sys.argv[1]); "
            "print(json.dumps([{'path': p, 'sc_sha': sc_sha.calculate_hash({'utf8_file_bytes': (repo / p).read_bytes()})"
            "['sha256']} for p in sorted(sys.argv[2:])]))")
    done = _run([python, "-I", "-c", code, repo, *paths])
    assert done.returncode == 0, done.stderr
    return {"version": "0.17.4", "commit": "0" * 40,
            "source": {"ref": "v0.17.4", "manifest_aggregate": "a" * 64}, "files": json.loads(done.stdout)}


@pytest.fixture
def install(tmp_path: Path, sc_sha_python: Path) -> Path:
    """A copied install folder: <repo>/.claude with skill files, the doctor at its installed path, and the record."""
    repo = tmp_path / "repo"
    for rel, text in SKILL_FILES.items():
        (repo / rel).parent.mkdir(parents=True, exist_ok=True)
        (repo / rel).write_text(text, encoding="utf-8")
    doctor = repo / ".claude/skills/atm-bd-orchestration/scripts/atm-hook-doctor.py"
    shutil.copy2(SCRIPT, doctor)
    paths = [*SKILL_FILES, doctor.relative_to(repo).as_posix()]
    record = _record(sc_sha_python, repo, paths)
    (repo / ".claude/atm-bd-install.json").write_text(json.dumps(record), encoding="utf-8")
    # Byte-compiled caches are not shipped files and never count as extra.
    cache = repo / ".claude/skills/atm-beads/__pycache__/x.cpython-311.pyc"
    cache.parent.mkdir()
    cache.write_bytes(b"\x00")
    return repo / ".claude"


def doctor_cli(python: Path, dest: Path, *flags: str) -> subprocess.CompletedProcess:
    return _run([python, "-I", dest / "skills/atm-bd-orchestration/scripts/atm-hook-doctor.py", *flags], cwd=dest.parent)


def doctor_hook(python: Path, dest: Path, *flags: str, request: dict = REQUEST) -> subprocess.CompletedProcess:
    return _run([python, "-I", *flags, dest / "skills/atm-bd-orchestration/scripts/atm-hook-doctor.py", "doctor"],
                input=json.dumps(request), cwd=dest.parent)


def test_untouched_install_is_ok_per_path_and_passes(install: Path, sc_sha_python: Path) -> None:
    done = doctor_cli(sc_sha_python, install)

    assert done.returncode == 0, done.stderr
    lines = done.stdout.splitlines()
    assert lines[-1] == "PASS"
    assert lines[:-1] == [f"ok {path}" for path in sorted([*SKILL_FILES, ".claude/skills/atm-bd-orchestration/scripts/atm-hook-doctor.py"])]


def test_console_install_record_is_readable_by_the_installed_doctor(tmp_path: Path, sc_sha_python: Path) -> None:
    repo = tmp_path / "repo"
    dest = repo / ".claude"
    files = {
        ".claude/skills/atm-bd-orchestration/SKILL.md": "# orchestration\n",
        ".claude/skills/atm-bd-orchestration/scripts/tool.py": "print('tool')\n",
    }
    for rel, contents in files.items():
        path = repo / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents, encoding="utf-8")
    doctor_rel = ".claude/skills/atm-bd-orchestration/scripts/atm-hook-doctor.py"
    doctor_path = repo / doctor_rel
    shutil.copy2(SCRIPT, doctor_path)
    app_src = Path(os.environ.get(
        "ATM_BD_CONSOLE_SRC", Path(__file__).resolve().parents[6] / "apps" / "atm-bd-console" / "src",
    )).resolve()
    if not (app_src / "atm_bd_console").is_dir():
        pytest.skip(f"console source not at {app_src} (an installed copy has none); set ATM_BD_CONSOLE_SRC")
    code = """
import importlib.util, sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
from atm_bd_console import skill_integrity
spec = importlib.util.spec_from_file_location('atm_hook_doctor', sys.argv[2])
doctor = importlib.util.module_from_spec(spec)
spec.loader.exec_module(doctor)
assert skill_integrity.RECORD_NAME == doctor.RECORD_NAME
repo = Path(sys.argv[3])
dest = repo / '.claude'
relative = ['.claude/skills/atm-bd-orchestration/SKILL.md',
            '.claude/skills/atm-bd-orchestration/scripts/tool.py',
            '.claude/skills/atm-bd-orchestration/scripts/atm-hook-doctor.py']
installed = {path: (repo / path).read_bytes() for path in relative}
manifest = {'aggregate': 'a' * 64}
record = skill_integrity.write_install_record(dest, '0.17.4', '0' * 40, manifest, installed,
                                             source='v0.17.4')
print(record.name)
"""
    made = _run([sc_sha_python, "-I", "-c", code, app_src, doctor_path, repo], cwd=repo)
    assert made.returncode == 0, made.stderr
    assert made.stdout == "atm-bd-install.json\n"

    done = doctor_cli(sc_sha_python, dest)
    assert done.returncode == 0, done.stderr
    assert done.stdout.splitlines()[-1] == "PASS"


def test_planted_modified_missing_and_extra_paths_fail(install: Path, sc_sha_python: Path) -> None:
    (install / "skills/atm-bd-orchestration/SKILL.md").write_text("# tampered\n", encoding="utf-8")
    (install / "skills/atm-beads/SKILL.md").unlink()
    (install / "skills/atm-bd-orchestration/scripts/planted.py").write_text("print('planted')\n", encoding="utf-8")
    (install / "agents/not-scanned.md").write_text("shared directory\n", encoding="utf-8")

    done = doctor_cli(sc_sha_python, install, "--dest", str(install))

    assert done.returncode == 1, done.stderr
    lines = done.stdout.splitlines()
    assert lines[-1] == "FAIL"
    assert [line for line in lines[:-1] if not line.startswith("ok ")] == [
        "modified .claude/skills/atm-bd-orchestration/SKILL.md",
        "extra .claude/skills/atm-bd-orchestration/scripts/planted.py",
        "missing .claude/skills/atm-beads/SKILL.md",
    ]
    assert not any("not-scanned" in line for line in lines)


def test_no_install_record_fails_closed(install: Path, sc_sha_python: Path) -> None:
    (install / "atm-bd-install.json").unlink()

    done = doctor_cli(sc_sha_python, install)
    assert (done.returncode, done.stdout) == (1, "missing atm-bd-install.json\nFAIL\n")

    hook = doctor_hook(sc_sha_python, install)
    assert hook.returncode == 0, hook.stderr
    reply = json.loads(hook.stdout)
    assert reply["success"] is False
    assert [(f["severity"], f["code"]) for f in reply["findings"]] == [("error", "install_record_missing")]
    assert reply["findings"][0]["remediation"].startswith("atm-bd skill")


def test_unreadable_install_record_fails_closed(install: Path, sc_sha_python: Path) -> None:
    (install / "atm-bd-install.json").write_text('{"files": "not a list"}', encoding="utf-8")

    done = doctor_cli(sc_sha_python, install)
    assert done.returncode == 1 and done.stdout.splitlines()[-1] == "FAIL"
    assert done.stdout.startswith("unreadable atm-bd-install.json:")
    reply = json.loads(doctor_hook(sc_sha_python, install).stdout)
    assert reply["success"] is False and [f["code"] for f in reply["findings"]] == ["install_record_unreadable"]


def test_doctor_mode_replies_with_exactly_the_contract_keys(install: Path, sc_sha_python: Path) -> None:
    hook = doctor_hook(sc_sha_python, install)

    assert hook.returncode == 0, hook.stderr
    reply = json.loads(hook.stdout)
    assert set(reply) == {"success", "message", "findings"}
    assert reply["success"] is True and reply["findings"] == []
    assert isinstance(reply["message"], str) and reply["message"]


def test_doctor_mode_reports_one_error_finding_per_modified_path(install: Path, sc_sha_python: Path) -> None:
    (install / "skills/atm-bd-orchestration/scripts/tool.py").write_text("print('tampered')\n", encoding="utf-8")

    reply = json.loads(doctor_hook(sc_sha_python, install).stdout)

    assert set(reply) == {"success", "message", "findings"} and reply["success"] is False
    assert reply["findings"] == [{
        "severity": "error", "code": "skill_file_modified",
        "message": "modified .claude/skills/atm-bd-orchestration/scripts/tool.py",
        "remediation": f"atm-bd skill 0.17.4 --dest {install.parent.resolve()} --overwrite",
    }]
    assert all(set(f) == {"severity", "code", "message", "remediation"} for f in reply["findings"])


@pytest.mark.parametrize(("request_text", "message"), [
    ('{"hook":', "doctor request must be valid JSON"),
    (json.dumps({**REQUEST, "hook": "task.close"}), 'doctor request must be an object with "hook": "doctor"'),
], ids=["invalid-json", "wrong-hook"])
def test_doctor_mode_replies_with_a_request_invalid_finding(
    install: Path, sc_sha_python: Path, request_text: str, message: str,
) -> None:
    hook = _run([sc_sha_python, "-I", install / "skills/atm-bd-orchestration/scripts/atm-hook-doctor.py", "doctor"],
                input=request_text, cwd=install.parent)
    assert hook.returncode == 2 and hook.stderr == ""
    reply = json.loads(hook.stdout)
    assert set(reply) == {"success", "message", "findings"}
    assert reply["success"] is False and reply["message"] == message
    assert reply["findings"] == [{
        "severity": "error", "code": "request_invalid", "message": message,
        "remediation": 'send a JSON object with "hook": "doctor"',
    }]


def test_without_sc_sha_the_cli_exits_2_with_the_install_command_and_doctor_still_replies(install: Path) -> None:
    # -S drops site-packages, so sc_sha cannot be importable whatever the test interpreter has installed.
    done = _run([sys.executable, "-I", "-S", install / "skills/atm-bd-orchestration/scripts/atm-hook-doctor.py"],
                cwd=install.parent)
    assert done.returncode == 2 and done.stdout == ""
    assert "uv pip install 'sc-sha>=1.7.0,<2'" in done.stderr

    hook = doctor_hook(Path(sys.executable), install, "-S")
    assert hook.returncode == 2
    reply = json.loads(hook.stdout)
    assert reply["success"] is False
    assert reply["findings"] == [{"severity": "error", "code": "sc_sha_missing",
                                  "message": "the sc_sha Python module is not installed",
                                  "remediation": "uv pip install 'sc-sha>=1.7.0,<2'"}]


def test_the_doctor_imports_only_the_standard_library_and_sc_sha() -> None:
    tree = ast.parse(SCRIPT.read_text(encoding="utf-8"))
    imported = {alias.name.split(".")[0] for node in ast.walk(tree) if isinstance(node, ast.Import) for alias in node.names}
    imported |= {node.module.split(".")[0] for node in ast.walk(tree) if isinstance(node, ast.ImportFrom) and node.module}
    assert imported - set(sys.stdlib_module_names) == {"sc_sha"}
