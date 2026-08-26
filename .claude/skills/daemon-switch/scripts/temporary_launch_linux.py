"""Narrow systemd-user drop-in overlay for one typed temporary daemon session."""

from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import re
import shlex
import stat
import subprocess
import tempfile
from typing import Callable, Sequence

from temporary_launch import (
    CapturedLaunchSpec,
    OverlayLaunchSpec,
    PeerWireSecurity,
    TemporaryLaunchError,
    TemporaryLaunchSession,
)


SYSTEMD_UNIT_NAME = re.compile(r"^[A-Za-z0-9_.@-]+(?:\.service)?$")
SAFE_SYSTEMD_ARGUMENT = re.compile(r"^[A-Za-z0-9_./:=,+@~-]+$")
SystemctlRunner = Callable[[Sequence[str], float], subprocess.CompletedProcess[str]]


class LinuxSystemdUserAdapter:
    """Own exactly one user drop-in; never rewrite a base unit or another override."""

    def __init__(self, user_unit_directory: Path, runner: SystemctlRunner) -> None:
        self.user_unit_directory = user_unit_directory
        self.runner = runner

    def capture(
        self,
        args: argparse.Namespace,
        _cli: Path,
        daemon: Path,
        _mode: PeerWireSecurity,
    ) -> CapturedLaunchSpec:
        """Read one direct base-unit invocation without changing service state."""
        source, contents, _argv = self._validated_source(args, daemon, expected_dropin=None)
        return CapturedLaunchSpec(str(source), self._digest(contents))

    def apply_overlay(
        self,
        args: argparse.Namespace,
        session: TemporaryLaunchSession,
    ) -> OverlayLaunchSpec:
        """Publish an owned drop-in before its durable overlay record is written."""
        source, contents, argv = self._validated_source(
            args,
            Path(session.daemon_path),
            expected_dropin=None,
        )
        if str(source) != session.original_reference or self._digest(contents) != session.original_digest:
            raise TemporaryLaunchError("temporary-launch systemd source unit changed before overlay creation")
        destination = self._dropin_path(session)
        encoded = self._render_dropin([*argv, "--peer-wire-security", session.peer_wire_security.value])
        self._publish_owned_dropin(destination, encoded)
        return OverlayLaunchSpec(str(destination), self._digest(encoded))

    def activate_overlay(self, args: argparse.Namespace, session: TemporaryLaunchSession) -> None:
        """Reload only after the exact owned drop-in is durable in the journal."""
        dropin = self._session_dropin(session)
        self._validated_source(args, Path(session.daemon_path), expected_dropin=None)
        self._daemon_reload()
        _source, _contents, _argv = self._validated_source(
            args,
            Path(session.daemon_path),
            expected_dropin=dropin,
        )

    def start_args(
        self,
        args: argparse.Namespace,
        session: TemporaryLaunchSession,
    ) -> argparse.Namespace:
        """systemd retains the selected drop-in; no direct-process argument path exists."""
        self._session_dropin(session)
        return args

    def restore_exact(self, args: argparse.Namespace, session: TemporaryLaunchSession) -> None:
        """Remove only the verified drop-in and reload, tolerating an interrupted prior removal."""
        source, contents, argv = self._validated_source(
            args,
            Path(session.daemon_path),
            expected_dropin=self._dropin_path(session),
            allow_missing_expected=True,
        )
        if str(source) != session.original_reference or self._digest(contents) != session.original_digest:
            raise TemporaryLaunchError("temporary-launch systemd source unit changed; refusing restore")
        dropin = self._dropin_path(session)
        if session.overlay_reference is None:
            self._remove_orphaned_expected_dropin(dropin, argv, session.peer_wire_security)
        else:
            if Path(session.overlay_reference) != dropin or not session.overlay_digest:
                raise TemporaryLaunchError("temporary-launch journal does not name this session's owned drop-in")
            try:
                metadata = dropin.lstat()
            except FileNotFoundError:
                pass
            else:
                if not stat.S_ISREG(metadata.st_mode) or self._digest(dropin.read_bytes()) != session.overlay_digest:
                    raise TemporaryLaunchError("temporary-launch owned drop-in changed; refusing restore")
                dropin.unlink()
                self._fsync_directory(dropin.parent)
        self._daemon_reload()
        self._validated_source(args, Path(session.daemon_path), expected_dropin=None)

    def _validated_source(
        self,
        args: argparse.Namespace,
        daemon: Path,
        *,
        expected_dropin: Path | None,
        allow_missing_expected: bool = False,
    ) -> tuple[Path, bytes, list[str]]:
        fragment, dropins = self._unit_layout(args)
        expected = [] if expected_dropin is None else [expected_dropin]
        expected_or_absent = allow_missing_expected and expected_dropin is not None and not dropins
        if dropins != expected and not expected_or_absent:
            raise TemporaryLaunchError("systemd user service has unsupported or changed drop-ins")
        try:
            metadata = fragment.lstat()
            contents = fragment.read_bytes()
        except OSError as error:
            raise TemporaryLaunchError(f"cannot read systemd user unit: {fragment}") from error
        if not stat.S_ISREG(metadata.st_mode):
            raise TemporaryLaunchError("temporary-launch systemd source unit must be a regular file")
        argv = self._parse_direct_exec_start(contents, daemon)
        return fragment, contents, argv

    def _unit_layout(self, args: argparse.Namespace) -> tuple[Path, list[Path]]:
        unit = self._unit_name(args)
        result = self.runner(
            [
                "systemctl",
                "--user",
                "show",
                unit,
                "--property=FragmentPath",
                "--property=DropInPaths",
                "--no-pager",
            ],
            20.0,
        )
        if result.returncode != 0:
            detail = (result.stderr or result.stdout).strip()
            raise TemporaryLaunchError(f"cannot inspect systemd user service {unit}: {detail}")
        properties = self._properties(result.stdout)
        raw_fragment = properties.get("FragmentPath")
        if not raw_fragment:
            raise TemporaryLaunchError("systemd user service does not expose one FragmentPath")
        raw_dropins = properties.get("DropInPaths", "")
        return Path(raw_fragment).expanduser().resolve(), [
            Path(value).expanduser().resolve() for value in raw_dropins.split() if value
        ]

    @staticmethod
    def _properties(output: str) -> dict[str, str]:
        properties: dict[str, str] = {}
        for line in output.splitlines():
            key, separator, value = line.partition("=")
            if separator:
                properties[key] = value
        return properties

    def _unit_name(self, args: argparse.Namespace) -> str:
        service = getattr(args, "service", None)
        if not isinstance(service, str) or not SYSTEMD_UNIT_NAME.fullmatch(service):
            raise TemporaryLaunchError("Linux temporary-launch requires one simple systemd user service name")
        return service if service.endswith(".service") else f"{service}.service"

    @staticmethod
    def _parse_direct_exec_start(contents: bytes, daemon: Path) -> list[str]:
        try:
            text = contents.decode("utf-8")
        except UnicodeDecodeError as error:
            raise TemporaryLaunchError("systemd user unit is not UTF-8 text") from error
        section = ""
        exec_starts: list[str] = []
        for line in text.splitlines():
            stripped = line.strip()
            if not stripped or stripped.startswith(("#", ";")):
                continue
            if stripped.startswith("[") and stripped.endswith("]"):
                section = stripped[1:-1]
                continue
            if section != "Service":
                continue
            key, separator, value = stripped.partition("=")
            if not separator:
                raise TemporaryLaunchError("systemd user unit has an unsupported Service directive")
            if key == "EnvironmentFile":
                raise TemporaryLaunchError("systemd user unit EnvironmentFile selection is unsupported")
            if key.startswith("ExecStart"):
                if key != "ExecStart" or not value:
                    raise TemporaryLaunchError("systemd user unit must have one direct ExecStart")
                exec_starts.append(value)
        if len(exec_starts) != 1:
            raise TemporaryLaunchError("systemd user unit must have exactly one direct ExecStart")
        try:
            argv = shlex.split(exec_starts[0], posix=True)
        except ValueError as error:
            raise TemporaryLaunchError("systemd user ExecStart cannot be parsed losslessly") from error
        if exec_starts[0] != " ".join(argv):
            raise TemporaryLaunchError("systemd user ExecStart has unsupported quoting or shell-like syntax")
        if not argv or Path(argv[0]).expanduser().resolve() != daemon.resolve():
            raise TemporaryLaunchError("systemd user unit does not launch the selected daemon directly")
        if any(not SAFE_SYSTEMD_ARGUMENT.fullmatch(argument) for argument in argv):
            raise TemporaryLaunchError("systemd user ExecStart has unsupported quoting or shell-like syntax")
        if any(
            argument == "--peer-wire-security" or argument.startswith("--peer-wire-security=")
            for argument in argv
        ):
            raise TemporaryLaunchError("systemd user unit already selects peer-wire security")
        return argv

    def _dropin_path(self, session: TemporaryLaunchSession) -> Path:
        unit = self._unit_name(argparse.Namespace(service=session.service))
        return (self.user_unit_directory / f"{unit}.d" / f"90-atm-temporary-{session.session_id}.conf").resolve()

    @staticmethod
    def _render_dropin(argv: Sequence[str]) -> bytes:
        return ("[Service]\nExecStart=\nExecStart=" + " ".join(argv) + "\n").encode("utf-8")

    def _session_dropin(self, session: TemporaryLaunchSession) -> Path:
        dropin = self._dropin_path(session)
        if not session.overlay_reference or Path(session.overlay_reference) != dropin:
            raise TemporaryLaunchError("temporary-launch journal does not name this session's owned drop-in")
        if not dropin.is_file():
            raise TemporaryLaunchError("temporary-launch owned drop-in is missing")
        if not session.overlay_digest or self._digest(dropin.read_bytes()) != session.overlay_digest:
            raise TemporaryLaunchError("temporary-launch owned drop-in changed before start")
        return dropin

    def _remove_orphaned_expected_dropin(
        self,
        dropin: Path,
        argv: Sequence[str],
        mode: PeerWireSecurity,
    ) -> None:
        if not dropin.exists():
            return
        expected = self._render_dropin([*argv, "--peer-wire-security", mode.value])
        if not dropin.is_file() or self._digest(dropin.read_bytes()) != self._digest(expected):
            raise TemporaryLaunchError("orphaned temporary-launch drop-in is not the expected owned file")
        dropin.unlink()
        self._fsync_directory(dropin.parent)

    def _publish_owned_dropin(self, destination: Path, contents: bytes) -> None:
        self._ensure_private_directory(destination.parent)
        if destination.exists():
            raise TemporaryLaunchError("temporary-launch owned drop-in already exists")
        descriptor, temporary_name = tempfile.mkstemp(dir=destination.parent, prefix=f".{destination.name}.")
        temporary = Path(temporary_name)
        try:
            os.fchmod(descriptor, 0o600)
            with os.fdopen(descriptor, "wb") as handle:
                handle.write(contents)
                handle.flush()
                os.fsync(handle.fileno())
            os.link(temporary, destination)
            self._fsync_directory(destination.parent)
        except FileExistsError as error:
            raise TemporaryLaunchError("temporary-launch owned drop-in already exists") from error
        except OSError as error:
            raise TemporaryLaunchError("cannot create temporary-launch owned drop-in") from error
        finally:
            temporary.unlink(missing_ok=True)

    def _daemon_reload(self) -> None:
        result = self.runner(["systemctl", "--user", "daemon-reload"], 20.0)
        if result.returncode != 0:
            detail = (result.stderr or result.stdout).strip()
            raise TemporaryLaunchError(f"systemd user daemon-reload failed: {detail}")

    @staticmethod
    def _ensure_private_directory(directory: Path) -> None:
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        metadata = directory.lstat()
        if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid():
            raise TemporaryLaunchError("temporary-launch drop-in directory is not private to this user")
        os.chmod(directory, 0o700)

    @staticmethod
    def _fsync_directory(directory: Path) -> None:
        try:
            descriptor = os.open(directory, os.O_RDONLY)
        except OSError:
            return
        try:
            os.fsync(descriptor)
        except OSError:
            pass
        finally:
            os.close(descriptor)

    @staticmethod
    def _digest(contents: bytes) -> str:
        return hashlib.sha256(contents).hexdigest()
