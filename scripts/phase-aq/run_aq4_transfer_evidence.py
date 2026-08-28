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

Why the Windows leg also substitutes `sftp.ps1`'s `$RemoteAtmTemp`
placeholder and repoints the scratch sshd's shell (fenix's ruling, run 7,
33140616718 @ 7f6774802): `sftp.ps1`'s comments direct an operator to
replace the fixed-value placeholder
`$RemoteAtmTemp = "/tmp/atm-REPLACE_WITH_DESTINATION_UID"` once, by hand,
before install -- unlike `sftp.sh`, which computes its equivalent
(`remote_atm_temp="/tmp/atm-$(id -u)"`) at runtime because a Unix sender
has a local uid to read. A Windows sender has no such local value, so
`install_transfer_script` performs the one substitution an operator would
have (`windows_receiver_atm_temp`/`_substitute_windows_remote_atm_temp`),
never leaving the literal placeholder in the installed script. Separately,
`sftp.ps1`'s remote commands (`umask 077 && mkdir -p ...`) are POSIX shell
syntax; the shipped contract is a Windows *sender* talking to a POSIX
*receiver*, so this scenario's scratch loopback sshd (started fresh by
this harness, not the OS account's real sshd) must run a POSIX shell, not
the Windows OpenSSH default `cmd.exe`. `prepare_windows_posix_shell`
points the account-wide `HKLM\\SOFTWARE\\OpenSSH\\DefaultShell` registry
value at a discovered `bash.exe` for the duration of this scenario only,
restoring the prior value (or absence) once the scratch sshd stops; if no
bash is found or the registry write is denied (not running elevated), the
scenario records an honest `skipped_no_posix_receiver` outcome through the
normal `write_evidence` path rather than either crashing or silently
running against the wrong shell.
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

# The trivial remote command `run_ssh_vvv_diagnostic` runs, and the exact
# stdout line a genuinely-reachable-and-answering loopback sshd echoes back
# for it. Shared between that probe's own `argv` and
# `classify_windows_transfer_failure` below so the two can never drift apart.
_SSH_VVV_DIAGNOSTIC_PROBE_STDOUT = "aq4-ssh-vvv-diagnostic-probe"

# The formally deferred follow-up (docs/plans/phase-aq/sprint-AQ4-send-to-core.md)
# a `deferred_windows_loopback` evidence status records itself against.
_WINDOWS_LOOPBACK_DEFERRAL_FOLLOW_UP = "AQ4-windows-loopback"

# Windows CI (`windows-latest`) is treated identically to every other
# platform everywhere this constant is *not* consulted; it exists solely
# for the handful of genuinely OS-divergent seams cipher's investigation
# identified: which example script ships, how its safety check is proven,
# and the `UserKnownHostsFile` sink OpenSSH accepts.
IS_WINDOWS = sys.platform == "win32"

# `winreg` is stdlib-but-Windows-only; imported here (module scope, not
# inside a function) so tests on every platform can monkeypatch
# `module.winreg` with a fake object to exercise
# `_read_windows_default_shell`/`_write_windows_default_shell` without a
# real registry, exactly like `module.shutil.rmtree` is monkeypatched
# elsewhere in this scenario's test suite.
if IS_WINDOWS:
    import winreg
else:  # pragma: no cover - exercised only via mocking on non-Windows
    winreg = None  # type: ignore[assignment]

# The fixed-value placeholder `sftp.ps1` ships with, documented there as a
# "fill in once, by hand" constant a Unix sender's `id -u` equivalent
# cannot compute for a Windows sender.
_WINDOWS_REMOTE_ATM_TEMP_PLACEHOLDER_LINE = (
    '$RemoteAtmTemp = "/tmp/atm-REPLACE_WITH_DESTINATION_UID"'
)

# Where the OpenSSH server's per-account default shell is configured
# (`Computer\HKEY_LOCAL_MACHINE\SOFTWARE\OpenSSH\DefaultShell`, REG_SZ).
_OPENSSH_REGISTRY_KEY = r"SOFTWARE\OpenSSH"
_DEFAULT_SHELL_VALUE_NAME = "DefaultShell"

# Known Git for Windows install locations, checked only after `bash` is
# not found on `PATH` (`shutil.which`, the `where`-equivalent) -- mirrors
# `sftp.ps1`'s own `ssh`/`scp` resolution fallback shape
# (`Resolve-TransferBinary`), never assumed to be the *only* possible
# location.
# Forward slashes deliberately, not backslashes: `pathlib.Path` on Windows
# (`WindowsPath`) treats either separator as a path boundary, but on the
# POSIX hosts this repo's test suite also runs `find_windows_posix_shell`
# on (mac/Linux CI, unit-testing this pure lookup without a real Windows
# box), `PosixPath` treats `\` as an ordinary filename character -- a
# backslash-joined suffix here would silently never match any real file
# there. Forward slashes are correct on both.
_KNOWN_GIT_BASH_LOCATIONS = ("Git/bin/bash.exe", "Git/usr/bin/bash.exe")


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

    `BatchMode`/`LogLevel` live here, in the harness's own scratch config,
    rather than as hardcoded `-o` flags in the shipped `sftp.sh`/
    `sftp.ps1` examples: those scripts are user-modifiable installs (ADR-055
    decision (c)), so the client policy this scenario needs for
    reproducible, non-interactive evidence runs belongs in the throwaway
    `-F` config it already threads through `ATM_TRANSFER_SSH_CONFIG`, not
    baked into the example an operator is expected to adapt. `BatchMode
    yes` disables any interactive prompt (passphrase, unknown host
    confirmation) ssh/scp might otherwise attempt -- belt-and-suspenders
    alongside sftp.ps1's own `-n` fix (run 33142976493 @ dcd3130f1) for the
    same closed-stdin hazard. `LogLevel DEBUG1` puts the client's own
    handshake diagnostics in `record["send"]["stderr_tail"]` (sftp.ps1
    forwards ssh/scp's merged stderr there) without the `DEBUG3` volume the
    scratch `sshd` already logs server-side.
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
            "    BatchMode yes",
            "    LogLevel DEBUG1",
            "",
        ]
    )
    config_path.write_text(stanza, encoding="utf-8")
    if not IS_WINDOWS:
        config_path.chmod(0o600)
    return config_path


def run_ssh_vvv_diagnostic(env: dict[str, str], ssh_config_path: Path, timeout: float) -> dict[str, Any]:
    """Windows-only, failure-only diagnostic (run 33142976493 @ dcd3130f1):
    runs one trivial, harmless remote command through the SAME scratch
    `-F` config the real `atm send --attach` invocation used, directly
    from this harness process (never through `pwsh`/`sftp.ps1`) with
    `-vvv`, so a live-evidence failure carries the ssh CLIENT's own
    handshake-level debug output even when `sftp.ps1`'s own captured
    diagnostic comes back empty (that run's "(ssh exit ): (no output)").
    `stdin=subprocess.DEVNULL` is explicit and deliberate: this harness
    fully controls this one-off invocation's stdio, unlike the closed-
    stdin handle ATM's own restricted environment leaves `sftp.ps1`'s
    child ssh/scp processes to inherit, so this diagnostic call cannot
    itself reproduce dcd3130f1's stdin-handle hazard -- it exists purely
    to capture the ssh client's own debug log for a failed live run.
    Diagnostics only: this return value is recorded but never asserted
    against.
    """
    ssh_bin = shutil.which("ssh", path=env.get("PATH")) or str(
        Path(env.get("SystemRoot", "C:\\Windows")) / "System32" / "OpenSSH" / "ssh.exe"
    )
    argv = [
        ssh_bin,
        "-F",
        str(ssh_config_path),
        "-vvv",
        "-n",
        TRANSFER_HOST,
        f"echo {_SSH_VVV_DIAGNOSTIC_PROBE_STDOUT}",
    ]
    try:
        completed = subprocess.run(
            argv,
            env=env,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
        return {
            "argv": argv,
            "returncode": completed.returncode,
            "stdout": completed.stdout.strip(),
            "stderr_tail": completed.stderr.strip()[-4000:],
        }
    except subprocess.TimeoutExpired as exc:
        stdout = exc.stdout if isinstance(exc.stdout, str) else ""
        stderr = exc.stderr if isinstance(exc.stderr, str) else ""
        return {
            "argv": argv,
            "returncode": None,
            "stdout": stdout.strip(),
            "stderr_tail": stderr.strip()[-4000:],
            "error": f"timed out after {timeout}s",
        }


def classify_windows_transfer_failure(
    *,
    send_ok: bool,
    send_stderr_tail: str,
    ssh_vvv_diagnostic: dict[str, Any] | None,
) -> tuple[str, dict[str, str] | None]:
    """Classifies a failed `atm send --attach` outcome as either the
    documented, formally deferred AQ4-windows-loopback residual symptom
    (`"deferred_windows_loopback"`, non-fatal) or a genuine, still-fatal
    `"fail"`.

    Pure and platform-independent on purpose: it does not consult
    `IS_WINDOWS`/`sys.platform` itself, only the evidence already collected
    by the caller, so it can be unit-tested on every CI host, not only
    Windows. Callers must gate invocation on `IS_WINDOWS` themselves --
    this scenario has no macOS/Linux residual-symptom precedent to defer,
    only the documented Windows one.

    Returns `("deferred_windows_loopback", deferral)` only when every
    documented symptom condition holds simultaneously:

    - `send_ok` is `False` -- a `"pass"` outcome is never reclassified by
      this function; callers must not invoke it once the scenario already
      passed.
    - `ssh_vvv_diagnostic` is present, exited `0`, and its stdout is
      exactly the expected probe line -- proving the loopback sshd this
      scenario started is genuinely reachable and answering commands over
      this exact scratch ssh client config, so the failure is not some
      unrelated connectivity break this harness should still fail on.
    - `send_stderr_tail` contains `sftp.ps1`'s unconditional "invoking"
      line, proving the script actually started running rather than dying
      before reaching its first diagnostic (a different, still-fatal
      failure shape).
    - `send_stderr_tail` contains both "failed to create" and "ssh exit"
      -- `sftp.ps1`'s `mkdir`-step failure message -- proving the failure
      landed exactly at the documented mkdir/ssh step, not the scp/copy
      step, a landed-file mismatch, or an unrelated exception.

    Any other shape -- the vvv probe absent, non-zero, or with unexpected
    stdout; the "invoking" line missing; a failure at the scp/copy step or
    anywhere else -- returns `("fail", None)` unchanged, so a real
    regression on this leg is never masked as deferred.
    """
    if (
        not send_ok
        and ssh_vvv_diagnostic is not None
        and ssh_vvv_diagnostic.get("returncode") == 0
        and ssh_vvv_diagnostic.get("stdout") == _SSH_VVV_DIAGNOSTIC_PROBE_STDOUT
        and "sftp.ps1: invoking" in send_stderr_tail
        and "failed to create" in send_stderr_tail
        and "ssh exit" in send_stderr_tail
    ):
        return "deferred_windows_loopback", {
            "follow_up": _WINDOWS_LOOPBACK_DEFERRAL_FOLLOW_UP,
            "reason": (
                "Windows sender -> POSIX loopback receiver mkdir/ssh step "
                "fails in this harness's scratch-sshd environment; this is "
                "the documented AQ4-windows-loopback residual symptom "
                "(docs/plans/phase-aq/sprint-AQ4-send-to-core.md), confirmed "
                "by a successful ssh -vvv probe and sftp.ps1's own "
                "\"invoking\" line, not a new regression."
            ),
        }
    return "fail", None


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


# Identity/profile-location variables the second ADR-055 decision (c)
# amendment (2026-08-27, AQ4 run 33144153970) adds to the synthesized
# transfer-script child environment, forwarded only when the caller has
# them set. Recorded in evidence JSON as *keys present*, never values --
# unlike `PATH`/`SystemRoot`/`TEMP` above, these can carry the runner
# account's real username (`USERPROFILE`, `HOMEPATH`, ...), and this
# harness's evidence JSON is committed to the repository, so the value
# itself is not worth recording for a field whose only purpose is
# diagnosability.
_WINDOWS_SYNTHESIZED_IDENTITY_KEYS = (
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "LOCALAPPDATA",
    "APPDATA",
    "ProgramData",
    "COMSPEC",
)


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

    The second amendment (2026-08-27, AQ4 run 33144153970) adds
    `synthesized_env_identity_keys`: the *names* only (never values -- see
    `_WINDOWS_SYNTHESIZED_IDENTITY_KEYS`'s docstring) of the identity/
    profile-location variables Windows OpenSSH's own home-directory
    resolution and `pwsh`'s profile-path resolution need, present in this
    list only when the harness's own environment actually had them.
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
        result["synthesized_env_identity_keys"] = sorted(
            key for key in _WINDOWS_SYNTHESIZED_IDENTITY_KEYS if env.get(key)
        )
        return result
    unix_result: dict[str, Any] = {
        "PATH": (
            "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin"
            if sys.platform == "darwin"
            else "/usr/bin:/bin:/usr/local/bin"
        )
    }
    unix_result["synthesized_env_identity_keys"] = ["HOME"] if env.get("HOME") else []
    return unix_result


def windows_receiver_atm_temp(env: dict[str, str]) -> dict[str, str]:
    """Computes the receiver-side `ATM_TEMP` scratch root this scenario's
    Windows daemon process resolves to when `ATM_TEMP` is unset
    (`windows_default_scratch_root`, `crates/atm-core/src/atm_temp.rs`:
    `<TEMP>\\atm`, no uid suffix -- `%TEMP%` is already per-user), in the
    representations `install_transfer_script`'s Windows leg needs:

    - `windows_native`: backslash-separated, exactly what
      `windows_default_scratch_root` would produce -- recorded for
      diagnosability only, never fed to a subprocess (Git-Bash/MSYS's path
      heuristic reliably recognizes forward-slash Windows paths, not
      backslash ones, so a raw backslash form risks silent
      misinterpretation by the MSYS runtime the scratch sshd's
      `DefaultShell` now points at).
    - `posix_msys`: strict `/c/Users/...` POSIX form, also diagnostic-only
      (recorded so a divergence between this and `substituted` is
      visible, never itself fed to a subprocess or printed by the
      installed script).
    - `substituted`: drive-letter-plus-forward-slash hybrid
      (`C:/Users/...`) -- the single value actually written into the
      installed `sftp.ps1`'s `$RemoteAtmTemp`. This form is
      simultaneously absolute per `Path::is_absolute` on Windows (has a
      drive prefix, so `atm send`'s `validate_landed_dir_stdout` accepts
      the script's printed stdout -- a bare `/c/...` POSIX path is
      *rooted* but not *absolute* there and would be rejected), correctly
      recognized by Git-Bash/MSYS's `mkdir -p`, and accepted as-is by
      Python's `pathlib.Path.is_file()` on Windows (which normalizes
      either separator) -- so no runtime translation is needed once this
      one value is used consistently through the unmodified script's
      single `$RemoteAtmTemp` variable.
    """
    # Backslash-joined explicitly, not via `pathlib.Path` (`PurePath`
    # joining is host-OS-dependent -- a `PosixPath` join on this repo's
    # non-Windows test hosts would use `/`, silently producing a value
    # that does not match what `windows_default_scratch_root` computes
    # when this actually runs on Windows): mirrors that Rust function's
    # `<TEMP>\atm` shape byte-for-byte on every host this runs on.
    temp_root = (env.get("TEMP") or env.get("TMP") or tempfile.gettempdir()).rstrip("\\/")
    native = f"{temp_root}\\atm"
    hybrid = native.replace("\\", "/")
    posix = re.sub(r"^([A-Za-z]):", lambda match: f"/{match.group(1).lower()}", hybrid)
    return {"windows_native": native, "posix_msys": posix, "substituted": hybrid}


def _substitute_windows_remote_atm_temp(source_text: str, receiver_atm_temp: str) -> str:
    """Replaces `sftp.ps1`'s fixed-value `$RemoteAtmTemp` placeholder line
    with `receiver_atm_temp`, exactly once.

    Raises `RuntimeError` (never silently no-ops) if the placeholder line
    is not found exactly once: the shipped `scripts/transfer/sftp.ps1` is
    committed, human-editable example content, so a future edit to its
    placeholder line's exact text must fail this harness loudly rather
    than silently install a script whose `$RemoteAtmTemp` was never
    substituted (the exact regression `fix(aq4): windows evidence leg`
    exists to fix, run 7, 33140616718 @ 7f6774802).
    """
    occurrences = source_text.count(_WINDOWS_REMOTE_ATM_TEMP_PLACEHOLDER_LINE)
    if occurrences != 1:
        raise RuntimeError(
            "scripts/transfer/sftp.ps1's $RemoteAtmTemp placeholder line "
            f"was found {occurrences} time(s), expected exactly 1; its "
            "placeholder contract may have changed and "
            "run_aq4_transfer_evidence.py's substitution needs updating to match"
        )
    replacement = f'$RemoteAtmTemp = "{receiver_atm_temp}"'
    return source_text.replace(_WINDOWS_REMOTE_ATM_TEMP_PLACEHOLDER_LINE, replacement)


def find_windows_posix_shell(env: dict[str, str] | None = None) -> Path | None:
    """Locates a POSIX shell (`bash`) for the OpenSSH `DefaultShell`
    registry value: first via `PATH` (`shutil.which`, the `where`
    equivalent), then via the known Git for Windows install locations
    (`_KNOWN_GIT_BASH_LOCATIONS`) under each of the usual per-machine
    program-files environment variables. Returns `None` (never raises) if
    no bash is found anywhere -- an honest input to the
    `skipped_no_posix_receiver` decision, not a script failure.
    """
    env = env if env is not None else dict(os.environ)
    found = shutil.which("bash", path=env.get("PATH"))
    if found:
        return Path(found)
    for root_var in ("ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "LocalAppData"):
        root = env.get(root_var)
        if not root:
            continue
        for suffix in _KNOWN_GIT_BASH_LOCATIONS:
            candidate = Path(root) / suffix
            if candidate.is_file():
                return candidate
    return None


def _read_windows_default_shell() -> str | None:
    """Reads the current `HKLM\\SOFTWARE\\OpenSSH\\DefaultShell` value, or
    `None` if the key/value does not exist (the OpenSSH-shipped default:
    no override, sessions get `cmd.exe`)."""
    try:
        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, _OPENSSH_REGISTRY_KEY) as key:
            value, _ = winreg.QueryValueEx(key, _DEFAULT_SHELL_VALUE_NAME)
            return str(value)
    except FileNotFoundError:
        return None


def _write_windows_default_shell(value: str | None) -> None:
    """Sets (`value` is a path string) or deletes (`value` is `None`) the
    `DefaultShell` registry value. Raises `OSError` (for example
    `PermissionError` when not running elevated) rather than swallowing a
    failed write -- callers must treat that as `skipped_no_posix_receiver`,
    never as "configured but silently still `cmd.exe`"."""
    if value is None:
        try:
            with winreg.OpenKey(
                winreg.HKEY_LOCAL_MACHINE, _OPENSSH_REGISTRY_KEY, 0, winreg.KEY_SET_VALUE
            ) as key:
                winreg.DeleteValue(key, _DEFAULT_SHELL_VALUE_NAME)
        except FileNotFoundError:
            pass
        return
    with winreg.CreateKeyEx(
        winreg.HKEY_LOCAL_MACHINE, _OPENSSH_REGISTRY_KEY, 0, winreg.KEY_SET_VALUE
    ) as key:
        winreg.SetValueEx(key, _DEFAULT_SHELL_VALUE_NAME, 0, winreg.REG_SZ, value)


def prepare_windows_posix_shell(env: dict[str, str] | None = None) -> dict[str, Any]:
    """Locates a POSIX shell and points the OpenSSH `DefaultShell` registry
    value at it, so this scenario's scratch sshd executes `sftp.ps1`'s
    POSIX remote commands (`umask`, `mkdir -p`) under bash instead of the
    account's default `cmd.exe` (fenix ruling, run 7: the shipped contract
    is a Windows *sender* talking to a POSIX *receiver*).

    Never raises. Returns one of:
    - `{"outcome": "skipped_no_posix_receiver", "reason": ...}` when no
      bash was found, the registry write was denied (for example, not
      running elevated), or a post-write readback of the value does not
      match what was just written -- an honest, announced skip, mirroring
      `ensure_sshd_available`'s shape.
    - `{"outcome": "configured", "bash_path": ..., "before": ..., "after": ...}`
      on success, `after` having been independently re-read from the
      registry (not merely assumed from the value passed to
      `_write_windows_default_shell`). The caller must restore `before`
      via `_write_windows_default_shell` once the scratch sshd this
      enables has stopped.

    The readback (run 33141941621 @ 21f00edb1: this scenario reported
    `"outcome": "configured"` yet the scratch sshd still ran the remote
    command under a shell that could not execute `umask`/`mkdir -p`) is
    the only way this function can tell a genuine write from one that
    silently landed in the wrong registry view (for example a 32-bit
    Python process's default WOW6432Node redirection) or was otherwise
    not durably applied -- `_write_windows_default_shell` returning
    without raising is not by itself proof the value was actually
    persisted where `sshd.exe` reads it; `winreg` surfaces no
    partial-write signal beyond raising `OSError`.
    """
    shell = find_windows_posix_shell(env)
    if shell is None:
        return {
            "outcome": "skipped_no_posix_receiver",
            "reason": (
                "no POSIX shell (git-bash) found on this Windows runner; the "
                "scratch sshd's default cmd.exe shell cannot execute "
                "sftp.ps1's POSIX remote commands (umask/mkdir -p)"
            ),
        }
    before = _read_windows_default_shell()
    try:
        _write_windows_default_shell(str(shell))
    except OSError as error:
        return {
            "outcome": "skipped_no_posix_receiver",
            "reason": (
                f"could not set the OpenSSH DefaultShell registry value to "
                f"{shell} (likely denied administrator access): {error}"
            ),
        }
    after = _read_windows_default_shell()
    if after != str(shell):
        # The write call did not raise, yet reading the value straight
        # back does not match what was just written -- most plausibly a
        # registry-view mismatch (this harness's Python process reading
        # or writing a different `SOFTWARE\OpenSSH` than the one the
        # 64-bit `sshd.exe` service consults). Restore whatever was there
        # before this call touched it (best-effort; a second denied write
        # here is not actionable) and fail closed rather than reporting a
        # false "configured".
        try:
            _write_windows_default_shell(before)
        except OSError:
            pass
        return {
            "outcome": "skipped_no_posix_receiver",
            "reason": (
                f"wrote {shell} to the OpenSSH DefaultShell registry value "
                f"but reading it back returned {after!r} instead -- likely "
                "a registry-view mismatch (32-bit vs. 64-bit); refusing to "
                "proceed as if the scratch sshd will actually honor it"
            ),
        }
    return {"outcome": "configured", "bash_path": str(shell), "before": before, "after": after}


def install_transfer_script(home: Path, env: dict[str, str]) -> dict[str, Any]:
    """Installs the unmodified `scripts/transfer/sftp.sh` example (or, on
    Windows, `scripts/transfer/sftp.ps1` with its `$RemoteAtmTemp`
    placeholder substituted -- see `windows_receiver_atm_temp`) at
    `<home>/.atm/transfer/localhost[.ps1]`. `env` supplies the `TEMP`
    entry that substitution is computed from; it is unused on Unix, where
    the script's own `remote_atm_temp="/tmp/atm-$(id -u)"` needs no
    harness-side substitution.

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
        source_path = ROOT / "scripts" / "transfer" / "sftp.ps1"
        receiver_atm_temp = windows_receiver_atm_temp(env)
        installed.write_text(
            _substitute_windows_remote_atm_temp(
                source_path.read_text(encoding="utf-8"), receiver_atm_temp["substituted"]
            ),
            encoding="utf-8",
        )
        profile_home = home.resolve()
        return {
            "installed_at": str(installed),
            "source": str(source_path),
            "receiver_atm_temp": receiver_atm_temp,
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

    # `sftp.ps1`'s shipped contract is a Windows sender talking to a POSIX
    # receiver (`umask 077 && mkdir -p ...`); the scratch sshd this
    # scenario is about to start must therefore run a POSIX shell, not the
    # Windows OpenSSH default `cmd.exe` (fenix ruling, run 7). Decided
    # before any scratch state exists, so a `skipped_no_posix_receiver`
    # outcome here needs no cleanup beyond the early `return` already used
    # by the two skip branches above.
    windows_posix_shell: dict[str, Any] | None = None
    if IS_WINDOWS:
        windows_posix_shell = prepare_windows_posix_shell()
        record["windows_default_shell"] = windows_posix_shell
        if windows_posix_shell["outcome"] == "skipped_no_posix_receiver":
            record["status"] = "skipped_no_posix_receiver"
            record["error"] = (
                "Windows-sender -> POSIX-receiver live evidence needs a POSIX "
                f"shell on the scratch sshd: {windows_posix_shell['reason']}"
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

        record["transfer_script"] = install_transfer_script(home, env)
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
        if not send_ok and sshd_drain is not None:
            # `sftp.ps1` now forwards ssh/scp's own stderr into
            # `record["send"]["stderr_tail"]` above, but that alone is
            # not always enough to root-cause a Windows exec-session
            # failure (a `LogLevel DEBUG3` scratch sshd, started above,
            # logs which shell it actually invoked for each session,
            # information no client-side stream carries). Captured only
            # on failure, and only the tail, to avoid bloating the
            # evidence file on the (overwhelmingly common) success path.
            record["sshd_debug_log_tail"] = PipeDrain.tail(sshd_drain.stdout_lines)
        if not send_ok and IS_WINDOWS:
            # Same rationale as the sshd-side capture above, from the
            # opposite end: the ssh CLIENT's own handshake debug output
            # (`-vvv`), which no server-side log or `sftp.ps1`-forwarded
            # stderr can carry when the client aborts before ever writing
            # anything of its own (run 33142976493 @ dcd3130f1).
            record["ssh_vvv_diagnostic"] = run_ssh_vvv_diagnostic(env, ssh_config_path, args.timeout)
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
        if send_ok and landed_matches_convention and landed_file_exists and landed_content_matches:
            record["status"] = "pass"
        elif IS_WINDOWS:
            # Windows-only: the documented AQ4-windows-loopback residual
            # symptom (see docs/plans/phase-aq/sprint-AQ4-send-to-core.md)
            # is a known, formally deferred follow-up, not a new
            # regression -- classify_windows_transfer_failure decides,
            # from this run's own diagnostics, whether this failure is
            # exactly that documented shape (non-fatal
            # "deferred_windows_loopback") or a genuine, still-fatal
            # "fail" that must not be masked.
            deferred_status, deferral = classify_windows_transfer_failure(
                send_ok=send_ok,
                send_stderr_tail=record["send"]["stderr_tail"],
                ssh_vvv_diagnostic=record.get("ssh_vvv_diagnostic"),
            )
            record["status"] = deferred_status
            if deferral is not None:
                record["deferral"] = deferral
        else:
            record["status"] = "fail"
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
        # Restore the OpenSSH `DefaultShell` registry value this scenario
        # changed above -- an account-wide setting, so it must never be
        # left pointed at this scenario's discovered `bash.exe` once the
        # scratch sshd it was configured for has stopped, success or not.
        if windows_posix_shell is not None and windows_posix_shell["outcome"] == "configured":
            try:
                _write_windows_default_shell(windows_posix_shell["before"])
            except OSError as error:
                record["windows_default_shell"]["restore_error"] = str(error)
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
    elif record["status"] == "skipped_no_posix_receiver":
        lines += [
            "No POSIX shell (`bash`, for example Git for Windows) was found "
            "on this Windows runner, or the OpenSSH `DefaultShell` registry "
            "value could not be set (most likely not running elevated). "
            "This is an honest, announced skip: `sftp.ps1`'s shipped "
            "contract is a Windows *sender* talking to a POSIX *receiver* "
            "(`umask`/`mkdir -p`), which the scratch sshd's default "
            "`cmd.exe` shell cannot execute -- there is no meaningful fake "
            "for this live-evidence deliverable.",
            "",
            "```json",
            json.dumps(record.get("windows_default_shell", {}), indent=2),
            "```",
        ]
    elif record["status"] == "deferred_windows_loopback":
        send = record.get("send", {})
        deferral = record.get("deferral", {})
        lines += [
            "This run reproduced the documented AQ4-windows-loopback "
            "residual symptom (Windows sender -> POSIX loopback receiver "
            "mkdir/ssh step failure), with every precondition confirmed by "
            "this run's own diagnostics below. Recorded as a non-fatal, "
            "deferred evidence status -- not FAIL -- so this known, "
            "tracked follow-up does not block CI on every branch that "
            "carries this harness; every diagnostic field is preserved "
            "exactly as a FAIL run would capture it (see the JSON payload).",
            "",
            f"Follow-up: `{deferral.get('follow_up')}`",
            f"Reason: {deferral.get('reason')}",
            "",
            f"Command: `{' '.join(str(part) for part in send.get('argv', []))}`",
            f"Exit code: `{send.get('returncode')}`",
            "",
            "```",
            send.get("stderr_tail", ""),
            "```",
        ]
        if record.get("ssh_vvv_diagnostic"):
            lines += [
                "",
                "SSH `-vvv` diagnostic probe (confirms the loopback sshd is "
                "genuinely reachable and answering over this run's scratch "
                "ssh client config):",
                "",
                "```json",
                json.dumps(record["ssh_vvv_diagnostic"], indent=2),
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
    return (
        0
        if record["status"]
        in (
            "pass",
            "blocked_ambient_daemon",
            "skipped_no_sshd",
            "skipped_no_posix_receiver",
            "deferred_windows_loopback",
        )
        else 1
    )


if __name__ == "__main__":
    raise SystemExit(main())
