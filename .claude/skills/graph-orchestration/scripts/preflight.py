#!/usr/bin/env python3
"""Dependency preflight for graph-orchestration.

The command deliberately has no workflow side effects.  It emits one JSON
object and exits 0 only when every required runtime dependency is usable.
Missing dependencies, an unusable Python environment, and old sc-compose
versions are operational errors (exit 2), so callers cannot silently proceed.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Iterable

_CLAUDE_DIR = Path(__file__).resolve().parents[3]
if str(_CLAUDE_DIR) not in sys.path:
    sys.path.insert(0, str(_CLAUDE_DIR))

from lib.sc_compose_dependency import (  # noqa: E402
    MIN_SC_COMPOSE,
    MIN_SC_COMPOSE_TEXT,
    SC_COMPOSE_INSTALL,
    parse_version as _version,
)


@dataclass(frozen=True)
class Check:
    name: str
    ok: bool
    detail: str
    path: str | None = None

    def as_dict(self) -> dict[str, object]:
        value: dict[str, object] = {
            "name": self.name,
            "ok": self.ok,
            "detail": self.detail,
        }
        if self.path is not None:
            value["path"] = self.path
        return value


def _run(command: list[str], *, timeout: float = 5.0) -> tuple[int, str, str]:
    try:
        result = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        return 127, "", str(exc)
    return result.returncode, result.stdout.strip(), result.stderr.strip()


def _find_command(name: str, fallbacks: Iterable[str] = ()) -> str | None:
    found = shutil.which(name)
    if found:
        return found
    for candidate in fallbacks:
        if candidate and os.access(candidate, os.X_OK):
            return candidate
    return None


def check_sc_compose() -> Check:
    path = _find_command(
        "sc-compose",
        (
            os.path.expanduser("~/.local/bin/sc-compose"),
            "/opt/homebrew/bin/sc-compose",
            "/usr/local/bin/sc-compose",
        ),
    )
    if path is None:
        return Check(
            "sc-compose",
            False,
            "not found; install sc-compose"
            f"{MIN_SC_COMPOSE_TEXT} (see references/installation-and-troubleshooting.md)",
        )
    rc, stdout, stderr = _run([path, "--version"])
    version = _version(" ".join((stdout, stderr)))
    if rc != 0 or version is None:
        return Check(
            "sc-compose",
            False,
            f"{path} did not return a parseable version: {stderr or stdout or 'no output'}",
            path,
        )
    if version < MIN_SC_COMPOSE:
        return Check(
            "sc-compose",
            False,
            f"{path} reports {version[0]}.{version[1]}.{version[2]}; "
            f"required {MIN_SC_COMPOSE_TEXT}",
            path,
        )
    return Check("sc-compose", True, f"version {version[0]}.{version[1]}.{version[2]}", path)


def check_jq() -> Check:
    path = _find_command("jq", ("/opt/homebrew/bin/jq", "/usr/local/bin/jq"))
    if path is None:
        return Check("jq", False, "not found; install jq (see references/installation-and-troubleshooting.md)")
    rc, stdout, stderr = _run([path, "--version"])
    if rc != 0:
        return Check("jq", False, f"{path} failed --version: {stderr or stdout}", path)
    return Check("jq", True, stdout or "version reported", path)


def _python_candidates() -> list[str]:
    # The graph scripts invoke python3 (or GRAPH_ORCH_PYTHON), so checking a
    # different interpreter would produce a false green preflight.  Only use
    # fallback interpreters when python3 is absent; when it is present, tell
    # the caller to fix PATH or set GRAPH_ORCH_PYTHON instead.
    explicit = os.environ.get("GRAPH_ORCH_PYTHON")
    if explicit:
        return [explicit] if os.access(explicit, os.X_OK) else []
    resolved = shutil.which("python3")
    if resolved:
        return [resolved]
    return [
        path
        for path in (
            os.path.expanduser("~/.local/bin/python3"),
            "/opt/homebrew/bin/python3",
            "/usr/local/bin/python3",
        )
        if os.access(path, os.X_OK)
    ]


def check_rdflib() -> Check:
    candidates = _python_candidates()
    if not candidates:
        return Check("python3/rdflib", False, "python3 not found")
    failures: list[str] = []
    for path in candidates:
        rc, stdout, stderr = _run(
            [path, "-c", "import rdflib; print(rdflib.__version__)"],
        )
        if rc == 0 and stdout:
            return Check("python3/rdflib", True, f"{path} rdflib {stdout}", path)
        failures.append(f"{path}: {stderr or stdout or 'import failed'}")
    return Check(
        "python3/rdflib",
        False,
        "rdflib is unavailable to every discovered python3 interpreter: " + "; ".join(failures),
    )


def check_sc_compose_binding() -> Check:
    """Verify the maturin/PyO3 binding is installed beside the chosen Python.

    The CLI and Python wheel are separate artifacts.  Keeping this check here
    catches a split environment where the CLI is 1.2.x but ``sc_compose`` is
    an older wheel (or belongs to another interpreter).
    """

    candidates = _python_candidates()
    if not candidates:
        return Check(
            "python3/sc_compose",
            False,
            "python3 not found; install the binding with: " + SC_COMPOSE_INSTALL,
        )
    failures: list[str] = []
    probe = (
        "from importlib.metadata import version; "
        "import sc_compose; "
        "print(version('sc-compose'))"
    )
    for path in candidates:
        rc, stdout, stderr = _run([path, "-c", probe])
        parsed = _version(stdout)
        if rc == 0 and parsed is not None:
            if parsed < MIN_SC_COMPOSE:
                return Check(
                    "python3/sc_compose",
                    False,
                    f"{path} has sc-compose {stdout}; required >= 1.2.0; "
                    "reinstall with: " + SC_COMPOSE_INSTALL,
                    path,
                )
            return Check("python3/sc_compose", True, f"{path} binding {stdout}", path)
        failures.append(f"{path}: {stderr or stdout or 'import failed'}")
    return Check(
        "python3/sc_compose",
        False,
        "sc_compose binding is unavailable to the selected python3 interpreter; "
        "install it with: "
        + SC_COMPOSE_INSTALL
        + ". Details: "
        + "; ".join(failures),
    )


def check_pytest() -> Check:
    path = _find_command("pytest", ("/opt/homebrew/bin/pytest", "/usr/local/bin/pytest"))
    if path is None:
        return Check("pytest", False, "not found; install pytest for the test suite")
    rc, stdout, stderr = _run([path, "--version"])
    if rc != 0:
        return Check("pytest", False, f"{path} failed --version: {stderr or stdout}", path)
    return Check("pytest", True, stdout or "version reported", path)


def run_preflight(*, for_tests: bool = False, checks: Iterable[Callable[[], Check]] | None = None) -> dict[str, object]:
    selected = list(checks) if checks is not None else [
        check_sc_compose,
        check_sc_compose_binding,
        check_jq,
        check_rdflib,
    ]
    if for_tests and checks is None:
        selected.append(check_pytest)
    results = [check().as_dict() for check in selected]
    failures = [result for result in results if not result["ok"]]
    return {
        "success": not failures,
        "data": {"checks": results, "required_sc_compose": ">=1.2.0", "for_tests": for_tests},
        "error": None
        if not failures
        else {
            "code": "DEPENDENCY.PREFLIGHT_FAILED",
            "message": "one or more graph-orchestration dependencies are missing or unsupported",
            "checks": failures,
        },
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Verify graph-orchestration dependencies")
    parser.add_argument("--for-tests", action="store_true", help="also require pytest")
    args = parser.parse_args(argv)
    result = run_preflight(for_tests=args.for_tests)
    print(json.dumps(result, sort_keys=True))
    return 0 if result["success"] else 2


if __name__ == "__main__":
    sys.exit(main())
