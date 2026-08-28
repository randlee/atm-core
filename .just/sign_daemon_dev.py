#!/usr/bin/env python3
"""Strictly sign managed local ATM binaries with Apple Development on macOS."""

from __future__ import annotations

import os
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
KEYCHAIN_SECRET_FILE_ENVIRONMENT_VARIABLE = "ATM_SIGNING_KEYCHAIN_SECRET_FILE"
BENCHMARK_KEYCHAIN_SECRET_FILE = Path.home() / ".atmbench" / "keychain-secret"
LOGIN_KEYCHAIN = Path.home() / "Library" / "Keychains" / "login.keychain-db"


def keychain_secret_file() -> Path | None:
    """Return the optional, account-local login-keychain unlock secret.

    The dedicated ``atmbench`` account receives this non-repository file during
    its macOS provisioning.  Other accounts may provide an equivalent path via
    the environment variable; absence deliberately preserves normal signing
    behavior for developer machines whose login keychain is already unlocked.
    """
    configured = os.environ.get(KEYCHAIN_SECRET_FILE_ENVIRONMENT_VARIABLE, "").strip()
    if configured:
        return Path(configured).expanduser()
    if BENCHMARK_KEYCHAIN_SECRET_FILE.is_file():
        return BENCHMARK_KEYCHAIN_SECRET_FILE
    return None


def unlock_login_keychain(secret_file: Path | None = None) -> None:
    """Unlock the current account's login keychain when provisioned for it.

    The secret is read only from the account-local, untracked file and is never
    written to stdout/stderr or a repository artifact.  Signing fails closed if
    an explicitly provisioned secret cannot unlock the keychain.
    """
    secret_file = keychain_secret_file() if secret_file is None else secret_file
    if secret_file is None:
        return
    secret = secret_file.read_text(encoding="utf-8").strip()
    if not secret:
        raise SigningIdentityError(
            f"signing keychain secret file is empty: {secret_file}; repair local account provisioning."
        )
    subprocess.run(
        ["security", "unlock-keychain", "-p", secret, str(LOGIN_KEYCHAIN)],
        cwd=REPO_ROOT,
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


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
        unlock_login_keychain()
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
