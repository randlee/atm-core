#!/usr/bin/env python3
"""Safely select one system-wide ATM CLI/daemon release pair."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
from typing import Protocol, Sequence


REPO_ROOT = Path(__file__).resolve().parents[4]
SCRIPTS_DIRECTORY = REPO_ROOT / "scripts"
if str(SCRIPTS_DIRECTORY) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIRECTORY))
DAEMON_SWITCH_SCRIPTS_DIRECTORY = Path(__file__).resolve().parent
if str(DAEMON_SWITCH_SCRIPTS_DIRECTORY) not in sys.path:
    sys.path.insert(0, str(DAEMON_SWITCH_SCRIPTS_DIRECTORY))

from macos_development_signing import (  # noqa: E402
    CLI_IDENTIFIER,
    DAEMON_IDENTIFIER,
    SigningIdentityError,
    resolve_apple_development_identity,
    verify_apple_signature,
)
from temporary_launch import (  # noqa: E402
    CapturedLaunchSpec,
    OverlayLaunchSpec,
    PeerWireSecurity,
    TemporaryLaunchError,
    TemporaryLaunchJournal,
    TemporaryLaunchPhase,
    TemporaryLaunchSession,
    account_identifier,
    sha256_file,
)
from temporary_launch_macos import MacosLaunchAgentAdapter  # noqa: E402
from temporary_launch_windows import (  # noqa: E402
    WindowsScmAdapter,
    parse_windows_command_line,
    quote_windows_command_line,
)
from temporary_launch_linux import LinuxSystemdUserAdapter  # noqa: E402


WINDOWS_SERVICE_NOT_FOUND = 1060
LIVE_PAIR_READINESS_ATTEMPTS = 200
MACOS_LAUNCH_AGENT_PATH = re.compile(r"^path = (.+)$")
# [cass: helpful starter-rust-logging] - retains the bounded readiness state
# as one named operational contract rather than an unexplained retry literal.
"""Bounded 20-second readiness window for a managed replacement daemon.

The daemon owns durable storage and may need more than five seconds to
complete startup on a real host.  Retrying this bounded probe is safer than
rolling selectors back while the new daemon is still becoming ready.
"""


class SwitchError(RuntimeError):
    """A precondition that protects the singleton daemon was not met."""


class TemporaryLaunchAdapter(Protocol):
    """Platform-owned service configuration seam for a typed overlay session."""

    def capture(
        self,
        args: argparse.Namespace,
        cli: Path,
        daemon: Path,
        mode: PeerWireSecurity,
    ) -> CapturedLaunchSpec:
        """Read/validate original service configuration without mutating it."""

    def apply_overlay(
        self,
        args: argparse.Namespace,
        session: TemporaryLaunchSession,
    ) -> OverlayLaunchSpec:
        """Apply one owned typed overlay after the singleton has stopped."""

    def activate_overlay(
        self,
        args: argparse.Namespace,
        session: TemporaryLaunchSession,
    ) -> None:
        """Mutate the native manager only after overlay recovery state is durable."""

    def start_args(
        self,
        args: argparse.Namespace,
        session: TemporaryLaunchSession,
    ) -> argparse.Namespace:
        """Return the controlled service selector for the active overlay."""

    def restore_exact(self, args: argparse.Namespace, session: TemporaryLaunchSession) -> None:
        """Restore only the captured original service configuration."""


def temporary_launch_journal() -> TemporaryLaunchJournal:
    """Keep an active overlay journal separate from ordinary default-pair state."""
    return TemporaryLaunchJournal(state_path().with_name("temporary-launch.json"))


def temporary_launch_adapter(_args: argparse.Namespace) -> TemporaryLaunchAdapter:
    """Resolve one reviewed native adapter; never provide a process fallback."""
    if platform.system() == "Darwin":
        return MacosLaunchAgentAdapter(
            temporary_launch_journal().path.parent / "temporary-launch-overlays"
        )
    if platform.system() == "Windows":
        return WindowsScmAdapter(lambda command, timeout: run(command, timeout=timeout))
    if platform.system() == "Linux":
        return LinuxSystemdUserAdapter(
            systemd_user_config_directory(),
            lambda command, timeout: run(command, timeout=timeout),
        )
    raise SwitchError(
        "temporary-launch requires a reviewed platform adapter; no direct-process fallback is available"
    )


def parse_peer_wire_security(value: str) -> PeerWireSecurity:
    """Make the CLI accept only the public typed launch spellings."""
    try:
        return PeerWireSecurity.parse(value)
    except TemporaryLaunchError as error:
        raise argparse.ArgumentTypeError(str(error)) from error


def require_no_active_temporary_launch_session() -> None:
    try:
        temporary_launch_journal().require_no_active_session()
    except TemporaryLaunchError as error:
        raise SwitchError(str(error)) from error


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


def macos_development_signing_identity_available() -> bool:
    """Return whether the required Apple Development identity is usable."""
    if platform.system() != "Darwin":
        return False
    try:
        resolve_apple_development_identity()
    except (OSError, subprocess.SubprocessError, SigningIdentityError):
        return False
    return True


def macos_binary_has_development_signature(
    binary: Path, identifier: str, team_identifier: str
) -> bool:
    """Prove one managed binary carries its stable Apple Development signature."""
    try:
        return verify_apple_signature(str(binary), identifier, team_identifier)
    except (OSError, subprocess.SubprocessError):
        return False


def require_macos_development_signatures(cli: Path, daemon: Path) -> None:
    """Fail closed before any managed-pair lifecycle mutation on macOS."""
    system = platform.system()
    if system == "Windows":
        print("warning: Windows signing not yet implemented; skipping ATM signature gate.", file=sys.stderr)
        return
    if system != "Darwin":
        return
    try:
        identity = resolve_apple_development_identity()
    except (OSError, subprocess.SubprocessError, SigningIdentityError) as error:
        raise SwitchError(f"Apple Development signing preflight failed: {error}") from error
    for label, binary, identifier in (
        ("CLI", cli, CLI_IDENTIFIER),
        ("daemon", daemon, DAEMON_IDENTIFIER),
    ):
        if not macos_binary_has_development_signature(binary, identifier, identity.team_identifier):
            raise SwitchError(
                f"{label} target is not strictly signed by the required Apple Development identity: "
                f"{binary}. "
                "Build with `just build` or run `python3 .just/sign_daemon_dev.py` before daemon-switch."
            )


def homebrew_release_metadata() -> dict[str, object]:
    """Return the installed ATM formula metadata used for release provenance."""
    brew = shutil.which("brew")
    if brew is None:
        raise SwitchError("cannot verify Homebrew release provenance: brew is not installed")
    try:
        result = run([brew, "info", "--json=v2", "--installed", "atm"], timeout=10.0)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise SwitchError(f"cannot verify Homebrew release provenance: {error}") from error
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise SwitchError(f"cannot verify Homebrew release provenance: {detail or 'brew info failed'}")
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise SwitchError("cannot verify Homebrew release provenance: brew info returned invalid JSON") from error
    formulae = payload.get("formulae") if isinstance(payload, dict) else None
    if not isinstance(formulae, list) or len(formulae) != 1 or not isinstance(formulae[0], dict):
        raise SwitchError("cannot verify Homebrew release provenance: ATM formula metadata is missing")
    return formulae[0]


def require_homebrew_release_provenance(cli: Path, daemon: Path) -> None:
    """Accept an ad-hoc Homebrew pair only when Homebrew proves its release origin.

    Homebrew verifies the release archive checksum before installing it.  The
    formula metadata is therefore the provenance boundary for the extracted
    binaries: it must identify this project, the same release version in both
    installed and stable metadata, the v-tagged GitHub Release asset, and a
    valid SHA-256 checksum.  This gate is intentionally restore-only; source
    switches continue to require the Apple Development identity.
    """
    if platform.system() != "Darwin":
        return
    cli = require_executable(cli, "Homebrew atm CLI")
    daemon = require_executable(daemon, "Homebrew atm daemon")
    brew = shutil.which("brew")
    if brew is None:
        raise SwitchError("cannot verify Homebrew release provenance: brew is not installed")
    try:
        prefix_result = run([brew, "--prefix", "atm"], timeout=10.0)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise SwitchError(f"cannot verify Homebrew release provenance: {error}") from error
    if prefix_result.returncode != 0:
        raise SwitchError("cannot verify Homebrew release provenance: brew prefix failed")
    try:
        prefix = Path(prefix_result.stdout.strip()).expanduser().resolve()
    except (OSError, RuntimeError) as error:
        raise SwitchError(f"cannot verify Homebrew release provenance: invalid formula prefix: {error}") from error
    expected_bin_dir = (prefix / "bin").resolve()
    for label, binary in (("CLI", cli), ("daemon", daemon)):
        try:
            binary.relative_to(expected_bin_dir)
        except ValueError as error:
            raise SwitchError(
                f"{label} target is not the installed Homebrew ATM binary under {expected_bin_dir}: {binary}"
            ) from error

    formula = homebrew_release_metadata()
    if formula.get("homepage") != "https://github.com/randlee/atm-core":
        raise SwitchError("Homebrew ATM formula has an unexpected project homepage")
    cli_version = selected_release_version(cli)
    daemon_version = selected_release_version(daemon)
    if cli_version != daemon_version:
        raise SwitchError(
            f"Homebrew ATM pair has mismatched versions: CLI {cli_version}, daemon {daemon_version}"
        )
    versions = formula.get("versions")
    installed = formula.get("installed")
    if not isinstance(versions, dict) or versions.get("stable") != cli_version:
        raise SwitchError("Homebrew ATM formula stable version does not match the selected binaries")
    if (
        not isinstance(installed, list)
        or len(installed) != 1
        or not isinstance(installed[0], dict)
        or installed[0].get("version") != cli_version
    ):
        raise SwitchError("Homebrew ATM installed version does not match the selected binaries")
    urls = formula.get("urls")
    stable = urls.get("stable") if isinstance(urls, dict) else None
    release_url = stable.get("url") if isinstance(stable, dict) else None
    checksum = stable.get("checksum") if isinstance(stable, dict) else None
    expected_prefix = f"https://github.com/randlee/atm-core/releases/download/v{cli_version}/"
    if not isinstance(release_url, str) or not release_url.startswith(expected_prefix):
        raise SwitchError("Homebrew ATM formula does not point at the matching GitHub Release asset")
    if not isinstance(checksum, str) or re.fullmatch(r"[0-9a-fA-F]{64}", checksum) is None:
        raise SwitchError("Homebrew ATM formula has no valid SHA-256 release asset checksum")


def require_macos_restore_provenance(cli: Path, daemon: Path) -> None:
    """Validate restore targets as either a verified Homebrew release or dev pair."""
    if platform.system() != "Darwin":
        return
    brew_pair = homebrew_pair()
    try:
        selected = (cli.expanduser().resolve(), daemon.expanduser().resolve())
    except (OSError, RuntimeError) as error:
        raise SwitchError(f"cannot resolve restore targets: {error}") from error
    if brew_pair is not None and selected == brew_pair:
        require_homebrew_release_provenance(*brew_pair)
        return
    # A non-Homebrew restore remains a development restore and keeps the
    # original strict signing policy. Only the managed Homebrew release gets
    # the ad-hoc-signature exception.
    require_macos_development_signatures(cli, daemon)


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


def systemd_user_config_directory() -> Path:
    """Return the current account's only user-service configuration root."""
    root = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    return root / "systemd" / "user"


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
        return ["sc.exe", action, args.service]
    return ["systemctl", "--user", action, args.service]


def windows_service_missing(result: subprocess.CompletedProcess[str]) -> bool:
    """Recognize only SCM's missing-service result for an optional stop."""
    if result.returncode == WINDOWS_SERVICE_NOT_FOUND:
        return True
    output = f"{result.stdout}\n{result.stderr}".lower()
    return "1060" in output and "openservice" in output


def run_service(args: argparse.Namespace, action: str, *, allow_absent: bool = False) -> None:
    command = service_commands(args, action)
    if platform.system() != "Darwin":
        result = run(command, timeout=20.0)
        if result.returncode == 0 or (
            allow_absent
            and action == "stop"
            and (
                platform.system() != "Windows"
                or windows_service_missing(result)
            )
        ):
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
            # defeat the normal polling window. The HTTP runtime may not own
            # the legacy UDS pathname, so identify the singleton through its
            # lock as well as through the UDS before a verified repair.
            repair_macos_orphan(macos_daemon_owner_pids())
            for _ in range(20):
                if run(["launchctl", "print", service], timeout=2.0).returncode != 0:
                    return
                time.sleep(0.1)
        raise SwitchError("LaunchAgent remained loaded after controlled stop")

    expected_plist = Path(args.launch_agent_plist).expanduser().resolve()
    last_detail = ""
    for _ in range(10):
        result = run(command, timeout=20.0)
        loaded_plist = macos_loaded_launch_agent_plist(service)
        if loaded_plist == expected_plist:
            return
        if loaded_plist is not None:
            raise SwitchError(
                f"service start retained {loaded_plist} instead of requested {expected_plist}"
            )
        last_detail = (result.stderr or result.stdout).strip()
        time.sleep(0.2)
    raise SwitchError(f"service start failed: {' '.join(command)}: {last_detail}")


def macos_loaded_launch_agent_plist(service: str) -> Path | None:
    """Return the exact plist launchd has loaded for one user LaunchAgent."""
    result = run(["launchctl", "print", service], timeout=2.0)
    if result.returncode != 0:
        return None
    for line in result.stdout.splitlines():
        match = MACOS_LAUNCH_AGENT_PATH.match(line.strip())
        if match is not None:
            return Path(match.group(1)).expanduser().resolve()
    return None


def macos_path_owner_pids(path: Path) -> list[int]:
    """Return PIDs that hold one existing, user-owned daemon-state path."""
    if not path.exists():
        return []
    lsof = shutil.which("lsof") or "/usr/sbin/lsof"
    try:
        result = run([lsof, "-t", str(path)], timeout=5.0)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise SwitchError(f"cannot inspect ATM daemon owner at {path}: {error}") from error
    return [int(line) for line in result.stdout.splitlines() if line.strip().isdigit()]


def macos_socket_owner_pids() -> list[int]:
    return macos_path_owner_pids(macos_socket_path())


def macos_owner_lock_path() -> Path:
    return Path.home() / ".atm" / "daemon" / "owner.lock"


def macos_daemon_owner_pids() -> list[int]:
    """Find one live daemon through either its legacy socket or singleton lock.

    The Tokio HTTP runtime does not publish the former local UDS socket, but
    it always holds the same OS-user-owned singleton lock.  Both paths are
    required so controlled replacement cannot start beside a live runtime.
    """
    return sorted({
        *macos_socket_owner_pids(),
        *macos_path_owner_pids(macos_owner_lock_path()),
    })


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
    if macos_socket_owner_pids():
        raise SwitchError("refusing to remove a daemon socket that still has an owner")
    metadata = socket_path.lstat()
    if metadata.st_uid != os.getuid():
        raise SwitchError(f"refusing to remove ATM socket not owned by this user: {socket_path}")
    socket_path.unlink()
    return not socket_path.exists()


def wait_for_macos_socket_release(pid: int, expected_socket: tuple[int, int] | None) -> None:
    """Wait for a SIGTERM'd daemon to release its singleton resources."""
    socket_path = macos_socket_path()
    for _ in range(50):
        try:
            os.kill(pid, 0)
            process_exists = True
        except ProcessLookupError:
            process_exists = False
        if not process_exists and not macos_daemon_owner_pids() and not socket_path.exists():
            return
        time.sleep(0.1)

    # A previous daemon can leave its pathname behind after it has closed the
    # listener.  The earlier owner proof authorizes cleanup only when the path
    # is still the exact socket inode that owner held before SIGTERM.  A path
    # replacement (or any non-socket) fails closed instead of being deleted.
    if not macos_daemon_owner_pids() and remove_verified_stale_macos_socket(expected_socket):
        return
    raise SwitchError(
        f"verified stale ATM daemon pid {pid} did not fully release {socket_path} after SIGTERM"
    )


def repair_macos_orphan(pids: list[int]) -> None:
    """Terminate only a verified stale daemon after its LaunchAgent is unloaded."""
    if len(pids) != 1:
        raise SwitchError(
            "managed stop left an ATM daemon owner, but it is not exactly one repairable daemon PID"
        )
    pid = pids[0]
    command = run(["ps", "-p", str(pid), "-o", "command="], timeout=5.0).stdout.strip()
    if "atm-daemon" not in command:
        raise SwitchError(f"refusing to terminate non-ATM daemon owner pid {pid}: {command}")
    expected_socket = socket_identity(macos_socket_path())
    os.kill(pid, signal.SIGTERM)
    wait_for_macos_socket_release(pid, expected_socket)


def require_stopped_daemon(args: argparse.Namespace, _cli: Path) -> None:
    if platform.system() != "Darwin":
        return
    pids = macos_daemon_owner_pids()
    if not pids:
        # A controlled stop can complete while an older daemon implementation
        # leaves its now-unowned UDS pathname behind. The next process must not
        # bind over it, so remove only the current user's verified socket.
        remove_verified_stale_macos_socket(None)
        return
    if not args.repair_orphan:
        raise SwitchError(
            "controlled service stop left an ATM daemon owner; refuse a split pair. "
            "On macOS, rerun with --repair-orphan only after verifying the service label/plist."
        )
    if pids:
        repair_macos_orphan(pids)
    if macos_daemon_owner_pids():
        raise SwitchError("ATM daemon remains owned after explicit orphan repair")
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


def selected_matched_pair(args: argparse.Namespace) -> tuple[Path, Path]:
    """Validate the selected pair before an overlay can stop its service."""
    cli_link, daemon_link = selected_links(args)
    cli = require_executable(cli_link, "selected atm CLI")
    daemon = require_executable(daemon_link, "selected atm daemon")
    if cli.parent != daemon.parent:
        raise SwitchError(
            "refusing a temporary launch for CLI/daemon selectors from different release directories"
        )
    require_macos_development_signatures(cli, daemon)
    return cli, daemon


def require_active_session_pair(
    args: argparse.Namespace,
    session: TemporaryLaunchSession,
) -> tuple[Path, Path]:
    """Refuse a resumed session if its selected release/service changed."""
    cli, daemon = selected_matched_pair(args)
    if not args.service or args.service != session.service:
        raise SwitchError("temporary-launch service does not match the active recovery journal")
    if platform.system() != session.platform:
        raise SwitchError("temporary-launch platform does not match the active recovery journal")
    if account_identifier() != session.account_id:
        raise SwitchError("temporary-launch account does not match the active recovery journal")
    if str(cli) != session.cli_path or sha256_file(cli) != session.cli_digest:
        raise SwitchError("temporary-launch selected CLI does not match the active recovery journal")
    if str(daemon) != session.daemon_path or sha256_file(daemon) != session.daemon_digest:
        raise SwitchError("temporary-launch selected daemon does not match the active recovery journal")
    return cli, daemon


def require_temporary_session(args: argparse.Namespace) -> tuple[TemporaryLaunchJournal, TemporaryLaunchSession]:
    """Load one caller-selected active session and prove its release identity."""
    try:
        journal = temporary_launch_journal()
        session = journal.require_session(args.session)
    except TemporaryLaunchError as error:
        raise SwitchError(str(error)) from error
    require_active_session_pair(args, session)
    return journal, session


def save_temporary_session(journal: TemporaryLaunchJournal, session: TemporaryLaunchSession) -> None:
    """Convert journal integrity errors into the daemon-switch public error type."""
    try:
        journal.save(session)
    except TemporaryLaunchError as error:
        raise SwitchError(str(error)) from error


def create_temporary_session(journal: TemporaryLaunchJournal, session: TemporaryLaunchSession) -> None:
    """Create the first journal with exclusive publication before a service stop."""
    try:
        journal.create(session)
    except TemporaryLaunchError as error:
        raise SwitchError(str(error)) from error


def transition_temporary_session(
    journal: TemporaryLaunchJournal,
    session: TemporaryLaunchSession,
    phase: TemporaryLaunchPhase,
) -> TemporaryLaunchSession:
    """Persist the next intent before its corresponding external mutation."""
    try:
        next_session = session.transition(phase)
    except TemporaryLaunchError as error:
        raise SwitchError(str(error)) from error
    save_temporary_session(journal, next_session)
    return next_session


def activate_temporary_overlay(
    adapter: TemporaryLaunchAdapter,
    args: argparse.Namespace,
    session: TemporaryLaunchSession,
) -> None:
    """Run a platform manager mutation only after its overlay is journaled."""
    try:
        adapter.activate_overlay(args, session)
    except TemporaryLaunchError as error:
        raise SwitchError(str(error)) from error


def doctor_peer_wire_security(payload: dict[str, object]) -> str | None:
    """Read only the daemon's typed public diagnostic wire-mode projection."""
    daemon_context = payload.get("daemon_context")
    if not isinstance(daemon_context, dict):
        return None
    value = daemon_context.get("peer_wire_security")
    return value if isinstance(value, str) else None


def wait_for_temporary_launch(
    cli: Path,
    daemon: Path,
    mode: PeerWireSecurity,
) -> tuple[bool, str]:
    """Require both the existing matched-pair proof and selected wire-mode proof."""
    detail = "daemon did not report the requested temporary peer-wire mode"
    for _ in range(LIVE_PAIR_READINESS_ATTEMPTS):
        matched, detail = live_pair_matches(cli, daemon)
        payload = doctor(cli)
        if matched and doctor_peer_wire_security(payload) == mode.value:
            return True, f"{detail}; peer-wire security={mode.value}"
        if matched:
            observed = doctor_peer_wire_security(payload) or "<missing>"
            detail = f"matched pair reports peer-wire security={observed}, expected {mode.value}"
        time.sleep(0.1)
    return False, detail


def begin_temporary_launch(args: argparse.Namespace) -> None:
    """Run the generic journal-first overlay transition through one adapter."""
    if not args.yes:
        raise SwitchError("temporary-launch begin changes the singleton daemon; re-run with --yes")
    require_no_active_temporary_launch_session()
    if not args.service:
        raise SwitchError("--service is required for a temporary managed-service launch")
    cli, daemon = selected_matched_pair(args)
    adapter = temporary_launch_adapter(args)
    try:
        captured = adapter.capture(args, cli, daemon, args.peer_wire_security)
    except TemporaryLaunchError as error:
        raise SwitchError(str(error)) from error
    session = TemporaryLaunchSession.captured(
        peer_wire_security=args.peer_wire_security,
        platform=platform.system(),
        account_id=account_identifier(),
        service=args.service,
        cli_path=cli,
        cli_digest=sha256_file(cli),
        daemon_path=daemon,
        daemon_digest=sha256_file(daemon),
        launch_spec=captured,
    )
    journal = temporary_launch_journal()
    create_temporary_session(journal, session)
    run_service(args, "stop", allow_absent=True)
    require_stopped_daemon(args, cli)
    session = transition_temporary_session(journal, session, TemporaryLaunchPhase.STOPPED)
    try:
        session = session.with_overlay(adapter.apply_overlay(args, session))
    except TemporaryLaunchError as error:
        raise SwitchError(str(error)) from error
    save_temporary_session(journal, session)
    activate_temporary_overlay(adapter, args, session)
    session = transition_temporary_session(journal, session, TemporaryLaunchPhase.OVERLAY_STARTED)
    try:
        overlay_args = adapter.start_args(args, session)
    except TemporaryLaunchError as error:
        raise SwitchError(str(error)) from error
    run_service(overlay_args, "start")
    matched, detail = wait_for_temporary_launch(cli, daemon, session.peer_wire_security)
    if not matched:
        raise SwitchError(f"temporary overlay start did not pass doctor proof: {detail}")
    print(json.dumps(temporary_launch_evidence(session, "overlay_started"), indent=2, sort_keys=True))


def quiesce_temporary_launch(args: argparse.Namespace) -> None:
    """Stop only the known active overlay daemon without changing selectors."""
    if not args.yes:
        raise SwitchError("temporary-launch quiesce changes the singleton daemon; re-run with --yes")
    journal, session = require_temporary_session(args)
    if session.phase is not TemporaryLaunchPhase.OVERLAY_STARTED:
        raise SwitchError("temporary-launch quiesce requires an overlay-started session")
    cli, _daemon = selected_matched_pair(args)
    run_service(args, "stop", allow_absent=True)
    require_stopped_daemon(args, cli)
    session = transition_temporary_session(journal, session, TemporaryLaunchPhase.QUIESCED)
    print(json.dumps(temporary_launch_evidence(session, "quiesced"), indent=2, sort_keys=True))


def restart_temporary_launch(args: argparse.Namespace) -> None:
    """Restart a known quiesced overlay using the same selected pair and mode."""
    if not args.yes:
        raise SwitchError("temporary-launch restart changes the singleton daemon; re-run with --yes")
    journal, session = require_temporary_session(args)
    if session.phase is not TemporaryLaunchPhase.QUIESCED:
        raise SwitchError("temporary-launch restart requires a quiesced session")
    cli, daemon = selected_matched_pair(args)
    adapter = temporary_launch_adapter(args)
    session = transition_temporary_session(journal, session, TemporaryLaunchPhase.OVERLAY_STARTED)
    try:
        overlay_args = adapter.start_args(args, session)
    except TemporaryLaunchError as error:
        raise SwitchError(str(error)) from error
    run_service(overlay_args, "start")
    matched, detail = wait_for_temporary_launch(cli, daemon, session.peer_wire_security)
    if not matched:
        raise SwitchError(f"temporary overlay restart did not pass doctor proof: {detail}")
    print(json.dumps(temporary_launch_evidence(session, "overlay_restarted"), indent=2, sort_keys=True))


def restore_temporary_launch(args: argparse.Namespace, *, recovery: bool) -> None:
    """Restore exactly one captured service specification through its adapter."""
    if not args.yes:
        raise SwitchError("temporary-launch restore changes the singleton daemon; re-run with --yes")
    journal, session = require_temporary_session(args)
    if session.phase is TemporaryLaunchPhase.COMPLETED:
        raise SwitchError("temporary-launch session is already completed")
    cli, daemon = selected_matched_pair(args)
    adapter = temporary_launch_adapter(args)
    # RESTORING is the durable intent written before any restoration mutation.
    # A crash after that write must resume from the same intent, not attempt an
    # illegal second transition and strand the managed service.
    if session.phase is not TemporaryLaunchPhase.RESTORING:
        session = transition_temporary_session(journal, session, TemporaryLaunchPhase.RESTORING)
    run_service(args, "stop", allow_absent=True)
    require_stopped_daemon(args, cli)
    try:
        adapter.restore_exact(args, session)
    except TemporaryLaunchError as error:
        raise SwitchError(str(error)) from error
    run_service(args, "start")
    matched, detail = wait_for_temporary_launch(cli, daemon, PeerWireSecurity.MUTUAL_TLS)
    if not matched:
        action = "recovery" if recovery else "restore"
        raise SwitchError(f"temporary overlay {action} did not pass normal-mode doctor proof: {detail}")
    session = transition_temporary_session(journal, session, TemporaryLaunchPhase.COMPLETED)
    try:
        journal.remove_after_completion(session)
    except TemporaryLaunchError as error:
        raise SwitchError(str(error)) from error
    print(json.dumps(temporary_launch_evidence(session, "recovered" if recovery else "restored"), indent=2, sort_keys=True))


def temporary_launch_evidence(session: TemporaryLaunchSession, outcome: str) -> dict[str, object]:
    """Expose redacted session state; references remain owner-only journal material."""
    return {
        "session_id": session.session_id,
        "outcome": outcome,
        "phase": session.phase.value,
        "platform": session.platform,
        "service": session.service,
        "peer_wire_security": session.peer_wire_security.value,
        "cli": {"path": session.cli_path, "sha256": session.cli_digest},
        "daemon": {"path": session.daemon_path, "sha256": session.daemon_digest},
        "original_launch_digest": session.original_digest,
        "overlay_launch_digest": session.overlay_digest,
    }


def switch_pair(
    args: argparse.Namespace,
    cli_target: Path,
    daemon_target: Path,
    *,
    require_development_signature: bool = True,
) -> None:
    require_no_active_temporary_launch_session()
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
    if require_development_signature:
        require_macos_development_signatures(cli_target, daemon_target)
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
    candidate_started = False
    try:
        replace_link(cli_link, cli_target)
        replace_link(daemon_link, daemon_target)
        run_service(args, "start")
        candidate_started = True
        matched, detail = wait_for_live_pair(cli_target, daemon_target)
        if not matched:
            raise SwitchError(f"refusing a split CLI/daemon pair: {detail}")
    except Exception:
        # Do not repoint selectors until the candidate service has stopped.
        # A readiness timeout can occur while the candidate is still running;
        # restoring the old links first would create exactly the split pair
        # this command promises never to leave behind.
        if candidate_started:
            try:
                run_service(args, "stop", allow_absent=True)
                require_stopped_daemon(args, cli_target)
            except SwitchError as recovery_error:
                raise SwitchError(
                    "candidate daemon could not be stopped after failed switch; "
                    "selectors remain on the candidate to avoid a split pair"
                ) from recovery_error
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
    require_no_active_temporary_launch_session()
    cli, daemon = selected_links(args)
    cli = require_executable(cli, "selected atm CLI")
    daemon = require_executable(daemon, "selected atm daemon")
    require_macos_development_signatures(cli, daemon)
    run_service(args, "stop", allow_absent=True)
    require_stopped_daemon(args, cli)
    run_service(args, "start")
    matched, detail = wait_for_live_pair(cli, daemon)
    if not matched:
        raise SwitchError(f"refusing a split CLI/daemon pair after restart: {detail}")


def quiesce(args: argparse.Namespace) -> None:
    """Stop the one managed daemon without changing either selected binary."""
    if not args.yes:
        raise SwitchError("quiesce changes the singleton daemon; re-run with --yes")
    cli, _daemon = selected_links(args)
    # Benchmark recovery may find that the verified LaunchAgent label is
    # already unloaded (launchctl reports "No such process").  Treat that as
    # an absent service, then let require_stopped_daemon perform the actual
    # ownership check and the explicitly authorized orphan repair if needed.
    # Failing before that check makes a safe no-op quiesce impossible.
    run_service(args, "stop", allow_absent=True)
    require_stopped_daemon(args, cli)


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


def macos_daemon_executable(pid: int) -> Path | None:
    """Return the executable image held by one verified live daemon process."""
    lsof = shutil.which("lsof") or "/usr/sbin/lsof"
    try:
        result = run([lsof, "-p", str(pid)], timeout=5.0)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise SwitchError(f"cannot inspect ATM daemon executable for pid {pid}: {error}") from error
    for line in result.stdout.splitlines():
        fields = line.split(maxsplit=8)
        if len(fields) == 9 and fields[3] == "txt":
            candidate = Path(fields[8])
            if candidate.name == executable_name("atm-daemon"):
                return candidate.resolve()
    return None


def macos_live_daemon_matches(daemon: Path) -> tuple[bool, str]:
    """Prove the singleton lock holder is the selected daemon executable."""
    pids = macos_daemon_owner_pids()
    if len(pids) != 1:
        return False, f"expected one ATM daemon owner, found {len(pids)}"
    actual = macos_daemon_executable(pids[0])
    expected = daemon.resolve()
    if actual != expected:
        return False, f"selected daemon {expected}, live daemon {actual or '<unresolved>'}"
    return True, f"live daemon pid {pids[0]} matches {expected}"


def live_pair_matches(cli: Path, daemon: Path | None = None) -> tuple[bool, str]:
    """Prove the running daemon changed together with both selectors."""
    expected = selected_release_version(cli)
    payload = doctor(cli)
    if "error" in payload:
        return False, f"live daemon is unavailable after switch: {payload['error']}"
    client = context_version(payload, "client_context")
    daemon_context = context_version(payload, "daemon_context")
    if client != expected:
        return False, (
            f"selected {expected}, but doctor reports client={client or '<missing>'}"
        )
    if daemon_context == expected:
        return True, f"CLI and daemon both report {expected}"
    if daemon_context is not None:
        return False, f"selected {expected}, but doctor reports daemon={daemon_context}"
    if platform.system() != "Darwin" or daemon is None:
        return False, f"selected {expected}, but doctor reports daemon=<missing>"
    summary = payload.get("summary")
    if not isinstance(summary, dict) or summary.get("status") != "healthy":
        return False, "daemon doctor is not healthy enough for executable identity fallback"
    return macos_live_daemon_matches(daemon)


def wait_for_live_pair(cli: Path, daemon: Path | None = None) -> tuple[bool, str]:
    """Allow the one managed daemon a bounded interval to become doctor-ready."""
    detail = "daemon did not report ready"
    for _ in range(LIVE_PAIR_READINESS_ATTEMPTS):
        matched, detail = live_pair_matches(cli, daemon)
        if matched:
            return True, detail
        time.sleep(0.1)
    return False, detail


def windows_service_status(service: str) -> dict[str, object]:
    """Expose SCM state so status cannot imply an absent service is managed."""
    result = run(["sc.exe", "query", service], timeout=5.0)
    output = (result.stdout or "") + (result.stderr or "")
    if result.returncode != 0:
        return {
            "installed": False,
            "state": "absent" if windows_service_missing(result) else "unknown",
            "detail": output.strip() or f"sc.exe exited with {result.returncode}",
        }

    state = "unknown"
    for line in output.splitlines():
        if "STATE" not in line or ":" not in line:
            continue
        state = line.split(":", 1)[1].strip().split(maxsplit=1)[-1].lower()
        break
    return {"installed": True, "state": state}


def status(args: argparse.Namespace) -> None:
    cli, daemon = selected_links(args)
    service = {"platform": platform.system(), "service": args.service}
    if platform.system() == "Darwin" and args.service:
        service["launch_agent_plist"] = args.launch_agent_plist
    if platform.system() == "Windows" and args.service:
        service["windows"] = windows_service_status(args.service)
    result: dict[str, object] = {
        "atm": {"selector": str(cli), "target": str(cli.resolve()), "version": version(cli)},
        "atm_daemon": {"selector": str(daemon), "target": str(daemon.resolve())},
        "service": service,
        "homebrew_restore_available": homebrew_pair() is not None,
    }
    if args.doctor:
        result["doctor"] = doctor(cli)
        try:
            matched, detail = live_pair_matches(cli, daemon)
        except SwitchError as error:
            matched, detail = False, str(error)
        result["live_pair"] = {"matched": matched, "detail": detail}
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
        help="macOS only: SIGTERM one verified stale ATM daemon owner after controlled service stop",
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
    quiesce_parser = sub.add_parser("quiesce", parents=[selectors])
    quiesce_parser.add_argument("--yes", action="store_true")
    temporary = sub.add_parser("temporary-launch", parents=[selectors])
    temporary_sub = temporary.add_subparsers(dest="temporary_command", required=True)
    temporary_begin = temporary_sub.add_parser("begin")
    temporary_begin.add_argument(
        "--peer-wire-security",
        required=True,
        type=parse_peer_wire_security,
        metavar="{mutual-tls,plaintext-test}",
    )
    temporary_begin.add_argument("--yes", action="store_true")
    for name in ("quiesce", "restart", "restore", "recover"):
        command = temporary_sub.add_parser(name)
        command.add_argument("--session", required=True)
        command.add_argument("--yes", action="store_true")
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
            require_macos_restore_provenance(cli, daemon)
            switch_pair(args, cli, daemon, require_development_signature=False)
        elif args.command == "restart":
            restart(args)
        elif args.command == "temporary-launch":
            if args.temporary_command == "begin":
                begin_temporary_launch(args)
            elif args.temporary_command == "quiesce":
                quiesce_temporary_launch(args)
            elif args.temporary_command == "restart":
                restart_temporary_launch(args)
            else:
                restore_temporary_launch(args, recovery=args.temporary_command == "recover")
        else:
            quiesce(args)
    except SwitchError as error:
        print(f"daemon-switch: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
