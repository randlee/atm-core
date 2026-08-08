#!/usr/bin/env python3
"""Safely select one system-wide ATM CLI/daemon release pair."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
from typing import Sequence


class SwitchError(RuntimeError):
    """A precondition that protects the singleton daemon was not met."""


def executable_name(name: str) -> str:
    return f"{name}.exe" if os.name == "nt" else name


def run(
    args: Sequence[str], *, timeout: float = 10.0, cwd: Path | None = None
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        text=True,
        capture_output=True,
        timeout=timeout,
        check=False,
        cwd=cwd,
    )


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
        if args.repair_orphan:
            # `bootout` has already prevented a replacement process. A
            # blocked daemon can still keep the job loaded long enough to
            # defeat the normal polling window, so repair the one verified
            # socket owner before declaring the singleton unrecoverable.
            repair_macos_orphan(macos_socket_owner_pids())
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


def macos_socket_owner_pids() -> list[int]:
    socket_path = macos_socket_path()
    result = run(["lsof", "-t", str(socket_path)], timeout=5.0)
    return [int(line) for line in result.stdout.splitlines() if line.strip().isdigit()]


def macos_socket_path() -> Path:
    return Path.home() / ".atm" / "daemon" / "atm-daemon.sock"


def socket_identity(path: Path) -> tuple[int, int] | None:
    """Return a Unix-socket inode identity, refusing non-socket paths."""
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return None
    if not stat.S_ISSOCK(metadata.st_mode):
        raise SwitchError(f"refusing to remove non-socket ATM path: {path}")
    return metadata.st_dev, metadata.st_ino


def remove_verified_stale_macos_socket(expected_socket: tuple[int, int] | None) -> bool:
    """Remove the selected daemon's unowned socket only after strict identity checks."""
    socket_path = macos_socket_path()
    current_socket = socket_identity(socket_path)
    if current_socket is None:
        return True
    if expected_socket is not None and current_socket != expected_socket:
        raise SwitchError(f"refusing to remove replaced ATM socket path: {socket_path}")
    metadata = socket_path.lstat()
    if metadata.st_uid != os.getuid():
        raise SwitchError(f"refusing to remove ATM socket not owned by this user: {socket_path}")
    socket_path.unlink()
    return not socket_path.exists()


def wait_for_macos_socket_release(pid: int, expected_socket: tuple[int, int] | None) -> None:
    """Wait for a SIGTERM'd daemon to release its UDS, then remove only its stale inode."""
    socket_path = macos_socket_path()
    for _ in range(50):
        try:
            os.kill(pid, 0)
            process_exists = True
        except ProcessLookupError:
            process_exists = False
        if not process_exists and not macos_socket_owner_pids() and not socket_path.exists():
            return
        time.sleep(0.1)

    # A previous daemon can leave its pathname behind after it has closed the
    # listener.  The earlier owner proof authorizes cleanup only when the path
    # is still the exact socket inode that owner held before SIGTERM.  A path
    # replacement (or any non-socket) fails closed instead of being deleted.
    if not macos_socket_owner_pids() and remove_verified_stale_macos_socket(expected_socket):
        return
    raise SwitchError(
        f"verified stale ATM daemon pid {pid} did not fully release {socket_path} after SIGTERM"
    )


def repair_macos_orphan(pids: list[int]) -> None:
    """Terminate only a verified stale daemon after its LaunchAgent is unloaded."""
    if len(pids) != 1:
        raise SwitchError(
            "managed stop left an ATM socket owner, but it is not exactly one repairable daemon PID"
        )
    pid = pids[0]
    command = run(["ps", "-p", str(pid), "-o", "command="], timeout=5.0).stdout.strip()
    if "atm-daemon" not in command:
        raise SwitchError(f"refusing to terminate non-ATM socket owner pid {pid}: {command}")
    expected_socket = socket_identity(macos_socket_path())
    os.kill(pid, signal.SIGTERM)
    wait_for_macos_socket_release(pid, expected_socket)


def require_stopped_daemon(args: argparse.Namespace, _cli: Path) -> None:
    if platform.system() != "Darwin":
        return
    pids = macos_socket_owner_pids()
    if not pids:
        # A controlled stop can complete while an older daemon implementation
        # leaves its now-unowned UDS pathname behind. The next process must not
        # bind over it, so remove only the current user's verified socket.
        remove_verified_stale_macos_socket(None)
        return
    if not args.repair_orphan:
        raise SwitchError(
            "controlled service stop left an ATM socket owner; refuse a split pair. "
            "On macOS, rerun with --repair-orphan only after verifying the service label/plist."
        )
    repair_macos_orphan(pids)
    if macos_socket_owner_pids():
        raise SwitchError("ATM daemon socket remains owned after explicit orphan repair")
    remove_verified_stale_macos_socket(None)


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


def switch_pair(args: argparse.Namespace, cli_target: Path, daemon_target: Path) -> None:
    cli_link, daemon_link = selected_links(args)
    validate_selectors(cli_link, daemon_link)
    old_pair: tuple[Path, Path] | None
    try:
        old_pair = (
            require_executable(cli_link, "selected atm CLI"),
            require_executable(daemon_link, "selected atm daemon"),
        )
    except SwitchError:
        if not args.repair_orphan:
            raise SwitchError(
                "selected ATM pair is missing or dangling; refuse to repair selectors without "
                "--repair-orphan after verifying the managed service"
            ) from None
        old_pair = None
    cli_target = require_executable(cli_target, "target atm CLI")
    daemon_target = require_executable(daemon_target, "target atm daemon")
    if cli_target.parent != daemon_target.parent:
        raise SwitchError(
            "refusing targets from different release directories; build or install the matched pair together"
        )
    if cli_link.resolve() == cli_target and daemon_link.resolve() == daemon_target:
        print("already selected; service left running")
        return
    if args.dry_run:
        print(json.dumps({"cli_link": str(cli_link), "daemon_link": str(daemon_link), "cli_target": str(cli_target), "daemon_target": str(daemon_target)}, indent=2))
        return
    if not args.yes:
        raise SwitchError("switch changes the system-wide pair; re-run with --yes")
    if old_pair is not None:
        save_default_pair(*old_pair)
    run_service(args, "stop", allow_absent=True)
    require_stopped_daemon(args, old_pair[0] if old_pair is not None else cli_link)
    try:
        replace_link(cli_link, cli_target)
        replace_link(daemon_link, daemon_target)
        run_service(args, "start")
        matched, detail = wait_for_live_pair(cli_target)
        if not matched:
            raise SwitchError(f"refusing a split CLI/daemon pair: {detail}")
    except Exception:
        if old_pair is not None:
            replace_link(cli_link, old_pair[0])
            replace_link(daemon_link, old_pair[1])
            try:
                run_service(args, "start")
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
    cli, _daemon = selected_links(args)
    run_service(args, "stop", allow_absent=True)
    require_stopped_daemon(args, cli)
    run_service(args, "start")
    matched, detail = wait_for_live_pair(cli)
    if not matched:
        raise SwitchError(f"refusing a split CLI/daemon pair after restart: {detail}")


def doctor(cli: Path) -> dict[str, object]:
    try:
        # The managed daemon must not be forced to traverse a caller's source
        # worktree merely to validate the selected service pair. In particular,
        # macOS privacy controls can hold a launch-agent request at that file
        # boundary. Pair validation has no workspace-config dependency.
        result = run([str(cli), "doctor", "--json"], timeout=10.0, cwd=Path.home())
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"error": str(error)}
    if result.returncode != 0:
        return {"error": (result.stderr or result.stdout).strip()}
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError:
        return {"error": "doctor returned non-JSON output"}


def context_version(payload: object, context: str) -> str | None:
    if not isinstance(payload, dict):
        return None
    value = payload.get(context)
    if not isinstance(value, dict):
        return None
    version_value = value.get("version")
    return version_value if isinstance(version_value, str) else None


def selected_release_version(cli: Path) -> str:
    value = version(cli)
    if not value:
        raise SwitchError(f"cannot determine selected ATM CLI version: {cli}")
    return value.rsplit(maxsplit=1)[-1]


def live_pair_matches(cli: Path) -> tuple[bool, str]:
    """Prove the running daemon changed together with both selectors."""
    expected = selected_release_version(cli)
    payload = doctor(cli)
    if "error" in payload:
        return False, f"live daemon is unavailable after switch: {payload['error']}"
    client = context_version(payload, "client_context")
    daemon = context_version(payload, "daemon_context")
    if client != expected or daemon != expected:
        return False, (
            f"selected {expected}, but doctor reports client={client or '<missing>'} "
            f"daemon={daemon or '<missing>'}"
        )
    return True, f"CLI and daemon both report {expected}"


def wait_for_live_pair(cli: Path) -> tuple[bool, str]:
    """Allow the one managed daemon a bounded interval to become doctor-ready."""
    detail = "daemon did not report ready"
    for _ in range(50):
        matched, detail = live_pair_matches(cli)
        if matched:
            return True, detail
        time.sleep(0.1)
    return False, detail


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
    selectors.add_argument(
        "--repair-orphan",
        action="store_true",
        help="macOS only: SIGTERM one verified stale ATM socket owner after controlled service stop",
    )
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
