"""Typed, durable state for one temporary daemon-service launch session.

Platform adapters deliberately live elsewhere.  This module owns only the
cross-platform control-plane contract: mode parsing, journal integrity, and
allowed phase transitions.  It has no ATM runtime, database, endpoint, or
service-manager dependency.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass, replace
from enum import Enum
import hashlib
import json
import os
from pathlib import Path
import stat
import tempfile
import uuid


class TemporaryLaunchError(RuntimeError):
    """Raised when temporary-launch recovery cannot be proven safe."""


class PeerWireSecurity(str, Enum):
    """The only peer-wire security values an overlay may select."""

    MUTUAL_TLS = "mutual-tls"
    PLAINTEXT_TEST = "plaintext-test"

    @classmethod
    def parse(cls, value: str) -> "PeerWireSecurity":
        try:
            return cls(value)
        except ValueError as error:
            allowed = ", ".join(member.value for member in cls)
            raise TemporaryLaunchError(
                f"unsupported peer-wire security {value!r}; expected one of: {allowed}"
            ) from error


class TemporaryLaunchPhase(str, Enum):
    """Persisted phases; every mutation requires an earlier journal phase."""

    CAPTURED = "captured"
    STOPPED = "stopped"
    OVERLAY_APPLIED = "overlay_applied"
    OVERLAY_STARTED = "overlay_started"
    QUIESCED = "quiesced"
    RESTORING = "restoring"
    COMPLETED = "completed"


_ALLOWED_TRANSITIONS: dict[TemporaryLaunchPhase, frozenset[TemporaryLaunchPhase]] = {
    TemporaryLaunchPhase.CAPTURED: frozenset(
        {TemporaryLaunchPhase.STOPPED, TemporaryLaunchPhase.RESTORING}
    ),
    TemporaryLaunchPhase.STOPPED: frozenset(
        {TemporaryLaunchPhase.OVERLAY_APPLIED, TemporaryLaunchPhase.RESTORING}
    ),
    TemporaryLaunchPhase.OVERLAY_APPLIED: frozenset(
        {TemporaryLaunchPhase.OVERLAY_STARTED, TemporaryLaunchPhase.RESTORING}
    ),
    TemporaryLaunchPhase.OVERLAY_STARTED: frozenset(
        {TemporaryLaunchPhase.QUIESCED, TemporaryLaunchPhase.RESTORING}
    ),
    TemporaryLaunchPhase.QUIESCED: frozenset(
        {TemporaryLaunchPhase.OVERLAY_STARTED, TemporaryLaunchPhase.RESTORING}
    ),
    TemporaryLaunchPhase.RESTORING: frozenset({TemporaryLaunchPhase.COMPLETED}),
    TemporaryLaunchPhase.COMPLETED: frozenset(),
}


@dataclass(frozen=True)
class CapturedLaunchSpec:
    """Private recovery material produced by one future platform adapter."""

    original_reference: str
    original_digest: str


@dataclass(frozen=True)
class OverlayLaunchSpec:
    """Private owned overlay reference produced by one future platform adapter."""

    overlay_reference: str
    overlay_digest: str


@dataclass(frozen=True)
class TemporaryLaunchSession:
    """Owner-only recovery journal schema for one selected CLI/daemon pair."""

    schema_version: int
    session_id: str
    phase: TemporaryLaunchPhase
    peer_wire_security: PeerWireSecurity
    platform: str
    account_id: str
    service: str
    cli_path: str
    cli_digest: str
    daemon_path: str
    daemon_digest: str
    original_reference: str
    original_digest: str
    overlay_reference: str | None = None
    overlay_digest: str | None = None

    @classmethod
    def captured(
        cls,
        *,
        peer_wire_security: PeerWireSecurity,
        platform: str,
        account_id: str,
        service: str,
        cli_path: Path,
        cli_digest: str,
        daemon_path: Path,
        daemon_digest: str,
        launch_spec: CapturedLaunchSpec,
    ) -> "TemporaryLaunchSession":
        return cls(
            schema_version=1,
            session_id=uuid.uuid4().hex,
            phase=TemporaryLaunchPhase.CAPTURED,
            peer_wire_security=peer_wire_security,
            platform=platform,
            account_id=account_id,
            service=service,
            cli_path=str(cli_path),
            cli_digest=cli_digest,
            daemon_path=str(daemon_path),
            daemon_digest=daemon_digest,
            original_reference=launch_spec.original_reference,
            original_digest=launch_spec.original_digest,
        )

    def with_overlay(self, overlay: OverlayLaunchSpec) -> "TemporaryLaunchSession":
        if self.phase is not TemporaryLaunchPhase.STOPPED:
            raise TemporaryLaunchError("an overlay may be recorded only after the stopped phase")
        return replace(
            self,
            phase=TemporaryLaunchPhase.OVERLAY_APPLIED,
            overlay_reference=overlay.overlay_reference,
            overlay_digest=overlay.overlay_digest,
        )

    def transition(self, target: TemporaryLaunchPhase) -> "TemporaryLaunchSession":
        if target not in _ALLOWED_TRANSITIONS[self.phase]:
            raise TemporaryLaunchError(
                f"cannot transition temporary-launch session from {self.phase.value} to {target.value}"
            )
        if target is TemporaryLaunchPhase.OVERLAY_STARTED and not self.overlay_digest:
            raise TemporaryLaunchError("cannot start a temporary overlay absent a recorded overlay digest")
        return replace(self, phase=target)

    def to_json(self) -> dict[str, object]:
        payload = asdict(self)
        payload["phase"] = self.phase.value
        payload["peer_wire_security"] = self.peer_wire_security.value
        return payload

    @classmethod
    def from_json(cls, payload: object) -> "TemporaryLaunchSession":
        if not isinstance(payload, dict):
            raise TemporaryLaunchError("temporary-launch journal must contain an object")
        required_strings = (
            "session_id",
            "platform",
            "account_id",
            "service",
            "cli_path",
            "cli_digest",
            "daemon_path",
            "daemon_digest",
            "original_reference",
            "original_digest",
        )
        if payload.get("schema_version") != 1 or any(
            not isinstance(payload.get(field), str) or not payload[field]
            for field in required_strings
        ):
            raise TemporaryLaunchError("temporary-launch journal has an unsupported or incomplete schema")
        for field in ("overlay_reference", "overlay_digest"):
            if payload.get(field) is not None and not isinstance(payload[field], str):
                raise TemporaryLaunchError(f"temporary-launch journal field {field} must be a string")
        try:
            session = cls(
                schema_version=1,
                session_id=payload["session_id"],
                phase=TemporaryLaunchPhase(payload.get("phase")),
                peer_wire_security=PeerWireSecurity(payload.get("peer_wire_security")),
                platform=payload["platform"],
                account_id=payload["account_id"],
                service=payload["service"],
                cli_path=payload["cli_path"],
                cli_digest=payload["cli_digest"],
                daemon_path=payload["daemon_path"],
                daemon_digest=payload["daemon_digest"],
                original_reference=payload["original_reference"],
                original_digest=payload["original_digest"],
                overlay_reference=payload.get("overlay_reference"),
                overlay_digest=payload.get("overlay_digest"),
            )
        except (TypeError, ValueError) as error:
            raise TemporaryLaunchError("temporary-launch journal has an invalid phase or mode") from error
        if session.phase in {
            TemporaryLaunchPhase.OVERLAY_APPLIED,
            TemporaryLaunchPhase.OVERLAY_STARTED,
            TemporaryLaunchPhase.QUIESCED,
        } and not (session.overlay_reference and session.overlay_digest):
            raise TemporaryLaunchError("temporary-launch overlay phase is missing its owned overlay evidence")
        return session


def sha256_file(path: Path) -> str:
    """Return a stable selected-pair identity without exposing file contents."""
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def account_identifier() -> str:
    """Use a portable local account identifier, never a hostname or team name."""
    if hasattr(os, "getuid"):
        return f"uid:{os.getuid()}"
    return f"user:{os.environ.get('USERNAME') or os.environ.get('USER') or 'unknown'}"


class TemporaryLaunchJournal:
    """One owner-only journal; its presence blocks competing lifecycle changes."""

    def __init__(self, path: Path) -> None:
        self.path = path

    def load(self) -> TemporaryLaunchSession | None:
        try:
            metadata = self.path.lstat()
        except FileNotFoundError:
            return None
        if not stat.S_ISREG(metadata.st_mode):
            raise TemporaryLaunchError(f"temporary-launch journal is not a regular file: {self.path}")
        self._require_private_owner(metadata)
        try:
            return TemporaryLaunchSession.from_json(json.loads(self.path.read_text(encoding="utf-8")))
        except (OSError, json.JSONDecodeError) as error:
            raise TemporaryLaunchError(f"cannot read temporary-launch journal: {self.path}") from error

    def require_no_active_session(self) -> None:
        session = self.load()
        if session is None:
            return
        if session.phase is TemporaryLaunchPhase.COMPLETED:
            raise TemporaryLaunchError(
                "completed temporary-launch journal was not finalized; inspect it before lifecycle mutation"
            )
        raise TemporaryLaunchError(
            "temporary-launch recovery is pending; run the explicit temporary-launch recover command "
            f"for session {session.session_id} before lifecycle mutation"
        )

    def require_session(self, session_id: str) -> TemporaryLaunchSession:
        session = self.load()
        if session is None:
            raise TemporaryLaunchError("no temporary-launch recovery session is active")
        if session.session_id != session_id:
            raise TemporaryLaunchError("temporary-launch session does not match the active recovery journal")
        if session.phase is TemporaryLaunchPhase.COMPLETED:
            raise TemporaryLaunchError("temporary-launch session is already completed")
        return session

    def create(self, session: TemporaryLaunchSession) -> None:
        """Publish the first active journal exactly once before a stop/mutation."""
        self._ensure_private_parent()
        encoded = self._encode(session)
        try:
            descriptor = os.open(self.path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        except FileExistsError as error:
            raise TemporaryLaunchError(
                "temporary-launch recovery is pending; refuse to overwrite its journal"
            ) from error
        try:
            if hasattr(os, "fchmod"):
                os.fchmod(descriptor, 0o600)
            with os.fdopen(descriptor, "wb") as destination:
                destination.write(encoded)
                destination.flush()
                os.fsync(destination.fileno())
            self._fsync_parent()
        except OSError as error:
            raise TemporaryLaunchError(f"cannot durably create temporary-launch journal: {self.path}") from error

    def save(self, session: TemporaryLaunchSession) -> None:
        """Advance an existing active session without allowing journal replacement."""
        self._ensure_private_parent()
        active = self.load()
        if active is None or active.session_id != session.session_id:
            raise TemporaryLaunchError("temporary-launch journal changed before its next transition")
        encoded = self._encode(session)
        descriptor, temporary_name = tempfile.mkstemp(
            dir=self.path.parent,
            prefix=f".{self.path.name}.",
        )
        temporary = Path(temporary_name)
        try:
            if hasattr(os, "fchmod"):
                os.fchmod(descriptor, 0o600)
            with os.fdopen(descriptor, "wb") as destination:
                destination.write(encoded)
                destination.flush()
                os.fsync(destination.fileno())
            os.replace(temporary, self.path)
            self._fsync_parent()
        except OSError as error:
            raise TemporaryLaunchError(f"cannot durably write temporary-launch journal: {self.path}") from error
        finally:
            temporary.unlink(missing_ok=True)

    @staticmethod
    def _encode(session: TemporaryLaunchSession) -> bytes:
        return (json.dumps(session.to_json(), indent=2, sort_keys=True) + "\n").encode("utf-8")

    def remove_after_completion(self, session: TemporaryLaunchSession) -> None:
        if session.phase is not TemporaryLaunchPhase.COMPLETED:
            raise TemporaryLaunchError("only a completed temporary-launch session may clear its journal")
        active = self.require_session_for_completion(session.session_id)
        if active != session:
            raise TemporaryLaunchError("temporary-launch journal changed before completion cleanup")
        try:
            self.path.unlink()
            self._fsync_parent()
        except OSError as error:
            raise TemporaryLaunchError(f"cannot finalize temporary-launch journal: {self.path}") from error

    def require_session_for_completion(self, session_id: str) -> TemporaryLaunchSession:
        session = self.load()
        if session is None or session.session_id != session_id:
            raise TemporaryLaunchError("temporary-launch journal changed before completion")
        return session

    def _ensure_private_parent(self) -> None:
        self.path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        metadata = self.path.parent.lstat()
        if not stat.S_ISDIR(metadata.st_mode):
            raise TemporaryLaunchError(f"temporary-launch journal parent is not a directory: {self.path.parent}")
        self._require_private_owner(metadata)
        if hasattr(os, "chmod"):
            os.chmod(self.path.parent, 0o700)

    def _require_private_owner(self, metadata: os.stat_result) -> None:
        if hasattr(os, "getuid") and metadata.st_uid != os.getuid():
            raise TemporaryLaunchError("temporary-launch journal is not owned by the current OS user")
        if hasattr(os, "getuid") and metadata.st_mode & 0o077:
            raise TemporaryLaunchError("temporary-launch journal is accessible by another OS user")

    def _fsync_parent(self) -> None:
        if os.name == "nt":
            return
        descriptor = os.open(self.path.parent, os.O_RDONLY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
