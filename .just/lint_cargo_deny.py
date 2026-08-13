#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

from lint_common import discover_repo_root
from lint_common import workspace_crate_section_lines


DEPRECATED_CONFIG_LINES = (
    "vulnerability = ",
    "unlicensed = ",
)
DNS_RETRY_ATTEMPTS = 3
DNS_RETRY_DELAY_SECONDS = 2
DNS_RESOLUTION_FAILURE_MARKER = "could not resolve hostname"


def build_command(repo_root: Path, config_path: Path) -> list[str]:
    return [
        "cargo-deny",
        "--config",
        str(config_path),
        "check",
        "advisories",
        "bans",
        "licenses",
        "sources",
    ]


def build_runtime_config(repo_root: Path) -> Path:
    source_path = repo_root / "deny.toml"
    text = source_path.read_text(encoding="utf-8")
    filtered_lines = [
        line
        for line in text.splitlines()
        if not any(line.lstrip().startswith(prefix) for prefix in DEPRECATED_CONFIG_LINES)
    ]
    temp_dir = Path(tempfile.mkdtemp(prefix="atm-lint-deny-"))
    runtime_path = temp_dir / "deny.toml"
    runtime_path.write_text("\n".join(filtered_lines).rstrip() + "\n", encoding="utf-8")
    return runtime_path


def emit_console_text(text: str, *, stream = sys.stdout) -> None:
    if not text:
        return
    encoding = getattr(stream, "encoding", None) or "utf-8"
    if hasattr(stream, "buffer"):
        stream.buffer.write(text.encode(encoding, errors="replace"))
        stream.flush()
        return
    stream.write(text.encode(encoding, errors="replace").decode(encoding))
    stream.flush()


def is_dns_resolution_failure(completed: subprocess.CompletedProcess[str]) -> bool:
    output = "\n".join((completed.stdout or "", completed.stderr or "")).lower()
    return completed.returncode != 0 and DNS_RESOLUTION_FAILURE_MARKER in output


def run_cargo_deny(command: list[str], repo_root: Path) -> subprocess.CompletedProcess[str]:
    for attempt in range(1, DNS_RETRY_ATTEMPTS + 1):
        completed = subprocess.run(
            command,
            cwd=repo_root,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
        if not is_dns_resolution_failure(completed) or attempt == DNS_RETRY_ATTEMPTS:
            return completed

        print(
            f"cargo-deny DNS resolution failed; retrying ({attempt}/{DNS_RETRY_ATTEMPTS})...",
            file=sys.stderr,
        )
        time.sleep(DNS_RETRY_DELAY_SECONDS)

    raise AssertionError("cargo-deny retry loop must return")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Run cargo-deny with the repo policy.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)

    if shutil.which("cargo-deny") is None:
        print("cargo-deny is not installed; install it to run this lint", file=sys.stderr)
        return 2

    for line in workspace_crate_section_lines(repo_root):
        print(line)

    runtime_config = build_runtime_config(repo_root)
    completed = run_cargo_deny(build_command(repo_root, runtime_config), repo_root)
    if completed.stdout:
        emit_console_text(completed.stdout)
    if completed.stderr:
        emit_console_text(completed.stderr, stream=sys.stderr)
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
