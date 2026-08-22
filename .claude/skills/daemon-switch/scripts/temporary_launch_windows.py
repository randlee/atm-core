"""Narrow Windows SCM `binPath` overlay for one typed temporary session."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import re
import subprocess
from typing import Callable, Sequence

from temporary_launch import (
    CapturedLaunchSpec,
    OverlayLaunchSpec,
    PeerWireSecurity,
    TemporaryLaunchError,
    TemporaryLaunchSession,
)


SCM_BINARY_PATH = re.compile(r"^\s*BINARY_PATH_NAME\s*:\s*(.*)$", re.MULTILINE)
ScmRunner = Callable[[Sequence[str], float], subprocess.CompletedProcess[str]]


class WindowsScmAdapter:
    """Capture and restore only an exact, validated SCM binary-path scalar."""

    def __init__(self, runner: ScmRunner) -> None:
        self.runner = runner

    def capture(
        self,
        args: argparse.Namespace,
        _cli: Path,
        daemon: Path,
        _mode: PeerWireSecurity,
    ) -> CapturedLaunchSpec:
        """Read an unambiguous service command without changing SCM state."""
        raw = self._current_binary_path(args)
        self._validated_argv(raw, daemon)
        return CapturedLaunchSpec(raw, self._digest(raw))

    def apply_overlay(
        self,
        args: argparse.Namespace,
        session: TemporaryLaunchSession,
    ) -> OverlayLaunchSpec:
        """Derive the exact typed `binPath` before its recovery record is saved."""
        raw = self._current_binary_path(args)
        if raw != session.original_reference or self._digest(raw) != session.original_digest:
            raise TemporaryLaunchError("temporary-launch SCM binary path changed before overlay creation")
        argv = self._validated_argv(raw, Path(session.daemon_path))
        overlay = quote_windows_command_line(
            [*argv, "--peer-wire-security", session.peer_wire_security.value]
        )
        return OverlayLaunchSpec(overlay, self._digest(overlay))

    def activate_overlay(self, args: argparse.Namespace, session: TemporaryLaunchSession) -> None:
        """Install only the journaled overlay, then verify SCM reports it exactly."""
        if not session.overlay_reference or not session.overlay_digest:
            raise TemporaryLaunchError("temporary-launch journal lacks a Windows overlay binary path")
        raw = self._current_binary_path(args)
        if raw != session.original_reference or self._digest(raw) != session.original_digest:
            raise TemporaryLaunchError("temporary-launch SCM binary path changed before overlay activation")
        self._configure_binary_path(args, session.overlay_reference)
        installed = self._current_binary_path(args)
        if installed != session.overlay_reference or self._digest(installed) != session.overlay_digest:
            raise TemporaryLaunchError("temporary-launch SCM did not retain the owned overlay binary path")

    def start_args(
        self,
        args: argparse.Namespace,
        session: TemporaryLaunchSession,
    ) -> argparse.Namespace:
        """SCM retains the journaled overlay; no alternate process arguments exist."""
        if not session.overlay_reference or not session.overlay_digest:
            raise TemporaryLaunchError("temporary-launch journal lacks a Windows overlay binary path")
        return args

    def restore_exact(self, args: argparse.Namespace, session: TemporaryLaunchSession) -> None:
        """Restore the captured scalar, including safe re-entry after a prior config write."""
        if not session.overlay_reference or not session.overlay_digest:
            raise TemporaryLaunchError("temporary-launch journal lacks a Windows overlay binary path")
        current = self._current_binary_path(args)
        if current == session.original_reference and self._digest(current) == session.original_digest:
            return
        if current != session.overlay_reference or self._digest(current) != session.overlay_digest:
            raise TemporaryLaunchError("temporary-launch SCM binary path changed; refusing restore")
        self._configure_binary_path(args, session.original_reference)
        restored = self._current_binary_path(args)
        if restored != session.original_reference or self._digest(restored) != session.original_digest:
            raise TemporaryLaunchError("temporary-launch SCM did not restore the captured binary path")

    def _current_binary_path(self, args: argparse.Namespace) -> str:
        service = getattr(args, "service", None)
        if not service:
            raise TemporaryLaunchError("Windows temporary-launch requires --service")
        result = self.runner(["sc.exe", "qc", service], 20.0)
        if result.returncode != 0:
            detail = (result.stderr or result.stdout).strip()
            raise TemporaryLaunchError(f"cannot inspect Windows service {service}: {detail}")
        match = SCM_BINARY_PATH.search(result.stdout)
        if match is None or not match.group(1):
            raise TemporaryLaunchError("Windows service does not expose one BINARY_PATH_NAME")
        return match.group(1)

    def _configure_binary_path(self, args: argparse.Namespace, value: str) -> None:
        result = self.runner(["sc.exe", "config", args.service, "binPath=", value], 20.0)
        if result.returncode != 0:
            detail = (result.stderr or result.stdout).strip()
            raise TemporaryLaunchError(f"cannot configure Windows service {args.service}: {detail}")

    @staticmethod
    def _validated_argv(raw: str, daemon: Path) -> list[str]:
        argv = parse_windows_command_line(raw)
        if not argv or Path(argv[0]).expanduser().resolve() != daemon.resolve():
            raise TemporaryLaunchError("Windows service does not launch the selected daemon directly")
        if any(
            argument == "--peer-wire-security" or argument.startswith("--peer-wire-security=")
            for argument in argv
        ):
            raise TemporaryLaunchError("Windows service already selects peer-wire security")
        return argv

    @staticmethod
    def _digest(value: str) -> str:
        return hashlib.sha256(value.encode("utf-8")).hexdigest()


def parse_windows_command_line(command: str) -> list[str]:
    """Parse the narrow CommandLineToArgvW-compatible subset SCM is allowed to use."""
    arguments: list[str] = []
    index = 0
    length = len(command)
    while index < length:
        while index < length and command[index] in " \t":
            index += 1
        if index == length:
            break
        argument: list[str] = []
        quoted = False
        while index < length:
            if command[index] in " \t" and not quoted:
                break
            slashes = 0
            while index < length and command[index] == "\\":
                slashes += 1
                index += 1
            if index < length and command[index] == '"':
                argument.extend("\\" * (slashes // 2))
                if slashes % 2:
                    argument.append('"')
                else:
                    quoted = not quoted
                index += 1
                continue
            argument.extend("\\" * slashes)
            if index == length or (command[index] in " \t" and not quoted):
                break
            argument.append(command[index])
            index += 1
        if quoted:
            raise TemporaryLaunchError("Windows service command has unmatched quotation marks")
        arguments.append("".join(argument))
        while index < length and command[index] in " \t":
            index += 1
    return arguments


def quote_windows_command_line(arguments: Sequence[str]) -> str:
    """Render argv losslessly for the paired parser, including Windows quote rules."""
    return " ".join(quote_windows_argument(argument) for argument in arguments)


def quote_windows_argument(argument: str) -> str:
    """Quote one argv item using the CommandLineToArgvW backslash convention."""
    if not argument or any(character in " \t\"" for character in argument):
        rendered = ['"']
        slashes = 0
        for character in argument:
            if character == "\\":
                slashes += 1
            elif character == '"':
                rendered.append("\\" * (slashes * 2 + 1))
                rendered.append('"')
                slashes = 0
            else:
                rendered.append("\\" * slashes)
                rendered.append(character)
                slashes = 0
        rendered.append("\\" * (slashes * 2))
        rendered.append('"')
        return "".join(rendered)
    return argument
