"""Regression tests for the daemon-switch singleton cleanup rules."""

from __future__ import annotations

import importlib.util
import io
import argparse
import json
import os
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
import plistlib
import socket
import subprocess
import tempfile
import unittest
import zipfile
from unittest import mock


SCRIPT = Path(__file__).parents[1] / "scripts" / "daemon-switch.py"
SPEC = importlib.util.spec_from_file_location("daemon_switch", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
DAEMON_SWITCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DAEMON_SWITCH)
import release_resolution as RELEASE_RESOLUTION


POSIX_ONLY = unittest.skipUnless(
    os.name == "posix",
    "launchd/systemd/AF_UNIX backends and POSIX permission bits do not exist on Windows",
)


@POSIX_ONLY
class StaleSocketCleanupTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.home = Path(self.temporary.name)
        self.socket_path = self.home / ".atm" / "daemon" / "atm-daemon.sock"
        self.socket_path.parent.mkdir(parents=True)
        self.original_path = DAEMON_SWITCH.Path
        self.original_owners = DAEMON_SWITCH.macos_socket_owner_pids
        self.original_daemon_owners = DAEMON_SWITCH.macos_daemon_owner_pids

        class TestPath:
            @staticmethod
            def home() -> Path:
                return self.home

        DAEMON_SWITCH.Path = TestPath
        DAEMON_SWITCH.macos_socket_owner_pids = lambda: []
        DAEMON_SWITCH.macos_daemon_owner_pids = lambda: []

    def tearDown(self) -> None:
        DAEMON_SWITCH.Path = self.original_path
        DAEMON_SWITCH.macos_socket_owner_pids = self.original_owners
        DAEMON_SWITCH.macos_daemon_owner_pids = self.original_daemon_owners
        self.temporary.cleanup()

    def test_removes_unowned_unix_socket(self) -> None:
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        listener.bind(str(self.socket_path))
        listener.close()

        self.assertTrue(DAEMON_SWITCH.remove_verified_stale_macos_socket(None))

        self.assertFalse(self.socket_path.exists())

    def test_refuses_regular_file_at_daemon_socket_path(self) -> None:
        self.socket_path.write_text("not a socket", encoding="utf-8")

        with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "non-socket"):
            DAEMON_SWITCH.remove_verified_stale_macos_socket(None)

        self.assertTrue(self.socket_path.is_file())


@POSIX_ONLY
class QuiesceTests(unittest.TestCase):
    def test_requires_explicit_confirmation(self) -> None:
        with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "--yes"):
            DAEMON_SWITCH.quiesce(mock.Mock(yes=False))

    def test_stops_managed_daemon_without_mutating_selectors(self) -> None:
        args = mock.Mock(yes=True)
        cli = Path("/selected/atm")
        daemon = Path("/selected/atm-daemon")
        with (
            mock.patch.object(DAEMON_SWITCH, "selected_links", return_value=(cli, daemon)) as selected,
            mock.patch.object(DAEMON_SWITCH, "run_service") as service,
            mock.patch.object(DAEMON_SWITCH, "require_stopped_daemon") as stopped,
        ):
            DAEMON_SWITCH.quiesce(args)

        selected.assert_called_once_with(args)
        service.assert_called_once_with(args, "stop", allow_absent=True)
        stopped.assert_called_once_with(args, cli)

    def test_macos_absent_launch_agent_is_safe_before_owner_verification(self) -> None:
        args = mock.Mock(service="com.atm.daemon.crosshost-smoke", launch_agent_plist="/tmp/atm.plist")
        bootout_missing = subprocess.CompletedProcess(
            ["launchctl", "bootout"], 3, stdout="", stderr="Boot-out failed: 3: No such process"
        )
        print_absent = subprocess.CompletedProcess(
            ["launchctl", "print"], 3, stdout="", stderr="Could not find service"
        )
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH.os, "getuid", return_value=501),
            mock.patch.object(DAEMON_SWITCH, "run", side_effect=[bootout_missing, print_absent]),
        ):
            DAEMON_SWITCH.run_service(args, "stop", allow_absent=True)

    def test_macos_start_rejects_a_different_loaded_plist(self) -> None:
        args = mock.Mock(service="com.atm.daemon", launch_agent_plist="/wanted/atm.plist")
        bootstrap = subprocess.CompletedProcess(
            ["launchctl", "bootstrap"], 5, stdout="", stderr="service already loaded"
        )
        loaded = subprocess.CompletedProcess(
            ["launchctl", "print"], 0, stdout="\tpath = /temporary/atm.plist\n", stderr=""
        )
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH.os, "getuid", return_value=501),
            mock.patch.object(DAEMON_SWITCH, "run", side_effect=[bootstrap, loaded]),
        ):
            with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "retained"):
                DAEMON_SWITCH.run_service(args, "start")


class WindowsScheduledTaskTests(unittest.TestCase):
    def setUp(self) -> None:
        self.args = argparse.Namespace(
            service="atm-daemon",
            cli_link=r"C:\atm-active\atm.exe",
            daemon_link=r"C:\atm-active\atm-daemon.exe",
            yes=True,
        )
        self.selector = Path(self.args.daemon_link)
        self.xml = """<?xml version=\"1.0\"?><Task xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\"><Actions><Exec><Command>C:\\atm-active\\atm-daemon.exe</Command></Exec></Actions></Task>"""

    def test_task_status_reads_one_executable_action_and_running_state(self) -> None:
        with mock.patch.object(
            DAEMON_SWITCH,
            "run",
            side_effect=[
                subprocess.CompletedProcess(["schtasks.exe"], 0, self.xml, ""),
                subprocess.CompletedProcess(["schtasks.exe"], 0, "Status: Running\n", ""),
            ],
        ):
            self.assertEqual(
                DAEMON_SWITCH.windows_task_status("atm-daemon"),
                {
                    "registered": True,
                    "state": "running",
                    "command": r"C:\atm-active\atm-daemon.exe",
                },
            )

    def test_task_status_reports_absent_task(self) -> None:
        missing = subprocess.CompletedProcess(
            ["schtasks.exe"], 1, "", "ERROR: The system cannot find the file specified."
        )
        with mock.patch.object(DAEMON_SWITCH, "run", return_value=missing):
            self.assertEqual(DAEMON_SWITCH.windows_task_status("atm-daemon")["state"], "absent")

    def test_optional_stop_does_not_hide_a_denied_task_query(self) -> None:
        denied = subprocess.CompletedProcess(["schtasks.exe"], 1, "", "ERROR: Access is denied.")
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Windows"),
            mock.patch.object(DAEMON_SWITCH, "run", return_value=denied),
        ):
            with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "Access is denied"):
                DAEMON_SWITCH.run_service(self.args, "stop", allow_absent=True)

    def test_optional_stop_skips_only_an_absent_task(self) -> None:
        missing = subprocess.CompletedProcess(
            ["schtasks.exe"], 1, "", "ERROR: The system cannot find the file specified."
        )
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Windows"),
            mock.patch.object(DAEMON_SWITCH, "run", return_value=missing),
        ):
            DAEMON_SWITCH.run_service(self.args, "stop", allow_absent=True)

    def test_linux_optional_stop_does_not_hide_a_denied_unit(self) -> None:
        args = argparse.Namespace(service="atm-daemon")
        denied = subprocess.CompletedProcess(["systemctl"], 1, "", "Failed to stop atm-daemon.service: Access denied")
        missing = subprocess.CompletedProcess(["systemctl"], 5, "", "Failed to stop atm-daemon.service: Unit atm-daemon.service not loaded.")
        with mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Linux"):
            with mock.patch.object(DAEMON_SWITCH, "run", return_value=denied):
                with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "Access denied"):
                    DAEMON_SWITCH.run_service(args, "stop", allow_absent=True)
            with mock.patch.object(DAEMON_SWITCH, "run", return_value=missing):
                DAEMON_SWITCH.run_service(args, "stop", allow_absent=True)

    def test_start_requires_the_task_to_launch_the_daemon_selector(self) -> None:
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Windows"),
            mock.patch.object(
                DAEMON_SWITCH,
                "windows_task_status",
                return_value={"registered": True, "state": "ready", "command": r"C:\old\atm-daemon.exe"},
            ),
            mock.patch.object(DAEMON_SWITCH, "selected_links", return_value=(Path(self.args.cli_link), self.selector)),
        ):
            with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "not the daemon selector"):
                DAEMON_SWITCH.run_service(self.args, "start")

    def test_start_runs_only_after_task_selector_validation(self) -> None:
        completed = subprocess.CompletedProcess(["schtasks.exe"], 0, "SUCCESS", "")
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Windows"),
            mock.patch.object(
                DAEMON_SWITCH,
                "windows_task_status",
                return_value={"registered": True, "state": "ready", "command": str(self.selector)},
            ),
            mock.patch.object(DAEMON_SWITCH, "selected_links", return_value=(Path(self.args.cli_link), self.selector)),
            mock.patch.object(DAEMON_SWITCH, "run", return_value=completed) as run,
        ):
            DAEMON_SWITCH.run_service(self.args, "start")

        self.assertEqual(run.call_args.args[0], ["schtasks.exe", "/Run", "/TN", "atm-daemon"])

    def test_temporary_launch_rejects_windows_scm_fallback(self) -> None:
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Windows"),
            self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "scheduled-task backend"),
        ):
            DAEMON_SWITCH.temporary_launch_adapter(self.args)


class MacosDevelopmentSigningTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        """Exercise extracted release checks at their canonical module boundary."""
        cls.switcher = DAEMON_SWITCH
        globals()["DAEMON_SWITCH"] = RELEASE_RESOLUTION

    @classmethod
    def tearDownClass(cls) -> None:
        globals()["DAEMON_SWITCH"] = cls.switcher

    def test_identity_discovery_uses_the_shared_apple_resolver(self) -> None:
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH, "resolve_apple_development_identity"),
        ):
            self.assertTrue(DAEMON_SWITCH.macos_development_signing_identity_available())

    def test_signer_and_switcher_share_the_apple_identity_resolver(self) -> None:
        signing_module = SCRIPT.parents[4] / ".just" / "sign_daemon_dev.py"
        spec = importlib.util.spec_from_file_location("sign_daemon_dev_for_gate_test", signing_module)
        assert spec is not None and spec.loader is not None
        signer = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(signer)

        self.assertIs(
            DAEMON_SWITCH.resolve_apple_development_identity,
            signer.resolve_apple_development_identity,
        )
        self.assertEqual(
            DAEMON_SWITCH.CLI_IDENTIFIER,
            signer.CLI_IDENTIFIER,
        )

    def test_non_macos_has_no_signing_gate(self) -> None:
        daemon = Path("/candidate/atm-daemon")
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="FreeBSD"),
            mock.patch.object(DAEMON_SWITCH, "macos_binary_has_development_signature") as signed,
        ):
            DAEMON_SWITCH.require_macos_development_signatures(Path("/candidate/atm"), daemon)
        signed.assert_not_called()

    def test_rejects_unsigned_cli_before_daemon_when_apple_identity_is_available(self) -> None:
        cli = Path("/candidate/atm")
        daemon = Path("/candidate/atm-daemon")
        identity = mock.Mock(team_identifier="4869P2ZYC6")
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH, "resolve_apple_development_identity", return_value=identity),
            mock.patch.object(DAEMON_SWITCH, "macos_binary_has_development_signature", return_value=False),
        ):
            with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "just build"):
                DAEMON_SWITCH.require_macos_development_signatures(cli, daemon)

    def test_rejects_unsigned_daemon_after_accepting_signed_cli(self) -> None:
        cli = Path("/candidate/atm")
        daemon = Path("/candidate/atm-daemon")
        identity = mock.Mock(team_identifier="4869P2ZYC6")
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH, "resolve_apple_development_identity", return_value=identity),
            mock.patch.object(
                DAEMON_SWITCH,
                "macos_binary_has_development_signature",
                side_effect=[True, False],
            ) as signed,
        ):
            with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "daemon target"):
                DAEMON_SWITCH.require_macos_development_signatures(cli, daemon)
        self.assertEqual(
            signed.call_args_list,
            [
                mock.call(cli, DAEMON_SWITCH.CLI_IDENTIFIER, identity),
                mock.call(daemon, DAEMON_SWITCH.DAEMON_IDENTIFIER, identity),
            ],
        )

    def test_accepts_cli_and_daemon_with_exact_development_authority(self) -> None:
        cli = Path("/candidate/atm")
        daemon = Path("/candidate/atm-daemon")
        identity = mock.Mock(team_identifier="4869P2ZYC6")
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH, "resolve_apple_development_identity", return_value=identity),
            mock.patch.object(DAEMON_SWITCH, "macos_binary_has_development_signature", return_value=True) as signed,
        ):
            DAEMON_SWITCH.require_macos_development_signatures(cli, daemon)
        self.assertEqual(
            signed.call_args_list,
            [
                mock.call(cli, DAEMON_SWITCH.CLI_IDENTIFIER, identity),
                mock.call(daemon, DAEMON_SWITCH.DAEMON_IDENTIFIER, identity),
            ],
        )

    def test_self_signed_identity_uses_leaf_and_common_name_pins(self) -> None:
        cli = Path("/candidate/atm")
        daemon = Path("/candidate/atm-daemon")
        identity = mock.Mock(team_identifier="", fingerprint="A" * 40, common_name="atm-daemon-dev")
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH, "resolve_apple_development_identity", return_value=identity),
            mock.patch.object(
                DAEMON_SWITCH,
                "macos_binary_has_development_signature",
                return_value=True,
            ) as signed,
        ):
            DAEMON_SWITCH.require_macos_development_signatures(cli, daemon)
        self.assertEqual(
            signed.call_args_list,
            [
                mock.call(cli, DAEMON_SWITCH.CLI_IDENTIFIER, identity),
                mock.call(daemon, DAEMON_SWITCH.DAEMON_IDENTIFIER, identity),
            ],
        )

    def test_restore_accepts_homebrew_pair_without_development_identity(self) -> None:
        cli = Path("/opt/homebrew/opt/atm/bin/atm")
        daemon = Path("/opt/homebrew/opt/atm/bin/atm-daemon")
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH, "homebrew_pair", return_value=(cli.resolve(), daemon.resolve())),
            mock.patch.object(DAEMON_SWITCH, "require_homebrew_release_provenance") as provenance,
            mock.patch.object(DAEMON_SWITCH, "require_macos_development_signatures") as development,
        ):
            DAEMON_SWITCH.require_macos_restore_provenance(cli, daemon)

        provenance.assert_called_once_with(cli.resolve(), daemon.resolve())
        development.assert_not_called()

    def test_restore_rejects_homebrew_pair_without_valid_provenance(self) -> None:
        cli = Path("/opt/homebrew/opt/atm/bin/atm")
        daemon = Path("/opt/homebrew/opt/atm/bin/atm-daemon")
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH, "homebrew_pair", return_value=(cli.resolve(), daemon.resolve())),
            mock.patch.object(
                DAEMON_SWITCH,
                "require_homebrew_release_provenance",
                side_effect=DAEMON_SWITCH.SwitchError("invalid release provenance"),
            ),
        ):
            with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "invalid release provenance"):
                DAEMON_SWITCH.require_macos_restore_provenance(cli, daemon)

    def test_homebrew_release_provenance_accepts_matching_release_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            prefix = Path(temporary) / "opt" / "atm"
            binary_dir = prefix / "bin"
            binary_dir.mkdir(parents=True)
            cli = binary_dir / "atm"
            daemon = binary_dir / "atm-daemon"
            for binary in (cli, daemon):
                binary.write_bytes(binary.name.encode("utf-8"))
                binary.chmod(0o700)
            formula = {
                "name": "atm",
                "full_name": "randlee/tap/atm",
                "homepage": "https://github.com/randlee/atm-core",
                "versions": {"stable": "1.4.4"},
                "installed": [{"version": "1.4.4"}],
                "urls": {
                    "stable": {
                        "url": "https://github.com/randlee/atm-core/releases/download/v1.4.4/atm.tar.gz",
                        "checksum": "a" * 64,
                    }
                },
            }
            with (
                mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
                mock.patch.object(DAEMON_SWITCH.shutil, "which", return_value="/opt/homebrew/bin/brew"),
                mock.patch.object(
                    DAEMON_SWITCH,
                    "run",
                    side_effect=[
                        subprocess.CompletedProcess([], 0, f"{prefix}\n", ""),
                        subprocess.CompletedProcess(
                            [],
                            0,
                            json.dumps(
                                {
                                    "formulae": [
                                        {"name": "openssl", "full_name": "homebrew/core/openssl"},
                                        formula,
                                        {"name": "python", "full_name": "homebrew/core/python"},
                                    ]
                                }
                            ),
                            "",
                        ),
                    ],
                ),
                mock.patch.object(DAEMON_SWITCH, "selected_release_version", return_value="1.4.4"),
            ):
                DAEMON_SWITCH.require_homebrew_release_provenance(cli, daemon)

    def test_homebrew_release_provenance_rejects_invalid_release_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            prefix = Path(temporary) / "opt" / "atm"
            binary_dir = prefix / "bin"
            binary_dir.mkdir(parents=True)
            cli = binary_dir / "atm"
            daemon = binary_dir / "atm-daemon"
            for binary in (cli, daemon):
                binary.write_bytes(binary.name.encode("utf-8"))
                binary.chmod(0o700)
            formula = {
                "name": "atm",
                "full_name": "randlee/tap/atm",
                "homepage": "https://github.com/randlee/atm-core",
                "versions": {"stable": "1.4.4"},
                "installed": [{"version": "1.4.4"}],
                "urls": {
                    "stable": {
                        "url": "https://example.invalid/atm.tar.gz",
                        "checksum": "not-a-sha256",
                    }
                },
            }
            with (
                mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
                mock.patch.object(DAEMON_SWITCH.shutil, "which", return_value="/opt/homebrew/bin/brew"),
                mock.patch.object(
                    DAEMON_SWITCH,
                    "run",
                    side_effect=[
                        subprocess.CompletedProcess([], 0, f"{prefix}\n", ""),
                        subprocess.CompletedProcess([], 0, json.dumps({"formulae": [formula]}), ""),
                    ],
                ),
                mock.patch.object(DAEMON_SWITCH, "selected_release_version", return_value="1.4.4"),
            ):
                with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "matching GitHub Release asset"):
                    DAEMON_SWITCH.require_homebrew_release_provenance(cli, daemon)

    def test_homebrew_release_provenance_does_not_probe_daemon_owner_lock_as_version(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            prefix = Path(temporary) / "opt" / "atm"
            binary_dir = prefix / "bin"
            binary_dir.mkdir(parents=True)
            cli = binary_dir / "atm"
            daemon = binary_dir / "atm-daemon"
            for binary in (cli, daemon):
                binary.write_bytes(binary.name.encode("utf-8"))
                binary.chmod(0o700)
            formula = {
                "name": "atm",
                "full_name": "randlee/tap/atm",
                "homepage": "https://github.com/randlee/atm-core",
                "versions": {"stable": "1.4.4"},
                "installed": [{"version": "1.4.4"}],
                "urls": {
                    "stable": {
                        "url": "https://github.com/randlee/atm-core/releases/download/v1.4.4/atm.tar.gz",
                        "checksum": "a" * 64,
                    }
                },
            }
            probed: list[Path] = []

            def version_probe(binary: Path) -> str:
                probed.append(binary)
                if binary == daemon:
                    return "an ATM daemon already owns ~/.atm/daemon/owner.lock ... retry."
                return "atm 1.4.4"

            with (
                mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
                mock.patch.object(DAEMON_SWITCH.shutil, "which", return_value="/opt/homebrew/bin/brew"),
                mock.patch.object(
                    DAEMON_SWITCH,
                    "run",
                    side_effect=[
                        subprocess.CompletedProcess([], 0, f"{prefix}\n", ""),
                        subprocess.CompletedProcess([], 0, json.dumps({"formulae": [formula]}), ""),
                    ],
                ),
                mock.patch.object(DAEMON_SWITCH, "version", side_effect=version_probe),
            ):
                DAEMON_SWITCH.require_homebrew_release_provenance(cli, daemon)

            self.assertEqual(probed, [cli.resolve()])

    def test_restore_dispatch_uses_provenance_gate_and_skips_dev_gate(self) -> None:
        switcher = self.switcher
        args = argparse.Namespace(command="restore")
        cli = Path("/release/atm")
        daemon = Path("/release/atm-daemon")
        argument_parser = mock.Mock(parse_args=mock.Mock(return_value=args))
        with (
            mock.patch.object(switcher, "parser", return_value=argument_parser),
            mock.patch.object(switcher, "restore_pair", return_value=(cli, daemon)),
            mock.patch.object(switcher, "require_macos_restore_provenance") as provenance,
            mock.patch.object(switcher, "switch_pair") as switch,
        ):
            self.assertEqual(switcher.main(), 0)

        provenance.assert_called_once_with(cli, daemon)
        switch.assert_called_once_with(args, cli, daemon, require_development_signature=False)

    def test_signature_check_uses_shared_stable_identifier_verifier(self) -> None:
        daemon = Path("/candidate/atm-daemon")
        with (
            mock.patch.object(RELEASE_RESOLUTION, "verify_signing_identity", return_value=True) as verify,
        ):
            identity = DAEMON_SWITCH.SigningIdentity("A" * 40, "Apple Development: test", "4869P2ZYC6")
            self.assertTrue(
                DAEMON_SWITCH.macos_binary_has_development_signature(
                    daemon, DAEMON_SWITCH.DAEMON_IDENTIFIER, identity
                )
            )
        verify.assert_called_once_with(str(daemon), DAEMON_SWITCH.DAEMON_IDENTIFIER, identity)

    def test_windows_warns_and_skips_the_unimplemented_signature_gate(self) -> None:
        stderr = io.StringIO()
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Windows"),
            mock.patch.object(DAEMON_SWITCH, "macos_binary_has_development_signature") as signed,
            mock.patch.object(DAEMON_SWITCH.sys, "stderr", stderr),
        ):
            DAEMON_SWITCH.require_macos_development_signatures(Path("/candidate/atm"), Path("/candidate/atm-daemon"))
        signed.assert_not_called()
        self.assertIn("Windows signing not yet implemented", stderr.getvalue())


class HttpRuntimeOwnerLockTests(unittest.TestCase):
    def test_owner_lock_identifies_http_runtime_without_legacy_socket(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            owner_lock = home / ".atm" / "daemon" / "owner.lock"
            owner_lock.parent.mkdir(parents=True)
            owner_lock.touch()
            original_path = DAEMON_SWITCH.Path

            class TestPath:
                @staticmethod
                def home() -> Path:
                    return home

            DAEMON_SWITCH.Path = TestPath
            try:
                completed = subprocess.CompletedProcess(["lsof"], 0, stdout="42\n", stderr="")
                with (
                    mock.patch.object(DAEMON_SWITCH.shutil, "which", return_value=None),
                    mock.patch.object(DAEMON_SWITCH, "run", return_value=completed) as run,
                ):
                    self.assertEqual(DAEMON_SWITCH.macos_daemon_owner_pids(), [42])
                run.assert_called_once_with(["/usr/sbin/lsof", "-t", str(owner_lock)], timeout=5.0)
            finally:
                DAEMON_SWITCH.Path = original_path

    def test_rejects_http_runtime_owner_without_explicit_repair(self) -> None:
        args = mock.Mock(repair_orphan=False)
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH, "macos_daemon_owner_pids", return_value=[42]),
        ):
            with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "daemon owner"):
                DAEMON_SWITCH.require_stopped_daemon(args, Path("/selected/atm"))

    def test_live_http_runtime_uses_executable_identity_when_doctor_has_no_daemon_context(self) -> None:
        cli = Path("/selected/atm")
        daemon = Path("/selected/atm-daemon")
        doctor = {
            "summary": {"status": "healthy"},
            "client_context": {"version": "1.4.1-beta-ai-1"},
        }
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH, "selected_release_version", return_value="1.4.1-beta-ai-1"),
            mock.patch.object(DAEMON_SWITCH, "doctor", return_value=doctor),
            mock.patch.object(DAEMON_SWITCH, "macos_live_daemon_matches", return_value=(True, "exact executable")) as matches,
        ):
            self.assertEqual(
                DAEMON_SWITCH.live_pair_matches(cli, daemon),
                (True, "exact executable"),
            )
        matches.assert_called_once_with(daemon)

    def test_live_http_runtime_rejects_unhealthy_doctor_without_daemon_context(self) -> None:
        doctor = {
            "summary": {"status": "degraded"},
            "client_context": {"version": "1.4.1-beta-ai-1"},
        }
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH, "selected_release_version", return_value="1.4.1-beta-ai-1"),
            mock.patch.object(DAEMON_SWITCH, "doctor", return_value=doctor),
        ):
            matched, detail = DAEMON_SWITCH.live_pair_matches(Path("/selected/atm"), Path("/selected/atm-daemon"))
        self.assertFalse(matched)
        self.assertIn("not healthy", detail)


class ReadinessAndRollbackTests(unittest.TestCase):
    def test_readiness_wait_accepts_a_daemon_ready_after_five_seconds(self) -> None:
        calls = 0

        def delayed_readiness(_cli: Path, _daemon: Path | None) -> tuple[bool, str]:
            nonlocal calls
            calls += 1
            return (calls == 51, "ready" if calls == 51 else "starting")

        with (
            mock.patch.object(DAEMON_SWITCH, "live_pair_matches", side_effect=delayed_readiness),
            mock.patch.object(DAEMON_SWITCH.time, "sleep"),
        ):
            self.assertEqual(
                DAEMON_SWITCH.wait_for_live_pair(Path("/candidate/atm"), Path("/candidate/atm-daemon")),
                (True, "ready"),
            )

    def test_failed_candidate_is_stopped_before_old_selectors_are_restored(self) -> None:
        args = mock.Mock(yes=True, dry_run=False)
        cli_link = Path("/selector/atm")
        daemon_link = Path("/selector/atm-daemon")
        old_cli = Path("/old/atm")
        old_daemon = Path("/old/atm-daemon")
        candidate_cli = Path("/candidate/atm")
        candidate_daemon = Path("/candidate/atm-daemon")
        with (
            mock.patch.object(DAEMON_SWITCH, "selected_links", return_value=(cli_link, daemon_link)),
            mock.patch.object(
                DAEMON_SWITCH,
                "require_executable",
                side_effect=[old_cli, old_daemon, candidate_cli, candidate_daemon],
            ),
            mock.patch.object(DAEMON_SWITCH, "validate_selectors"),
            mock.patch.object(DAEMON_SWITCH, "save_default_pair"),
            mock.patch.object(DAEMON_SWITCH, "require_macos_development_signatures"),
            mock.patch.object(DAEMON_SWITCH, "run_service") as service,
            mock.patch.object(DAEMON_SWITCH, "require_stopped_daemon") as stopped,
            mock.patch.object(DAEMON_SWITCH, "replace_link") as replace,
            mock.patch.object(DAEMON_SWITCH, "wait_for_live_pair", return_value=(False, "still starting")),
        ):
            with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "split CLI/daemon pair"):
                DAEMON_SWITCH.switch_pair(args, candidate_cli, candidate_daemon)

        self.assertEqual(
            service.call_args_list,
            [
                mock.call(args, "stop", allow_absent=True),
                mock.call(args, "start"),
                mock.call(args, "stop", allow_absent=True),
                mock.call(args, "start"),
            ],
        )
        self.assertEqual(
            stopped.call_args_list,
            [mock.call(args, old_cli), mock.call(args, candidate_cli)],
        )
        self.assertEqual(
            replace.call_args_list,
            [
                mock.call(cli_link, candidate_cli),
                mock.call(daemon_link, candidate_daemon),
                mock.call(cli_link, old_cli),
                mock.call(daemon_link, old_daemon),
            ],
        )

    def test_switch_path_retains_the_development_signature_gate(self) -> None:
        args = argparse.Namespace(yes=False, dry_run=True, repair_orphan=False)
        cli_link = Path("/selected/atm")
        daemon_link = Path("/selected/atm-daemon")
        old_cli = Path("/old/atm")
        old_daemon = Path("/old/atm-daemon")
        candidate_cli = Path("/candidate/atm")
        candidate_daemon = Path("/candidate/atm-daemon")
        with (
            mock.patch.object(DAEMON_SWITCH, "selected_links", return_value=(cli_link, daemon_link)),
            mock.patch.object(
                DAEMON_SWITCH,
                "require_executable",
                side_effect=[old_cli, old_daemon, candidate_cli, candidate_daemon],
            ),
            mock.patch.object(DAEMON_SWITCH, "validate_selectors"),
            mock.patch.object(DAEMON_SWITCH, "require_macos_development_signatures") as development,
            redirect_stdout(io.StringIO()),
        ):
            DAEMON_SWITCH.switch_pair(args, candidate_cli, candidate_daemon)

        development.assert_called_once_with(candidate_cli, candidate_daemon)


@POSIX_ONLY
class TemporaryLaunchJournalTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.cli = self.root / "atm"
        self.daemon = self.root / "atm-daemon"
        for binary in (self.cli, self.daemon):
            binary.write_bytes(binary.name.encode("utf-8"))
            binary.chmod(0o700)
        self.journal = DAEMON_SWITCH.TemporaryLaunchJournal(self.root / "state" / "temporary-launch.json")

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def captured_session(self) -> object:
        return DAEMON_SWITCH.TemporaryLaunchSession.captured(
            peer_wire_security=DAEMON_SWITCH.PeerWireSecurity.PLAINTEXT_TEST,
            platform="Darwin",
            account_id="uid:501",
            service="com.atm.daemon.test",
            cli_path=self.cli,
            cli_digest=DAEMON_SWITCH.sha256_file(self.cli),
            daemon_path=self.daemon,
            daemon_digest=DAEMON_SWITCH.sha256_file(self.daemon),
            launch_spec=DAEMON_SWITCH.CapturedLaunchSpec("/original.plist", "original-sha"),
        )

    def test_journal_round_trip_is_private_and_blocks_another_session(self) -> None:
        session = self.captured_session()
        self.journal.create(session)

        self.assertEqual(self.journal.load(), session)
        self.assertEqual(self.journal.path.stat().st_mode & 0o777, 0o600)
        with self.assertRaisesRegex(DAEMON_SWITCH.TemporaryLaunchError, "recovery is pending"):
            self.journal.require_no_active_session()

    def test_journal_creation_refuses_to_overwrite_an_active_session(self) -> None:
        session = self.captured_session()
        self.journal.create(session)

        with self.assertRaisesRegex(DAEMON_SWITCH.TemporaryLaunchError, "refuse to overwrite"):
            self.journal.create(session)

    def test_transition_requires_overlay_before_overlay_start(self) -> None:
        session = self.captured_session()
        stopped = session.transition(DAEMON_SWITCH.TemporaryLaunchPhase.STOPPED)
        with self.assertRaisesRegex(DAEMON_SWITCH.TemporaryLaunchError, "cannot transition"):
            stopped.transition(DAEMON_SWITCH.TemporaryLaunchPhase.OVERLAY_STARTED)

        overlay = stopped.with_overlay(DAEMON_SWITCH.OverlayLaunchSpec("/overlay.plist", "overlay-sha"))
        self.assertEqual(overlay.phase, DAEMON_SWITCH.TemporaryLaunchPhase.OVERLAY_APPLIED)
        self.assertEqual(
            overlay.transition(DAEMON_SWITCH.TemporaryLaunchPhase.OVERLAY_STARTED).phase,
            DAEMON_SWITCH.TemporaryLaunchPhase.OVERLAY_STARTED,
        )

    def test_completion_requires_durable_completed_state_before_removal(self) -> None:
        session = self.captured_session()
        restoring = session.transition(DAEMON_SWITCH.TemporaryLaunchPhase.RESTORING)
        completed = restoring.transition(DAEMON_SWITCH.TemporaryLaunchPhase.COMPLETED)
        self.journal.create(session)
        self.journal.save(completed)

        self.journal.remove_after_completion(completed)

        self.assertFalse(self.journal.path.exists())

    def test_non_private_existing_journal_fails_closed(self) -> None:
        self.journal.path.parent.mkdir(mode=0o700)
        self.journal.path.write_text("{}", encoding="utf-8")
        self.journal.path.chmod(0o644)

        with self.assertRaisesRegex(DAEMON_SWITCH.TemporaryLaunchError, "accessible"):
            self.journal.load()


class TemporaryLaunchControlPlaneTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.cli = self.root / "atm"
        self.daemon = self.root / "atm-daemon"
        for binary in (self.cli, self.daemon):
            binary.write_bytes(binary.name.encode("utf-8"))
            binary.chmod(0o700)
        self.journal = DAEMON_SWITCH.TemporaryLaunchJournal(self.root / "state" / "temporary-launch.json")
        self.args = argparse.Namespace(
            yes=True,
            service="com.atm.daemon.test",
            peer_wire_security=DAEMON_SWITCH.PeerWireSecurity.PLAINTEXT_TEST,
            repair_orphan=False,
        )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_parser_exposes_only_typed_peer_wire_modes(self) -> None:
        parser = DAEMON_SWITCH.parser()
        args = parser.parse_args(
            [
                "temporary-launch",
                "--service",
                "com.atm.daemon.test",
                "begin",
                "--peer-wire-security",
                "plaintext-test",
                "--yes",
            ]
        )

        self.assertEqual(args.peer_wire_security, DAEMON_SWITCH.PeerWireSecurity.PLAINTEXT_TEST)
        with redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            parser.parse_args(
                [
                    "temporary-launch",
                    "begin",
                    "--peer-wire-security",
                    "plaintext-test",
                    "--daemon-arg",
                    "--anything",
                ]
            )

    def test_active_session_blocks_ordinary_restart_before_service_call(self) -> None:
        session = self.create_active_session()
        self.journal.create(session)
        with mock.patch.object(DAEMON_SWITCH, "temporary_launch_journal", return_value=self.journal):
            with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "recovery is pending"):
                DAEMON_SWITCH.restart(self.args)

    def test_active_session_blocks_ordinary_pair_switch_before_selector_lookup(self) -> None:
        self.journal.create(self.create_active_session())
        with mock.patch.object(DAEMON_SWITCH, "temporary_launch_journal", return_value=self.journal):
            with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "recovery is pending"):
                DAEMON_SWITCH.switch_pair(self.args, self.cli, self.daemon)

    def test_no_platform_adapter_refuses_before_journal_or_service_mutation(self) -> None:
        with (
            mock.patch.object(DAEMON_SWITCH, "temporary_launch_journal", return_value=self.journal),
            mock.patch.object(DAEMON_SWITCH, "selected_matched_pair", return_value=(self.cli, self.daemon)),
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="FreeBSD"),
            mock.patch.object(DAEMON_SWITCH, "run_service") as service,
        ):
            with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "no direct-process fallback"):
                DAEMON_SWITCH.begin_temporary_launch(self.args)

        self.assertIsNone(self.journal.load())
        service.assert_not_called()

    def test_begin_writes_captured_intent_before_service_stop(self) -> None:
        class FakeAdapter:
            def capture(self, _args: object, _cli: Path, _daemon: Path, _mode: object) -> object:
                return DAEMON_SWITCH.CapturedLaunchSpec("/original.plist", "original-sha")

            def apply_overlay(self, _args: object, _session: object) -> object:
                return DAEMON_SWITCH.OverlayLaunchSpec("/overlay.plist", "overlay-sha")

            def activate_overlay(self, _args: object, _session: object) -> None:
                return None

            def start_args(self, arguments: object, _session: object) -> object:
                return arguments

            def restore_exact(self, _args: object, _session: object) -> None:
                raise AssertionError("restore is not part of begin")

        with (
            mock.patch.object(DAEMON_SWITCH, "temporary_launch_journal", return_value=self.journal),
            mock.patch.object(DAEMON_SWITCH, "selected_matched_pair", return_value=(self.cli, self.daemon)),
            mock.patch.object(DAEMON_SWITCH, "temporary_launch_adapter", return_value=FakeAdapter()),
            mock.patch.object(DAEMON_SWITCH, "account_identifier", return_value="uid:501"),
            mock.patch.object(DAEMON_SWITCH, "run_service") as service,
            mock.patch.object(DAEMON_SWITCH, "require_stopped_daemon") as stopped,
            mock.patch.object(DAEMON_SWITCH, "wait_for_temporary_launch", return_value=(True, "ready")),
            redirect_stdout(io.StringIO()),
        ):
            DAEMON_SWITCH.begin_temporary_launch(self.args)

        active = self.journal.load()
        self.assertIsNotNone(active)
        assert active is not None
        self.assertEqual(active.phase, DAEMON_SWITCH.TemporaryLaunchPhase.OVERLAY_STARTED)
        self.assertEqual(active.overlay_digest, "overlay-sha")
        self.assertEqual(
            service.call_args_list,
            [
                mock.call(self.args, "stop", allow_absent=True),
                mock.call(self.args, "start"),
            ],
        )
        stopped.assert_called_once_with(self.args, self.cli)

    def test_recover_resumes_a_durably_restoring_session(self) -> None:
        """A crash after RESTORING is journaled must not require manual repair."""
        class FakeAdapter:
            def restore_exact(self, _args: object, session: object) -> None:
                self.restored = session

        session = self.create_active_session().transition(DAEMON_SWITCH.TemporaryLaunchPhase.RESTORING)
        self.journal.create(session)
        self.args.session = session.session_id
        adapter = FakeAdapter()
        with (
            mock.patch.object(DAEMON_SWITCH, "temporary_launch_journal", return_value=self.journal),
            mock.patch.object(DAEMON_SWITCH, "selected_matched_pair", return_value=(self.cli, self.daemon)),
            mock.patch.object(DAEMON_SWITCH, "account_identifier", return_value="uid:501"),
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH, "temporary_launch_adapter", return_value=adapter),
            mock.patch.object(DAEMON_SWITCH, "run_service") as service,
            mock.patch.object(DAEMON_SWITCH, "require_stopped_daemon") as stopped,
            mock.patch.object(DAEMON_SWITCH, "wait_for_temporary_launch", return_value=(True, "ready")),
            redirect_stdout(io.StringIO()),
        ):
            DAEMON_SWITCH.restore_temporary_launch(self.args, recovery=True)

        self.assertEqual(adapter.restored, session)
        self.assertIsNone(self.journal.load())
        self.assertEqual(
            service.call_args_list,
            [
                mock.call(self.args, "stop", allow_absent=True),
                mock.call(self.args, "start"),
            ],
        )
        stopped.assert_called_once_with(self.args, self.cli)

    def create_active_session(self) -> object:
        return DAEMON_SWITCH.TemporaryLaunchSession.captured(
            peer_wire_security=DAEMON_SWITCH.PeerWireSecurity.PLAINTEXT_TEST,
            platform="Darwin",
            account_id="uid:501",
            service=self.args.service,
            cli_path=self.cli,
            cli_digest=DAEMON_SWITCH.sha256_file(self.cli),
            daemon_path=self.daemon,
            daemon_digest=DAEMON_SWITCH.sha256_file(self.daemon),
            launch_spec=DAEMON_SWITCH.CapturedLaunchSpec("/original.plist", "original-sha"),
        )


@POSIX_ONLY
class MacosTemporaryLaunchAdapterTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.cli = self.root / "atm"
        self.daemon = self.root / "atm-daemon"
        for binary in (self.cli, self.daemon):
            binary.write_bytes(binary.name.encode("utf-8"))
            binary.chmod(0o700)
        self.source = self.root / "com.atm.daemon.test.plist"
        self.args = argparse.Namespace(
            yes=True,
            service="com.atm.daemon.test",
            launch_agent_plist=str(self.source),
            peer_wire_security=DAEMON_SWITCH.PeerWireSecurity.PLAINTEXT_TEST,
            repair_orphan=False,
        )
        self.adapter = DAEMON_SWITCH.MacosLaunchAgentAdapter(self.root / "state" / "overlays")
        self.write_source()

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def write_source(self) -> None:
        payload = {
            "Label": self.args.service,
            "ProgramArguments": [str(self.daemon), "--log-format", "json"],
            "KeepAlive": True,
        }
        self.source.write_bytes(plistlib.dumps(payload, fmt=plistlib.FMT_XML, sort_keys=False))
        self.source.chmod(0o600)

    def captured_session(self) -> object:
        captured = self.adapter.capture(
            self.args,
            self.cli,
            self.daemon,
            self.args.peer_wire_security,
        )
        return DAEMON_SWITCH.TemporaryLaunchSession.captured(
            peer_wire_security=self.args.peer_wire_security,
            platform="Darwin",
            account_id="uid:501",
            service=self.args.service,
            cli_path=self.cli,
            cli_digest=DAEMON_SWITCH.sha256_file(self.cli),
            daemon_path=self.daemon,
            daemon_digest=DAEMON_SWITCH.sha256_file(self.daemon),
            launch_spec=captured,
        )

    def test_overlay_preserves_source_and_adds_only_typed_mode(self) -> None:
        original = self.source.read_bytes()
        session = self.captured_session().transition(DAEMON_SWITCH.TemporaryLaunchPhase.STOPPED)
        overlay_spec = self.adapter.apply_overlay(self.args, session)
        session = session.with_overlay(overlay_spec)

        self.assertEqual(self.source.read_bytes(), original)
        overlay = Path(overlay_spec.overlay_reference)
        payload = plistlib.loads(overlay.read_bytes())
        self.assertEqual(payload["KeepAlive"], True)
        self.assertEqual(
            payload["ProgramArguments"],
            [str(self.daemon), "--log-format", "json", "--peer-wire-security", "plaintext-test"],
        )
        self.assertEqual(self.adapter.start_args(self.args, session).launch_agent_plist, str(overlay))

        self.adapter.restore_exact(self.args, session)

        self.assertFalse(overlay.exists())
        self.assertEqual(self.source.read_bytes(), original)

    def test_capture_rejects_a_source_that_already_selects_peer_wire_security(self) -> None:
        payload = plistlib.loads(self.source.read_bytes())
        payload["ProgramArguments"].extend(("--peer-wire-security", "mutual-tls"))
        self.source.write_bytes(plistlib.dumps(payload, fmt=plistlib.FMT_XML, sort_keys=False))

        with self.assertRaisesRegex(DAEMON_SWITCH.TemporaryLaunchError, "already selects"):
            self.adapter.capture(self.args, self.cli, self.daemon, self.args.peer_wire_security)

    def test_restore_refuses_an_operator_changed_source_and_retains_overlay(self) -> None:
        session = self.captured_session().transition(DAEMON_SWITCH.TemporaryLaunchPhase.STOPPED)
        overlay_spec = self.adapter.apply_overlay(self.args, session)
        session = session.with_overlay(overlay_spec)
        payload = plistlib.loads(self.source.read_bytes())
        payload["KeepAlive"] = False
        self.source.write_bytes(plistlib.dumps(payload, fmt=plistlib.FMT_XML, sort_keys=False))

        with self.assertRaisesRegex(DAEMON_SWITCH.TemporaryLaunchError, "source LaunchAgent changed"):
            self.adapter.restore_exact(self.args, session)

        self.assertTrue(Path(overlay_spec.overlay_reference).exists())

    def test_restore_is_idempotent_after_the_owned_overlay_was_removed(self) -> None:
        session = self.captured_session().transition(DAEMON_SWITCH.TemporaryLaunchPhase.STOPPED)
        overlay_spec = self.adapter.apply_overlay(self.args, session)
        session = session.with_overlay(overlay_spec)

        self.adapter.restore_exact(self.args, session)
        self.adapter.restore_exact(self.args, session)

        self.assertFalse(Path(overlay_spec.overlay_reference).exists())

    def test_begin_then_restore_uses_only_the_owned_overlay_at_fake_service_boundary(self) -> None:
        journal = DAEMON_SWITCH.TemporaryLaunchJournal(self.root / "state" / "temporary-launch.json")
        original = self.source.read_bytes()
        with (
            mock.patch.object(DAEMON_SWITCH, "temporary_launch_journal", return_value=journal),
            mock.patch.object(DAEMON_SWITCH, "selected_matched_pair", return_value=(self.cli, self.daemon)),
            mock.patch.object(DAEMON_SWITCH, "temporary_launch_adapter", return_value=self.adapter),
            mock.patch.object(DAEMON_SWITCH, "account_identifier", return_value="uid:501"),
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH, "run_service") as service,
            mock.patch.object(DAEMON_SWITCH, "require_stopped_daemon"),
            mock.patch.object(DAEMON_SWITCH, "wait_for_temporary_launch", return_value=(True, "ready")),
            redirect_stdout(io.StringIO()),
        ):
            DAEMON_SWITCH.begin_temporary_launch(self.args)
            active = journal.load()
            assert active is not None
            self.args.session = active.session_id
            DAEMON_SWITCH.restore_temporary_launch(self.args, recovery=False)

        self.assertEqual(self.source.read_bytes(), original)
        self.assertIsNone(journal.load())
        self.assertEqual(service.call_args_list[0], mock.call(self.args, "stop", allow_absent=True))
        overlay_start = service.call_args_list[1]
        self.assertEqual(overlay_start.args[1], "start")
        self.assertNotEqual(overlay_start.args[0].launch_agent_plist, str(self.source))
        self.assertEqual(service.call_args_list[2], mock.call(self.args, "stop", allow_absent=True))
        self.assertEqual(service.call_args_list[3], mock.call(self.args, "start"))

    def test_recover_completes_after_crash_between_overlay_removal_and_completion(self) -> None:
        journal = DAEMON_SWITCH.TemporaryLaunchJournal(self.root / "state" / "temporary-launch.json")
        with (
            mock.patch.object(DAEMON_SWITCH, "temporary_launch_journal", return_value=journal),
            mock.patch.object(DAEMON_SWITCH, "selected_matched_pair", return_value=(self.cli, self.daemon)),
            mock.patch.object(DAEMON_SWITCH, "temporary_launch_adapter", return_value=self.adapter),
            mock.patch.object(DAEMON_SWITCH, "account_identifier", return_value="uid:501"),
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH, "require_stopped_daemon"),
            mock.patch.object(DAEMON_SWITCH, "wait_for_temporary_launch", return_value=(True, "ready")),
            redirect_stdout(io.StringIO()),
        ):
            with mock.patch.object(DAEMON_SWITCH, "run_service"):
                DAEMON_SWITCH.begin_temporary_launch(self.args)
            active = journal.load()
            assert active is not None
            self.args.session = active.session_id
            with mock.patch.object(
                DAEMON_SWITCH,
                "run_service",
                side_effect=[None, DAEMON_SWITCH.SwitchError("injected normal start failure")],
            ):
                with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "injected normal start failure"):
                    DAEMON_SWITCH.restore_temporary_launch(self.args, recovery=False)

            interrupted = journal.load()
            assert interrupted is not None
            self.assertEqual(interrupted.phase, DAEMON_SWITCH.TemporaryLaunchPhase.RESTORING)
            self.assertFalse(Path(interrupted.overlay_reference or "").exists())
            with mock.patch.object(DAEMON_SWITCH, "run_service"):
                DAEMON_SWITCH.restore_temporary_launch(self.args, recovery=True)

        self.assertIsNone(journal.load())


class WindowsCommandLineCodecTests(unittest.TestCase):
    def test_windows_argv_codec_round_trips_quoted_arguments(self) -> None:
        argv = [
            r"C:\\Program Files\\ATM\\atm-daemon.exe",
            "--log-format",
            "json",
            "--label",
            'quote " and trailing slash\\',
            "",
        ]
        self.assertEqual(
            DAEMON_SWITCH.parse_windows_command_line(
                DAEMON_SWITCH.quote_windows_command_line(argv)
            ),
            argv,
        )


@POSIX_ONLY
class LinuxTemporaryLaunchAdapterTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.cli = self.root / "atm"
        self.daemon = self.root / "atm-daemon"
        for binary in (self.cli, self.daemon):
            binary.write_bytes(binary.name.encode("utf-8"))
            binary.chmod(0o700)
        self.args = argparse.Namespace(
            yes=True,
            service="atm-daemon-test",
            peer_wire_security=DAEMON_SWITCH.PeerWireSecurity.PLAINTEXT_TEST,
            repair_orphan=False,
        )
        self.fragment = self.root / "units" / "atm-daemon-test.service"
        self.fragment.parent.mkdir()
        self.fragment.write_text(
            f"[Service]\nExecStart={self.daemon} --log-format json\n",
            encoding="utf-8",
        )
        self.user_units = self.root / "config" / "systemd" / "user"
        self.loaded_dropins: list[Path] = []
        self.reloads = 0
        self.before_reload: object | None = None
        self.adapter = DAEMON_SWITCH.LinuxSystemdUserAdapter(self.user_units, self.run_systemctl)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def run_systemctl(self, command: object, _timeout: float) -> object:
        values = list(command)
        if values[2] == "show":
            dropins = " ".join(str(path) for path in self.loaded_dropins)
            stdout = f"FragmentPath={self.fragment}\nDropInPaths={dropins}\n"
            return subprocess.CompletedProcess(values, 0, stdout, "")
        if values[2] == "daemon-reload":
            if callable(self.before_reload):
                self.before_reload()
            self.reloads += 1
            directory = self.user_units / "atm-daemon-test.service.d"
            self.loaded_dropins = sorted(directory.glob("*.conf")) if directory.exists() else []
            return subprocess.CompletedProcess(values, 0, "", "")
        raise AssertionError(f"unexpected systemctl command: {values}")

    def captured_session(self) -> object:
        captured = self.adapter.capture(
            self.args,
            self.cli,
            self.daemon,
            self.args.peer_wire_security,
        )
        return DAEMON_SWITCH.TemporaryLaunchSession.captured(
            peer_wire_security=self.args.peer_wire_security,
            platform="Linux",
            account_id="uid:501",
            service=self.args.service,
            cli_path=self.cli,
            cli_digest=DAEMON_SWITCH.sha256_file(self.cli),
            daemon_path=self.daemon,
            daemon_digest=DAEMON_SWITCH.sha256_file(self.daemon),
            launch_spec=captured,
        )

    def test_overlay_preserves_source_and_replaces_only_exec_start(self) -> None:
        original = self.fragment.read_bytes()
        session = self.captured_session().transition(DAEMON_SWITCH.TemporaryLaunchPhase.STOPPED)
        overlay_spec = self.adapter.apply_overlay(self.args, session)
        session = session.with_overlay(overlay_spec)

        self.assertEqual(self.fragment.read_bytes(), original)
        self.assertEqual(
            Path(overlay_spec.overlay_reference).read_text(encoding="utf-8"),
            f"[Service]\nExecStart=\nExecStart={self.daemon} --log-format json "
            "--peer-wire-security plaintext-test\n",
        )
        self.adapter.activate_overlay(self.args, session)
        self.assertEqual(
            [dropin.resolve() for dropin in self.loaded_dropins],
            [Path(overlay_spec.overlay_reference).resolve()],
        )

        self.adapter.restore_exact(self.args, session)
        self.adapter.restore_exact(self.args, session)

        self.assertEqual(self.fragment.read_bytes(), original)
        self.assertFalse(Path(overlay_spec.overlay_reference).exists())
        self.assertEqual(self.loaded_dropins, [])

    def test_capture_rejects_existing_dropin_and_preexisting_peer_wire_mode(self) -> None:
        other = self.root / "other.conf"
        other.write_text("[Service]\n", encoding="utf-8")
        self.loaded_dropins = [other]
        with self.assertRaisesRegex(DAEMON_SWITCH.TemporaryLaunchError, "unsupported or changed drop-ins"):
            self.adapter.capture(self.args, self.cli, self.daemon, self.args.peer_wire_security)

        self.loaded_dropins = []
        self.fragment.write_text(
            f"[Service]\nExecStart={self.daemon} --peer-wire-security mutual-tls\n",
            encoding="utf-8",
        )
        with self.assertRaisesRegex(DAEMON_SWITCH.TemporaryLaunchError, "already selects"):
            self.adapter.capture(self.args, self.cli, self.daemon, self.args.peer_wire_security)

        self.fragment.write_text(
            f"[Service]\nExecStart={self.daemon} --state %h/atm\n",
            encoding="utf-8",
        )
        with self.assertRaisesRegex(DAEMON_SWITCH.TemporaryLaunchError, "unsupported quoting or shell-like"):
            self.adapter.capture(self.args, self.cli, self.daemon, self.args.peer_wire_security)

        self.fragment.write_text(
            f"[Service]\nExecStart={self.daemon} --state '/tmp/atm state'\n",
            encoding="utf-8",
        )
        with self.assertRaisesRegex(DAEMON_SWITCH.TemporaryLaunchError, "unsupported quoting or shell-like"):
            self.adapter.capture(self.args, self.cli, self.daemon, self.args.peer_wire_security)

    def test_restore_refuses_operator_changed_source_unit(self) -> None:
        session = self.captured_session().transition(DAEMON_SWITCH.TemporaryLaunchPhase.STOPPED)
        session = session.with_overlay(self.adapter.apply_overlay(self.args, session))
        self.adapter.activate_overlay(self.args, session)
        self.fragment.write_text(
            f"[Service]\nExecStart={self.daemon} --operator-change\n",
            encoding="utf-8",
        )

        with self.assertRaisesRegex(DAEMON_SWITCH.TemporaryLaunchError, "source unit changed"):
            self.adapter.restore_exact(self.args, session)

    def test_begin_journals_dropin_before_fake_systemd_reload(self) -> None:
        journal = DAEMON_SWITCH.TemporaryLaunchJournal(self.root / "state" / "temporary-launch.json")

        def assert_durable_overlay() -> None:
            active = journal.load()
            assert active is not None
            self.assertEqual(active.phase, DAEMON_SWITCH.TemporaryLaunchPhase.OVERLAY_APPLIED)
            self.assertIsNotNone(active.overlay_reference)
            self.assertIsNotNone(active.overlay_digest)

        self.before_reload = assert_durable_overlay
        with (
            mock.patch.object(DAEMON_SWITCH, "temporary_launch_journal", return_value=journal),
            mock.patch.object(DAEMON_SWITCH, "selected_matched_pair", return_value=(self.cli, self.daemon)),
            mock.patch.object(DAEMON_SWITCH, "temporary_launch_adapter", return_value=self.adapter),
            mock.patch.object(DAEMON_SWITCH, "account_identifier", return_value="uid:501"),
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Linux"),
            mock.patch.object(DAEMON_SWITCH, "run_service"),
            mock.patch.object(DAEMON_SWITCH, "require_stopped_daemon"),
            mock.patch.object(DAEMON_SWITCH, "wait_for_temporary_launch", return_value=(True, "ready")),
            redirect_stdout(io.StringIO()),
        ):
            DAEMON_SWITCH.begin_temporary_launch(self.args)

        active = journal.load()
        assert active is not None
        self.assertEqual(active.phase, DAEMON_SWITCH.TemporaryLaunchPhase.OVERLAY_STARTED)


class SwitchModeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        """Keep resolution tests coupled to the extracted pure-resolution module."""
        cls.switcher = DAEMON_SWITCH
        globals()["DAEMON_SWITCH"] = RELEASE_RESOLUTION

    @classmethod
    def tearDownClass(cls) -> None:
        globals()["DAEMON_SWITCH"] = cls.switcher

    def test_latest_release_uses_the_github_release_api(self) -> None:
        releases = [{"draft": False, "prerelease": False, "tag_name": "v1.5.1"}]
        with mock.patch.object(DAEMON_SWITCH, "github_json", return_value=releases) as request:
            self.assertEqual(DAEMON_SWITCH.latest_published_release_version(), "1.5.1")
        request.assert_called_once_with("")

    def test_prerelease_resolution_accepts_only_prerelease_tags(self) -> None:
        releases = [{"draft": False, "prerelease": True, "tag_name": "prerelease/v1.5.11"}]
        with mock.patch.object(DAEMON_SWITCH, "github_json", return_value=releases):
            self.assertEqual(DAEMON_SWITCH.prerelease_release("latest")[0], "1.5.11")

    def test_prerelease_resolution_rejects_stable_tags(self) -> None:
        with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "prerelease Release is missing"):
            with mock.patch.object(DAEMON_SWITCH, "github_json", return_value={"draft": False, "prerelease": False}):
                DAEMON_SWITCH.prerelease_release("1.5.11")

    def test_prerelease_resolution_extracts_a_windows_zip_fixture(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            archive_buffer = io.BytesIO()
            with zipfile.ZipFile(archive_buffer, "w") as archive:
                archive.writestr("atm-1.5.11/bin/atm.exe", "fixture")
                archive.writestr("atm-1.5.11/bin/atm-daemon.exe", "fixture")
            archive_bytes = archive_buffer.getvalue()
            archive_name = "atm_1.5.11_x86_64-pc-windows-msvc.zip"
            checksum = DAEMON_SWITCH.hashlib.sha256(archive_bytes).hexdigest()
            release = {
                "assets": [
                    {"name": "checksums.txt", "browser_download_url": "checksums"},
                    {"name": archive_name, "browser_download_url": "archive"},
                ]
            }
            with (
                mock.patch.object(DAEMON_SWITCH, "PRERELEASE_INSTALL_ROOT", Path(temporary)),
                mock.patch.object(DAEMON_SWITCH, "prerelease_release", return_value=("1.5.11", release)),
                mock.patch.object(DAEMON_SWITCH, "release_archive_triple", return_value=("x86_64-pc-windows-msvc", "zip")),
                mock.patch.object(DAEMON_SWITCH, "executable_name", side_effect=lambda name: f"{name}.exe"),
                mock.patch.object(DAEMON_SWITCH, "_download", side_effect=[f"{checksum}  {archive_name}\n".encode(), archive_bytes]),
                mock.patch.object(DAEMON_SWITCH, "require_pair_version") as versions,
            ):
                cli, daemon, version = DAEMON_SWITCH.resolve_prerelease_pair("1.5.11")
            self.assertEqual(version, "1.5.11")
            self.assertTrue(cli.is_file())
            self.assertTrue(daemon.is_file())
            versions.assert_called_once_with(cli, daemon, "1.5.11")

    def test_prerelease_resolution_rejects_a_bad_checksum_without_staging(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            archive_name = "atm_1.5.11_x86_64-pc-windows-msvc.zip"
            release = {
                "assets": [
                    {"name": "checksums.txt", "browser_download_url": "checksums"},
                    {"name": archive_name, "browser_download_url": "archive"},
                ]
            }
            with (
                mock.patch.object(DAEMON_SWITCH, "PRERELEASE_INSTALL_ROOT", Path(temporary)),
                mock.patch.object(DAEMON_SWITCH, "prerelease_release", return_value=("1.5.11", release)),
                mock.patch.object(DAEMON_SWITCH, "release_archive_triple", return_value=("x86_64-pc-windows-msvc", "zip")),
                mock.patch.object(DAEMON_SWITCH, "_download", side_effect=[f"{'0' * 64}  {archive_name}\n".encode(), b"wrong archive"]),
            ):
                with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "checksum mismatch"):
                    DAEMON_SWITCH.resolve_prerelease_pair("1.5.11")
            self.assertFalse((Path(temporary) / "v1.5.11").exists())

    def test_prerelease_signing_unlocks_then_uses_the_shared_signer(self) -> None:
        cli = Path("/staged/atm")
        daemon = Path("/staged/atm-daemon")
        identity = mock.sentinel.identity
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH, "unlock_login_keychain") as unlock,
            mock.patch.object(DAEMON_SWITCH, "resolve_apple_development_identity", return_value=identity),
            mock.patch.object(DAEMON_SWITCH, "sign_and_verify_binary") as sign,
        ):
            DAEMON_SWITCH.sign_prerelease_pair(cli, daemon)
        unlock.assert_called_once_with()
        self.assertEqual(
            sign.call_args_list,
            [
                mock.call(cli, DAEMON_SWITCH.CLI_IDENTIFIER, identity),
                mock.call(daemon, DAEMON_SWITCH.DAEMON_IDENTIFIER, identity),
            ],
        )

    def test_release_resolution_uses_platform_owned_pair_and_verifies_both_versions(self) -> None:
        cli = Path("/release/bin/atm")
        daemon = Path("/release/bin/atm-daemon")
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Windows"),
            mock.patch.object(DAEMON_SWITCH, "release_install_roots", return_value=[Path("/release")]),
            mock.patch.object(DAEMON_SWITCH, "pair_from_root", return_value=(cli, daemon)),
            mock.patch.object(DAEMON_SWITCH, "require_pair_version") as versions,
        ):
            self.assertEqual(
                DAEMON_SWITCH.resolve_release_pair("1.5.1"),
                (cli, daemon, "1.5.1"),
            )
        versions.assert_called_once_with(cli, daemon, "1.5.1")

    def test_macos_release_resolution_uses_the_homebrew_pair(self) -> None:
        cli = Path("/opt/homebrew/opt/atm/bin/atm")
        daemon = Path("/opt/homebrew/opt/atm/bin/atm-daemon")
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH, "release_install_roots", return_value=[cli.parent]),
            mock.patch.object(DAEMON_SWITCH, "pair_from_root", return_value=(cli, daemon)),
            mock.patch.object(DAEMON_SWITCH, "require_pair_version"),
        ):
            self.assertEqual(
                DAEMON_SWITCH.resolve_release_pair("1.5.1"),
                (cli, daemon, "1.5.1"),
            )

    def test_linux_release_resolution_downloads_only_after_package_roots_miss(self) -> None:
        cli = Path("/cache/bin/atm")
        daemon = Path("/cache/bin/atm-daemon")
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Linux"),
            mock.patch.object(DAEMON_SWITCH, "release_install_roots", return_value=[Path("/usr/bin")]),
            mock.patch.object(DAEMON_SWITCH, "pair_from_root", return_value=None),
            mock.patch.object(DAEMON_SWITCH, "extract_linux_release_archive", return_value=(cli, daemon)) as download,
            mock.patch.object(DAEMON_SWITCH, "require_pair_version") as versions,
        ):
            self.assertEqual(
                DAEMON_SWITCH.resolve_release_pair("1.5.1"),
                (cli, daemon, "1.5.1"),
            )
        download.assert_called_once_with("1.5.1")
        versions.assert_called_once_with(cli, daemon, "1.5.1")

    def test_pair_version_refuses_a_mismatch_before_selector_mutation(self) -> None:
        cli = Path("/candidate/atm")
        daemon = Path("/candidate/atm-daemon")
        with mock.patch.object(DAEMON_SWITCH, "binary_release_version", side_effect=["1.5.1", "1.5.0"]):
            with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "both equal 1.5.1"):
                DAEMON_SWITCH.require_pair_version(cli, daemon, "1.5.1")

    def test_worktree_requires_exact_prerelease_tag_at_head_with_remedy(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            worktree = Path(temporary)
            (worktree / "Cargo.toml").write_text(
                '[workspace.package]\nversion = "1.5.1"\n', encoding="utf-8"
            )
            with mock.patch.object(
                DAEMON_SWITCH,
                "run",
                return_value=subprocess.CompletedProcess([], 0, "prerelease/v1.5.0\n", ""),
            ):
                with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "prerelease_tag.py"):
                    DAEMON_SWITCH.exact_prerelease_tag(worktree)

    def test_worktree_pair_requires_tag_and_binary_versions_before_switch(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            worktree = Path(temporary)
            (worktree / ".git").write_text("gitdir: elsewhere\n", encoding="utf-8")
            release = worktree / "target" / "release"
            release.mkdir(parents=True)
            cli = release / DAEMON_SWITCH.executable_name("atm")
            daemon = release / DAEMON_SWITCH.executable_name("atm-daemon")
            for binary in (cli, daemon):
                binary.write_text("fixture", encoding="utf-8")
                binary.chmod(0o700)
            with (
                mock.patch.object(DAEMON_SWITCH, "exact_prerelease_tag", return_value="1.5.1"),
                mock.patch.object(DAEMON_SWITCH, "require_pair_version") as versions,
            ):
                self.assertEqual(
                    DAEMON_SWITCH.prepare_worktree_pair(worktree, False),
                    (cli.resolve(), daemon.resolve(), "1.5.1"),
                )
            versions.assert_called_once_with(cli.resolve(), daemon.resolve(), "1.5.1")

    def test_raw_path_rejects_published_version_outside_release_root_without_opt_out(self) -> None:
        switcher = self.switcher
        cli = Path("/worktree/target/release/atm")
        daemon = Path("/worktree/target/release/atm-daemon")
        with (
            mock.patch.object(switcher, "binary_release_version", side_effect=["1.5.0", "1.5.0"]),
            mock.patch.object(switcher, "pair_is_in_release_install_root", return_value=False),
            mock.patch.object(switcher, "release_is_published", return_value=True),
            redirect_stdout(io.StringIO()),
        ):
            with self.assertRaisesRegex(switcher.SwitchError, "--allow-release-version"):
                switcher.validate_raw_pair_mode(cli, daemon, allow_release_version=False)

    def test_switch_parser_exposes_the_prerelease_mode(self) -> None:
        parsed = self.switcher.parser().parse_args(["switch", "--release", "latest", "--yes"])
        self.assertEqual(parsed.release, "latest")
        self.assertIsNone(parsed.prerelease)
        self.assertIsNone(parsed.worktree)
        self.assertFalse(parsed.bump)


class LegacyDaemonSwitchRegressionTests(unittest.TestCase):
    """Keep every lifecycle assertion from the retired `.just` test canonical."""

    def setUp(self) -> None:
        self.module = DAEMON_SWITCH
        self.cli_link, self.daemon_link = Path("/selectors/atm"), Path("/selectors/atm-daemon")
        self.old_cli, self.old_daemon = Path("/installed/atm"), Path("/installed/atm-daemon")
        self.new_cli, self.new_daemon = Path("/candidate/atm"), Path("/candidate/atm-daemon")
        self.args = argparse.Namespace(cli_link=None, daemon_link=None, yes=True, dry_run=False, service="atm-daemon", launch_agent_plist="/tmp/atm-daemon.plist", repair_orphan=False)

    def switch_inputs(self):
        return mock.patch.multiple(self.module, selected_links=mock.DEFAULT, validate_selectors=mock.DEFAULT, require_executable=mock.DEFAULT, save_default_pair=mock.DEFAULT, replace_link=mock.DEFAULT, run_service=mock.DEFAULT, live_pair_matches=mock.DEFAULT, require_stopped_daemon=mock.DEFAULT, require_macos_development_signatures=mock.DEFAULT)

    def test_switch_rejects_unsigned_target_before_touching_selectors_or_service(self) -> None:
        with self.switch_inputs() as patched:
            patched["selected_links"].return_value = self.cli_link, self.daemon_link
            patched["require_executable"].side_effect = [self.old_cli, self.old_daemon, self.new_cli, self.new_daemon]
            patched["require_macos_development_signatures"].side_effect = self.module.SwitchError("unsigned CLI")
            with self.assertRaisesRegex(self.module.SwitchError, "unsigned CLI"):
                self.module.switch_pair(self.args, self.new_cli, self.new_daemon)
        patched["run_service"].assert_not_called()
        patched["replace_link"].assert_not_called()

    def test_restart_rejects_unsigned_selected_pair_before_stopping_service(self) -> None:
        with (mock.patch.object(self.module, "selected_links", return_value=(self.old_cli, self.old_daemon)), mock.patch.object(self.module, "require_executable", side_effect=[self.old_cli, self.old_daemon]), mock.patch.object(self.module, "require_macos_development_signatures", side_effect=self.module.SwitchError("unsigned CLI")), mock.patch.object(self.module, "run_service") as service):
            with self.assertRaisesRegex(self.module.SwitchError, "unsigned CLI"):
                self.module.restart(argparse.Namespace(yes=True))
        service.assert_not_called()

    def test_signature_gate_rejects_signed_cli_with_unsigned_daemon(self) -> None:
        identity = type("Identity", (), {"team_identifier": "TEAMID"})()
        with (mock.patch.object(RELEASE_RESOLUTION.platform, "system", return_value="Darwin"), mock.patch.object(RELEASE_RESOLUTION, "resolve_apple_development_identity", return_value=identity), mock.patch.object(RELEASE_RESOLUTION, "macos_binary_has_development_signature", side_effect=[True, False]) as verify):
            with self.assertRaisesRegex(RELEASE_RESOLUTION.SwitchError, "daemon target is not strictly signed"):
                RELEASE_RESOLUTION.require_macos_development_signatures(self.new_cli, self.new_daemon)
        self.assertEqual(verify.call_count, 2)

    def test_switch_pair_stops_then_replaces_both_selectors_then_starts(self) -> None:
        with self.switch_inputs() as patched:
            patched["selected_links"].return_value = self.cli_link, self.daemon_link
            patched["require_executable"].side_effect = [self.old_cli, self.old_daemon, self.new_cli, self.new_daemon]
            patched["live_pair_matches"].return_value = True, "matched"
            self.module.switch_pair(self.args, self.new_cli, self.new_daemon)
        self.assertEqual(patched["run_service"].call_args_list, [mock.call(self.args, "stop", allow_absent=True), mock.call(self.args, "start")])
        self.assertEqual(patched["replace_link"].call_args_list, [mock.call(self.cli_link, self.new_cli), mock.call(self.daemon_link, self.new_daemon)])

    def test_switch_pair_rolls_back_both_selectors_and_restarts_after_replace_failure(self) -> None:
        with self.switch_inputs() as patched:
            patched["selected_links"].return_value = self.cli_link, self.daemon_link
            patched["require_executable"].side_effect = [self.old_cli, self.old_daemon, self.new_cli, self.new_daemon]
            patched["live_pair_matches"].return_value = True, "matched"
            patched["replace_link"].side_effect = [None, OSError("replace failed"), None, None]
            with self.assertRaises(OSError):
                self.module.switch_pair(self.args, self.new_cli, self.new_daemon)
        self.assertEqual(patched["replace_link"].call_count, 4)
        self.assertEqual(patched["run_service"].call_args_list[-1], mock.call(self.args, "start"))

    def test_switch_pair_repairs_dangling_selectors_only_with_explicit_repair(self) -> None:
        self.args.repair_orphan = True
        with self.switch_inputs() as patched:
            patched["selected_links"].return_value = self.cli_link, self.daemon_link
            patched["require_executable"].side_effect = [self.module.SwitchError("missing"), self.new_cli, self.new_daemon]
            patched["live_pair_matches"].return_value = True, "matched"
            self.module.switch_pair(self.args, self.new_cli, self.new_daemon)
        patched["save_default_pair"].assert_not_called()
        patched["require_stopped_daemon"].assert_called_once_with(self.args, self.cli_link)

    def test_invalid_selector_is_rejected_before_service_stop(self) -> None:
        with self.switch_inputs() as patched:
            patched["selected_links"].return_value = self.cli_link, self.daemon_link
            patched["validate_selectors"].side_effect = self.module.SwitchError("not symlinks")
            with self.assertRaisesRegex(self.module.SwitchError, "not symlinks"):
                self.module.switch_pair(self.args, self.new_cli, self.new_daemon)
        patched["run_service"].assert_not_called()

    def test_switch_pair_rolls_back_when_live_doctor_reports_the_old_daemon(self) -> None:
        with self.switch_inputs() as patched:
            patched["selected_links"].return_value = self.cli_link, self.daemon_link
            patched["require_executable"].side_effect = [self.old_cli, self.old_daemon, self.new_cli, self.new_daemon]
            patched["live_pair_matches"].return_value = False, "selected beta.29, daemon beta.24"
            with self.assertRaisesRegex(self.module.SwitchError, "split CLI/daemon pair"):
                self.module.switch_pair(self.args, self.new_cli, self.new_daemon)
        self.assertEqual(patched["replace_link"].call_count, 4)

    def test_reachable_daemon_requires_explicit_orphan_repair(self) -> None:
        with (mock.patch.object(self.module, "macos_daemon_owner_pids", return_value=[42]), mock.patch.object(self.module.platform, "system", return_value="Darwin")):
            with self.assertRaisesRegex(self.module.SwitchError, "refuse a split pair"):
                self.module.require_stopped_daemon(argparse.Namespace(repair_orphan=False), self.old_cli)

    def test_restart_requires_a_single_live_pair_after_controlled_stop(self) -> None:
        args = argparse.Namespace(yes=True)
        with (mock.patch.object(self.module, "selected_links", return_value=(self.old_cli, self.old_daemon)), mock.patch.object(self.module, "require_executable", side_effect=[self.old_cli, self.old_daemon]), mock.patch.object(self.module, "require_macos_development_signatures"), mock.patch.object(self.module, "herdr_restart_pending_endpoints", return_value=[]), mock.patch.object(self.module, "run_service") as service, mock.patch.object(self.module, "require_stopped_daemon") as stopped, mock.patch.object(self.module, "live_pair_matches", return_value=(True, "matched"))):
            self.module.restart(args)
        stopped.assert_called_once_with(args, self.old_cli)
        self.assertEqual(service.call_args_list, [mock.call(args, "stop", allow_absent=True), mock.call(args, "start")])

    def test_restart_repairs_one_verified_orphan_then_rebootstraps_the_selected_agent(self) -> None:
        args = argparse.Namespace(yes=True, repair_orphan=False)
        with (
            mock.patch.object(self.module, "selected_links", return_value=(self.old_cli, self.old_daemon)),
            mock.patch.object(self.module, "require_executable", side_effect=[self.old_cli, self.old_daemon]),
            mock.patch.object(self.module, "require_macos_development_signatures"),
            mock.patch.object(self.module, "herdr_restart_pending_endpoints", return_value=[]),
            mock.patch.object(self.module, "platform") as platform,
            mock.patch.object(self.module, "run_service") as service,
            mock.patch.object(self.module, "require_stopped_daemon", side_effect=[self.module.SwitchError("owner remains"), None]) as stopped,
            mock.patch.object(self.module, "macos_daemon_owner_pids", return_value=[42]),
            mock.patch.object(self.module, "repair_macos_orphan") as repair,
            mock.patch.object(self.module, "live_pair_matches", return_value=(True, "matched")),
        ):
            platform.system.return_value = "Darwin"
            self.module.restart(args)
        repair.assert_called_once_with([42])
        self.assertEqual(stopped.call_count, 2)
        self.assertEqual(
            service.call_args_list,
            [mock.call(args, "stop", allow_absent=True), mock.call(args, "start")],
        )

    def test_restart_rebootstraps_when_orphan_repair_fails(self) -> None:
        args = argparse.Namespace(yes=True, repair_orphan=False)
        with (
            mock.patch.object(self.module, "selected_links", return_value=(self.old_cli, self.old_daemon)),
            mock.patch.object(self.module, "require_executable", side_effect=[self.old_cli, self.old_daemon]),
            mock.patch.object(self.module, "require_macos_development_signatures"),
            mock.patch.object(self.module, "herdr_restart_pending_endpoints", return_value=[]),
            mock.patch.object(self.module, "platform") as platform,
            mock.patch.object(self.module, "run_service") as service,
            mock.patch.object(self.module, "require_stopped_daemon", side_effect=self.module.SwitchError("owner remains")),
            mock.patch.object(self.module, "macos_daemon_owner_pids", return_value=[42]),
            mock.patch.object(self.module, "repair_macos_orphan", side_effect=self.module.SwitchError("SIGTERM failed")),
        ):
            platform.system.return_value = "Darwin"
            with self.assertRaisesRegex(self.module.SwitchError, "after re-bootstrapping"):
                self.module.restart(args)
        self.assertEqual(
            service.call_args_list,
            [mock.call(args, "stop", allow_absent=True), mock.call(args, "start")],
            "recovery must attempt to re-bootstrap the selected LaunchAgent",
        )

    def test_restart_reports_rebootstrap_failure_after_orphan_repair_failure(self) -> None:
        args = argparse.Namespace(yes=True, repair_orphan=False)
        with (
            mock.patch.object(self.module, "selected_links", return_value=(self.old_cli, self.old_daemon)),
            mock.patch.object(self.module, "require_executable", side_effect=[self.old_cli, self.old_daemon]),
            mock.patch.object(self.module, "require_macos_development_signatures"),
            mock.patch.object(self.module, "herdr_restart_pending_endpoints", return_value=[]),
            mock.patch.object(self.module, "platform") as platform,
            mock.patch.object(self.module, "run_service", side_effect=[None, self.module.SwitchError("bootstrap failed")]) as service,
            mock.patch.object(self.module, "require_stopped_daemon", side_effect=self.module.SwitchError("owner remains")),
            mock.patch.object(self.module, "macos_daemon_owner_pids", return_value=[42]),
            mock.patch.object(self.module, "repair_macos_orphan", side_effect=self.module.SwitchError("SIGTERM failed")),
        ):
            platform.system.return_value = "Darwin"
            with self.assertRaisesRegex(self.module.SwitchError, "could not be re-bootstrapped"):
                self.module.restart(args)
        self.assertEqual(
            service.call_args_list,
            [mock.call(args, "stop", allow_absent=True), mock.call(args, "start")],
            "even a failed re-bootstrap is attempted and reported explicitly",
        )

    def test_restore_prefers_homebrew_then_explicit_then_saved_state(self) -> None:
        explicit = argparse.Namespace(default_cli="/explicit/atm", default_daemon="/explicit/atm-daemon")
        with mock.patch.object(self.module, "homebrew_pair", return_value=(self.old_cli, self.old_daemon)):
            self.assertEqual(self.module.restore_pair(explicit), (self.old_cli, self.old_daemon))
        with mock.patch.object(self.module, "homebrew_pair", return_value=None):
            self.assertEqual(self.module.restore_pair(explicit), (Path("/explicit/atm"), Path("/explicit/atm-daemon")))
        with (mock.patch.object(self.module, "homebrew_pair", return_value=None), mock.patch.object(self.module, "load_state", return_value={"default_cli": "/saved/atm", "default_daemon": "/saved/atm-daemon"})):
            self.assertEqual(self.module.restore_pair(argparse.Namespace(default_cli=None, default_daemon=None)), (Path("/saved/atm"), Path("/saved/atm-daemon")))

    def test_documented_post_subcommand_service_options_parse(self) -> None:
        parsed = self.module.parser().parse_args(["switch", "--cli", "/candidate/atm", "--daemon", "/candidate/atm-daemon", "--yes", "--service", "atm-daemon", "--launch-agent-plist", "/tmp/atm-daemon.plist"])
        self.assertEqual((parsed.service, parsed.launch_agent_plist), ("atm-daemon", "/tmp/atm-daemon.plist"))

    def test_windows_status_reports_absent_task(self) -> None:
        missing = subprocess.CompletedProcess(["schtasks.exe"], 1, "", "ERROR: The system cannot find the file specified.\r\n")
        with mock.patch.object(self.module, "run", return_value=missing) as run:
            self.assertEqual(self.module.windows_task_status("atm-daemon")["state"], "absent")
        run.assert_called_once_with(["schtasks.exe", "/Query", "/TN", "atm-daemon", "/XML"], timeout=5.0)

    def test_windows_optional_stop_does_not_hide_access_denied(self) -> None:
        denied = subprocess.CompletedProcess(["schtasks.exe"], 1, "", "ERROR: Access is denied.")
        with (mock.patch.object(self.module.platform, "system", return_value="Windows"), mock.patch.object(self.module, "run", return_value=denied)):
            with self.assertRaisesRegex(self.module.SwitchError, "Access is denied"):
                self.module.run_service(argparse.Namespace(service="atm-daemon"), "stop", allow_absent=True)

    def test_macos_start_retries_bootstrap_after_unload_race(self) -> None:
        args = argparse.Namespace(service="atm-daemon", launch_agent_plist="/tmp/atm-daemon.plist")
        results = [subprocess.CompletedProcess([], 5, "", "Bootstrap failed: 5"), subprocess.CompletedProcess([], 1, "", "not loaded"), subprocess.CompletedProcess([], 0, "", ""), subprocess.CompletedProcess([], 0, "path = /tmp/atm-daemon.plist\n", "")]
        with (mock.patch.object(self.module.platform, "system", return_value="Darwin"), mock.patch.object(self.module.os, "getuid", return_value=501, create=True), mock.patch.object(self.module, "run", side_effect=results), mock.patch.object(self.module.time, "sleep")):
            self.module.run_service(args, "start")

    def test_macos_stop_repairs_one_verified_orphan_after_bootout(self) -> None:
        args = argparse.Namespace(service="atm-daemon", launch_agent_plist="/tmp/atm-daemon.plist", repair_orphan=True)
        loaded, unloaded = subprocess.CompletedProcess([], 0, "", ""), subprocess.CompletedProcess([], 1, "", "not loaded")
        with (mock.patch.object(self.module.platform, "system", return_value="Darwin"), mock.patch.object(self.module.os, "getuid", return_value=501, create=True), mock.patch.object(self.module, "run", side_effect=[subprocess.CompletedProcess([], 0, "", ""), *([loaded] * 20), unloaded]), mock.patch.object(self.module, "macos_daemon_owner_pids", return_value=[42]), mock.patch.object(self.module, "repair_macos_orphan") as repair, mock.patch.object(self.module.time, "sleep")):
            self.module.run_service(args, "stop", allow_absent=True)
        repair.assert_called_once_with([42])

    def test_live_pair_doctor_uses_a_home_directory_not_the_calling_worktree(self) -> None:
        report = {"client_context": {"version": "1.3.2-beta.29"}, "daemon_context": {"version": "1.3.2-beta.29"}}
        with (mock.patch.object(self.module, "selected_release_version", return_value="1.3.2-beta.29"), mock.patch.object(self.module, "run", return_value=subprocess.CompletedProcess([], 0, json.dumps(report), "")) as run):
            matched, _detail = self.module.live_pair_matches(self.new_cli)
        self.assertTrue(matched)
        self.assertEqual(run.call_args.kwargs["cwd"], Path.home())


class HerdrEntryPlatformFake:
    def __init__(self, root: Path, name: str = "Linux") -> None:
        self.root = root
        self.name = name
        self.registered: set[str] = set()
        self.account_is_current = True
        self.account_checks: list[str] = []
        self.started: list[str] = []

    def path_for(self, identifier: str) -> Path:
        return self.root / identifier

    def register(self, identifier: str, _object_path: Path) -> None:
        self.registered.add(identifier)

    def unregister(self, identifier: str) -> None:
        self.registered.discard(identifier)

    def is_registered(self, identifier: str) -> bool:
        return identifier in self.registered

    def start(self, identifier: str, _timeout: float = 30.0) -> None:
        if identifier not in self.registered:
            raise RuntimeError("entry is not registered")
        self.started.append(identifier)

    def account_matches(self, identifier: str) -> bool:
        self.account_checks.append(identifier)
        return self.account_is_current


class HerdrEntryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.platform = HerdrEntryPlatformFake(self.root / "objects")
        self.manager = DAEMON_SWITCH.HerdrEntryManager(self.root / "journal", self.platform)
        self.default = DAEMON_SWITCH.HerdrEndpoint("default")
        self.session = DAEMON_SWITCH.HerdrEndpoint("blue")

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_identifiers_are_deterministic_for_each_platform(self) -> None:
        self.assertEqual(DAEMON_SWITCH.identifier("Darwin", "default"), "com.randlee.atm.herdr-server")
        self.assertEqual(DAEMON_SWITCH.identifier("Darwin", "blue"), "com.randlee.atm.herdr-server.blue")
        self.assertEqual(DAEMON_SWITCH.identifier("Linux", "default"), "atm-herdr-server.service")
        self.assertEqual(DAEMON_SWITCH.identifier("Windows", "blue"), "ATM Herdr Server (blue)")

    def test_default_and_sessions_are_independently_owned_and_registered(self) -> None:
        entries = [self.manager.install(endpoint) for endpoint in (self.default, self.session)]
        self.assertEqual([entry["endpoint"] for entry in entries], ["default", "blue"])
        self.assertTrue(all(entry["owned"] and entry["registered"] and entry["digest_matches"] for entry in entries))
        self.assertFalse(self.manager.journal_path.exists())

    def test_each_platform_fake_supports_install_status_remove(self) -> None:
        for name in ("Darwin", "Linux", "Windows"):
            with self.subTest(platform=name):
                platform_fake = HerdrEntryPlatformFake(self.root / name, name)
                manager = DAEMON_SWITCH.HerdrEntryManager(self.root / f"{name}-journal", platform_fake)
                installed = manager.install(self.default)
                self.assertTrue(installed["owned"] and installed["registered"])
                removed = manager.remove(self.default)
                self.assertFalse(removed["owned"] or removed["registered"])

    def test_foreign_digest_and_socket_path_refuse_without_mutation(self) -> None:
        path = self.platform.path_for(DAEMON_SWITCH.identifier("Linux", "default"))
        path.parent.mkdir(parents=True)
        path.write_text("foreign", encoding="utf-8")
        with self.assertRaisesRegex(DAEMON_SWITCH.HerdrEntryError, "unowned"):
            self.manager.install(self.default)
        self.assertEqual(path.read_text(encoding="utf-8"), "foreign")
        with self.assertRaisesRegex(DAEMON_SWITCH.HerdrEntryError, "externally owned"):
            self.manager.install(DAEMON_SWITCH.HerdrEndpoint("socket", "/tmp/herdr.sock"))

    def test_interrupted_install_blocks_then_repair_rolls_back_or_completes(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "after write"):
            self.manager.install(self.default, lambda phase: (_ for _ in ()).throw(RuntimeError("after write")) if phase == "after_write" else None)
        self.assertTrue(self.manager.journal_path.exists())
        with self.assertRaisesRegex(DAEMON_SWITCH.HerdrEntryError, "incomplete"):
            self.manager.install(self.default)
        self.assertEqual(self.manager.repair(), {"repaired": "rolled_back"})
        self.assertFalse(self.platform.path_for("atm-herdr-server.service").exists())

        with self.assertRaisesRegex(RuntimeError, "after register"):
            self.manager.install(self.default, lambda phase: (_ for _ in ()).throw(RuntimeError("after register")) if phase == "after_register" else None)
        self.assertEqual(self.manager.repair(), {"repaired": "completed"})
        self.assertTrue(self.platform.is_registered("atm-herdr-server.service"))

    def test_remove_touches_only_matching_owned_object(self) -> None:
        self.manager.install(self.default)
        self.manager.remove(self.default)
        self.assertFalse(self.platform.path_for("atm-herdr-server.service").exists())
        self.assertFalse(self.platform.is_registered("atm-herdr-server.service"))

    def test_windows_account_mismatch_refuses_before_write(self) -> None:
        platform_fake = HerdrEntryPlatformFake(self.root / "windows", "Windows")
        platform_fake.account_is_current = False
        manager = DAEMON_SWITCH.HerdrEntryManager(self.root / "windows-journal", platform_fake)
        with self.assertRaisesRegex(DAEMON_SWITCH.HerdrEntryError, "another account") as captured:
            manager.install(self.default)
        self.assertEqual(captured.exception.code, "HERDR_ENTRY_ACCOUNT_MISMATCH")
        self.assertEqual(captured.exception.remedy, "Reinstall both per-user under one account")
        self.assertFalse(platform_fake.path_for("ATM Herdr Server").exists())
        self.assertEqual(platform_fake.registered, set())
        self.assertFalse(manager.journal_path.exists())

    def test_windows_installs_one_interactive_logon_task_per_distinct_endpoint(self) -> None:
        platform_fake = HerdrEntryPlatformFake(self.root / "windows", "Windows")
        manager = DAEMON_SWITCH.HerdrEntryManager(self.root / "windows-journal", platform_fake)
        endpoints = [
            DAEMON_SWITCH.HerdrEndpoint("default"),
            DAEMON_SWITCH.HerdrEndpoint("blue"),
            DAEMON_SWITCH.HerdrEndpoint("green"),
        ]

        entries = [manager.install(endpoint) for endpoint in endpoints]

        expected = {
            "ATM Herdr Server",
            "ATM Herdr Server (blue)",
            "ATM Herdr Server (green)",
        }
        self.assertEqual(platform_fake.registered, expected)
        self.assertEqual(
            platform_fake.account_checks,
            ["ATM Herdr Server", "ATM Herdr Server (blue)", "ATM Herdr Server (green)"],
        )
        self.assertTrue(all(entry["owned"] and entry["registered"] for entry in entries))
        for entry_id in expected:
            rendered = platform_fake.path_for(entry_id).read_text(encoding="utf-8")
            self.assertIn("trigger=logon", rendered)
            self.assertIn("interactive=true", rendered)
            self.assertIn("managed-by=atm daemon-switch", rendered)

    def test_doctor_ingestion_uses_only_native_projection(self) -> None:
        payload = {"herdr": {"configured": True, "endpoints": [{"endpoint": "default"}, {"session": "blue"}]}}
        with mock.patch.object(DAEMON_SWITCH, "doctor", return_value=payload):
            endpoints = DAEMON_SWITCH.herdr_entry_endpoints(Path("/selected/atm"), install=True)
        self.assertEqual([endpoint.name for endpoint in endpoints], ["default", "blue"])
        with mock.patch.object(DAEMON_SWITCH, "doctor", return_value={"herdr": {"configured": False, "endpoints": []}}):
            with self.assertRaisesRegex(DAEMON_SWITCH.HerdrEntryError, "not configured"):
                DAEMON_SWITCH.herdr_entry_endpoints(Path("/selected/atm"), install=True)

    def test_doctor_ingestion_rejects_null_missing_malformed_and_error(self) -> None:
        invalid_payloads = [
            {"herdr": {"configured": None, "endpoints": []}},
            {"herdr": {"configured": True}},
            {"herdr": {"configured": True, "endpoints": ["not-an-object"]}},
            {"error": "doctor failed"},
        ]
        for payload in invalid_payloads:
            with self.subTest(payload=payload), mock.patch.object(DAEMON_SWITCH, "doctor", return_value=payload):
                with self.assertRaisesRegex(DAEMON_SWITCH.HerdrEntryError, "doctor"):
                    DAEMON_SWITCH.herdr_entry_endpoints(Path("/selected/atm"), install=True)

    def test_entry_result_is_exactly_one_json_object(self) -> None:
        output = io.StringIO()
        with redirect_stdout(output):
            DAEMON_SWITCH.herdr_entry_result(True, "HERDR_ENTRY_STATUS_OK", "ok", "none", [])
        self.assertEqual(json.loads(output.getvalue()), {"ok": True, "code": "HERDR_ENTRY_STATUS_OK", "message": "ok", "remedy": "none", "entries": []})

    def test_ordinary_switch_lifecycle_has_no_implicit_entry_invocation(self) -> None:
        source = SCRIPT.read_text(encoding="utf-8")
        switch_block = source.split('elif args.command == "switch":', 1)[1].split('elif args.command == "restore":', 1)[0]
        restore_block = source.split('elif args.command == "restore":', 1)[1].split('elif args.command == "restart":', 1)[0]
        restart_block = source.split('elif args.command == "restart":', 1)[1].split('elif args.command == "temporary-launch":', 1)[0]
        self.assertNotIn("run_herdr_entry", switch_block)
        self.assertNotIn("run_herdr_entry", restore_block)
        self.assertNotIn("run_herdr_entry", restart_block)


class HerdrRestartTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.platform = HerdrEntryPlatformFake(self.root / "objects")
        self.manager = DAEMON_SWITCH.HerdrEntryManager(self.root / "journal", self.platform)
        self.default = DAEMON_SWITCH.HerdrEndpoint("default")
        self.session = DAEMON_SWITCH.HerdrEndpoint("blue")
        self.manager.install(self.default)
        self.manager.install(self.session)
        self.args = argparse.Namespace(restart_herdr="", stop_herdr_panes=False, restart_timeout_secs=120.0)
        self.cli = Path("/selected/atm")

    def tearDown(self) -> None:
        self.temporary.cleanup()

    @staticmethod
    def endpoint(name: str = "default", *, state: str = "client_server_mismatch", handoff: bool | None = True, client: str | None = "0.8.2", server: str | None = "0.8.0", provenance: str = "herdr_default") -> dict[str, object]:
        return {
            "session": name,
            "provenance": provenance,
            "endpoint": "/safe/socket" if provenance == "socket_path" else None,
            "state": {"kind": state, "client": client, "server": server},
            "capabilities": {"live_handoff": handoff},
        }

    @classmethod
    def doctor_payload(cls, *endpoints: dict[str, object]) -> dict[str, object]:
        return {"herdr": {"configured": True, "endpoints": list(endpoints)}}

    def test_selector_refusals_and_socket_path_are_exact(self) -> None:
        with mock.patch.object(DAEMON_SWITCH, "doctor", return_value=self.doctor_payload(self.endpoint(), self.endpoint("blue"))):
            with self.assertRaisesRegex(DAEMON_SWITCH.HerdrEntryError, "more than one") as error:
                DAEMON_SWITCH.select_herdr_restart_endpoint(self.cli, None)
            self.assertEqual(error.exception.code, "HERDR_RESTART_ENDPOINT_REQUIRED")
            with self.assertRaisesRegex(DAEMON_SWITCH.HerdrEntryError, "not configured") as error:
                DAEMON_SWITCH.select_herdr_restart_endpoint(self.cli, "missing")
            self.assertEqual(error.exception.code, "HERDR_RESTART_ENDPOINT_UNKNOWN")
        with mock.patch.object(DAEMON_SWITCH, "doctor", return_value=self.doctor_payload(self.endpoint(provenance="socket_path"))):
            with self.assertRaisesRegex(DAEMON_SWITCH.HerdrEntryError, "externally owned") as error:
                DAEMON_SWITCH.select_herdr_restart_endpoint(self.cli, None)
            self.assertEqual(error.exception.code, "HERDR_RESTART_SOCKET_PATH")

    def test_restart_entry_argv_and_identifiers_are_platform_specific(self) -> None:
        cases = {
            "Darwin": ("com.randlee.atm.herdr-server.blue", ["launchctl", "kickstart", "-k", f"gui/{os.getuid()}/com.randlee.atm.herdr-server.blue"]),
            "Linux": ("atm-herdr-server@blue.service", ["systemctl", "--user", "restart", "atm-herdr-server@blue.service"]),
            "Windows": ("ATM Herdr Server (blue)", ["schtasks.exe", "/Run", "/TN", "ATM Herdr Server (blue)"]),
        }
        for platform_name, (entry_id, expected) in cases.items():
            with self.subTest(platform=platform_name):
                runner = mock.Mock(return_value=subprocess.CompletedProcess([], 0, "", ""))
                adapter = DAEMON_SWITCH.NativeEntryPlatform(self.root / platform_name, runner)
                adapter.name = platform_name
                self.assertEqual(DAEMON_SWITCH.identifier(platform_name, "blue"), entry_id)
                adapter.start(entry_id, 30.0)
                self.assertEqual(runner.call_args.args[0], expected)

    def test_live_handoff_scopes_default_and_never_restarts_atm(self) -> None:
        mismatch = self.doctor_payload(self.endpoint())
        ready = self.doctor_payload(self.endpoint(state="ok", client=None, server=None))
        with (
            mock.patch.object(DAEMON_SWITCH, "selected_links", return_value=(self.cli, Path("/selected/atm-daemon"))),
            mock.patch.object(DAEMON_SWITCH, "herdr_entry_manager", return_value=self.manager),
            mock.patch.object(DAEMON_SWITCH, "doctor", side_effect=[mismatch, ready]),
            mock.patch.object(DAEMON_SWITCH, "run", return_value=subprocess.CompletedProcess([], 0, "", "")) as runner,
            mock.patch.object(DAEMON_SWITCH, "run_service") as service,
        ):
            DAEMON_SWITCH.run_herdr_restart(self.args)
        runner.assert_called_once_with(["herdr", "server", "live-handoff"], timeout=mock.ANY)
        service.assert_not_called()
        self.assertEqual(self.platform.started, [])

    def test_stop_requires_ack_then_relaunches_scoped_session_and_verifies(self) -> None:
        self.args.restart_herdr = "blue"
        mismatch = self.doctor_payload(self.endpoint("blue", handoff=None))
        ready = self.doctor_payload(self.endpoint("blue", state="ok", handoff=None, client=None, server=None))
        with (
            mock.patch.object(DAEMON_SWITCH, "selected_links", return_value=(self.cli, Path("/selected/atm-daemon"))),
            mock.patch.object(DAEMON_SWITCH, "herdr_entry_manager", return_value=self.manager),
            mock.patch.object(DAEMON_SWITCH, "doctor", return_value=mismatch),
        ):
            with self.assertRaisesRegex(DAEMON_SWITCH.HerdrEntryError, "terminates") as error:
                DAEMON_SWITCH.run_herdr_restart(self.args)
        self.assertEqual(error.exception.code, "HERDR_RESTART_NO_LIVE_HANDOFF")
        self.args.stop_herdr_panes = True
        with (
            mock.patch.object(DAEMON_SWITCH, "selected_links", return_value=(self.cli, Path("/selected/atm-daemon"))),
            mock.patch.object(DAEMON_SWITCH, "herdr_entry_manager", return_value=self.manager),
            mock.patch.object(DAEMON_SWITCH, "doctor", side_effect=[mismatch, ready]),
            mock.patch.object(DAEMON_SWITCH, "run", return_value=subprocess.CompletedProcess([], 0, "", "")) as runner,
        ):
            DAEMON_SWITCH.run_herdr_restart(self.args)
        runner.assert_called_once_with(["herdr", "--session", "blue", "server", "stop"], timeout=mock.ANY)
        self.assertEqual(self.platform.started, ["atm-herdr-server@blue.service"])

    def test_hung_command_and_slow_ready_fixture_are_bounded_without_real_sleep(self) -> None:
        mismatch = self.doctor_payload(self.endpoint(handoff=True))
        with (
            mock.patch.object(DAEMON_SWITCH, "selected_links", return_value=(self.cli, Path("/selected/atm-daemon"))),
            mock.patch.object(DAEMON_SWITCH, "herdr_entry_manager", return_value=self.manager),
            mock.patch.object(DAEMON_SWITCH, "doctor", return_value=mismatch),
            mock.patch.object(DAEMON_SWITCH, "run", side_effect=subprocess.TimeoutExpired(["herdr"], 30)),
        ):
            with self.assertRaisesRegex(DAEMON_SWITCH.HerdrEntryError, "timed out") as error:
                DAEMON_SWITCH.run_herdr_restart(self.args)
        self.assertEqual(error.exception.code, "HERDR_RESTART_TIMEOUT")

        ready = self.doctor_payload(self.endpoint(state="ok", client=None, server=None))
        with (
            mock.patch.object(DAEMON_SWITCH, "selected_links", return_value=(self.cli, Path("/selected/atm-daemon"))),
            mock.patch.object(DAEMON_SWITCH, "herdr_entry_manager", return_value=self.manager),
            mock.patch.object(DAEMON_SWITCH, "doctor", side_effect=[mismatch, mismatch, mismatch, ready]),
            mock.patch.object(DAEMON_SWITCH, "run", return_value=subprocess.CompletedProcess([], 0, "", "")),
            mock.patch.object(DAEMON_SWITCH.time, "sleep") as sleeper,
        ):
            DAEMON_SWITCH.run_herdr_restart(self.args)
        self.assertEqual(sleeper.call_args_list, [mock.call(2.0), mock.call(4.0)])

    def test_ordinary_restart_refuses_every_protocol_mismatch_before_service_mutation(self) -> None:
        args = argparse.Namespace(command="restart", yes=True)
        payload = self.doctor_payload(self.endpoint(), self.endpoint("blue"), self.endpoint("green", state="ok", client=None, server=None))
        stdout = io.StringIO()
        stderr = io.StringIO()
        with (
            mock.patch.object(DAEMON_SWITCH, "selected_links", return_value=(self.cli, Path("/selected/atm-daemon"))),
            mock.patch.object(DAEMON_SWITCH, "require_executable", side_effect=[self.cli, Path("/selected/atm-daemon")]),
            mock.patch.object(DAEMON_SWITCH, "require_macos_development_signatures"),
            mock.patch.object(DAEMON_SWITCH, "doctor", return_value=payload),
            mock.patch.object(DAEMON_SWITCH, "run_service") as service,
            mock.patch.object(
                DAEMON_SWITCH,
                "parser",
                return_value=mock.Mock(parse_args=mock.Mock(return_value=args)),
            ),
        ):
            with redirect_stdout(stdout), redirect_stderr(stderr):
                exit_code = DAEMON_SWITCH.main()
        service.assert_not_called()
        self.assertEqual(exit_code, 3)
        self.assertEqual(stderr.getvalue(), "")
        self.assertEqual(
            json.loads(stdout.getvalue()),
            {
                "ok": False,
                "code": "HERDR_RESTART_ENDPOINTS_PENDING",
                "message": "restart Herdr endpoints first: default, blue",
                "remedy": "Restart every listed Herdr endpoint first, then rerun the ordinary ATM restart",
                "entries": [
                    {
                        "endpoint": name,
                        "identifier": DAEMON_SWITCH.identifier(DAEMON_SWITCH.platform.system(), name),
                    }
                    for name in ("default", "blue")
                ],
            },
        )

    def test_restart_refusal_envelope_has_only_safe_endpoint_identifiers(self) -> None:
        with mock.patch.object(DAEMON_SWITCH, "doctor", return_value=self.doctor_payload(self.endpoint(), self.endpoint("blue"))):
            with self.assertRaises(DAEMON_SWITCH.HerdrEntryError) as captured:
                DAEMON_SWITCH.select_herdr_restart_endpoint(self.cli, None)
        self.assertEqual(
            captured.exception.entries,
            [
                {"endpoint": "default", "identifier": DAEMON_SWITCH.identifier(DAEMON_SWITCH.platform.system(), "default")},
                {"endpoint": "blue", "identifier": DAEMON_SWITCH.identifier(DAEMON_SWITCH.platform.system(), "blue")},
            ],
        )


if __name__ == "__main__":
    unittest.main()
