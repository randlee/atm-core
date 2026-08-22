"""Fail-closed identity contract for the disposable physical-benchmark account.

This module deliberately does not choose an ATM root.  ADR-026 derives that
root from the executing OS user.  The manifest only proves that this user was
explicitly bootstrapped as the disposable benchmark account before the harness
may start a daemon or touch benchmark state.
"""
from __future__ import annotations

import csv
from dataclasses import dataclass
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import secrets
import stat
import subprocess
from typing import Any


MANIFEST_NAME = "benchmark-account.json"
MANIFEST_SCHEMA_VERSION = 1
TOKEN_LENGTH = 43


class BenchmarkAccountError(RuntimeError):
    """The runner cannot establish a safe disposable benchmark account."""


@dataclass(frozen=True)
class BenchmarkAccount:
    """Validated facts about the current benchmark OS account."""

    account_id: str
    home: Path
    durable_state_root: Path
    manifest_path: Path


def account_home() -> Path:
    """Return the account home from OS identity, never a shell HOME override."""
    if os.name == "nt":
        profile = os.environ.get("USERPROFILE", "").strip()
        if not profile:
            raise BenchmarkAccountError("could not resolve the Windows benchmark-account profile")
        return Path(profile).resolve()
    import pwd

    return Path(pwd.getpwuid(os.geteuid()).pw_dir).resolve()


def canonical_durable_state_root(home: Path | None = None) -> Path:
    """Return the ADR-026 durable state root for exactly one OS account."""
    return (home or account_home()) / ".atm" / "db"


def manifest_path(home: Path | None = None) -> Path:
    """Return the one account-local manifest location; callers cannot override it."""
    return (home or account_home()) / ".atm" / MANIFEST_NAME


def current_account_id() -> str:
    """Return a stable UID/SID-shaped identifier for the executing OS account."""
    if os.name != "nt":
        return f"uid:{os.geteuid()}"
    result = subprocess.run(
        ["whoami", "/user", "/fo", "csv", "/nh"], capture_output=True, text=True, check=False,
    )
    if result.returncode != 0:
        raise BenchmarkAccountError("could not resolve the Windows benchmark-account SID")
    rows = list(csv.reader(line for line in result.stdout.splitlines() if line.strip()))
    if len(rows) != 1 or len(rows[0]) < 2 or not rows[0][1].strip():
        raise BenchmarkAccountError("Windows benchmark-account SID response was malformed")
    return f"sid:{rows[0][1].strip()}"


def _windows_current_owner_sids() -> set[str]:
    """Return the account and group SIDs carried by the executing token."""
    result = subprocess.run(
        [
            "powershell",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            """
            $identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
            $identity.User.Value
            $identity.Groups | ForEach-Object { $_.Value }
            """,
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    values = {f"sid:{line.strip()}".casefold() for line in result.stdout.splitlines() if line.strip()}
    if result.returncode != 0 or not values:
        detail = result.stderr.strip() or "no output"
        raise BenchmarkAccountError(
            f"could not resolve Windows benchmark-account owner principals (exit {result.returncode}: {detail})"
        )
    return values


def _windows_file_owner(path: Path) -> str:
    literal_path = str(path).replace("'", "''")
    result = subprocess.run(
        [
            "powershell",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-Acl -LiteralPath "
            f"'{literal_path}').GetOwner([System.Security.Principal.SecurityIdentifier]).Value",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    owner = result.stdout.strip()
    if result.returncode != 0 or not owner:
        detail = result.stderr.strip() or "no output"
        raise BenchmarkAccountError(
            f"could not verify Windows manifest owner for {path} (exit {result.returncode}: {detail})"
        )
    return f"sid:{owner}".casefold()


def _verify_manifest_owner(path: Path, metadata: os.stat_result) -> None:
    if os.name != "nt":
        if metadata.st_uid != os.geteuid():
            raise BenchmarkAccountError("benchmark-account manifest is not owned by the executing account")
        return
    if _windows_file_owner(path) not in _windows_current_owner_sids():
        raise BenchmarkAccountError("benchmark-account manifest is not owned by the executing account")


def _verify_no_symlink(path: Path, label: str) -> os.stat_result:
    try:
        metadata = path.lstat()
    except FileNotFoundError as error:
        raise BenchmarkAccountError(f"benchmark-account {label} is missing: {path}") from error
    except OSError as error:
        raise BenchmarkAccountError(f"could not inspect benchmark-account {label} {path}: {error}") from error
    if stat.S_ISLNK(metadata.st_mode):
        raise BenchmarkAccountError(f"benchmark-account {label} must not be a symlink: {path}")
    return metadata


def _verify_directory(path: Path, label: str) -> None:
    metadata = _verify_no_symlink(path, label)
    if not stat.S_ISDIR(metadata.st_mode):
        raise BenchmarkAccountError(f"benchmark-account {label} is not a directory: {path}")


def _manifest_payload(account: BenchmarkAccount, created_at: str, token: str) -> dict[str, object]:
    return {
        "schema_version": MANIFEST_SCHEMA_VERSION,
        "account_id": account.account_id,
        "home": str(account.home),
        "durable_state_root": str(account.durable_state_root),
        "created_at": created_at,
        "bootstrap_token": token,
    }


def _parse_manifest(path: Path) -> BenchmarkAccount:
    metadata = _verify_no_symlink(path, "manifest")
    if not stat.S_ISREG(metadata.st_mode):
        raise BenchmarkAccountError(f"benchmark-account manifest is not a regular file: {path}")
    if os.name != "nt" and metadata.st_mode & (stat.S_IWGRP | stat.S_IWOTH):
        raise BenchmarkAccountError("benchmark-account manifest must not be group- or world-writable")
    _verify_manifest_owner(path, metadata)
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise BenchmarkAccountError(f"benchmark-account manifest is malformed: {error}") from error
    if not isinstance(payload, dict):
        raise BenchmarkAccountError("benchmark-account manifest must contain a JSON object")
    expected_keys = {
        "schema_version",
        "account_id",
        "home",
        "durable_state_root",
        "created_at",
        "bootstrap_token",
    }
    if set(payload) != expected_keys or payload.get("schema_version") != MANIFEST_SCHEMA_VERSION:
        raise BenchmarkAccountError("benchmark-account manifest schema is unsupported")
    account_id = payload.get("account_id")
    home_text = payload.get("home")
    state_text = payload.get("durable_state_root")
    created_at = payload.get("created_at")
    token = payload.get("bootstrap_token")
    if not all(isinstance(value, str) and value for value in (account_id, home_text, state_text, created_at, token)):
        raise BenchmarkAccountError("benchmark-account manifest has missing required values")
    if len(token) != TOKEN_LENGTH or not all(character.isalnum() or character in "-_" for character in token):
        raise BenchmarkAccountError("benchmark-account manifest bootstrap token is malformed")
    try:
        datetime.fromisoformat(created_at.replace("Z", "+00:00"))
    except ValueError as error:
        raise BenchmarkAccountError("benchmark-account manifest creation time is malformed") from error
    home = account_home()
    state_root = canonical_durable_state_root(home)
    if account_id != current_account_id():
        raise BenchmarkAccountError("benchmark-account manifest does not match the executing account")
    if Path(home_text) != home or Path(state_text) != state_root:
        raise BenchmarkAccountError("benchmark-account manifest does not match this account's canonical state")
    return BenchmarkAccount(account_id, home, state_root, path)


def require_benchmark_account() -> BenchmarkAccount:
    """Validate the current account before any benchmark setup side effect."""
    home = account_home()
    manifest_parent = home / ".atm"
    _verify_directory(manifest_parent, "manifest directory")
    return _parse_manifest(manifest_path(home))


def bootstrap_benchmark_account() -> BenchmarkAccount:
    """Create the account-local manifest only for a clean disposable account.

    This is an explicit bootstrap action, never a side effect of benchmark
    execution.  A user with an existing ATM durable root cannot label their
    interactive account as disposable.
    """
    home = account_home()
    state_root = canonical_durable_state_root(home)
    if state_root.exists():
        raise BenchmarkAccountError(
            "refusing to bootstrap a benchmark manifest beside an existing ATM durable state"
        )
    parent = home / ".atm"
    if parent.exists():
        _verify_directory(parent, "manifest directory")
    else:
        try:
            parent.mkdir(mode=0o700)
        except OSError as error:
            raise BenchmarkAccountError(f"could not create benchmark-account directory {parent}: {error}") from error
    path = manifest_path(home)
    if path.exists() or path.is_symlink():
        raise BenchmarkAccountError(f"benchmark-account manifest already exists: {path}")
    account = BenchmarkAccount(current_account_id(), home, state_root, path)
    created_at = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
    payload = json.dumps(
        _manifest_payload(account, created_at, secrets.token_urlsafe(32)), sort_keys=True, indent=2,
    ) + "\n"
    try:
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
    except OSError as error:
        raise BenchmarkAccountError(f"could not create benchmark-account manifest {path}: {error}") from error
    if os.name != "nt":
        try:
            directory_descriptor = os.open(parent, os.O_RDONLY)
            try:
                os.fsync(directory_descriptor)
            finally:
                os.close(directory_descriptor)
        except OSError as error:
            try:
                path.unlink()
            except OSError:
                pass
            raise BenchmarkAccountError(f"could not finalize benchmark-account manifest {path}: {error}") from error
    return require_benchmark_account()
