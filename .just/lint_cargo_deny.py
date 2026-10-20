#!/usr/bin/env python3
"""Delegate cargo-deny to sc-lint (delete when randlee/sc-lint#371 ships)."""
from __future__ import annotations

from pathlib import Path
import subprocess
import sys
import time


# delete when randlee/sc-lint#371 ships: retry transient DNS resolution
# failures from the published cargo-deny runner.
DNS_RETRY_ATTEMPTS = 3
DNS_RETRY_DELAY_SECONDS = 2
DNS_RESOLUTION_FAILURE_MARKER = "could not resolve hostname"


def is_dns_resolution_failure(completed: subprocess.CompletedProcess[str]) -> bool:
    output = "\n".join((completed.stdout or "", completed.stderr or "")).lower()
    return completed.returncode != 0 and DNS_RESOLUTION_FAILURE_MARKER in output


def run_published_deny(command: list[str], repo_root: Path) -> subprocess.CompletedProcess[str]:
    for attempt in range(1, DNS_RETRY_ATTEMPTS + 1):
        completed = subprocess.run(
            command,
            cwd=repo_root,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        if not is_dns_resolution_failure(completed) or attempt == DNS_RETRY_ATTEMPTS:
            return completed
        print(
            f"cargo-deny DNS resolution failed; retrying ({attempt}/{DNS_RETRY_ATTEMPTS})...",
            file=sys.stderr,
        )
        time.sleep(DNS_RETRY_DELAY_SECONDS)
    raise AssertionError("cargo-deny retry loop must return")


def emit_output(completed: subprocess.CompletedProcess[str]) -> None:
    if completed.stdout:
        print(completed.stdout, end="")
    if completed.stderr:
        print(completed.stderr, end="", file=sys.stderr)


def main(argv: list[str]) -> int:
    repo_root = Path(__file__).resolve().parent.parent
    venv_python = repo_root / ".sc-lint/venv" / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
    completed = run_published_deny(
        [str(venv_python), "-m", "sc_lint.lint_cargo_deny", "--root", str(repo_root), *argv[1:]],
        repo_root,
    )
    emit_output(completed)
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
