#!/usr/bin/env python3
"""Strictly sign managed local ATM binaries when the development key is available."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPTS_DIRECTORY = REPO_ROOT / "scripts"
if str(SCRIPTS_DIRECTORY) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIRECTORY))

from macos_development_signing import (  # noqa: E402
    DEVELOPMENT_SIGNING_IDENTITY,
    find_identity_output_has_development_identity,
)


IDENTITY = DEVELOPMENT_SIGNING_IDENTITY
MANAGED_TARGETS = (
    REPO_ROOT / "target" / "debug" / "atm",
    REPO_ROOT / "target" / "debug" / "atm-daemon",
    REPO_ROOT / "target" / "release" / "atm",
    REPO_ROOT / "target" / "release" / "atm-daemon",
)
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
    return result.returncode == 0 and find_identity_output_has_development_identity(result.stdout)


def sign_and_verify_binary(binary: Path) -> None:
    """Sign and strictly verify one managed binary or raise on any failure."""
    subprocess.run(
        ["codesign", "--force", "--sign", IDENTITY, str(binary)],
        cwd=REPO_ROOT,
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    subprocess.run(
        ["codesign", "--verify", "--strict", str(binary)],
        cwd=REPO_ROOT,
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def main() -> int:
    """Sign and verify existing managed binaries on macOS; otherwise do nothing."""
    if sys.platform != "darwin" or not has_development_identity():
        return 0
    for binary in MANAGED_TARGETS:
        try:
            exists = binary.is_file()
        except OSError:
            exists = False
        if exists:
            try:
                sign_and_verify_binary(binary)
            except (OSError, subprocess.SubprocessError) as error:
                print(f"error: unable to sign and verify {binary}: {error}", file=sys.stderr)
                return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
