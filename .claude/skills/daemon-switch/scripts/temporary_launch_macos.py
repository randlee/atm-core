"""Narrow current-user LaunchAgent overlay for one typed temporary session."""

from __future__ import annotations

import argparse
import copy
import hashlib
import os
from pathlib import Path
import plistlib
import stat
import tempfile

from temporary_launch import (
    CapturedLaunchSpec,
    OverlayLaunchSpec,
    PeerWireSecurity,
    TemporaryLaunchError,
    TemporaryLaunchSession,
)


class MacosLaunchAgentAdapter:
    """Preserve a validated source plist while owning only a private overlay."""

    def __init__(self, overlay_directory: Path) -> None:
        self.overlay_directory = overlay_directory

    def capture(
        self,
        args: argparse.Namespace,
        _cli: Path,
        daemon: Path,
        _mode: PeerWireSecurity,
    ) -> CapturedLaunchSpec:
        """Read one source LaunchAgent without changing its bytes or launch state."""
        source, contents, _plist = self._validated_source(args, daemon)
        return CapturedLaunchSpec(str(source), self._digest(contents))

    def apply_overlay(
        self,
        args: argparse.Namespace,
        session: TemporaryLaunchSession,
    ) -> OverlayLaunchSpec:
        """Atomically publish a session-owned plist with exactly one typed option."""
        source, contents, plist = self._validated_source(args, Path(session.daemon_path))
        if str(source) != session.original_reference or self._digest(contents) != session.original_digest:
            raise TemporaryLaunchError("temporary-launch source LaunchAgent changed before overlay creation")
        arguments = list(plist["ProgramArguments"])
        arguments.extend(("--peer-wire-security", session.peer_wire_security.value))
        overlay = copy.deepcopy(plist)
        overlay["ProgramArguments"] = arguments
        encoded = plistlib.dumps(overlay, fmt=plistlib.FMT_BINARY, sort_keys=False)
        destination = self._overlay_path(session)
        self._publish_owned_overlay(destination, encoded)
        return OverlayLaunchSpec(str(destination), self._digest(encoded))

    def start_args(
        self,
        args: argparse.Namespace,
        session: TemporaryLaunchSession,
    ) -> argparse.Namespace:
        """Select only the session's verified private overlay for bootstrap."""
        overlay = self._session_overlay(session)
        if self._digest(overlay.read_bytes()) != session.overlay_digest:
            raise TemporaryLaunchError("temporary-launch owned overlay digest changed before start")
        overlay_args = vars(args).copy()
        overlay_args["launch_agent_plist"] = str(overlay)
        return argparse.Namespace(**overlay_args)

    def restore_exact(self, args: argparse.Namespace, session: TemporaryLaunchSession) -> None:
        """Verify the untouched source and remove only this session's exact overlay."""
        source, contents, _plist = self._validated_source(args, Path(session.daemon_path))
        if str(source) != session.original_reference or self._digest(contents) != session.original_digest:
            raise TemporaryLaunchError("temporary-launch source LaunchAgent changed; refusing restore")
        overlay = self._overlay_path(session)
        if session.overlay_reference is None:
            self._remove_orphaned_expected_overlay(overlay, contents, session.peer_wire_security)
            return
        if Path(session.overlay_reference) != overlay or not session.overlay_digest:
            raise TemporaryLaunchError("temporary-launch journal does not name this session's owned overlay")
        if not overlay.is_file() or self._digest(overlay.read_bytes()) != session.overlay_digest:
            raise TemporaryLaunchError("temporary-launch owned overlay changed or is missing; refusing restore")
        overlay.unlink()
        self._fsync_directory(overlay.parent)

    def _validated_source(
        self,
        args: argparse.Namespace,
        daemon: Path,
    ) -> tuple[Path, bytes, dict[str, object]]:
        raw_path = getattr(args, "launch_agent_plist", None)
        if not raw_path:
            raise TemporaryLaunchError("macOS temporary-launch requires --launch-agent-plist")
        source = Path(raw_path).expanduser().resolve()
        try:
            metadata = source.lstat()
            contents = source.read_bytes()
        except OSError as error:
            raise TemporaryLaunchError(f"cannot read LaunchAgent plist: {source}") from error
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != os.getuid():
            raise TemporaryLaunchError("temporary-launch source plist must be a current-user regular file")
        try:
            plist = plistlib.loads(contents)
        except (plistlib.InvalidFileException, ValueError) as error:
            raise TemporaryLaunchError("temporary-launch source plist is malformed") from error
        if not isinstance(plist, dict) or plist.get("Label") != getattr(args, "service", None):
            raise TemporaryLaunchError("temporary-launch source plist label does not match --service")
        program_arguments = plist.get("ProgramArguments")
        if not isinstance(program_arguments, list) or not program_arguments or not all(
            isinstance(argument, str) for argument in program_arguments
        ):
            raise TemporaryLaunchError("temporary-launch source plist needs one string ProgramArguments array")
        if Path(program_arguments[0]).expanduser().resolve() != daemon.resolve():
            raise TemporaryLaunchError("temporary-launch source plist does not launch the selected daemon")
        if any(
            argument == "--peer-wire-security" or argument.startswith("--peer-wire-security=")
            for argument in program_arguments
        ):
            raise TemporaryLaunchError("temporary-launch source plist already selects peer-wire security")
        return source, contents, plist

    def _overlay_path(self, session: TemporaryLaunchSession) -> Path:
        return self.overlay_directory / f"{session.session_id}.plist"

    def _session_overlay(self, session: TemporaryLaunchSession) -> Path:
        overlay = self._overlay_path(session)
        if not session.overlay_reference or Path(session.overlay_reference) != overlay:
            raise TemporaryLaunchError("temporary-launch journal does not name this session's owned overlay")
        if not overlay.is_file():
            raise TemporaryLaunchError("temporary-launch owned overlay is missing")
        return overlay

    def _remove_orphaned_expected_overlay(
        self,
        overlay: Path,
        source_contents: bytes,
        mode: PeerWireSecurity,
    ) -> None:
        """Clean an overlay published just before a crash, but only if reconstructable."""
        if not overlay.exists():
            return
        try:
            source = plistlib.loads(source_contents)
            assert isinstance(source, dict)
            arguments = list(source["ProgramArguments"])
            arguments.extend(("--peer-wire-security", mode.value))
            source["ProgramArguments"] = arguments
            expected = plistlib.dumps(source, fmt=plistlib.FMT_BINARY, sort_keys=False)
        except (AssertionError, KeyError, plistlib.InvalidFileException, ValueError) as error:
            raise TemporaryLaunchError("cannot verify an orphaned temporary-launch overlay") from error
        if not overlay.is_file() or self._digest(overlay.read_bytes()) != self._digest(expected):
            raise TemporaryLaunchError("orphaned temporary-launch overlay is not the expected owned file")
        overlay.unlink()
        self._fsync_directory(overlay.parent)

    def _publish_owned_overlay(self, destination: Path, contents: bytes) -> None:
        self._ensure_private_directory(destination.parent)
        if destination.exists():
            raise TemporaryLaunchError("temporary-launch owned overlay already exists")
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
            raise TemporaryLaunchError("temporary-launch owned overlay already exists") from error
        except OSError as error:
            raise TemporaryLaunchError("cannot create temporary-launch owned overlay") from error
        finally:
            temporary.unlink(missing_ok=True)

    @staticmethod
    def _ensure_private_directory(directory: Path) -> None:
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        metadata = directory.lstat()
        if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid():
            raise TemporaryLaunchError("temporary-launch overlay directory is not private to this user")
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
