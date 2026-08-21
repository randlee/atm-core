#!/usr/bin/env python3
"""Strictly sign managed local ATM binaries with Apple Development on macOS."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPTS_DIRECTORY = REPO_ROOT / "scripts"
if str(SCRIPTS_DIRECTORY) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIRECTORY))

from macos_development_signing import (  # noqa: E402
    CLI_IDENTIFIER,
    DAEMON_IDENTIFIER,
    SigningIdentity,
    SigningIdentityError,
    resolve_apple_development_identity,
    verify_apple_signature,
)


MANAGED_TARGETS = (
    (REPO_ROOT / "target" / "debug" / "atm", CLI_IDENTIFIER),
    (REPO_ROOT / "target" / "debug" / "atm-daemon", DAEMON_IDENTIFIER),
    (REPO_ROOT / "target" / "release" / "atm", CLI_IDENTIFIER),
    (REPO_ROOT / "target" / "release" / "atm-daemon", DAEMON_IDENTIFIER),
)
ENTITLEMENTS = REPO_ROOT / "scripts" / "macos_debug.entitlements"


def sign_and_verify_binary(binary: Path, identifier: str, identity: SigningIdentity) -> None:
    """Sign and strictly verify one managed binary or raise on any failure."""
    subprocess.run(
        [
            "codesign",
            "--force",
            "--sign",
            identity.fingerprint,
            "--identifier",
            identifier,
            "--entitlements",
            str(ENTITLEMENTS),
            str(binary),
        ],
        cwd=REPO_ROOT,
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if not verify_apple_signature(str(binary), identifier, identity.team_identifier):
        raise SigningIdentityError(
            f"post-sign verification failed for {binary}; expected {identifier} "
            "and the configured Apple Development team."
        )


def main() -> int:
    """Sign and verify existing managed binaries on macOS; otherwise do nothing."""
    if sys.platform == "win32":
        print("warning: Windows signing not yet implemented; skipping ATM binary signing.", file=sys.stderr)
        return 0
    if sys.platform != "darwin":
        return 0
    try:
        identity = resolve_apple_development_identity()
    except (OSError, subprocess.SubprocessError, SigningIdentityError) as error:
        print(f"error: unable to resolve Apple Development signing identity: {error}", file=sys.stderr)
        return 1
    for binary, identifier in MANAGED_TARGETS:
        try:
            exists = binary.is_file()
        except OSError:
            exists = False
        if exists:
            try:
                sign_and_verify_binary(binary, identifier, identity)
            except (OSError, subprocess.SubprocessError, SigningIdentityError) as error:
                print(f"error: unable to sign and verify {binary}: {error}", file=sys.stderr)
                return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
