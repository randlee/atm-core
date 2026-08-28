"""Contract tests for the AQ4 example transfer scripts (`scripts/transfer/`).

`sftp.sh`, `tailscale.sh`, and `sftp.ps1` are user-modifiable examples, not
production code, but ADR-055 decision (c) fixes their invocation contract
(argv-array exec of `<script> <host> <transfer-id> <file>...`, restricted
child environment, exactly-one-line stdout on success). This module proves
each shipped example honors that contract without any real SSH
configuration (R6): it puts fake `ssh`/`scp` executables -- the only two
external binaries any of these scripts actually invoke; none of them shells
out to a literal `sftp` or `tailscale` binary -- on `PATH` that record every
invocation and can be told to fail in controlled ways, so the assertions
below never depend on network access or a real remote host.
"""

from __future__ import annotations

import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SFTP_SH = ROOT / "scripts" / "transfer" / "sftp.sh"
TAILSCALE_SH = ROOT / "scripts" / "transfer" / "tailscale.sh"
SFTP_PS1 = ROOT / "scripts" / "transfer" / "sftp.ps1"

PWSH = shutil.which("pwsh")

FAKE_SSH = textwrap.dedent(
    """\
    #!/usr/bin/env python3
    # Fake `ssh` for AQ4 transfer-script contract tests: records its argv,
    # then either runs the one remote command shape sftp.sh/tailscale.sh/
    # sftp.ps1 actually send ("umask 077 && mkdir -p '<dir>'") against a
    # real local directory, or fails in a way controlled by env vars, so
    # the tests below never touch a real network or remote host.
    import os
    import pathlib
    import sys

    from fake_transfer_lib import log_invocation, resolve_within_profile, strip_ssh_config_flag

    def main() -> int:
        argv = sys.argv[1:]
        log_invocation("ssh", argv)
        positional = strip_ssh_config_flag(argv)
        if len(positional) < 1:
            print("fake ssh: missing host argument", file=sys.stderr)
            return 2
        host = positional[0]
        command = positional[1] if len(positional) > 1 else ""

        allowed = os.environ.get("FAKE_SSH_ALLOWED_HOSTS")
        if allowed is not None and host not in allowed.split(","):
            print(f"ssh: could not resolve hostname {host}: nodename nor servname provided", file=sys.stderr)
            return 6

        forced = os.environ.get("FAKE_SSH_EXIT_CODE")
        if forced is not None:
            print("fake ssh: forced failure for test", file=sys.stderr)
            return int(forced)

        marker = "mkdir -p '"
        if marker in command:
            target = command.split(marker, 1)[1].rsplit("'", 1)[0]
            resolved = resolve_within_profile(target)
            if resolved is None:
                print(f"ssh: refusing to create {target} outside the receiver profile", file=sys.stderr)
                return 13
            resolved.mkdir(parents=True, exist_ok=True)
        return 0

    if __name__ == "__main__":
        sys.exit(main())
    """
)

FAKE_SCP = textwrap.dedent(
    """\
    #!/usr/bin/env python3
    # Fake `scp` for AQ4 transfer-script contract tests. See fake_ssh's
    # module docstring for the rationale.
    import os
    import pathlib
    import shutil
    import sys

    from fake_transfer_lib import log_invocation, resolve_within_profile, strip_ssh_config_flag

    def main() -> int:
        argv = sys.argv[1:]
        log_invocation("scp", argv)

        forced = os.environ.get("FAKE_SCP_EXIT_CODE")
        if forced is not None:
            print("fake scp: forced failure for test", file=sys.stderr)
            return int(forced)

        remaining = strip_ssh_config_flag(argv)
        positional = [arg for arg in remaining if not arg.startswith("-")]
        if len(positional) != 2:
            print(f"fake scp: expected exactly one source and one destination, got {positional}", file=sys.stderr)
            return 1
        source, destination = positional
        if ":" not in destination:
            print(f"fake scp: destination {destination!r} is missing a host: prefix", file=sys.stderr)
            return 1
        _host, remote_path = destination.split(":", 1)
        remote_path = remote_path.rstrip("/")

        resolved = resolve_within_profile(remote_path)
        if resolved is None:
            print(f"scp: refusing to write outside the receiver profile: {remote_path}", file=sys.stderr)
            return 13
        resolved.mkdir(parents=True, exist_ok=True)
        shutil.copy(source, resolved / pathlib.Path(source).name)
        return 0

    if __name__ == "__main__":
        sys.exit(main())
    """
)

FAKE_TRANSFER_LIB = textwrap.dedent(
    """\
    # Shared helpers for the fake ssh/scp used by AQ4 transfer-script tests.
    import json
    import os
    import pathlib


    def log_invocation(binary: str, argv: list[str]) -> None:
        log_path = os.environ.get("FAKE_LOG")
        if not log_path:
            return
        with open(log_path, "a", encoding="utf-8") as handle:
            handle.write(json.dumps({"bin": binary, "argv": argv}) + "\\n")


    def resolve_within_profile(raw_path: str):
        \"\"\"Resolve `raw_path`, refusing (returns None) any path that would
        land outside `FAKE_PROFILE_ROOT` when that containment env var is
        set. When it is unset, every path is accepted (the default,
        permissive shape most contract tests use).\"\"\"
        resolved = pathlib.Path(raw_path).resolve()
        profile_root = os.environ.get("FAKE_PROFILE_ROOT")
        if profile_root is None:
            return resolved
        root_resolved = pathlib.Path(profile_root).resolve()
        try:
            resolved.relative_to(root_resolved)
        except ValueError:
            return None
        return resolved


    def strip_ssh_config_flag(argv: list[str]) -> list[str]:
        \"\"\"Strips a leading `-F <path>` pair (QA-2 B6's
        ATM_TRANSFER_SSH_CONFIG passthrough) before host/positional
        derivation, exactly like real OpenSSH's own flag parsing -- so
        contract tests can assert on the pair's presence in the logged
        argv while the rest of this fake's logic still finds the real
        host/command/source/destination arguments regardless of whether
        it is there.\"\"\"
        if len(argv) >= 2 and argv[0] == "-F":
            return argv[2:]
        return argv
    """
)


def _install_fake_bin(bin_dir: Path) -> None:
    """Writes the fake `ssh`/`scp` (and their shared helper module) into
    `bin_dir`, marking the two executables owner-executable."""
    bin_dir.mkdir(parents=True, exist_ok=True)
    (bin_dir / "fake_transfer_lib.py").write_text(FAKE_TRANSFER_LIB, encoding="utf-8")
    for name, body in (("ssh", FAKE_SSH), ("scp", FAKE_SCP)):
        script_path = bin_dir / name
        script_path.write_text(body, encoding="utf-8")
        mode = script_path.stat().st_mode
        script_path.chmod(mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def _read_log(log_path: Path) -> list[dict]:
    if not log_path.exists():
        return []
    return [json.loads(line) for line in log_path.read_text(encoding="utf-8").splitlines() if line.strip()]


class _TransferScriptContractTestsMixin:
    """Shared contract assertions for `sftp.sh` and `tailscale.sh`. Both
    scripts invoke `ssh`/`scp` identically (Tailscale's contribution is
    reachability of `<host>`, not a different transport), so the same
    fake-binary harness and assertions cover both verbatim."""

    SCRIPT: Path

    def _invoke(self, args, env, cwd):
        return subprocess.run(
            [str(self.SCRIPT), *args],
            env=env,
            cwd=cwd,
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
        )

    def _fixture(self, tmp: Path, *, with_fake_bin: bool = True, profile_root: Path | None = None):
        bin_dir = tmp / "bin"
        log_path = tmp / "invocations.jsonl"
        if with_fake_bin:
            _install_fake_bin(bin_dir)
        env = {
            "PATH": (str(bin_dir) + os.pathsep + os.environ.get("PATH", "")) if with_fake_bin else "",
            "FAKE_LOG": str(log_path),
            # Ambient ATM child-environment allow-list variables (ADR-055
            # decision (c)): present in a real invocation, harmless here
            # since these scripts never read them directly.
            "ATM_TEMP": str(tmp / "atm-temp"),
            "ATM_IDENTITY": "test-agent",
            "ATM_TEAM": "test-team",
        }
        if profile_root is not None:
            env["FAKE_PROFILE_ROOT"] = str(profile_root)
        return bin_dir, log_path, env

    def test_happy_path_exact_argv_and_single_line_landed_dir(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = Path(raw_tmp)
            _bin_dir, log_path, env = self._fixture(tmp)
            attach_a = tmp / "report.pdf"
            attach_a.write_bytes(b"pdf-bytes")
            attach_b = tmp / "notes.txt"
            attach_b.write_text("notes", encoding="utf-8")

            result = self._invoke(["m5", "01J00000000000000000000042", str(attach_a), str(attach_b)], env, str(tmp))

            self.assertEqual(result.returncode, 0, result.stderr)
            stdout_lines = result.stdout.splitlines()
            self.assertEqual(len(stdout_lines), 1, f"expected exactly one stdout line, got {result.stdout!r}")
            landed_dir = stdout_lines[0]
            self.assertTrue(Path(landed_dir).is_absolute(), landed_dir)
            self.assertTrue(landed_dir.endswith("send-to/01J00000000000000000000042"), landed_dir)

            invocations = _read_log(log_path)
            self.assertEqual(len(invocations), 3, invocations)
            ssh_call, scp_calls = invocations[0], invocations[1:]
            self.assertEqual(ssh_call["bin"], "ssh")
            self.assertEqual(ssh_call["argv"][0], "m5")
            self.assertIn(f"mkdir -p '{landed_dir}'", ssh_call["argv"][1])
            self.assertEqual([call["bin"] for call in scp_calls], ["scp", "scp"])
            self.assertEqual(scp_calls[0]["argv"], ["-q", str(attach_a), f"m5:{landed_dir}/"])
            self.assertEqual(scp_calls[1]["argv"], ["-q", str(attach_b), f"m5:{landed_dir}/"])

    def test_missing_binary_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = Path(raw_tmp)
            _bin_dir, log_path, env = self._fixture(tmp, with_fake_bin=False)
            attach = tmp / "report.pdf"
            attach.write_bytes(b"pdf-bytes")

            result = self._invoke(["m5", "01J00000000000000000000043", str(attach)], env, str(tmp))

            self.assertNotEqual(result.returncode, 0, result.stdout)
            self.assertEqual(_read_log(log_path), [])

    def test_unreachable_host_fails_closed_before_any_copy(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = Path(raw_tmp)
            _bin_dir, log_path, env = self._fixture(tmp)
            env["FAKE_SSH_ALLOWED_HOSTS"] = "some-other-host"
            attach = tmp / "report.pdf"
            attach.write_bytes(b"pdf-bytes")

            result = self._invoke(["m5", "01J00000000000000000000044", str(attach)], env, str(tmp))

            self.assertNotEqual(result.returncode, 0, result.stdout)
            invocations = _read_log(log_path)
            self.assertEqual([call["bin"] for call in invocations], ["ssh"], invocations)

    def test_ssh_config_override_is_passed_through_when_set(self) -> None:
        # QA-2 B6: an opt-in fourth allow-list entry, ATM_TRANSFER_SSH_CONFIG,
        # unset by every ordinary install (the fixture in `_fixture` never
        # sets it, and `test_happy_path_exact_argv_and_single_line_landed_dir`
        # above already proves ssh/scp's argv is unaffected when it is
        # absent). When a caller *does* set it -- only
        # `scripts/phase-aq/run_aq4_transfer_evidence.py` does, to avoid
        # touching the real `~/.ssh/config` -- both ssh and scp must receive
        # it as `-F <path>`.
        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = Path(raw_tmp)
            _bin_dir, log_path, env = self._fixture(tmp)
            scratch_config = tmp / "scratch_ssh_config"
            scratch_config.write_text("Host m5\n    Hostname 127.0.0.1\n", encoding="utf-8")
            env["ATM_TRANSFER_SSH_CONFIG"] = str(scratch_config)
            attach = tmp / "report.pdf"
            attach.write_bytes(b"pdf-bytes")

            result = self._invoke(["m5", "01J00000000000000000000f01", str(attach)], env, str(tmp))

            self.assertEqual(result.returncode, 0, result.stderr)
            invocations = _read_log(log_path)
            ssh_call, scp_call = invocations[0], invocations[1]
            self.assertEqual(ssh_call["argv"][:2], ["-F", str(scratch_config)])
            self.assertEqual(scp_call["argv"][:2], ["-F", str(scratch_config)])

    def test_remote_mkdir_failure_fails_closed_before_any_copy(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = Path(raw_tmp)
            _bin_dir, log_path, env = self._fixture(tmp)
            env["FAKE_SSH_EXIT_CODE"] = "1"
            attach = tmp / "report.pdf"
            attach.write_bytes(b"pdf-bytes")

            result = self._invoke(["m5", "01J00000000000000000000045", str(attach)], env, str(tmp))

            self.assertNotEqual(result.returncode, 0, result.stdout)
            invocations = _read_log(log_path)
            self.assertEqual([call["bin"] for call in invocations], ["ssh"], invocations)

    def test_copy_failure_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = Path(raw_tmp)
            _bin_dir, log_path, env = self._fixture(tmp)
            env["FAKE_SCP_EXIT_CODE"] = "1"
            attach = tmp / "report.pdf"
            attach.write_bytes(b"pdf-bytes")

            result = self._invoke(["m5", "01J00000000000000000000046", str(attach)], env, str(tmp))

            self.assertNotEqual(result.returncode, 0, result.stdout)
            invocations = _read_log(log_path)
            self.assertEqual([call["bin"] for call in invocations], ["ssh", "scp"], invocations)

    def test_landed_path_escaping_the_receiver_profile_fails_closed(self) -> None:
        """Defense-in-depth containment: even though the real caller only
        ever supplies a caller-generated ULID `transfer-id` (never user
        input, never containing `/`, `\\`, or `..`), this proves that if a
        resolved remote landing path ever did escape a receiver's
        designated profile root, the receiving side refuses it and the
        script's `set -eu` propagates that refusal as a whole-invocation
        failure rather than silently landing files outside the profile."""
        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = Path(raw_tmp)
            profile_root = tmp / "receiver-profile"
            profile_root.mkdir()
            _bin_dir, log_path, env = self._fixture(tmp, profile_root=profile_root)
            attach = tmp / "report.pdf"
            attach.write_bytes(b"pdf-bytes")

            escaping_transfer_id = "../../../../etc/evil"
            result = self._invoke(["m5", escaping_transfer_id, str(attach)], env, str(tmp))

            self.assertNotEqual(result.returncode, 0, result.stdout)
            invocations = _read_log(log_path)
            self.assertEqual([call["bin"] for call in invocations], ["ssh"], invocations)
            self.assertFalse((tmp / "etc").exists(), "must never create anything outside the receiver profile")

    def test_never_mutates_the_callers_ambient_environment(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = Path(raw_tmp)
            _bin_dir, _log_path, env = self._fixture(tmp)
            attach = tmp / "report.pdf"
            attach.write_bytes(b"pdf-bytes")

            before = dict(os.environ)
            result = self._invoke(["m5", "01J00000000000000000000047", str(attach)], env, str(tmp))
            after = dict(os.environ)

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(before, after, "invoking a transfer script must never mutate this process's own environment")


class SftpShTests(_TransferScriptContractTestsMixin, unittest.TestCase):
    SCRIPT = SFTP_SH

    def test_script_is_committed_owner_executable(self) -> None:
        self.assertTrue(SFTP_SH.exists())
        self.assertTrue(os.access(SFTP_SH, os.X_OK))


class TailscaleShTests(_TransferScriptContractTestsMixin, unittest.TestCase):
    SCRIPT = TAILSCALE_SH

    def test_script_is_committed_owner_executable(self) -> None:
        self.assertTrue(TAILSCALE_SH.exists())
        self.assertTrue(os.access(TAILSCALE_SH, os.X_OK))


@unittest.skipUnless(sys.platform == "win32" or PWSH is not None, "sftp.ps1 needs pwsh, and only ships for win32; skipping with a recorded reason")
class SftpPs1Tests(unittest.TestCase):
    """`sftp.ps1` (Windows/`pwsh` example) exercises the identical
    invocation contract as `sftp.sh`, through `pwsh`. Gated exactly like
    the rest of this repository's cross-platform script suite: it always
    runs on a `win32` CI lane (where `pwsh` -- PowerShell 7+ -- ships by
    default on `windows-latest`), and opportunistically on any other
    platform where a developer happens to have `pwsh` installed; everywhere
    else it is skipped with an explicit, non-silent reason rather than
    reporting a false pass."""

    def _invoke(self, args, env, cwd):
        assert PWSH is not None
        return subprocess.run(
            [PWSH, "-NoProfile", "-File", str(SFTP_PS1), *args],
            env=env,
            cwd=cwd,
            capture_output=True,
            text=True,
            timeout=20,
            check=False,
        )

    def _fixture(self, tmp: Path):
        bin_dir = tmp / "bin"
        log_path = tmp / "invocations.jsonl"
        _install_fake_bin(bin_dir)
        env = dict(os.environ)
        env["PATH"] = str(bin_dir) + os.pathsep + env.get("PATH", "")
        env["FAKE_LOG"] = str(log_path)
        env["ATM_TEMP"] = str(tmp / "atm-temp")
        env["ATM_IDENTITY"] = "test-agent"
        env["ATM_TEAM"] = "test-team"
        return bin_dir, log_path, env

    def test_happy_path_exact_argv_and_single_line_landed_dir(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = Path(raw_tmp)
            _bin_dir, log_path, env = self._fixture(tmp)
            attach = tmp / "report.pdf"
            attach.write_bytes(b"pdf-bytes")

            result = self._invoke(["m5", "01J00000000000000000000048", str(attach)], env, str(tmp))

            self.assertEqual(result.returncode, 0, result.stderr)
            stdout_lines = [line for line in result.stdout.splitlines() if line.strip()]
            self.assertEqual(len(stdout_lines), 1, f"expected exactly one stdout line, got {result.stdout!r}")
            landed_dir = stdout_lines[0].strip()
            self.assertTrue(landed_dir.endswith("send-to/01J00000000000000000000048"), landed_dir)

            invocations = _read_log(log_path)
            self.assertEqual([call["bin"] for call in invocations], ["ssh", "scp"], invocations)

    def test_ssh_config_override_is_passed_through_when_set(self) -> None:
        # QA-2 B6: mirrors the sh mixin's equivalent test for the pwsh
        # example -- ATM_TRANSFER_SSH_CONFIG, unset by every ordinary
        # install, is passed to both ssh and scp as -F <path>.
        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = Path(raw_tmp)
            _bin_dir, log_path, env = self._fixture(tmp)
            scratch_config = tmp / "scratch_ssh_config"
            scratch_config.write_text("Host m5\n    Hostname 127.0.0.1\n", encoding="utf-8")
            env["ATM_TRANSFER_SSH_CONFIG"] = str(scratch_config)
            attach = tmp / "report.pdf"
            attach.write_bytes(b"pdf-bytes")

            result = self._invoke(["m5", "01J00000000000000000000f02", str(attach)], env, str(tmp))

            self.assertEqual(result.returncode, 0, result.stderr)
            invocations = _read_log(log_path)
            ssh_call, scp_call = invocations[0], invocations[1]
            self.assertEqual(ssh_call["argv"][:2], ["-F", str(scratch_config)])
            self.assertEqual(scp_call["argv"][:2], ["-F", str(scratch_config)])

    def test_missing_binary_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = Path(raw_tmp)
            env = dict(os.environ)
            env["PATH"] = ""
            attach = tmp / "report.pdf"
            attach.write_bytes(b"pdf-bytes")

            result = self._invoke(["m5", "01J00000000000000000000049", str(attach)], env, str(tmp))

            self.assertNotEqual(result.returncode, 0, result.stdout)

    def test_copy_failure_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            tmp = Path(raw_tmp)
            _bin_dir, log_path, env = self._fixture(tmp)
            env["FAKE_SCP_EXIT_CODE"] = "1"
            attach = tmp / "report.pdf"
            attach.write_bytes(b"pdf-bytes")

            result = self._invoke(["m5", "01J0000000000000000000004A", str(attach)], env, str(tmp))

            self.assertNotEqual(result.returncode, 0, result.stdout)
            invocations = _read_log(log_path)
            self.assertEqual([call["bin"] for call in invocations], ["ssh", "scp"], invocations)


if __name__ == "__main__":
    unittest.main()
