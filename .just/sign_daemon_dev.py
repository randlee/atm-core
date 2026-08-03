#!/usr/bin/env python3
"""Best-effort signing of local ATM daemon binaries for macOS development."""

from __future__ import annotations

from pathlib import Path
import re
import subprocess
import sys


IDENTITY = "atm-daemon-dev"
REPO_ROOT = Path(__file__).resolve().parents[1]
DAEMON_TARGETS = (
    REPO_ROOT / "target" / "debug" / "atm-daemon",
    REPO_ROOT / "target" / "release" / "atm-daemon",
)
IDENTITY_PATTERN = re.compile(rf'"{re.escape(IDENTITY)}"')


def has_development_identity() -> bool:
    """Return whether the local keychain exposes the exact development identity."""
    try:
        result = subprocess.run(
            ["security", "find-identity", "-v", "-p", "codesigning"],
            cwd=REPO_ROOT,
            check=False,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.SubprocessError):
        return False
    return result.returncode == 0 and any(
        IDENTITY_PATTERN.search(line) for line in result.stdout.splitlines()
    )


def sign_binary(binary: Path) -> None:
    """Sign one binary, swallowing unavailable-identity/tool failures."""
    try:
        subprocess.run(
            ["codesign", "-s", IDENTITY, "--force", str(binary)],
            cwd=REPO_ROOT,
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except (OSError, subprocess.SubprocessError):
        return


def main() -> int:
    """Sign existing debug/release binaries on macOS; otherwise do nothing."""
    if sys.platform != "darwin" or not has_development_identity():
        return 0
    for binary in DAEMON_TARGETS:
        try:
            exists = binary.is_file()
        except OSError:
            exists = False
        if exists:
            sign_binary(binary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
