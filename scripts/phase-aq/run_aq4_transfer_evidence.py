#!/usr/bin/env python3
"""Run the AQ4 cross-host file-transfer live evidence scenario.

Drives the real, committed `scripts/transfer/sftp.sh` example (unmodified,
exactly as an operator would install it) through a real `atm send --attach`
invocation against a real loopback `sshd` this script starts on a scratch
port, proving end to end that a file attached on the sending side lands
under the receiving side's `$ATM_TEMP` staging convention
(`send_to_staging_dir`) via nothing but genuine SSH/SCP I/O -- no daemon
transport, no envelope change (ADR-055 decisions (c)/(d)).

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

Why this script writes into the real (not scratch) `$HOME/.ssh/config`:
`invoke_transfer_script` (`crates/atm/src/commands/send_to.rs`) calls
`Command::env_clear()` before inserting only `ATM_TEMP`, `ATM_IDENTITY`,
`ATM_TEAM` (ADR-055 decision (c)'s allow-list) -- `HOME` is deliberately not
forwarded to the spawned `ssh`/`scp` child, so those processes resolve their
own home directory (and therefore `~/.ssh/config`) via the OS account,
never via any `$HOME` this script sets for the outer `atm` CLI process. The
addition is backed up and restored; see `_install_ssh_client_config`.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
TEAM = "aq4-transfer-evidence"
SENDER = "aq4-sender"
RECEIVER = "aq4-receiver"
TRANSFER_HOST = "localhost"
READY_TIMEOUT_SECONDS = 15.0
SSHD_READY_TIMEOUT_SECONDS = 10.0
ATTACHMENT_FILE_NAME = "aq4-report.pdf"
ATTACHMENT_BODY = b"%PDF-1.4\naq4 live evidence attachment\n"
MESSAGE_TEXT = "AQ4 live transfer evidence: see attached file"


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
    `skipped_no_sshd` evidence outcome, not a script failure."""
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
    # executable -- record that honestly rather than guessing why.
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


def start_sshd(sshd_bin: Path, config_path: Path, port: int) -> dict[str, Any]:
    process = subprocess.Popen(
        [str(sshd_bin), "-f", str(config_path), "-D", "-e"],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
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
    log_tail = ""
    if process.poll() is not None and process.stdout is not None:
        log_tail = process.stdout.read()
    return {"process": process, "ready": ready, "pid": process.pid, "log_tail": log_tail}


def stop_sshd(process: subprocess.Popen[str] | None) -> None:
    if process is None or process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


class SshClientConfigOverride:
    """Temporarily replaces the real OS account's `~/.ssh/config` with one
    stanza routing the literal hostname `localhost` at this script's
    loopback `sshd`, restoring whatever was there (or its absence)
    afterward. See this module's docstring for why the real home directory
    -- not a scratch one -- is unavoidable here."""

    def __init__(self, port: int, identity: Path) -> None:
        self._port = port
        self._identity = identity
        self._ssh_dir = Path.home() / ".ssh"
        self._config_path = self._ssh_dir / "config"
        self._backup_path: Path | None = None
        self._had_config = False

    def __enter__(self) -> "SshClientConfigOverride":
        self._ssh_dir.mkdir(mode=0o700, exist_ok=True)
        self._had_config = self._config_path.is_file()
        if self._had_config:
            self._backup_path = self._ssh_dir / "config.aq4-evidence-backup"
            shutil.move(str(self._config_path), str(self._backup_path))
        stanza = "\n".join(
            [
                f"Host {TRANSFER_HOST}",
                "    Hostname 127.0.0.1",
                f"    Port {self._port}",
                f"    User {os.environ.get('USER') or os.environ.get('USERNAME') or ''}",
                f"    IdentityFile {self._identity}",
                "    IdentitiesOnly yes",
                "    StrictHostKeyChecking no",
                "    UserKnownHostsFile /dev/null",
                "    PasswordAuthentication no",
                "",
            ]
        )
        self._config_path.write_text(stanza, encoding="utf-8")
        self._config_path.chmod(0o600)
        return self

    def __exit__(self, *_exc: object) -> None:
        self._config_path.unlink(missing_ok=True)
        if self._had_config and self._backup_path is not None:
            shutil.move(str(self._backup_path), str(self._config_path))


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


def run_cli(atm: Path, env: dict[str, str], args: list[str], *, identity: str, timeout: float) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(atm), *args],
        cwd=ROOT,
        env={**env, "ATM_IDENTITY": identity},
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
    )


def add_roster_member(atm: Path, env: dict[str, str], home: Path, member: str, timeout: float) -> dict[str, Any]:
    completed = run_cli(
        atm,
        env,
        ["teams", "add-member", TEAM, member, "--home-dir", str(home), "--json"],
        identity=SENDER,
        timeout=timeout,
    )
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
    deadline = time.monotonic() + timeout
    lines: list[str] = []
    ready = False
    while time.monotonic() < deadline:
        if process.poll() is not None:
            break
        line = process.stdout.readline() if process.stdout is not None else ""
        if line:
            lines.append(line.rstrip("\n"))
            if line.strip() == "ATM_DAEMON_READY":
                ready = True
                break
    stderr_tail = ""
    if process.poll() is not None:
        stderr_tail = (process.stderr.read() if process.stderr is not None else "").strip()
    return {
        "process": process,
        "ready": ready,
        "pid": process.pid,
        "stdout_tail": lines,
        "exited_early": process.poll() is not None,
        "returncode": process.returncode,
        "stderr_tail": stderr_tail,
    }


def stop_daemon(process: subprocess.Popen[str] | None) -> None:
    if process is None or process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


def extract_landed_path(peek_stdout: str, file_name: str) -> str | None:
    pattern = re.compile(r"Attached files \(on this host\):\\?n?- (\S*" + re.escape(file_name) + r")")
    match = pattern.search(peek_stdout)
    return match.group(1) if match else None


def install_transfer_script(home: Path) -> Path:
    transfer_dir = home / ".atm" / "transfer"
    transfer_dir.mkdir(parents=True, exist_ok=True)
    installed = transfer_dir / TRANSFER_HOST
    shutil.copyfile(ROOT / "scripts" / "transfer" / "sftp.sh", installed)
    installed.chmod(0o700)
    return installed


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
    with tempfile.TemporaryDirectory(prefix="aq4-evidence-") as directory:
        root = Path(directory)
        env = fixture_environment(root)
        home = Path(env["HOME"])
        sshd_root = root / "sshd"
        sshd_root.mkdir()
        daemon_process: subprocess.Popen[str] | None = None
        sshd_process: subprocess.Popen[str] | None = None
        remote_landing_dir: Path | None = None
        try:
            keys = generate_ssh_keys(sshd_root)
            config_path = write_sshd_config(sshd_root, port, keys)
            sshd = start_sshd(sshd_bin, config_path, port)
            sshd_process = sshd.pop("process")
            record["sshd_start"] = {**sshd, "port": port}
            if not sshd["ready"]:
                record["status"] = "blocked_sshd_start_failed"
                record["error"] = sshd.get("log_tail") or "sshd did not open its loopback port"
                return record

            with SshClientConfigOverride(port, keys["identity"]):
                record["roster"] = {
                    "sender": add_roster_member(args.atm, env, home, SENDER, args.timeout),
                    "receiver": add_roster_member(args.atm, env, home, RECEIVER, args.timeout),
                }

                installed_script = install_transfer_script(home)
                record["transfer_script"] = {
                    "installed_at": str(installed_script),
                    "source": str(ROOT / "scripts" / "transfer" / "sftp.sh"),
                }

                daemon = start_daemon(args.daemon, env, args.timeout)
                daemon_process = daemon.pop("process")
                record["daemon_start"] = daemon
                if not daemon["ready"]:
                    record["status"] = "blocked_daemon_start_failed"
                    record["error"] = daemon.get("stderr_tail") or "daemon did not report ready"
                    return record

                attach_source_dir = root / "attach-source"
                attach_source_dir.mkdir()
                attach_path = attach_source_dir / ATTACHMENT_FILE_NAME
                attach_path.write_bytes(ATTACHMENT_BODY)

                send_completed = run_cli(
                    args.atm,
                    env,
                    ["send", f"{RECEIVER}@{TEAM}", MESSAGE_TEXT, "--host", TRANSFER_HOST, "--attach", str(attach_path)],
                    identity=SENDER,
                    timeout=args.timeout,
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
            stop_daemon(daemon_process)
            stop_sshd(sshd_process)
            if remote_landing_dir is not None and remote_landing_dir.exists():
                shutil.rmtree(remote_landing_dir, ignore_errors=True)
            record["finished_at_ns"] = time.time_ns()
    return record


def write_evidence(args: argparse.Namespace, record: dict[str, Any]) -> tuple[Path, Path]:
    evidence_dir = args.evidence_dir or ROOT / "docs" / "plans" / "phase-aq" / "evidence" / "AQ4"
    evidence_dir.mkdir(parents=True, exist_ok=True)
    json_path = evidence_dir / f"transfer-{args.host}.json"
    markdown_path = evidence_dir / f"transfer-{args.host}.md"
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
    markdown_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return json_path, markdown_path


def main() -> int:
    args = parse_args()
    if args.timeout <= 0:
        raise SystemExit("--timeout must be positive")
    if not args.daemon.is_file():
        raise SystemExit(f"owned daemon binary does not exist: {args.daemon}")
    if not args.atm.is_file():
        raise SystemExit(f"matched atm binary does not exist: {args.atm}")
    record = run_scenario(args)
    json_path, markdown_path = write_evidence(args, record)
    print(f"{record['status'].upper()} AQ4 transfer evidence: {json_path}")
    print(f"transcript: {markdown_path}")
    return 0 if record["status"] in ("pass", "blocked_ambient_daemon", "skipped_no_sshd") else 1


if __name__ == "__main__":
    raise SystemExit(main())
