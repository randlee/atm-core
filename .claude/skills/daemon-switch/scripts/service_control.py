"""Platform service-manager operations for daemon-switch.

Selectors and release resolution stay outside this module.  The caller supplies
the small selector/orphan seams so tests can prove lifecycle ordering without a
live service manager.
"""

from __future__ import annotations

import argparse
import csv
import io
import os
from pathlib import Path
import platform
import plistlib
import re
import subprocess
import time
from typing import Callable
from xml.etree import ElementTree

from release_resolution import SwitchError, executable_name, require_executable, run


def systemd_user_config_directory() -> Path:
    root = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    return root / "systemd" / "user"


def _normalized_executable(value: str | Path) -> str:
    return os.path.normcase(os.path.abspath(os.path.expandvars(os.path.expanduser(str(value)))))


def _is_owned_file(path: Path) -> bool:
    try:
        return path.is_file() and (os.name == "nt" or path.stat().st_uid == os.getuid())
    except OSError:
        return False


def _unique_service(candidates: list[tuple[str, Path | None]], daemon: Path) -> tuple[str, Path | None]:
    if not candidates:
        raise SwitchError(
            f"no current-user managed service launches the ATM daemon selector {daemon}; "
            "provision the native singleton service or pass its explicit selector"
        )
    if len(candidates) != 1:
        names = ", ".join(sorted(name for name, _path in candidates))
        raise SwitchError(
            f"multiple current-user managed services launch the ATM daemon selector {daemon}: {names}; "
            "pass --service explicitly"
        )
    return candidates[0]


def _macos_candidates(
    daemon: Path,
    loaded_launch_agent_plist: Callable[[str], Path | None],
    launch_agents_root: Path | None = None,
) -> list[tuple[str, Path | None]]:
    candidates: list[tuple[str, Path | None]] = []
    root = launch_agents_root or Path.home() / "Library" / "LaunchAgents"
    for plist in sorted(root.glob("*.plist")):
        if not _is_owned_file(plist):
            continue
        try:
            payload = plistlib.loads(plist.read_bytes())
        except (OSError, plistlib.InvalidFileException):
            continue
        label = payload.get("Label")
        arguments = payload.get("ProgramArguments")
        executable = arguments[0] if isinstance(arguments, list) and arguments else payload.get("Program")
        if not isinstance(label, str) or not isinstance(executable, str):
            continue
        loaded = loaded_launch_agent_plist(f"gui/{os.getuid()}/{label}")
        if _normalized_executable(executable) == _normalized_executable(daemon) and loaded == plist.resolve():
            candidates.append((label, plist.resolve()))
    return candidates


def _systemd_property(unit: str, name: str) -> str | None:
    result = run(
        ["systemctl", "--user", "show", unit, f"--property={name}", "--value"],
        timeout=5.0,
    )
    return result.stdout.strip() if result.returncode == 0 else None


def _linux_candidates(daemon: Path) -> list[tuple[str, Path | None]]:
    listed = run(
        ["systemctl", "--user", "list-unit-files", "--type=service", "--no-legend", "--no-pager"],
        timeout=10.0,
    )
    if listed.returncode != 0:
        raise SwitchError(f"cannot enumerate current-user systemd services: {(listed.stderr or listed.stdout).strip()}")
    candidates: list[tuple[str, Path | None]] = []
    for line in listed.stdout.splitlines():
        unit = line.split(maxsplit=1)[0] if line.split() else ""
        if not unit.endswith(".service"):
            continue
        fragment_value = _systemd_property(unit, "FragmentPath")
        execution = _systemd_property(unit, "ExecStart")
        fragment = Path(fragment_value).expanduser() if fragment_value else None
        match = re.search(r"(?:^|[ {;])path=([^ ;}]+)", execution or "")
        if fragment is None or not _is_owned_file(fragment) or match is None:
            continue
        if _normalized_executable(match.group(1)) == _normalized_executable(daemon):
            candidates.append((unit, None))
    return candidates


def _windows_task_names() -> list[str]:
    result = run(["schtasks.exe", "/Query", "/FO", "CSV", "/NH"], timeout=10.0)
    if result.returncode != 0:
        raise SwitchError(f"cannot enumerate current-user scheduled tasks: {(result.stderr or result.stdout).strip()}")
    return [row[0] for row in csv.reader(io.StringIO(result.stdout)) if row]


def _windows_current_identities() -> set[str]:
    domain, username = os.environ.get("USERDOMAIN", ""), os.environ.get("USERNAME", "")
    identities = {value.casefold() for value in (username, f"{domain}\\{username}") if value.strip("\\")}
    result = run(["whoami.exe", "/user", "/fo", "csv", "/nh"], timeout=5.0)
    if result.returncode == 0:
        for row in csv.reader(io.StringIO(result.stdout)):
            identities.update(value.casefold() for value in row[:2] if value)
    if not identities:
        raise SwitchError("cannot identify the current Windows account for managed-service discovery")
    return identities


def _windows_candidates(
    daemon: Path,
    task_status: Callable[[str], dict[str, object]],
    current_identities: set[str] | None = None,
) -> list[tuple[str, Path | None]]:
    identities = current_identities or _windows_current_identities()
    candidates: list[tuple[str, Path | None]] = []
    for name in _windows_task_names():
        task = task_status(name)
        command, owner = task.get("command"), task.get("user_id")
        if (
            task.get("registered")
            and isinstance(command, str)
            and isinstance(owner, str)
            and owner.casefold() in identities
            and _normalized_executable(command) == _normalized_executable(daemon)
        ):
            candidates.append((name, None))
    return candidates


def resolve_managed_service(
    args: argparse.Namespace,
    selected_links: Callable[[argparse.Namespace], tuple[Path, Path]],
    loaded_launch_agent_plist: Callable[[str], Path | None],
    task_status: Callable[[str], dict[str, object]],
) -> None:
    """Resolve an explicitly requested, uniquely owned managed-service selector."""
    if not getattr(args, "discover_managed_service", False):
        return
    if args.service or args.launch_agent_plist:
        raise SwitchError(
            "--discover-managed-service cannot be combined with --service or --launch-agent-plist"
        )
    daemon = selected_links(args)[1]
    system = platform.system()
    if system == "Darwin":
        candidates = _macos_candidates(daemon, loaded_launch_agent_plist)
    elif system == "Linux":
        candidates = _linux_candidates(daemon)
    elif system == "Windows":
        candidates = _windows_candidates(daemon, task_status)
    else:
        raise SwitchError(f"managed-service discovery is unsupported on {system}")
    args.service, plist = _unique_service(candidates, daemon)
    args.launch_agent_plist = str(plist) if plist is not None else None


def service_commands(args: argparse.Namespace, action: str) -> list[str]:
    system = platform.system()
    if not args.service:
        raise SwitchError("--service is required; never switch an unmanaged daemon")
    if system == "Darwin":
        if not args.launch_agent_plist:
            raise SwitchError("macOS requires --launch-agent-plist for controlled singleton restart")
        domain = f"gui/{os.getuid()}"
        return ["launchctl", "bootout", f"{domain}/{args.service}"] if action == "stop" else ["launchctl", "bootstrap", domain, str(Path(args.launch_agent_plist).expanduser())]
    if system == "Windows":
        return ["schtasks.exe", "/End" if action == "stop" else "/Run", "/TN", args.service]
    return ["systemctl", "--user", action, args.service]


def windows_task_missing(result: subprocess.CompletedProcess[str]) -> bool:
    output = f"{result.stdout}\n{result.stderr}".lower()
    return "cannot find the file specified" in output or "does not exist" in output


def windows_task_status(task: str) -> dict[str, object]:
    result = run(["schtasks.exe", "/Query", "/TN", task, "/XML"], timeout=5.0)
    output = (result.stdout or "") + (result.stderr or "")
    if result.returncode != 0:
        return {"registered": False, "state": "absent" if windows_task_missing(result) else "unknown", "detail": output.strip() or f"schtasks.exe exited with {result.returncode}"}
    try:
        root = ElementTree.fromstring(result.stdout)
    except ElementTree.ParseError:
        return {"registered": True, "state": "unknown", "detail": "task XML was invalid"}
    commands = [command.text.strip() for command in root.findall(".//{*}Actions/{*}Exec/{*}Command") if command.text and command.text.strip()]
    if len(commands) != 1:
        return {"registered": True, "state": "unknown", "detail": "task must define exactly one executable action"}
    state_result = run(["schtasks.exe", "/Query", "/TN", task, "/FO", "LIST", "/V"], timeout=5.0)
    state = "unknown"
    if state_result.returncode == 0:
        for line in ((state_result.stdout or "") + (state_result.stderr or "")).splitlines():
            if line.lower().startswith("status:"):
                state = line.split(":", 1)[1].strip().lower()
                break
    owners = [
        user.text.strip()
        for user in root.findall(".//{*}Principals/{*}Principal/{*}UserId")
        if user.text and user.text.strip()
    ]
    status: dict[str, object] = {
        "registered": True,
        "state": state,
        "command": commands[0],
    }
    if len(owners) == 1:
        status["user_id"] = owners[0]
    return status


def require_windows_task_selector(args: argparse.Namespace, selected_links: Callable[[argparse.Namespace], tuple[Path, Path]]) -> None:
    assert args.service is not None
    task = windows_task_status(args.service)
    if not task.get("registered"):
        raise SwitchError(f"Windows scheduled task {args.service!r} is not registered: {task.get('detail', 'task is absent')}")
    expected, command = str(selected_links(args)[1]), task.get("command")
    if command != expected:
        raise SwitchError(f"Windows scheduled task {args.service!r} launches {command!r}, not the daemon selector {expected!r}")


def provision_windows_task(args: argparse.Namespace, selected_links: Callable[[argparse.Namespace], tuple[Path, Path]]) -> None:
    if platform.system() != "Windows":
        raise SwitchError("windows-provision is only available on Windows")
    if not args.yes:
        raise SwitchError("windows-provision registers a logon task; re-run with --yes")
    if not args.service:
        raise SwitchError("--service is required; never create an unnamed daemon task")
    _cli, daemon = selected_links(args)
    require_executable(daemon, "selected atm daemon")
    account = os.environ.get("USERDOMAIN", "") + "\\" + os.environ.get("USERNAME", "")
    if account == "\\":
        raise SwitchError("cannot identify the current Windows account for the daemon task")
    command = ["schtasks.exe", "/Create", "/TN", args.service, "/TR", str(daemon), "/SC", "ONLOGON", "/RU", account, "/IT", "/RL", "LIMITED", "/F"]
    result = run(command, timeout=20.0)
    if result.returncode != 0:
        raise SwitchError(f"Windows task provisioning failed: {' '.join(command)}: {(result.stderr or result.stdout).strip()}")
    require_windows_task_selector(args, selected_links)


def systemd_unit_missing(detail: str) -> bool:
    lowered = detail.lower()
    return "not loaded" in lowered or "not found" in lowered


def run_service(
    args: argparse.Namespace,
    action: str,
    *,
    allow_absent: bool,
    selected_links: Callable[[argparse.Namespace], tuple[Path, Path]],
    macos_loaded_launch_agent_plist: Callable[[str], Path | None],
    macos_daemon_owner_pids: Callable[[], list[int]],
    repair_macos_orphan: Callable[[list[int]], None],
    windows_task_status_fn: Callable[[str], dict[str, object]],
) -> None:
    if platform.system() == "Windows":
        if not args.service:
            raise SwitchError("--service is required; never switch an unmanaged daemon")
        task = windows_task_status_fn(args.service)
        if not task.get("registered"):
            if action == "stop" and allow_absent and task.get("state") == "absent":
                return
            raise SwitchError(f"Windows scheduled task {args.service!r} is not registered: {task.get('detail', 'task is absent')}")
        if action == "stop" and task.get("state") in {"ready", "disabled"}:
            return
        if action == "start":
            task_command = task.get("command")
            expected = str(selected_links(args)[1])
            if task_command != expected:
                raise SwitchError(
                    f"Windows scheduled task {args.service!r} launches {task_command!r}, "
                    f"not the daemon selector {expected!r}"
                )
        command = service_commands(args, action)
        result = run(command, timeout=20.0)
        if result.returncode == 0 or (action == "stop" and "not currently running" in (result.stderr or result.stdout).lower()):
            return
        raise SwitchError(f"Windows task {action} failed: {' '.join(command)}: {(result.stderr or result.stdout).strip()}")
    command = service_commands(args, action)
    if platform.system() != "Darwin":
        result = run(command, timeout=20.0)
        if result.returncode == 0 or (allow_absent and action == "stop" and systemd_unit_missing((result.stderr or result.stdout).strip())):
            return
        raise SwitchError(f"service {action} failed: {' '.join(command)}: {(result.stderr or result.stdout).strip()}")
    domain, service = f"gui/{os.getuid()}", f"gui/{os.getuid()}/{args.service}"
    if action == "stop":
        result = run(command, timeout=20.0)
        if result.returncode != 0 and not allow_absent:
            raise SwitchError(f"service stop failed: {' '.join(command)}: {(result.stderr or result.stdout).strip()}")
        for _ in range(20):
            if run(["launchctl", "print", service], timeout=2.0).returncode != 0:
                return
            time.sleep(0.1)
        if args.repair_orphan:
            repair_macos_orphan(macos_daemon_owner_pids())
            for _ in range(20):
                if run(["launchctl", "print", service], timeout=2.0).returncode != 0:
                    return
                time.sleep(0.1)
        raise SwitchError("LaunchAgent remained loaded after controlled stop")
    expected_plist, last_detail = Path(args.launch_agent_plist).expanduser().resolve(), ""
    for _ in range(10):
        result = run(command, timeout=20.0)
        loaded_plist = macos_loaded_launch_agent_plist(service)
        if loaded_plist == expected_plist:
            return
        if loaded_plist is not None:
            raise SwitchError(f"service start retained {loaded_plist} instead of requested {expected_plist}")
        last_detail = (result.stderr or result.stdout).strip()
        time.sleep(0.2)
    raise SwitchError(f"service start failed: {' '.join(command)}: {last_detail}")
