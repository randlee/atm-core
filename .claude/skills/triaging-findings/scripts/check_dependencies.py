#!/usr/bin/env python3
"""Fail-closed dependency preflight for the triaging-findings skill."""

from __future__ import annotations

import json
import importlib.metadata
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

_CLAUDE_DIR = Path(__file__).resolve().parents[3]
if str(_CLAUDE_DIR) not in sys.path:
    sys.path.insert(0, str(_CLAUDE_DIR))

from lib.sc_compose_dependency import (  # noqa: E402
    MIN_SC_COMPOSE,
    MIN_SC_COMPOSE_BINDING,
    MIN_SC_COMPOSE_BINDING_TEXT,
    MIN_SC_COMPOSE_TEXT,
    SC_COMPOSE_BINDING_INSTALL,
    parse_version as _version,
)

REFERENCE = "references/installation-and-troubleshooting.md"


def _candidates(name: str) -> list[Path]:
    home = Path.home()
    values = [
        Path(shutil.which(name) or ""),
        home / ".local" / "bin" / name,
        home / ".cargo" / "bin" / name,
        Path(sys.prefix) / "bin" / name,
        Path("/opt/homebrew/bin") / name,
        Path("/usr/local/bin") / name,
    ]
    if sys.platform == "win32":
        values.extend([
            home / "AppData" / "Local" / "Programs" / name / name,
            home / "AppData" / "Roaming" / "Python" / "Scripts" / f"{name}.exe",
        ])
    return list(dict.fromkeys(path for path in values if str(path)))


def _find(name: str) -> Path | None:
    return next(
        (
            path
            for path in _candidates(name)
            if path.is_file() and (sys.platform == "win32" or os.access(path, os.X_OK))
        ),
        None,
    )


def _run_version(path: Path) -> tuple[str | None, str | None]:
    try:
        result = subprocess.run([str(path), "--version"], capture_output=True, text=True, timeout=10)
    except (OSError, subprocess.SubprocessError) as exc:
        return None, str(exc)
    output = (result.stdout or result.stderr).strip()
    return output if result.returncode == 0 else None, output


def _entry(name: str, required: str, *, minimum: tuple[int, int, int] | None = None) -> dict[str, Any]:
    path = _find(name)
    if path is None:
        return {"name": name, "required": required, "ok": False, "error": "not found on PATH or fallback paths"}
    version_text, detail = _run_version(path)
    parsed = _version(version_text)
    if version_text is None:
        return {"name": name, "required": required, "path": str(path), "ok": False, "error": detail or "--version failed"}
    if minimum is not None and (parsed is None or parsed < minimum):
        return {"name": name, "required": required, "path": str(path), "version": version_text, "ok": False, "error": f"requires >= {'.'.join(map(str, minimum))}"}
    return {"name": name, "required": required, "path": str(path), "version": version_text, "ok": True}


def _python_binding_entry() -> dict[str, Any]:
    try:
        import sc_compose  # noqa: F401
        installed = _version(importlib.metadata.version("sc-compose"))
        version_text = importlib.metadata.version("sc-compose")
    except (ImportError, importlib.metadata.PackageNotFoundError):
        return {
            "name": "sc_compose",
            "required": f"Python binding {MIN_SC_COMPOSE_BINDING_TEXT}",
            "path": sys.executable,
            "ok": False,
            "error": (
                "sc-compose Python bindings not installed. Run: "
                + SC_COMPOSE_BINDING_INSTALL
            ),
        }
    if installed is None or installed < MIN_SC_COMPOSE_BINDING:
        return {
            "name": "sc_compose",
            "required": f"Python binding {MIN_SC_COMPOSE_BINDING_TEXT}",
            "path": sys.executable,
            "version": version_text,
            "ok": False,
            "error": "requires " + MIN_SC_COMPOSE_BINDING_TEXT + "; reinstall with: " + SC_COMPOSE_BINDING_INSTALL,
        }
    return {"name": "sc_compose", "required": f"Python binding {MIN_SC_COMPOSE_BINDING_TEXT}", "path": sys.executable, "version": version_text, "ok": True}


def run() -> dict[str, Any]:
    checks = [
        _entry("sc-compose", MIN_SC_COMPOSE_TEXT, minimum=MIN_SC_COMPOSE),
        _entry("oxigraph", "installed"),
        _entry("rg", "installed"),
    ]
    try:
        import rdflib  # noqa: F401
        checks.append({"name": "rdflib", "required": "importable by this Python", "path": sys.executable, "version": getattr(rdflib, "__version__", "unknown"), "ok": True})
    except ImportError as exc:
        checks.append({"name": "rdflib", "required": "importable by this Python", "path": sys.executable, "ok": False, "error": str(exc)})
    checks.append(_python_binding_entry())
    failures = [item for item in checks if not item["ok"]]
    return {
        "success": not failures,
        "data": {"dependencies": checks, "reference": REFERENCE} if not failures else None,
        "error": None if not failures else {"code": "EXECUTION.DEPENDENCY", "message": "dependency preflight failed", "failures": failures, "reference": REFERENCE},
    }


def main() -> int:
    result = run()
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result["success"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
