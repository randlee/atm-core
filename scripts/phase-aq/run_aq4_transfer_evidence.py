#!/usr/bin/env python3
"""Run the AQ4 cross-host file-transfer live evidence scenario.

Drives the real, committed `scripts/transfer/sftp.sh` example (unmodified,
exactly as an operator would install it) through a real `atm send --attach`
invocation against a real loopback `sshd` this script starts on a scratch
port, proving end to end that a file attached on the sending side lands
under the receiving side's `$ATM_TEMP` staging convention
(`send_to_staging_dir`) via nothing but genuine SSH/SCP I/O -- no daemon
transport, no envelope change (ADR-055 decisions (c)/(d)). On Windows this
drives `scripts/transfer/sftp.ps1` instead (via `pwsh`), matching how
`crates/atm-core/src/transfer_script.rs` resolves a `.ps1` transfer script
there.

Mirrors `scripts/phase-aq/run_aq25_queue_delivery_trigger_evidence.py`'s
shape: one real, owned `atm-daemon` (refuses to start if an ambient one
already owns this OS account's singleton runtime lock, exactly like AQ1.9's
`run_hermes_atm_restart_matrix.py`), a scratch `$HOME`/`$ATM_HOME` for ATM's
own state, and a self-diagnosing JSON + Markdown transcript.

Why the recipient host is the literal string ``localhost``, not
``127.0.0.1``: `atm send --host <h>` only skips the (heavier, trusted-peer)
authority-lookup path for the exact case-insensitive literal `localhost`
(`crates/atm/src/commands/send.rs::is_legacy_direct_host`) -- every other
host string, including a dotted IP literal, requires a registered
`TrustedPeer`. Attachment-locality classification
(`classify_recipient_locality`) still correctly resolves `localhost` as
*remote* (it does not special-case the string; only an unset host or a host
equal to the sender's configured `local_host` is same-host), so this one
label gets both properties this scenario needs: no peer/mTLS setup required
for message delivery, and the real transfer-script path exercised for the
attachment.

Why this script never touches the real `$HOME/.ssh/config` (QA-2 B6): an
earlier revision backed up and overwrote the OS account's real
`~/.ssh/config`, because `invoke_transfer_script`
(`crates/atm/src/commands/send_to.rs`) calls `Command::env_clear()` before
inserting only the allow-listed variables into the spawned `ssh`/`scp`
child, and `HOME` is not one of them -- so those processes always resolve
`~` via the OS account, never via any `$HOME` this script sets for the
outer `atm` CLI process. Mutating a developer or CI account's real SSH
config from a test harness is exactly the kind of blast radius a harness
must not have. Instead this script writes a throwaway ssh client config
under its own scratch root and exports `ATM_TRANSFER_SSH_CONFIG` (an opt-in
fourth entry in `TRANSFER_SCRIPT_ALLOWED_ENV_KEYS`, unset for every ordinary
install) so `sftp.sh`/`sftp.ps1` pass it through to `ssh`/`scp` as
`-F <scratch config>`. See `write_scratch_ssh_client_config`.

Why this script also writes a sender-side `.atm.toml` with `local_host`
(confirmed live on clean-runner CI, run 33125703487: `atm send` exited 3,
"recipient is qualified for host 'localhost' but this machine's `.atm.toml`
has no `local_host` set"): decision (f) fails a host-qualified recipient
closed with `LocalHostUnset` unless the sender's own `local_host` is
configured. See `write_sender_atm_config`'s docstring for why its value is
deliberately a *different* label than the recipient's `localhost`.

Why the Windows transfer-script safety check is not POSIX-mode-based
(cipher's investigation on #1066): NTFS reports `chmod` results as
essentially always `0777`/`0666` regardless of what was requested -- mode
bits are not a meaningful safety signal there. `check_script_safety` and
`check_transfer_root_metadata` (`crates/atm-core/src/transfer_script.rs`,
`#[cfg(windows)]` branches) instead require the installed `.atm/transfer`
directory and script to sit under the resolved profile home
(`$HOME`/`%USERPROFILE%`, falling back to the OS account profile) and not be
a reparse point. `install_transfer_script` mirrors that check on Windows and
records its result under `windows_profile_containment` instead of POSIX
mode strings; the `0o700` assertions remain Unix-only.
"""

from __future__ import annotations

import argparse
import collections
import json
import os
from pathlib import Path
import re
import shutil
import socket
import stat
import subprocess
import sys
import tempfile
import threading
import time
import traceback
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
TEAM = "aq4-transfer-evidence"
SENDER = "aq4-sender"
RECEIVER = "aq4-receiver"
TRANSFER_HOST = "localhost"
SENDER_LOCAL_HOST = "aq4-sender"
READY_TIMEOUT_SECONDS = 15.0
SSHD_READY_TIMEOUT_SECONDS = 10.0
ATTACHMENT_FILE_NAME = "aq4-report.pdf"
ATTACHMENT_BODY = b"%PDF-1.4\naq4 live evidence attachment\n"
MESSAGE_TEXT = "AQ4 live transfer evidence: see attached file"

# Windows CI (`windows-latest`) is treated identically to every other
# platform everywhere this constant is *not* consulted; it exists solely
# for the handful of genuinely OS-divergent seams cipher's investigation
# identified: which example script ships, how its safety check is proven,
# and the `UserKnownHostsFile` sink OpenSSH accepts.
IS_WINDOWS = sys.platform == "win32"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="local", help="evidence host label, for example local or clean-runner-<os>")
    parser.add_argument(
        "--evidence-dir",
        type=Path,
        default=None,
        help="directory for the JSON and Markdown evidence files",
    )
    parser.add_argument(
        "--daemon",
        type=Path,
        default=Path(os.environ.get("ATM_DAEMON_BIN", ROOT / "target" / "debug" / "atm-daemon")),
        help="owned atm-daemon binary",
    )
    parser.add_argument(
        "--atm",
        type=Path,
        default=Path(os.environ.get("ATM_BIN", ROOT / "target" / "debug" / "atm")),
        help="matched atm CLI binary",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=READY_TIMEOUT_SECONDS,
        help="daemon readiness / CLI round-trip timeout in seconds",
    )
    return parser.parse_args()


def ambient_daemon_pids() -> list[int]:
    """Return same-account daemon processes before touching host state."""
    if os.name == "nt":
        completed = subprocess.run(
            ["tasklist", "/FO", "CSV", "/NH"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        pids: list[int] = []
        for line in completed.stdout.splitlines():
            columns = [item.strip('"') for item in line.split('","')]
            if columns and columns[0].lower() == "atm-daemon.exe" and len(columns) > 1 and columns[1].isdigit():
                pids.append(int(columns[1]))
        return pids
    completed = subprocess.run(
        ["ps", "-axo", "uid=,pid=,command="],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError("could not inspect same-account daemon processes")
    owner_uid = os.getuid()
    pids = []
    for line in completed.stdout.splitlines():
        parts = line.strip().split(None, 2)
        if len(parts) != 3 or not parts[0].isdigit() or not parts[1].isdigit():
            continue
        executable = parts[2].split(None, 1)[0]
        if int(parts[0]) == owner_uid and Path(executable).name == "atm-daemon":
            pids.append(int(parts[1]))
    return pids


def find_sshd() -> Path | None:
    which = shutil.which("sshd")
    if which:
        return Path(which)
    for candidate in ("/usr/sbin/sshd", "/usr/bin/sshd"):
        path = Path(candidate)
        if path.is_file():
            return path
    return None


def ensure_sshd_available() -> tuple[Path | None, dict[str, Any]]:
    """Locates `sshd`, installing it on ubuntu if missing and root/sudo is
    available. Never raises: an unavailable `sshd` is an honest
    `skipped_no_sshd` evidence outcome, not a script failure -- this covers
    Windows runners with no OpenSSH Server feature installed too, since
    `find_sshd()` above already checks `PATH` (and the Unix-only fallback
    paths are simply absent there) before this function's platform branches
    run."""
    found = find_sshd()
    if found is not None:
        return found, {"found": True, "path": str(found)}

    if sys.platform.startswith("linux"):
        install = subprocess.run(
            ["sudo", "-n", "apt-get", "install", "-y", "openssh-server"],
            capture_output=True,
            text=True,
            check=False,
            timeout=180,
        )
        found = find_sshd()
        return found, {
            "found": found is not None,
            "install_attempted": True,
            "install_returncode": install.returncode,
            "install_stderr_tail": install.stderr.strip()[-2000:],
        }

    # macOS ships /usr/sbin/sshd but running it standalone (outside the
    # launchd-managed Remote Login service) can be refused by the runner's
    # security policy; find_sshd() above already checked the well-known
    # path, so reaching here on macOS means it is genuinely absent or not
    # executable -- record that honestly rather than guessing why. Windows
    # runners fall through to this same branch: there is no equivalent
    # well-known-path fallback or unattended install path for OpenSSH
    # Server there, so an absent `sshd` is reported the same honest way.
    return None, {"found": False, "install_attempted": False}


def free_loopback_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def generate_ssh_keys(root: Path) -> dict[str, Path]:
    identity = root / "id_ed25519"
    host_key = root / "ssh_host_ed25519_key"
    for path in (identity, host_key):
        completed = subprocess.run(
            ["ssh-keygen", "-t", "ed25519", "-N", "", "-f", str(path), "-q"],
            capture_output=True,
            text=True,
            check=False,
            timeout=30,
        )
        if completed.returncode != 0:
            raise RuntimeError(f"ssh-keygen failed for {path}: {completed.stderr.strip()}")
        if not IS_WINDOWS:
            path.chmod(0o600)
    return {"identity": identity, "identity_pub": root / "id_ed25519.pub", "host_key": host_key}


def write_sshd_config(root: Path, port: int, keys: dict[str, Path]) -> Path:
    config_path = root / "sshd_config"
    config_path.write_text(
        "\n".join(
            [
                f"Port {port}",
                "ListenAddress 127.0.0.1",
                f"HostKey {keys['host_key']}",
                f"AuthorizedKeysFile {keys['identity_pub']}",
                f"PidFile {root / 'sshd.pid'}",
                "UsePAM no",
                "StrictModes no",
                "PasswordAuthentication no",
                "PubkeyAuthentication yes",
                "PermitRootLogin no",
                "Subsystem sftp internal-sftp",
                "LogLevel DEBUG3",
                "",
            ]
        ),
        encoding="utf-8",
    )
    return config_path


class PipeDrain:
    """Continuously drains a subprocess's stdout (and stderr, when it is a
    distinct pipe) into bounded in-memory deques on background daemon
    threads, for the process's entire lifetime -- not only while a
    readiness probe happens to be reading (FTQ-002/003). Without this, a
    chatty child (this scenario runs `sshd` at `LogLevel DEBUG3` and the
    daemon at `ATM_LOG=debug`) can fill its OS pipe buffer and block on a
    write the instant nothing is actively draining it, wedging the whole
    scenario with no timeout to save it.
    """

    def __init__(self, process: subprocess.Popen[str], max_lines: int = 4000) -> None:
        self.stdout_lines: collections.deque[str] = collections.deque(maxlen=max_lines)
        self.stderr_lines: collections.deque[str] = collections.deque(maxlen=max_lines)
        self._threads: list[threading.Thread] = []
        if process.stdout is not None:
            self._threads.append(self._spawn_reader(process.stdout, self.stdout_lines))
        if process.stderr is not None:
            self._threads.append(self._spawn_reader(process.stderr, self.stderr_lines))

    @staticmethod
    def _spawn_reader(stream: Any, sink: "collections.deque[str]") -> threading.Thread:
        def _drain() -> None:
            for line in iter(stream.readline, ""):
                sink.append(line.rstrip("\n"))

        thread = threading.Thread(target=_drain, daemon=True)
        thread.start()
        return thread

    @staticmethod
    def tail(sink: "collections.deque[str]", count: int = 200) -> str:
        return "\n".join(list(sink)[-count:])

    def join(self, timeout: float = 2.0) -> None:
        for thread in self._threads:
            thread.join(timeout=timeout)


def start_sshd(sshd_bin: Path, config_path: Path, port: int) -> dict[str, Any]:
    process = subprocess.Popen(
        [str(sshd_bin), "-f", str(config_path), "-D", "-e"],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    drain = PipeDrain(process)
    deadline = time.monotonic() + SSHD_READY_TIMEOUT_SECONDS
    ready = False
    while time.monotonic() < deadline:
        if process.poll() is not None:
            break
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                ready = True
                break
        except OSError:
            time.sleep(0.1)
    log_tail = PipeDrain.tail(drain.stdout_lines) if process.poll() is not None else ""
    return {"process": process, "drain": drain, "ready": ready, "pid": process.pid, "log_tail": log_tail}


def stop_sshd(process: subprocess.Popen[str] | None, drain: PipeDrain | None = None) -> None:
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
    if drain is not None:
        drain.join()


def write_scratch_ssh_client_config(root: Path, port: int, identity: Path) -> Path:
    """Writes a throwaway `ssh`/`scp` client config under this scenario's
    own scratch `root` (QA-2 B6) -- never the real OS account
    `~/.ssh/config` -- routing the literal hostname `localhost` at this
    script's loopback `sshd`. The caller threads its path through
    `ATM_TRANSFER_SSH_CONFIG` (an opt-in fourth entry in
    `TRANSFER_SCRIPT_ALLOWED_ENV_KEYS`), which `sftp.sh`/`sftp.ps1` pass to
    `ssh`/`scp` as `-F <path>` when set. Cleanup is implicit: the caller's
    scratch `root` is a `tempfile.TemporaryDirectory` removed when the
    scenario ends, so this never needs its own backup/restore step -- the
    real file it might otherwise have touched is never opened at all.

    `UserKnownHostsFile` is `NUL` on Windows and `/dev/null` everywhere
    else: OpenSSH on Windows does not treat the Unix device path as a sink,
    so a POSIX-style `/dev/null` there would create (and then try to
    parse as a known-hosts file) a literal file named `/dev/null` instead of
    discarding host-key state.
    """
    known_hosts_sink = "NUL" if IS_WINDOWS else "/dev/null"
    config_path = root / "ssh_client_config"
    stanza = "\n".join(
        [
            f"Host {TRANSFER_HOST}",
            "    Hostname 127.0.0.1",
            f"    Port {port}",
            f"    User {os.environ.get('USER') or os.environ.get('USERNAME') or ''}",
            f"    IdentityFile {identity}",
            "    IdentitiesOnly yes",
            "    StrictHostKeyChecking no",
            f"    UserKnownHostsFile {known_hosts_sink}",
            "    PasswordAuthentication no",
            "",
        ]
    )
    config_path.write_text(stanza, encoding="utf-8")
    if not IS_WINDOWS:
        config_path.chmod(0o600)
    return config_path


def fixture_environment(root: Path) -> dict[str, str]:
    home = root / "home"
    atm_home = root / "atm-home"
    logs = root / "logs"
    for directory in (home, atm_home, logs):
        directory.mkdir(parents=True)
    temp_dir = root / "tmp"
    temp_dir.mkdir()
    return {
        **os.environ,
        "HOME": str(home),
        "ATM_HOME": str(atm_home),
        "ATM_CONFIG_HOME": str(atm_home),
        "ATM_TEAM": TEAM,
        "ATM_LOG": "debug",
        "ATM_LOG_DIR": str(logs),
        "TMPDIR": str(temp_dir),
        "TMP": str(temp_dir),
        "TEMP": str(temp_dir),
    }


def run_cli(
    atm: Path,
    env: dict[str, str],
    args: list[str],
    *,
    identity: str,
    timeout: float,
    cwd: Path | None = None,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(atm), *args],
        cwd=cwd or ROOT,
        env={**env, "ATM_IDENTITY": identity},
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
    )


def add_roster_member(
    atm: Path,
    env: dict[str, str],
    home: Path,
    member: str,
    timeout: float,
    *,
    host: str | None = None,
) -> dict[str, Any]:
    args = ["teams", "add-member", TEAM, member, "--home-dir", str(home)]
    if host is not None:
        # Decision (e): the roster's own registered host binding, recorded
        # for picker-projection fidelity -- independent of (and consistent
        # with) the `--host` this scenario also passes explicitly on `atm
        # send` below, since the legacy direct-host path
        # (`is_legacy_direct_host`) never consults the roster for host
        # resolution.
        args += ["--host", host]
    args.append("--json")
    completed = run_cli(atm, env, args, identity=SENDER, timeout=timeout)
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise RuntimeError(f"could not add {member} to the isolated roster: {detail}")
    return {"argv": completed.args, "stdout": completed.stdout.strip()}


def start_daemon(daemon: Path, env: dict[str, str], timeout: float) -> dict[str, Any]:
    process = subprocess.Popen(
        [str(daemon), "--peer-wire-security", "plaintext-test"],
        cwd=ROOT,
        env={**env, "ATM_DAEMON_READY_STDOUT": "1"},
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    drain = PipeDrain(process)
    deadline = time.monotonic() + timeout
    ready = False
    while time.monotonic() < deadline:
        if process.poll() is not None:
            break
        if any(line.strip() == "ATM_DAEMON_READY" for line in drain.stdout_lines):
            ready = True
            break
        time.sleep(0.05)
    return {
        "process": process,
        "drain": drain,
        "ready": ready,
        "pid": process.pid,
        "stdout_tail": list(drain.stdout_lines),
        "exited_early": process.poll() is not None,
        "returncode": process.returncode,
        "stderr_tail": PipeDrain.tail(drain.stderr_lines),
    }


def stop_daemon(process: subprocess.Popen[str] | None, drain: PipeDrain | None = None) -> None:
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
    if drain is not None:
        drain.join()


def extract_landed_path(peek_stdout: str, file_name: str) -> str | None:
    pattern = re.compile(r"Attached files \(on this host\):\\?n?- (\S*" + re.escape(file_name) + r")")
    match = pattern.search(peek_stdout)
    return match.group(1) if match else None


def write_sender_atm_config(cwd: Path, local_host: str) -> Path:
    """Writes `.atm.toml` with `[atm] local_host = "<local_host>"` directly
    into `cwd` (ADR-055 decision (f)). Required: a host-qualified recipient
    (this scenario's `--host localhost`) with no `local_host` configured
    fails closed with `LocalHostUnset` before ever reaching the transfer
    script -- confirmed live on clean-runner CI (run 33125703487, exit code
    3, "recipient is qualified for host 'localhost' but this machine's
    `.atm.toml` has no `local_host` set"). `local_host` is deliberately a
    *different* label ("aq4-sender") than the recipient's `localhost`: equal
    values would classify the recipient same-host
    (`classify_recipient_locality`), skipping the transfer-script path this
    scenario exists to exercise.
    """
    config_path = cwd / ".atm.toml"
    config_path.write_text(f'[atm]\nlocal_host = "{local_host}"\n', encoding="utf-8")
    return config_path


def _mode_octal(path: Path) -> str:
    return oct(stat.S_IMODE(path.stat().st_mode))


def _is_within_profile(path: Path, profile_home: Path) -> bool:
    """Python mirror of `crate::transfer_script::path_is_within`'s
    component-wise containment compare, for evidence recording only (the
    Rust safety check itself is what actually gates `atm send`; this exists
    so the JSON evidence can honestly report whether the scenario's own
    Windows install landed somewhere the real check would accept, without
    calling into Rust from Python)."""
    try:
        path.relative_to(profile_home)
        return True
    except ValueError:
        return False


def _synthesized_transfer_script_path(env: dict[str, str]) -> dict[str, Any]:
    """Python mirror of `atm_core::transfer_script::
    synthesized_transfer_script_env` (ADR-055 decision (c) amendment), for
    evidence recording only -- this harness never calls into that Rust
    function directly; it computes the same value here purely so the JSON
    evidence records what the real `atm send --attach` invocation below
    should have synthesized for its transfer-script child, for
    diagnosability (run 33135390308 shipped with no visibility into what
    `PATH`, if any, the child actually received). This never gates or
    otherwise changes this harness's own behavior -- it is recorded
    alongside `record["transfer_script"]`, exactly like
    `windows_profile_containment` mirrors the Windows safety check for the
    same reason.
    """
    if IS_WINDOWS:
        system_root = env.get("SystemRoot") or env.get("SYSTEMROOT") or r"C:\Windows"
        parts = [f"{system_root}\\System32", f"{system_root}\\System32\\OpenSSH"]
        pwsh = shutil.which("pwsh", path=env.get("PATH"))
        if pwsh:
            parts.append(str(Path(pwsh).parent))
        result: dict[str, Any] = {
            "PATH": ";".join(parts),
            "SystemRoot": system_root,
            "SYSTEMROOT": system_root,
        }
        temp = env.get("TEMP") or env.get("TMP")
        if temp:
            result["TEMP"] = temp
        return result
    if sys.platform == "darwin":
        return {"PATH": "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin"}
    return {"PATH": "/usr/bin:/bin:/usr/local/bin"}


def install_transfer_script(home: Path) -> dict[str, Any]:
    """Installs the unmodified `scripts/transfer/sftp.sh` example (or, on
    Windows, `scripts/transfer/sftp.ps1`) at
    `<home>/.atm/transfer/localhost[.ps1]`.

    Unix: explicitly enforces `0700` on the `.atm` and `.atm/transfer`
    directories (`check_transfer_root_metadata`,
    `crates/atm-core/src/transfer_script.rs`) and the script file itself
    (`check_script_safety`) -- the only two paths ADR-055's Unix
    transfer-script safety check inspects; nothing else under `home` is on
    that check's path.

    `os.makedirs(..., mode=0o700)`'s `mode` argument governs only the leaf
    directory it creates (`transfer`), never any parent it also creates
    along the way (`.atm`) -- Python's own documented behavior, mirroring
    POSIX `mkdir -p`. Both `mkdir`/`makedirs`' `mode` argument and a bare
    `Path.chmod` are additionally subject to the process umask (CI runners
    commonly default to 022, which is exactly the 0755 the safety check
    refused live on clean-runner CI, run 33126676155) unless the requested
    mode has no bits in the umask's cleared positions -- 0700 has none, so
    it is umask-proof, but this still `os.chmod`s every directory level
    explicitly after creation rather than relying on that interaction.

    Windows (cipher's investigation on #1066): NTFS `chmod` results are not
    a meaningful safety signal (`stat.S_IMODE` reports essentially always
    `0o777`/`0o666` there), and the production check
    (`check_transfer_root_metadata`/`check_script_safety`,
    `#[cfg(windows)]`) does not consult mode bits at all -- it requires the
    installed path to sit under the resolved profile home
    (`$HOME`/`%USERPROFILE%`, which `fixture_environment` already points at
    this scenario's scratch `home`) and not be a reparse point. This
    function mirrors that containment check (`_is_within_profile`) and
    records it under `windows_profile_containment` instead of POSIX mode
    strings.
    """
    atm_dir = home / ".atm"
    transfer_dir = atm_dir / "transfer"
    os.makedirs(transfer_dir, mode=0o700, exist_ok=True)

    if IS_WINDOWS:
        installed = transfer_dir / f"{TRANSFER_HOST}.ps1"
        shutil.copyfile(ROOT / "scripts" / "transfer" / "sftp.ps1", installed)
        profile_home = home.resolve()
        return {
            "installed_at": str(installed),
            "source": str(ROOT / "scripts" / "transfer" / "sftp.ps1"),
            "windows_profile_containment": {
                "profile_home": str(profile_home),
                "transfer_dir_contained": _is_within_profile(transfer_dir.resolve(), profile_home),
                "script_contained": _is_within_profile(installed.resolve(), profile_home),
                "transfer_dir_is_reparse_point": transfer_dir.is_symlink(),
                "script_is_reparse_point": installed.is_symlink(),
            },
        }

    os.chmod(atm_dir, 0o700)
    os.chmod(transfer_dir, 0o700)

    installed = transfer_dir / TRANSFER_HOST
    shutil.copyfile(ROOT / "scripts" / "transfer" / "sftp.sh", installed)
    os.chmod(installed, 0o700)

    return {
        "installed_at": str(installed),
        "source": str(ROOT / "scripts" / "transfer" / "sftp.sh"),
        "atm_dir_mode": _mode_octal(atm_dir),
        "transfer_dir_mode": _mode_octal(transfer_dir),
        "script_mode": _mode_octal(installed),
    }


def _remove_tree_tolerant(path: Path, *, attempts: int = 6, initial_delay: float = 0.15) -> str | None:
    """Best-effort recursive removal of a scenario's scratch directory.

    Windows can lag briefly between a child process (or this scenario's own
    `atm send`/`atm peek` invocations, which already ran to completion and
    were fully waited on via `subprocess.run`) exiting and the OS actually
    releasing every handle it held on a directory that was ever used as a
    process's current working directory (observed live: WinError 32/5
    sharing violations against `sender-cwd`, raised only *after* the
    scenario's real work -- and its result -- had already been computed).
    Retrying with backoff absorbs that transient lag; if the directory still
    will not budge, this reports a warning string instead of raising, so an
    OS-level cleanup race can never discard or crash the evidence this
    scenario already collected (the previous behavior: relying on
    `tempfile.TemporaryDirectory`'s automatic `__exit__` cleanup, whose
    exception propagated straight out of `run_scenario` -- past its own
    `except Exception` -- discarding the already-computed `record`
    entirely). Returns `None` on success, or a description of the final
    failure.
    """
    delay = initial_delay
    last_error: OSError | None = None
    for attempt in range(attempts):
        try:
            shutil.rmtree(path)
            return None
        except FileNotFoundError:
            return None
        except OSError as error:
            last_error = error
            if attempt == attempts - 1:
                break
            time.sleep(delay)
            delay *= 2
    return f"could not remove {path} after {attempts} attempts: {last_error}"


def run_scenario(args: argparse.Namespace) -> dict[str, Any]:
    started_at = time.time_ns()
    record: dict[str, Any] = {
        "sprint": "AQ4",
        "host": args.host,
        "started_at_ns": started_at,
        "status": "blocked",
    }

    ambient = ambient_daemon_pids()
    if ambient:
        record["status"] = "blocked_ambient_daemon"
        record["ambient_daemon_pids"] = ambient
        record["error"] = (
            "an ATM daemon already owns this OS account's singleton runtime "
            "root; refusing to start a second one. Run on a dedicated host / "
            "OS account with no ambient atm-daemon to produce positive-path "
            "evidence, exactly like AQ1.9/AQ2.5's harnesses."
        )
        return record

    sshd_bin, sshd_probe = ensure_sshd_available()
    record["sshd_probe"] = sshd_probe
    if sshd_bin is None:
        record["status"] = "skipped_no_sshd"
        record["error"] = (
            "no usable sshd on this runner and it could not be installed "
            "(see sshd_probe for the attempt); this live-evidence scenario "
            "needs a real loopback SSH server, honoring ADR-055 decision "
            "(c)'s real ssh/scp transport -- not skipped silently."
        )
        return record

    port = free_loopback_port()
    # A manually-managed `mkdtemp` (not `tempfile.TemporaryDirectory`'s
    # `with`-block auto-cleanup) deliberately: that context manager's
    # `__exit__` calls `cleanup()`, which on Windows can raise a
    # PermissionError (WinError 32/5) if any handle -- this scenario's own
    # `sender_cwd`, used as an `atm send` child's cwd, included -- has not
    # yet been released by the OS. Because `__exit__` runs while unwinding
    # the `return record` below, that exception previously propagated
    # straight out of this function, discarding the already-computed
    # `record` entirely and crashing the harness with no evidence written.
    # Cleanup now happens explicitly in `finally`, below, through
    # `_remove_tree_tolerant`, which can never raise past this function.
    directory = tempfile.mkdtemp(prefix="aq4-evidence-")
    root = Path(directory)
    env = fixture_environment(root)
    home = Path(env["HOME"])
    sshd_root = root / "sshd"
    sshd_root.mkdir()
    daemon_process: subprocess.Popen[str] | None = None
    daemon_drain: PipeDrain | None = None
    sshd_process: subprocess.Popen[str] | None = None
    sshd_drain: PipeDrain | None = None
    remote_landing_dir: Path | None = None
    try:
        keys = generate_ssh_keys(sshd_root)
        config_path = write_sshd_config(sshd_root, port, keys)
        sshd = start_sshd(sshd_bin, config_path, port)
        sshd_process = sshd.pop("process")
        sshd_drain = sshd.pop("drain")
        record["sshd_start"] = {**sshd, "port": port}
        if not sshd["ready"]:
            record["status"] = "blocked_sshd_start_failed"
            record["error"] = sshd.get("log_tail") or "sshd did not open its loopback port"
            return record

        sender_cwd = root / "sender-cwd"
        sender_cwd.mkdir()
        sender_config_path = write_sender_atm_config(sender_cwd, SENDER_LOCAL_HOST)
        record["sender_atm_config"] = {
            "path": str(sender_config_path),
            "local_host": SENDER_LOCAL_HOST,
        }

        # QA-2 B6: a scratch ssh client config, never the real
        # ~/.ssh/config -- threaded to the transfer script's spawned
        # ssh/scp children through ATM_TRANSFER_SSH_CONFIG, an opt-in
        # entry in TRANSFER_SCRIPT_ALLOWED_ENV_KEYS every ordinary
        # install leaves unset.
        ssh_config_path = write_scratch_ssh_client_config(root, port, keys["identity"])
        env["ATM_TRANSFER_SSH_CONFIG"] = str(ssh_config_path)
        record["scratch_ssh_client_config"] = str(ssh_config_path)

        record["roster"] = {
            "sender": add_roster_member(args.atm, env, home, SENDER, args.timeout),
            # Decision (e): register the receiver's roster host as
            # the same literal "localhost" the send below targets
            # with --host, so the recipient this scenario delivers
            # to is consistently recorded as reachable at that host,
            # not merely accepted through the send-time override.
            "receiver": add_roster_member(args.atm, env, home, RECEIVER, args.timeout, host=TRANSFER_HOST),
        }

        record["transfer_script"] = install_transfer_script(home)
        # Diagnosability only (never asserted against): what the real
        # `atm send --attach` invocation below should synthesize for
        # the transfer-script child's environment (ADR-055 decision
        # (c) amendment).
        record["transfer_script"]["synthesized_env"] = _synthesized_transfer_script_path(env)
        # Not a bare `assert` (stripped under `python -O`): the
        # whole point of installing at 0700 (Unix) / under the
        # profile home (Windows) explicitly is to make the safety
        # check pass, so a mismatch here must be a loud,
        # unconditional harness failure, not a silently skippable
        # assertion.
        if IS_WINDOWS:
            containment = record["transfer_script"]["windows_profile_containment"]
            if not (containment["transfer_dir_contained"] and containment["script_contained"]):
                raise RuntimeError(
                    f"install_transfer_script did not land under the resolved profile "
                    f"home on Windows: {containment}"
                )
            if containment["transfer_dir_is_reparse_point"] or containment["script_is_reparse_point"]:
                raise RuntimeError(
                    f"install_transfer_script produced a reparse point, which the real "
                    f"Windows safety check refuses outright: {containment}"
                )
        else:
            for mode_key in ("atm_dir_mode", "transfer_dir_mode", "script_mode"):
                recorded_mode = record["transfer_script"][mode_key]
                if recorded_mode != "0o700":
                    raise RuntimeError(
                        f"install_transfer_script did not achieve 0700 for {mode_key}: "
                        f"got {recorded_mode} (record: {record['transfer_script']})"
                    )

        daemon = start_daemon(args.daemon, env, args.timeout)
        daemon_process = daemon.pop("process")
        daemon_drain = daemon.pop("drain")
        record["daemon_start"] = daemon
        if not daemon["ready"]:
            record["status"] = "blocked_daemon_start_failed"
            record["error"] = daemon.get("stderr_tail") or "daemon did not report ready"
            return record

        attach_source_dir = root / "attach-source"
        attach_source_dir.mkdir()
        attach_path = attach_source_dir / ATTACHMENT_FILE_NAME
        attach_path.write_bytes(ATTACHMENT_BODY)

        # cwd=sender_cwd: `atm send` resolves `.atm.toml`'s
        # `local_host` (decision (f)) by walking upward from the
        # process's current directory, so the sender-side config
        # written above is only found if this invocation actually
        # runs from there -- unlike every other command in this
        # scenario, which has no need for `local_host` and keeps
        # running from ROOT.
        send_completed = run_cli(
            args.atm,
            env,
            ["send", f"{RECEIVER}@{TEAM}", MESSAGE_TEXT, "--host", TRANSFER_HOST, "--attach", str(attach_path)],
            identity=SENDER,
            timeout=args.timeout,
            cwd=sender_cwd,
        )
        record["send"] = {
            "argv": send_completed.args,
            "returncode": send_completed.returncode,
            "stdout": send_completed.stdout.strip(),
            "stderr_tail": send_completed.stderr.strip()[-2000:],
        }

        send_ok = send_completed.returncode == 0
        landed_path: str | None = None
        landed_matches_convention = False
        landed_file_exists = False
        landed_content_matches = False

        if send_ok:
            peek_completed = run_cli(
                args.atm,
                env,
                ["peek", "--json", "--all", "--team", TEAM, "--as", RECEIVER],
                identity=RECEIVER,
                timeout=args.timeout,
            )
            record["peek"] = {
                "returncode": peek_completed.returncode,
                "stdout": peek_completed.stdout,
                "stderr_tail": peek_completed.stderr.strip()[-2000:],
            }
            landed_path = extract_landed_path(peek_completed.stdout, ATTACHMENT_FILE_NAME)
            if landed_path is not None:
                remote_landing_dir = Path(landed_path).parent
                # Containment (ADR-055's send_to_staging_dir convention,
                # mirrored by sftp.sh's fixed remote_atm_temp choice):
                # the landed directory must be exactly
                # <remote atm-temp root>/send-to/<transfer-id>, never
                # anywhere else on the receiving filesystem.
                landed_matches_convention = bool(
                    re.fullmatch(r".*/send-to/[0-9A-Za-z]+", str(remote_landing_dir))
                )
                landed_file = Path(landed_path)
                landed_file_exists = landed_file.is_file()
                if landed_file_exists:
                    landed_content_matches = landed_file.read_bytes() == ATTACHMENT_BODY

        record["landed_path"] = landed_path
        record["landed_matches_send_to_convention"] = landed_matches_convention
        record["landed_file_exists"] = landed_file_exists
        record["landed_content_matches"] = landed_content_matches
        record["status"] = (
            "pass"
            if send_ok and landed_matches_convention and landed_file_exists and landed_content_matches
            else "fail"
        )
    except Exception as error:  # noqa: BLE001 - evidence must retain the failure
        record["error"] = f"{type(error).__name__}: {error}"
        record["status"] = "fail"
    finally:
        # Stop and fully wait() on every child this scenario owns, and
        # join its pipe-drain reader threads, before ever touching `root`
        # -- `atm send`/`atm peek` above already ran through the blocking
        # `subprocess.run` (via `run_cli`), so only the long-lived daemon
        # and sshd children can still be holding anything under `root`
        # open at this point.
        stop_daemon(daemon_process, daemon_drain)
        stop_sshd(sshd_process, sshd_drain)
        if remote_landing_dir is not None and remote_landing_dir.exists():
            shutil.rmtree(remote_landing_dir, ignore_errors=True)
        cleanup_warning = _remove_tree_tolerant(root)
        if cleanup_warning is not None:
            record["cleanup_warning"] = cleanup_warning
        record["finished_at_ns"] = time.time_ns()
    return record


def _evidence_output_paths(args: argparse.Namespace) -> tuple[Path, Path]:
    evidence_dir = args.evidence_dir or ROOT / "docs" / "plans" / "phase-aq" / "evidence" / "AQ4"
    return evidence_dir / f"transfer-{args.host}.json", evidence_dir / f"transfer-{args.host}.md"


def _clear_stale_evidence(*paths: Path) -> None:
    """Deletes any pre-existing evidence file at these exact output paths
    before the scenario runs. Evidence directories are committed to the
    repo, so without this a harness that crashes before `write_evidence`
    ever runs (see `main`'s top-level guard) would otherwise leave the
    previous, stale run's committed file in place -- and a CI workflow's
    `if: always()` artifact-upload step would then publish that stale file
    as if it were fresh for this run. Deleting first means a genuine crash
    the guard itself cannot recover from (for example the interpreter being
    killed outright) leaves this run's evidence *missing*, never *stale* --
    an honest signal `if-no-files-found: warn` already tolerates.
    """
    for path in paths:
        path.unlink(missing_ok=True)


def write_evidence(args: argparse.Namespace, record: dict[str, Any]) -> tuple[Path, Path]:
    json_path, markdown_path = _evidence_output_paths(args)
    json_path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "schema_version": 1,
        "sprint": "AQ4",
        "host": args.host,
        "commit": subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout.strip(),
        "record": record,
    }
    json_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")

    lines = [
        "# AQ4 cross-host transfer live evidence",
        "",
        f"Host: `{args.host}`",
        f"Commit: `{payload['commit']}`",
        f"Status: **{record['status'].upper()}**",
        "",
    ]
    if record["status"] == "blocked_ambient_daemon":
        lines += [
            "This host has an ambient, already-running `atm-daemon` that "
            f"legitimately owns the OS account's singleton runtime lock "
            f"(pid(s) {record.get('ambient_daemon_pids')}). This runner "
            "refuses to start a second daemon rather than risk the ambient "
            "session, exactly as `run_hermes_atm_restart_matrix.py` (AQ1.9) "
            "and `run_aq25_queue_delivery_trigger_evidence.py` (AQ2.5) do "
            "for the same reason.",
        ]
    elif record["status"] == "skipped_no_sshd":
        lines += [
            "No usable `sshd` was available on this runner and it could not "
            "be installed. This is an honest, announced skip -- ADR-055 "
            "decision (c)'s real `ssh`/`scp` transport genuinely needs a "
            "reachable SSH server; there is no meaningful fake for the "
            "live-transcript deliverable (the filesystem/loopback-fake "
            "contract tests already cover the script logic in "
            "`.just/tests/test_transfer_scripts.py`).",
            "",
            "```json",
            json.dumps(record.get("sshd_probe", {}), indent=2),
            "```",
        ]
    elif record["status"] == "harness_crashed":
        lines += [
            "The harness raised an unhandled exception before it could "
            "finish running the scenario. This transcript is written by a "
            "top-level guard specifically so a crash can never leave this "
            "run's evidence stale (a previous run's committed file, "
            "reused unchanged) or missing (no file at all) -- see "
            "`main`'s top-level `try`/`except` around `run_scenario`.",
            "",
            f"Error: `{record.get('error')}`",
            "",
            "```",
            record.get("traceback", "").rstrip(),
            "```",
        ]
    else:
        send = record.get("send", {})
        lines += [
            "## Real `atm send --attach` over a loopback `sshd`",
            "",
            f"Command: `{' '.join(str(part) for part in send.get('argv', []))}`",
            f"Exit code: `{send.get('returncode')}`",
            "",
            f"Landed path (from the receiver's real mailbox): `{record.get('landed_path')}`",
            f"Matches `send_to_staging_dir` convention (`.../send-to/<transfer-id>`): **{record.get('landed_matches_send_to_convention')}**",
            f"Landed file exists: **{record.get('landed_file_exists')}**",
            f"Landed content byte-for-byte matches the source attachment: **{record.get('landed_content_matches')}**",
        ]
        if record["status"] != "pass":
            lines += ["", f"Error: `{record.get('error')}`"]
    if record.get("cleanup_warning"):
        lines += [
            "",
            "**Cleanup warning** (best-effort only; does not affect the "
            f"result above): `{record['cleanup_warning']}`",
        ]
    markdown_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return json_path, markdown_path


def main() -> int:
    args = parse_args()
    # Deleted before any other work: a harness crash the guard below
    # cannot itself recover from must leave this run's evidence missing,
    # never a stale copy of a previous run's committed file.
    _clear_stale_evidence(*_evidence_output_paths(args))
    if args.timeout <= 0:
        raise SystemExit("--timeout must be positive")
    if not args.daemon.is_file():
        raise SystemExit(f"owned daemon binary does not exist: {args.daemon}")
    if not args.atm.is_file():
        raise SystemExit(f"matched atm binary does not exist: {args.atm}")
    try:
        record = run_scenario(args)
    except Exception as error:  # noqa: BLE001 - a crash must still produce evidence, not none
        record = {
            "sprint": "AQ4",
            "host": args.host,
            "status": "harness_crashed",
            "error": f"{type(error).__name__}: {error}",
            "traceback": traceback.format_exc(),
        }
    json_path, markdown_path = write_evidence(args, record)
    print(f"{record['status'].upper()} AQ4 transfer evidence: {json_path}")
    print(f"transcript: {markdown_path}")
    return 0 if record["status"] in ("pass", "blocked_ambient_daemon", "skipped_no_sshd") else 1


if __name__ == "__main__":
    raise SystemExit(main())
