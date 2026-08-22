"""Verified SQLite snapshots for the disposable physical-benchmark account.

This deliberately operates on no caller-supplied root.  Every public action
first revalidates the account-local manifest from :mod:`benchmark_account`, so
the interactive user's ADR-026 state can never be selected by an environment
variable or a convenient path argument.
"""
from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import sqlite3
import stat
from typing import Any

from scripts.smoke.benchmark_account import BenchmarkAccount, BenchmarkAccountError, require_benchmark_account


SNAPSHOT_SCHEMA_VERSION = 1
SNAPSHOT_ROOT_NAME = "benchmark-snapshots"
SNAPSHOT_MANIFEST_NAME = "snapshot.json"
MAIL_DATABASE_NAME = "mail.db"
SNAPSHOT_ID = re.compile(r"snapshot-[0-9]{8}T[0-9]{6}Z-[a-f0-9]{16}")


class BenchmarkSnapshotError(RuntimeError):
    """A benchmark-account snapshot cannot be safely created or restored."""


@dataclass(frozen=True)
class VerifiedSnapshot:
    """The account-bound facts verified before a snapshot may be restored."""

    snapshot_id: str
    directory: Path
    database: Path
    account_id: str
    durable_state_root: Path
    user_version: int
    page_count: int
    byte_count: int
    sha256: str


def _reject_symlink(path: Path, label: str) -> os.stat_result:
    try:
        metadata = path.lstat()
    except FileNotFoundError as error:
        raise BenchmarkSnapshotError(f"benchmark snapshot {label} is missing: {path}") from error
    except OSError as error:
        raise BenchmarkSnapshotError(f"could not inspect benchmark snapshot {label} {path}: {error}") from error
    if stat.S_ISLNK(metadata.st_mode):
        raise BenchmarkSnapshotError(f"benchmark snapshot {label} must not be a symlink: {path}")
    return metadata


def _require_directory(path: Path, label: str) -> None:
    metadata = _reject_symlink(path, label)
    if not stat.S_ISDIR(metadata.st_mode):
        raise BenchmarkSnapshotError(f"benchmark snapshot {label} is not a directory: {path}")


def _require_regular_file(path: Path, label: str) -> None:
    metadata = _reject_symlink(path, label)
    if not stat.S_ISREG(metadata.st_mode):
        raise BenchmarkSnapshotError(f"benchmark snapshot {label} is not a regular file: {path}")


def _snapshot_root(account: BenchmarkAccount) -> Path:
    parent = account.home / ".atm"
    _require_directory(parent, "account directory")
    root = parent / SNAPSHOT_ROOT_NAME
    if root.exists() or root.is_symlink():
        _require_directory(root, "root")
    else:
        try:
            root.mkdir(mode=0o700)
        except OSError as error:
            raise BenchmarkSnapshotError(f"could not create benchmark snapshot root {root}: {error}") from error
        _fsync_directory(parent, "benchmark-account directory")
    return root


def _mail_database(account: BenchmarkAccount) -> Path:
    _require_directory(account.durable_state_root, "durable state root")
    database = account.durable_state_root / MAIL_DATABASE_NAME
    _require_regular_file(database, "source database")
    return database


def _fsync_file(path: Path, label: str) -> None:
    try:
        with path.open("rb") as handle:
            os.fsync(handle.fileno())
    except OSError as error:
        raise BenchmarkSnapshotError(f"could not fsync benchmark snapshot {label} {path}: {error}") from error


def _fsync_directory(path: Path, label: str) -> None:
    if os.name == "nt":
        # Python exposes no portable directory-sync primitive on Windows.
        return
    try:
        descriptor = os.open(path, os.O_RDONLY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
    except OSError as error:
        raise BenchmarkSnapshotError(f"could not fsync benchmark snapshot {label} {path}: {error}") from error


def _sha256(path: Path) -> tuple[int, str]:
    digest = hashlib.sha256()
    byte_count = 0
    try:
        with path.open("rb") as handle:
            while chunk := handle.read(1024 * 1024):
                byte_count += len(chunk)
                digest.update(chunk)
    except OSError as error:
        raise BenchmarkSnapshotError(f"could not hash benchmark snapshot database {path}: {error}") from error
    return byte_count, digest.hexdigest()


def _database_facts(path: Path, label: str) -> tuple[int, int]:
    _require_regular_file(path, label)
    try:
        uri = f"{path.resolve().as_uri()}?mode=ro"
        with sqlite3.connect(uri, uri=True) as connection:
            quick_check = connection.execute("PRAGMA quick_check;").fetchone()
            user_version = connection.execute("PRAGMA user_version;").fetchone()
            page_count = connection.execute("PRAGMA page_count;").fetchone()
    except (OSError, sqlite3.Error, ValueError) as error:
        raise BenchmarkSnapshotError(f"benchmark snapshot {label} SQLite verification failed: {error}") from error
    if quick_check is None or quick_check[0] != "ok":
        raise BenchmarkSnapshotError(f"benchmark snapshot {label} SQLite quick_check failed: {quick_check}")
    if user_version is None or page_count is None:
        raise BenchmarkSnapshotError(f"benchmark snapshot {label} SQLite metadata was unavailable")
    return int(user_version[0]), int(page_count[0])


def _snapshot_id() -> str:
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    return f"snapshot-{timestamp}-{secrets.token_hex(8)}"


def _snapshot_manifest(
    account: BenchmarkAccount,
    snapshot_id: str,
    user_version: int,
    page_count: int,
    byte_count: int,
    sha256: str,
) -> dict[str, object]:
    return {
        "schema_version": SNAPSHOT_SCHEMA_VERSION,
        "snapshot_id": snapshot_id,
        "account_id": account.account_id,
        "durable_state_root": str(account.durable_state_root),
        "database": MAIL_DATABASE_NAME,
        "created_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "user_version": user_version,
        "page_count": page_count,
        "byte_count": byte_count,
        "sha256": sha256,
    }


def _write_manifest(path: Path, payload: dict[str, object]) -> None:
    temporary = path.with_name(f".{path.name}.{secrets.token_hex(8)}.tmp")
    try:
        descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            handle.write(json.dumps(payload, sort_keys=True, indent=2) + "\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    except OSError as error:
        raise BenchmarkSnapshotError(f"could not publish benchmark snapshot manifest {path}: {error}") from error
    finally:
        temporary.unlink(missing_ok=True)
    _fsync_directory(path.parent, "staging directory")


def _parse_verified_snapshot(account: BenchmarkAccount, snapshot_id: str) -> VerifiedSnapshot:
    if not SNAPSHOT_ID.fullmatch(snapshot_id):
        raise BenchmarkSnapshotError("benchmark snapshot identifier is malformed")
    directory = _snapshot_root(account) / snapshot_id
    _require_directory(directory, "directory")
    manifest_path = directory / SNAPSHOT_MANIFEST_NAME
    _require_regular_file(manifest_path, "manifest")
    try:
        payload: Any = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise BenchmarkSnapshotError(f"benchmark snapshot manifest is malformed: {error}") from error
    expected_keys = {
        "schema_version", "snapshot_id", "account_id", "durable_state_root", "database",
        "created_at", "user_version", "page_count", "byte_count", "sha256",
    }
    if not isinstance(payload, dict) or set(payload) != expected_keys:
        raise BenchmarkSnapshotError("benchmark snapshot manifest schema is unsupported")
    if (
        payload.get("schema_version") != SNAPSHOT_SCHEMA_VERSION
        or payload.get("snapshot_id") != snapshot_id
        or payload.get("account_id") != account.account_id
        or payload.get("durable_state_root") != str(account.durable_state_root)
        or payload.get("database") != MAIL_DATABASE_NAME
    ):
        raise BenchmarkSnapshotError("benchmark snapshot manifest does not match this benchmark account")
    created_at = payload.get("created_at")
    try:
        datetime.fromisoformat(str(created_at).replace("Z", "+00:00"))
    except ValueError as error:
        raise BenchmarkSnapshotError("benchmark snapshot manifest creation time is malformed") from error
    numeric_fields = ("user_version", "page_count", "byte_count")
    if any(not isinstance(payload.get(field), int) or payload[field] < 0 for field in numeric_fields):
        raise BenchmarkSnapshotError("benchmark snapshot manifest numeric metadata is malformed")
    sha256 = payload.get("sha256")
    if not isinstance(sha256, str) or not re.fullmatch(r"[a-f0-9]{64}", sha256):
        raise BenchmarkSnapshotError("benchmark snapshot manifest SHA-256 is malformed")
    database = directory / MAIL_DATABASE_NAME
    _fsync_file(database, "database")
    user_version, page_count = _database_facts(database, "database")
    byte_count, actual_sha256 = _sha256(database)
    if (user_version, page_count, byte_count, actual_sha256) != (
        payload["user_version"], payload["page_count"], payload["byte_count"], sha256,
    ):
        raise BenchmarkSnapshotError("benchmark snapshot database does not match its verified manifest")
    return VerifiedSnapshot(
        snapshot_id=snapshot_id,
        directory=directory,
        database=database,
        account_id=account.account_id,
        durable_state_root=account.durable_state_root,
        user_version=user_version,
        page_count=page_count,
        byte_count=byte_count,
        sha256=sha256,
    )


def create_verified_snapshot() -> VerifiedSnapshot:
    """Create and atomically publish a verified snapshot for this account only.

    A failed attempt deliberately leaves its hidden staging directory in place
    for diagnosis, but no completed snapshot manifest becomes a restore
    candidate until the database is consistent, fsynced, and hash-verified.
    """
    try:
        account = require_benchmark_account()
    except BenchmarkAccountError as error:
        raise BenchmarkSnapshotError(str(error)) from error
    source = _mail_database(account)
    root = _snapshot_root(account)
    snapshot_id = _snapshot_id()
    staging = root / f".{snapshot_id}.staging"
    final = root / snapshot_id
    try:
        staging.mkdir(mode=0o700)
        destination = staging / MAIL_DATABASE_NAME
        with sqlite3.connect(f"file:{source.resolve()}?mode=ro", uri=True) as reader:
            with sqlite3.connect(destination) as writer:
                reader.backup(writer)
        _fsync_file(destination, "staged database")
        user_version, page_count = _database_facts(destination, "staged database")
        byte_count, sha256 = _sha256(destination)
        _write_manifest(
            staging / SNAPSHOT_MANIFEST_NAME,
            _snapshot_manifest(account, snapshot_id, user_version, page_count, byte_count, sha256),
        )
        _fsync_directory(staging, "staging directory")
        os.replace(staging, final)
        _fsync_directory(root, "snapshot root")
    except (OSError, sqlite3.Error, BenchmarkSnapshotError) as error:
        raise BenchmarkSnapshotError(
            f"benchmark snapshot creation failed; preserved staging material at {staging}: {error}"
        ) from error
    return _parse_verified_snapshot(account, snapshot_id)


def verify_completed_snapshot(snapshot_id: str) -> VerifiedSnapshot:
    """Return a completed snapshot only after revalidating the current account."""
    try:
        account = require_benchmark_account()
    except BenchmarkAccountError as error:
        raise BenchmarkSnapshotError(str(error)) from error
    return _parse_verified_snapshot(account, snapshot_id)


def _assert_restore_sidecars_absent(database: Path) -> None:
    sidecars = [database.with_name(f"{database.name}{suffix}") for suffix in ("-wal", "-shm")]
    present = [str(path) for path in sidecars if path.exists() or path.is_symlink()]
    if present:
        raise BenchmarkSnapshotError(
            "benchmark restore requires the benchmark daemon to be stopped and SQLite sidecars absent; "
            f"found {', '.join(present)}"
        )


def restore_verified_snapshot(snapshot_id: str) -> VerifiedSnapshot:
    """Atomically activate a previously verified account-local SQLite snapshot.

    The caller must stop the paired benchmark daemon first.  The helper
    refuses active WAL sidecars rather than attempting a directory swap or
    deleting state that could belong to an active process.
    """
    try:
        account = require_benchmark_account()
    except BenchmarkAccountError as error:
        raise BenchmarkSnapshotError(str(error)) from error
    snapshot = _parse_verified_snapshot(account, snapshot_id)
    live_database = _mail_database(account)
    _assert_restore_sidecars_absent(live_database)
    staging = account.durable_state_root / f".{MAIL_DATABASE_NAME}.restore-staging-{secrets.token_hex(8)}"
    try:
        with sqlite3.connect(f"file:{snapshot.database.resolve()}?mode=ro", uri=True) as reader:
            with sqlite3.connect(staging) as writer:
                reader.backup(writer)
        _fsync_file(staging, "restore staging database")
        user_version, page_count = _database_facts(staging, "restore staging database")
        byte_count, sha256 = _sha256(staging)
        if (user_version, page_count, byte_count, sha256) != (
            snapshot.user_version, snapshot.page_count, snapshot.byte_count, snapshot.sha256,
        ):
            raise BenchmarkSnapshotError("restore staging database does not match the verified snapshot")
        os.replace(staging, live_database)
        _fsync_directory(account.durable_state_root, "durable state root")
    except (OSError, sqlite3.Error, BenchmarkSnapshotError) as error:
        raise BenchmarkSnapshotError(
            f"benchmark snapshot restore failed; preserved staging material at {staging}: {error}"
        ) from error
    return _parse_verified_snapshot(account, snapshot_id)
