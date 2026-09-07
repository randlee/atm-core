"""Fail-closed operator control plane for Herdr start-at-login entries.

This module deliberately owns only entry definitions and their native
registration.  It never starts Herdr, starts ATM, or inspects ATM roster data;
the caller supplies the already-native ``atm doctor --json`` projection.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass
from hashlib import sha256
import json
import os
from pathlib import Path
import platform as platform_module
import tempfile
from typing import Callable, Literal, Protocol


MARKER = "managed-by=atm daemon-switch"


class HerdrEntryError(Exception):
    """A stable machine-consumed entry-control failure."""

    def __init__(self, code: str, message: str, remedy: str, exit_code: int) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.remedy = remedy
        self.exit_code = exit_code


@dataclass(frozen=True)
class HerdrEndpoint:
    name: str
    socket_path: str | None = None


@dataclass(frozen=True)
class HerdrEntryJournal:
    entry_id: str
    platform: str
    digest: str
    action: Literal["install", "remove"]
    phase: Literal["planned", "written", "registered", "verified"]


class EntryPlatform(Protocol):
    """Small native-manager seam; tests provide platform fakes."""

    name: str

    def path_for(self, identifier: str) -> Path: ...
    def register(self, identifier: str, object_path: Path) -> None: ...
    def unregister(self, identifier: str) -> None: ...
    def is_registered(self, identifier: str) -> bool: ...
    def start(self, identifier: str) -> None: ...
    def account_matches(self, identifier: str) -> bool: ...


def identifier(platform: str, endpoint: str) -> str:
    """Return the deterministic start-at-login identifier for an endpoint."""
    if platform == "Darwin":
        return "com.randlee.atm.herdr-server" if endpoint == "default" else f"com.randlee.atm.herdr-server.{endpoint}"
    if platform == "Windows":
        return "ATM Herdr Server" if endpoint == "default" else f"ATM Herdr Server ({endpoint})"
    return "atm-herdr-server.service" if endpoint == "default" else f"atm-herdr-server@{endpoint}.service"


def canonical_object(platform: str, endpoint: str) -> str:
    """Render the complete owned object without any caller configuration."""
    command = "herdr server" if endpoint == "default" else f"herdr --session {endpoint} server"
    if platform == "Darwin":
        return "\n".join(("<?xml version=\"1.0\" encoding=\"UTF-8\"?>", "<plist version=\"1.0\"><dict>", f"<key>Label</key><string>{identifier(platform, endpoint)}</string>", f"<key>ProgramArguments</key><array><string>{command}</string></array>", "<key>RunAtLoad</key><true/>", "<key>KeepAlive</key><false/>", f"<key>Comment</key><string>{MARKER}</string>", "</dict></plist>", ""))
    if platform == "Windows":
        return "\n".join((f"task={identifier(platform, endpoint)}", "trigger=logon", "interactive=true", f"command={command}", f"comment={MARKER}", ""))
    return "\n".join(("[Unit]", f"Description=ATM Herdr entry ({endpoint})", f"X-ATM-Ownership={MARKER}", "[Service]", "Type=simple", f"ExecStart={command}", "[Install]", "WantedBy=default.target", ""))


def object_digest(rendered: str) -> str:
    return sha256(rendered.encode("utf-8")).hexdigest()


def _atomic_write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


class HerdrEntryManager:
    def __init__(self, root: Path, entry_platform: EntryPlatform) -> None:
        self.root = root
        self.platform = entry_platform

    @property
    def journal_path(self) -> Path:
        return self.root / "herdr-entry-journal.json"

    def _save_journal(self, journal: HerdrEntryJournal) -> None:
        _atomic_write(self.journal_path, json.dumps(asdict(journal), sort_keys=True) + "\n")

    def _load_journal(self) -> HerdrEntryJournal | None:
        if not self.journal_path.exists():
            return None
        try:
            data = json.loads(self.journal_path.read_text(encoding="utf-8"))
            return HerdrEntryJournal(**data)
        except (OSError, TypeError, ValueError) as error:
            raise HerdrEntryError("HERDR_ENTRY_JOURNAL_ACTIVE", "entry journal is unreadable", "Run status --repair after inspecting the journal", 3) from error

    def _clear_journal(self) -> None:
        self.journal_path.unlink(missing_ok=True)

    def _entry(self, endpoint: HerdrEndpoint) -> tuple[str, str, str, Path]:
        entry_id = identifier(self.platform.name, endpoint.name)
        rendered = canonical_object(self.platform.name, endpoint.name)
        return entry_id, rendered, object_digest(rendered), self.platform.path_for(entry_id)

    def _owned(self, path: Path, digest: str) -> tuple[bool, bool]:
        if not path.exists():
            return False, False
        content = path.read_text(encoding="utf-8")
        return MARKER in content, object_digest(content) == digest

    def _assert_no_active_journal(self) -> None:
        if self._load_journal() is not None:
            raise HerdrEntryError("HERDR_ENTRY_JOURNAL_ACTIVE", "an entry transaction is incomplete", "Run herdr-entry status --repair", 3)

    def install(self, endpoint: HerdrEndpoint, inject: Callable[[str], None] | None = None) -> dict[str, object]:
        self._assert_no_active_journal()
        if self.platform.name == "Windows" and not self.platform.account_matches(identifier(self.platform.name, endpoint.name)):
            raise HerdrEntryError("HERDR_ENTRY_ACCOUNT_MISMATCH", "Windows entry belongs to another account or session", "Reinstall both per-user under one account", 3)
        if endpoint.socket_path is not None:
            raise HerdrEntryError("HERDR_SOCKET_PATH_NO_ENTRY", "socket-path endpoint is externally owned", "Manage this Herdr endpoint outside daemon-switch", 3)
        entry_id, rendered, digest, path = self._entry(endpoint)
        owned, matching = self._owned(path, digest)
        if path.exists() and not owned:
            raise HerdrEntryError("HERDR_ENTRY_FOREIGN", "entry identifier is occupied by an unowned object", "Remove or rename the foreign object manually", 3)
        if owned and not matching:
            raise HerdrEntryError("HERDR_ENTRY_DIGEST_MISMATCH", "owned marker has a different canonical digest", "Inspect and explicitly repair or remove the entry", 3)
        journal = HerdrEntryJournal(entry_id, self.platform.name, digest, "install", "planned")
        self._save_journal(journal)
        _atomic_write(path, rendered)
        journal = HerdrEntryJournal(entry_id, self.platform.name, digest, "install", "written")
        self._save_journal(journal)
        if inject:
            inject("after_write")
        try:
            self.platform.register(entry_id, path)
        except Exception as error:
            raise HerdrEntryError("HERDR_ENTRY_REGISTER_FAILED", "platform registration failed", "Correct the platform failure, then run status --repair", 4) from error
        journal = HerdrEntryJournal(entry_id, self.platform.name, digest, "install", "registered")
        self._save_journal(journal)
        if inject:
            inject("after_register")
        owned, matching = self._owned(path, digest)
        if not owned or not matching or not self.platform.is_registered(entry_id):
            raise HerdrEntryError("HERDR_ENTRY_REGISTER_FAILED", "platform registration could not be verified", "Run status --repair after correcting the platform state", 4)
        self._save_journal(HerdrEntryJournal(entry_id, self.platform.name, digest, "install", "verified"))
        self._clear_journal()
        return self.entry_status(endpoint)

    def remove(self, endpoint: HerdrEndpoint) -> dict[str, object]:
        self._assert_no_active_journal()
        if self.platform.name == "Windows" and not self.platform.account_matches(identifier(self.platform.name, endpoint.name)):
            raise HerdrEntryError("HERDR_ENTRY_ACCOUNT_MISMATCH", "Windows entry belongs to another account or session", "Reinstall both per-user under one account", 3)
        if endpoint.socket_path is not None:
            raise HerdrEntryError("HERDR_SOCKET_PATH_NO_ENTRY", "socket-path endpoint is externally owned", "Manage this Herdr endpoint outside daemon-switch", 3)
        entry_id, _rendered, digest, path = self._entry(endpoint)
        if not path.exists():
            return self.entry_status(endpoint)
        owned, matching = self._owned(path, digest)
        if not owned:
            raise HerdrEntryError("HERDR_ENTRY_FOREIGN", "refusing to remove an unowned object", "Remove the foreign object manually", 3)
        if not matching:
            raise HerdrEntryError("HERDR_ENTRY_DIGEST_MISMATCH", "owned marker has a different canonical digest", "Inspect and explicitly repair or remove the entry", 3)
        self._save_journal(HerdrEntryJournal(entry_id, self.platform.name, digest, "remove", "planned"))
        self.platform.unregister(entry_id)
        self._save_journal(HerdrEntryJournal(entry_id, self.platform.name, digest, "remove", "registered"))
        path.unlink()
        if path.exists() or self.platform.is_registered(entry_id):
            raise HerdrEntryError("HERDR_ENTRY_REGISTER_FAILED", "entry removal could not be verified", "Run status --repair after correcting the platform state", 4)
        self._clear_journal()
        return self.entry_status(endpoint)

    def repair(self) -> dict[str, object] | None:
        journal = self._load_journal()
        if journal is None:
            return None
        path = self.platform.path_for(journal.entry_id)
        if journal.action == "install":
            owned, matching = self._owned(path, journal.digest)
            if not owned:
                raise HerdrEntryError("HERDR_ENTRY_FOREIGN", "repair found an unowned entry object", "Inspect the object manually", 3)
            if not matching:
                raise HerdrEntryError("HERDR_ENTRY_DIGEST_MISMATCH", "repair found a marker-bearing object with another digest", "Inspect the object manually", 3)
            if self.platform.is_registered(journal.entry_id):
                self._clear_journal()
                return {"repaired": "completed"}
            path.unlink(missing_ok=True)
            self._clear_journal()
            return {"repaired": "rolled_back"}
        if path.exists() and MARKER not in path.read_text(encoding="utf-8"):
            raise HerdrEntryError("HERDR_ENTRY_FOREIGN", "repair found an unowned entry object", "Inspect the object manually", 3)
        self.platform.unregister(journal.entry_id)
        path.unlink(missing_ok=True)
        self._clear_journal()
        return {"repaired": "rolled_back"}

    def entry_status(self, endpoint: HerdrEndpoint) -> dict[str, object]:
        entry_id, _rendered, digest, path = self._entry(endpoint)
        owned, matching = self._owned(path, digest)
        return {"endpoint": endpoint.name, "identifier": entry_id, "owned": owned, "registered": self.platform.is_registered(entry_id), "digest_matches": matching if owned else False, "journal_phase": (self._load_journal().phase if self._load_journal() else None)}

    def start_owned(self, endpoint: HerdrEndpoint) -> dict[str, object]:
        """Explicitly relaunch one verified AY.5 entry; never create or repair it."""
        self._assert_no_active_journal()
        status = self.entry_status(endpoint)
        if not status["owned"]:
            raise HerdrEntryError("HERDR_ENTRY_FOREIGN", "entry is missing or not owned", "Install the owned entry before restarting Herdr", 3)
        if not status["digest_matches"]:
            raise HerdrEntryError("HERDR_ENTRY_DIGEST_MISMATCH", "owned entry has a different canonical digest", "Inspect and explicitly repair the entry", 3)
        if not status["registered"]:
            raise HerdrEntryError("HERDR_ENTRY_REGISTER_FAILED", "owned entry is not registered", "Repair the entry before restarting Herdr", 4)
        try:
            self.platform.start(str(status["identifier"]))
        except Exception as error:
            raise HerdrEntryError("HERDR_ENTRY_REGISTER_FAILED", "owned entry could not be relaunched", "Correct the native entry and retry", 4) from error
        return status


class NativeEntryPlatform:
    """Native file/registration adapter; all calls are explicit operator actions."""
    def __init__(self, root: Path, runner: Callable[..., object]) -> None:
        self.name = platform_module.system()
        self.root = root
        self.runner = runner

    def path_for(self, entry_id: str) -> Path:
        suffix = ".plist" if self.name == "Darwin" else (".task" if self.name == "Windows" else "")
        return self.root / f"{entry_id}{suffix}"

    def register(self, entry_id: str, object_path: Path) -> None:
        if self.name == "Darwin":
            command = ["launchctl", "bootstrap", f"gui/{os.getuid()}", str(object_path)]
        elif self.name == "Windows":
            command = ["schtasks.exe", "/Create", "/TN", entry_id, "/XML", str(object_path), "/F"]
        else:
            command = ["systemctl", "--user", "enable", entry_id]
        result = self.runner(command, timeout=20.0)
        if getattr(result, "returncode", 1) != 0:
            raise RuntimeError(getattr(result, "stderr", "platform registration failed"))

    def unregister(self, entry_id: str) -> None:
        if self.name == "Darwin":
            command = ["launchctl", "bootout", f"gui/{os.getuid()}/{entry_id}"]
        elif self.name == "Windows":
            command = ["schtasks.exe", "/Delete", "/TN", entry_id, "/F"]
        else:
            command = ["systemctl", "--user", "disable", "--now", entry_id]
        result = self.runner(command, timeout=20.0)
        if getattr(result, "returncode", 1) != 0:
            raise RuntimeError(getattr(result, "stderr", "platform unregistration failed"))

    def is_registered(self, entry_id: str) -> bool:
        # Native registrations are verified through their manager at repair
        # time. The object cannot be assumed registered merely because it exists.
        command = (["launchctl", "print", f"gui/{os.getuid()}/{entry_id}"] if self.name == "Darwin" else (["schtasks.exe", "/Query", "/TN", entry_id] if self.name == "Windows" else ["systemctl", "--user", "is-enabled", entry_id]))
        return getattr(self.runner(command, timeout=5.0), "returncode", 1) == 0

    def start(self, entry_id: str) -> None:
        if self.name == "Darwin":
            command = ["launchctl", "kickstart", "-k", f"gui/{os.getuid()}/{entry_id}"]
        elif self.name == "Windows":
            command = ["schtasks.exe", "/Run", "/TN", entry_id]
        else:
            command = ["systemctl", "--user", "restart", entry_id]
        result = self.runner(command, timeout=30.0)
        if getattr(result, "returncode", 1) != 0:
            raise RuntimeError(getattr(result, "stderr", "platform entry start failed"))

    def account_matches(self, _entry_id: str) -> bool:
        return True
