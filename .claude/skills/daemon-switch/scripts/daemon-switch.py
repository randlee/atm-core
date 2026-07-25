#!/usr/bin/env python3
"""Safely select one system-wide ATM CLI/daemon release pair."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import sys
import tempfile
import time
from typing import Sequence


class SwitchError(RuntimeError):
    """A precondition that protects the singleton daemon was not met."""


def executable_name(name: str) -> str:
    return f"{name}.exe" if os.name == "nt" else name


def run(args: Sequence[str], *, timeout: float = 10.0) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, text=True, capture_output=True, timeout=timeout, check=False)


def version(path: Path) -> str | None:
    try:
        result = run([str(path), "--version"], timeout=5.0)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return result.stdout.strip() or result.stderr.strip() or None


def command_path(name: str, override: str | None, option: str) -> Path:
    raw = override or shutil.which(executable_name(name))
    if raw is None:
        raise SwitchError(f"cannot find {executable_name(name)} on PATH; pass {option}")
    return Path(raw).expanduser().absolute()


def require_executable(path: Path, label: str) -> Path:
    resolved = path.expanduser().resolve()
    if not resolved.is_file():
        raise SwitchError(f"{label} does not exist: {path}")
    if not os.access(resolved, os.X_OK):
        raise SwitchError(f"{label} is not executable: {resolved}")
    return resolved


def homebrew_pair() -> tuple[Path, Path] | None:
    brew = shutil.which("brew")
    if brew is None:
        return None
    result = run([brew, "--prefix", "atm"], timeout=10.0)
    if result.returncode != 0:
        return None
    prefix = Path(result.stdout.strip())
    cli = prefix / "bin" / executable_name("atm")
    daemon = prefix / "bin" / executable_name("atm-daemon")
    if cli.is_file() and daemon.is_file():
        return cli.resolve(), daemon.resolve()
    return None


def state_path() -> Path:
    if os.name == "nt":
        root = Path(os.environ.get("APPDATA", Path.home() / "AppData" / "Roaming"))
    else:
        root = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    return root / "atm" / "daemon-switch.json"


def load_state() -> dict[str, str]:
    path = state_path()
    if not path.is_file():
        return {}
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}
    return data if isinstance(data, dict) else {}


def save_default_pair(cli: Path, daemon: Path) -> None:
    path = state_path()
    data = load_state()
    data.setdefault("default_cli", str(cli))
    data.setdefault("default_daemon", str(daemon))
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def service_commands(args: argparse.Namespace, action: str) -> list[str]:
    system = platform.system()
    if not args.service:
        raise SwitchError("--service is required; never switch an unmanaged daemon")
    if system == "Darwin":
        if not args.launch_agent_plist:
            raise SwitchError("macOS requires --launch-agent-plist for controlled singleton restart")
        domain = f"gui/{os.getuid()}"
        if action == "stop":
            return ["launchctl", "bootout", f"{domain}/{args.service}"]
        plist = str(Path(args.launch_agent_plist).expanduser())
        return ["launchctl", "bootstrap", domain, plist]
    if system == "Windows":
        return ["sc", action, args.service]
    return ["systemctl", "--user", action, args.service]


def run_service(args: argparse.Namespace, action: str, *, allow_absent: bool = False) -> None:
    command = service_commands(args, action)
    if platform.system() != "Darwin":
        result = run(command, timeout=20.0)
        if result.returncode == 0 or (allow_absent and action == "stop"):
            return
        detail = (result.stderr or result.stdout).strip()
        raise SwitchError(f"service {action} failed: {' '.join(command)}: {detail}")

    domain = f"gui/{os.getuid()}"
    service = f"{domain}/{args.service}"
    if action == "stop":
        result = run(command, timeout=20.0)
        if result.returncode != 0 and not allow_absent:
            detail = (result.stderr or result.stdout).strip()
            raise SwitchError(f"service stop failed: {' '.join(command)}: {detail}")
        for _ in range(20):
            if run(["launchctl", "print", service], timeout=2.0).returncode != 0:
                return
            time.sleep(0.1)
        raise SwitchError("LaunchAgent remained loaded after controlled stop")

    last_detail = ""
    for _ in range(10):
        result = run(command, timeout=20.0)
        if result.returncode == 0 or run(["launchctl", "print", service], timeout=2.0).returncode == 0:
            return
        last_detail = (result.stderr or result.stdout).strip()
        time.sleep(0.2)
    raise SwitchError(f"service start failed: {' '.join(command)}: {last_detail}")


def replace_link(link: Path, target: Path) -> None:
    with tempfile.NamedTemporaryFile(dir=link.parent, prefix=f".{link.name}.", delete=False) as handle:
        temporary = Path(handle.name)
    temporary.unlink()
    try:
        temporary.symlink_to(target)
        os.replace(temporary, link)
    finally:
        temporary.unlink(missing_ok=True)


def selected_links(args: argparse.Namespace) -> tuple[Path, Path]:
    return (
        command_path("atm", args.cli_link, "--cli-link"),
        command_path("atm-daemon", args.daemon_link, "--daemon-link"),
    )


def validate_selectors(cli_link: Path, daemon_link: Path) -> None:
    for label, link in (("atm CLI", cli_link), ("atm daemon", daemon_link)):
        if not link.is_symlink():
            raise SwitchError(f"refusing to replace non-symlink {label} selector: {link}")


def validate_macos_launch_agent(args: argparse.Namespace, daemon_link: Path) -> None:
    """Require the managed LaunchAgent to execute the daemon selector itself.

    A plist which names a Cellar/worktree binary bypasses the pair switch and
    can leave a new CLI talking to an old daemon.  The service must execute the
    selector path so both binaries move as one controlled unit.
    """
    if platform.system() != "Darwin":
        return
    if not args.launch_agent_plist:
        raise SwitchError("macOS requires --launch-agent-plist for controlled singleton restart")
    plist_path = Path(args.launch_agent_plist).expanduser()
    try:
        with plist_path.open("rb") as handle:
            plist = plistlib.load(handle)
    except (OSError, plistlib.InvalidFileException) as error:
        raise SwitchError(f"cannot read LaunchAgent plist {plist_path}: {error}") from error
    program_arguments = plist.get("ProgramArguments") if isinstance(plist, dict) else None
    if not isinstance(program_arguments, list) or not program_arguments or not isinstance(program_arguments[0], str):
        raise SwitchError(f"LaunchAgent plist {plist_path} has no executable ProgramArguments entry")
    if Path(program_arguments[0]).expanduser().absolute() != daemon_link:
        raise SwitchError(
            "LaunchAgent daemon executable must be the daemon selector "
            f"{daemon_link}, not {program_arguments[0]}"
        )


def daemon_version_from_doctor(report: dict[str, object]) -> str | None:
    context = report.get("daemon_context")
    if not isinstance(context, dict):
        return None
    value = context.get("version")
    return value if isinstance(value, str) else None


def verify_live_pair(cli: Path, expected_cli_version: str | None) -> None:
    """Fail closed unless the selected CLI reaches a healthy matching daemon."""
    report = doctor(cli)
    if "error" in report:
        raise SwitchError(f"managed daemon failed doctor after switch: {report['error']}")
    daemon_version = daemon_version_from_doctor(report)
    if not expected_cli_version or not daemon_version:
        raise SwitchError("cannot verify selected CLI and daemon versions after switch")
    normalized_cli = expected_cli_version.removeprefix("atm ").strip()
    if daemon_version != normalized_cli:
        raise SwitchError(
            "managed daemon version does not match the selected CLI: "
            f"daemon={daemon_version}, cli={normalized_cli}"
        )


def switch_pair(args: argparse.Namespace, cli_target: Path, daemon_target: Path) -> None:
    cli_link, daemon_link = selected_links(args)
    validate_selectors(cli_link, daemon_link)
    validate_macos_launch_agent(args, daemon_link)
    old_cli = require_executable(cli_link, "selected atm CLI")
    old_daemon = require_executable(daemon_link, "selected atm daemon")
    cli_target = require_executable(cli_target, "target atm CLI")
    daemon_target = require_executable(daemon_target, "target atm daemon")
    if cli_target.parent != daemon_target.parent:
        raise SwitchError(
            "refusing targets from different release directories; build or install the matched pair together"
        )
    if args.dry_run:
        print(json.dumps({"cli_link": str(cli_link), "daemon_link": str(daemon_link), "cli_target": str(cli_target), "daemon_target": str(daemon_target)}, indent=2))
        return
    if not args.yes:
        raise SwitchError("switch changes the system-wide pair; re-run with --yes")
    save_default_pair(old_cli, old_daemon)
    run_service(args, "stop", allow_absent=True)
    try:
        if cli_link.resolve() != cli_target:
            replace_link(cli_link, cli_target)
        if daemon_link.resolve() != daemon_target:
            replace_link(daemon_link, daemon_target)
        run_service(args, "start")
        verify_live_pair(cli_link, version(cli_link))
    except Exception:
        replace_link(cli_link, old_cli)
        replace_link(daemon_link, old_daemon)
        try:
            run_service(args, "start")
            verify_live_pair(cli_link, version(cli_link))
        except SwitchError:
            pass
        raise


def restore_pair(args: argparse.Namespace) -> tuple[Path, Path]:
    brew_pair = homebrew_pair()
    if brew_pair is not None:
        return brew_pair
    if args.default_cli and args.default_daemon:
        return Path(args.default_cli), Path(args.default_daemon)
    state = load_state()
    if state.get("default_cli") and state.get("default_daemon"):
        return Path(state["default_cli"]), Path(state["default_daemon"])
    raise SwitchError("cannot discover an installed release; pass --default-cli and --default-daemon")


def restart(args: argparse.Namespace) -> None:
    if not args.yes:
        raise SwitchError("restart changes the singleton daemon; re-run with --yes")
    run_service(args, "stop", allow_absent=True)
    run_service(args, "start")


def doctor(cli: Path) -> dict[str, object]:
    try:
        result = run([str(cli), "doctor", "--json"], timeout=10.0)
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"error": str(error)}
    if result.returncode != 0:
        return {"error": (result.stderr or result.stdout).strip()}
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError:
        return {"error": "doctor returned non-JSON output"}


def status(args: argparse.Namespace) -> None:
    cli, daemon = selected_links(args)
    service = {"platform": platform.system(), "service": args.service}
    if platform.system() == "Darwin" and args.service:
        service["launch_agent_plist"] = args.launch_agent_plist
    result: dict[str, object] = {
        "atm": {"selector": str(cli), "target": str(cli.resolve()), "version": version(cli)},
        "atm_daemon": {"selector": str(daemon), "target": str(daemon.resolve())},
        "service": service,
        "homebrew_restore_available": homebrew_pair() is not None,
    }
    if args.doctor:
        result["doctor"] = doctor(cli)
    print(json.dumps(result, indent=2, sort_keys=True))


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    selectors = argparse.ArgumentParser(add_help=False)
    selectors.add_argument("--cli-link", help="system selector symlink for atm")
    selectors.add_argument("--daemon-link", help="system selector symlink for atm-daemon")
    selectors.add_argument("--service", help="LaunchAgent label or system service name")
    selectors.add_argument("--launch-agent-plist", help="macOS LaunchAgent plist used to restart the singleton")
    sub = result.add_subparsers(dest="command", required=True)
    status_parser = sub.add_parser("status", parents=[selectors])
    status_parser.add_argument("--doctor", action="store_true", help="query the live daemon through the selected CLI")
    switch = sub.add_parser("switch", parents=[selectors])
    switch.add_argument("--cli", required=True, help="branch/release atm binary")
    switch.add_argument("--daemon", required=True, help="matching branch/release atm-daemon binary")
    switch.add_argument("--yes", action="store_true")
    switch.add_argument("--dry-run", action="store_true")
    restore = sub.add_parser("restore", parents=[selectors])
    restore.add_argument("--default-cli")
    restore.add_argument("--default-daemon")
    restore.add_argument("--yes", action="store_true")
    restore.add_argument("--dry-run", action="store_true")
    restart_parser = sub.add_parser("restart", parents=[selectors])
    restart_parser.add_argument("--yes", action="store_true")
    return result


def main() -> int:
    args = parser().parse_args()
    try:
        if args.command == "status":
            status(args)
        elif args.command == "switch":
            switch_pair(args, Path(args.cli), Path(args.daemon))
        elif args.command == "restore":
            cli, daemon = restore_pair(args)
            switch_pair(args, cli, daemon)
        else:
            restart(args)
    except SwitchError as error:
        print(f"daemon-switch: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
