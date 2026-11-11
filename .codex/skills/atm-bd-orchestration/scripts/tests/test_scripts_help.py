"""Every self-contained script declares the dependencies needed to start."""
import ast
import os
import subprocess
import sys
from pathlib import Path

import pytest
from subprocess_helpers import HANG_GUARD_SECONDS, run_command

SCRIPTS = Path(__file__).resolve().parents[1]
SCRIPT_PATHS = sorted(path for path in SCRIPTS.iterdir()
                      if path.is_file() and "# /// script" in path.read_text(encoding="utf-8", errors="replace")[:500])


def has_main_entry(path):
    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    return any(isinstance(node, ast.If) and isinstance(node.test, ast.Compare)
               and isinstance(node.test.left, ast.Name) and node.test.left.id == "__name__"
               and len(node.test.ops) == 1 and isinstance(node.test.ops[0], ast.Eq)
               and len(node.test.comparators) == 1 and isinstance(node.test.comparators[0], ast.Constant)
               and node.test.comparators[0].value == "__main__" for node in tree.body)


def test_script_population():
    python_files = set(SCRIPTS.glob("*.py"))
    main_entries = {path for path in python_files if has_main_entry(path)}
    selected = set(SCRIPT_PATHS)
    assert main_entries
    assert main_entries <= selected, f"Python entrypoints missing PEP 723 metadata: {main_entries - selected}"
    other_pep_scripts = {"phase_settings.py", "bead-groups", "bv-analyze", "fix-round-scope", "sanity-close-vars",
                         "sanity-merge", "sanity-run-history", "sanity-split", "sanity-stack-link"}
    assert selected == main_entries | {SCRIPTS / name for name in other_pep_scripts}
    excluded_libraries = {"bd_models.py", "bead_formulas.py", "command_timeout.py", "qa_reviewers.py",
                          "stack_top.py", "task_close_validation.py", "transitions.py"}
    excluded_shell_scripts = {"bead-queues"}
    assert {path.name for path in python_files - selected} == excluded_libraries
    shell_scripts = {path.name for path in SCRIPTS.iterdir()
                     if path.is_file() and path.read_bytes().startswith(b"#!/bin/sh\n")}
    assert shell_scripts == excluded_shell_scripts
    assert all(SCRIPTS / name not in selected for name in excluded_shell_scripts)


def test_cron_declares_its_script_dependencies():
    assert SCRIPTS / "bd_cache_cron.py" in SCRIPT_PATHS


def assert_help_reached(script, result):
    output = result.stdout + result.stderr
    startup_failure = next((marker for marker in ("ModuleNotFoundError", "ImportError", "Traceback")
                            if marker in output), None)
    assert startup_failure is None, (
        f"{script.name} --help failed during startup (exit {result.returncode}; {startup_failure}):\n{output}"
    )
    if script.name == "phase_settings.py":
        assert result.returncode == 0, f"{script.name} import failed (exit {result.returncode}):\n{output}"
        return
    has_usage = any(line.lstrip().lower().startswith("usage:") for line in output.splitlines())
    assert has_usage, f"{script.name} --help did not reach usage output (exit {result.returncode}):\n{output}"
    assert result.returncode == 0, f"{script.name} --help exited {result.returncode}:\n{output}"


@pytest.mark.parametrize(
    ("source", "reason"),
    [
        ("raise ImportError('fixture dependency is missing')\n", "failed during startup"),
        ("import sys\nsys.stderr.write('Failed to resolve dependencies\\n')\nsys.exit(2)\n",
         "did not reach usage output"),
    ],
    ids=("import-error", "dependency-resolution-error"),
)
def test_script_help_rejects_startup_failures(tmp_path, source, reason):
    script = tmp_path / "fixture.py"
    script.write_text(source)
    result = run_command([sys.executable, str(script)], timeout=HANG_GUARD_SECONDS,
                         capture_output=True, text=True, check=False)

    with pytest.raises(AssertionError, match=reason):
        assert_help_reached(script, result)


def test_script_help_rejects_nonzero_exit_even_after_usage():
    script = SCRIPTS / "bead-groups"
    result = subprocess.CompletedProcess(
        ["uv", "run", "--script", str(script), "--help"], 2, "usage: bead-groups --help\n", ""
    )

    with pytest.raises(AssertionError, match=r"--help exited 2"):
        assert_help_reached(script, result)


@pytest.mark.parametrize("script", SCRIPT_PATHS, ids=lambda path: path.name)
def test_script_declared_dependencies_can_start(script):
    # The pytest interpreter comes from the frozen app environment, which already
    # contains the pinned script dependencies. Avoid resolving PEP 723 metadata
    # against the caller's uv cache or the network for every --help invocation.
    env = {**os.environ, "UV_OFFLINE": "1"}
    result = run_command([sys.executable, str(script), "--help"], env=env,
                         timeout=HANG_GUARD_SECONDS, capture_output=True, text=True)
    assert_help_reached(script, result)
