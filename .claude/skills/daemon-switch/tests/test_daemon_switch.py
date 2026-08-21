"""Regression tests for the daemon-switch singleton cleanup rules."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import socket
import subprocess
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).parents[1] / "scripts" / "daemon-switch.py"
SPEC = importlib.util.spec_from_file_location("daemon_switch", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
DAEMON_SWITCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DAEMON_SWITCH)


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


class MacosDevelopmentSigningTests(unittest.TestCase):
    def test_identity_discovery_requires_the_exact_quoted_identity(self) -> None:
        partial = subprocess.CompletedProcess(
            ["security"], 0, stdout='1) HASH "not-atm-daemon-dev"\n', stderr=""
        )
        exact = subprocess.CompletedProcess(
            ["security"], 0, stdout='1) HASH "atm-daemon-dev"\n', stderr=""
        )
        with (
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Darwin"),
            mock.patch.object(DAEMON_SWITCH.shutil, "which", return_value="/usr/bin/security"),
            mock.patch.object(DAEMON_SWITCH, "run", side_effect=[partial, exact]),
        ):
            self.assertFalse(DAEMON_SWITCH.macos_development_signing_identity_available())
            self.assertTrue(DAEMON_SWITCH.macos_development_signing_identity_available())

    def test_signer_and_switcher_share_the_identity_detection_helper(self) -> None:
        signing_module = SCRIPT.parents[4] / ".just" / "sign_daemon_dev.py"
        spec = importlib.util.spec_from_file_location("sign_daemon_dev_for_gate_test", signing_module)
        assert spec is not None and spec.loader is not None
        signer = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(signer)

        self.assertIs(
            DAEMON_SWITCH.find_identity_output_has_development_identity,
            signer.find_identity_output_has_development_identity,
        )
        self.assertEqual(
            DAEMON_SWITCH.DEVELOPMENT_SIGNING_IDENTITY,
            signer.DEVELOPMENT_SIGNING_IDENTITY,
        )

    def test_no_signing_gate_when_identity_is_not_installed(self) -> None:
        daemon = Path("/candidate/atm-daemon")
        with (
            mock.patch.object(DAEMON_SWITCH, "macos_development_signing_identity_available", return_value=False),
            mock.patch.object(DAEMON_SWITCH, "macos_binary_has_development_signature") as signed,
        ):
            DAEMON_SWITCH.require_macos_development_signatures(Path("/candidate/atm"), daemon)
        signed.assert_not_called()

    def test_rejects_unsigned_cli_before_daemon_when_development_identity_is_installed(self) -> None:
        cli = Path("/candidate/atm")
        daemon = Path("/candidate/atm-daemon")
        with (
            mock.patch.object(DAEMON_SWITCH, "macos_development_signing_identity_available", return_value=True),
            mock.patch.object(DAEMON_SWITCH, "macos_binary_has_development_signature", return_value=False),
        ):
            with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "just build"):
                DAEMON_SWITCH.require_macos_development_signatures(cli, daemon)

    def test_rejects_unsigned_daemon_after_accepting_signed_cli(self) -> None:
        cli = Path("/candidate/atm")
        daemon = Path("/candidate/atm-daemon")
        with (
            mock.patch.object(DAEMON_SWITCH, "macos_development_signing_identity_available", return_value=True),
            mock.patch.object(
                DAEMON_SWITCH,
                "macos_binary_has_development_signature",
                side_effect=[True, False],
            ) as signed,
        ):
            with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "daemon target"):
                DAEMON_SWITCH.require_macos_development_signatures(cli, daemon)
        self.assertEqual(signed.call_args_list, [mock.call(cli), mock.call(daemon)])

    def test_accepts_cli_and_daemon_with_exact_development_authority(self) -> None:
        cli = Path("/candidate/atm")
        daemon = Path("/candidate/atm-daemon")
        with (
            mock.patch.object(DAEMON_SWITCH, "macos_development_signing_identity_available", return_value=True),
            mock.patch.object(DAEMON_SWITCH, "macos_binary_has_development_signature", return_value=True) as signed,
        ):
            DAEMON_SWITCH.require_macos_development_signatures(cli, daemon)
        self.assertEqual(signed.call_args_list, [mock.call(cli), mock.call(daemon)])

    def test_signature_check_requires_exact_authority_line(self) -> None:
        daemon = Path("/candidate/atm-daemon")
        result = subprocess.CompletedProcess(
            ["codesign"],
            0,
            stdout="Authority=another-atm-daemon-dev\n",
            stderr="Authority=atm-daemon-dev-extra\n",
        )
        with (
            mock.patch.object(DAEMON_SWITCH.shutil, "which", return_value="/usr/bin/codesign"),
            mock.patch.object(DAEMON_SWITCH, "run", side_effect=[subprocess.CompletedProcess(["codesign"], 0, stdout="", stderr=""), result]),
        ):
            self.assertFalse(DAEMON_SWITCH.macos_binary_has_development_signature(daemon))

    def test_signature_check_accepts_exact_authority_line_from_codesign_stderr(self) -> None:
        daemon = Path("/candidate/atm-daemon")
        result = subprocess.CompletedProcess(
            ["codesign"], 0, stdout="", stderr="Authority=atm-daemon-dev\n"
        )
        with (
            mock.patch.object(DAEMON_SWITCH.shutil, "which", return_value="/usr/bin/codesign"),
            mock.patch.object(DAEMON_SWITCH, "run", side_effect=[subprocess.CompletedProcess(["codesign"], 0, stdout="", stderr=""), result]),
        ):
            self.assertTrue(DAEMON_SWITCH.macos_binary_has_development_signature(daemon))

    def test_strict_verification_failure_rejects_matching_authority(self) -> None:
        daemon = Path("/candidate/atm-daemon")
        failed_verification = subprocess.CompletedProcess(["codesign"], 1, stdout="", stderr="invalid")
        with (
            mock.patch.object(DAEMON_SWITCH.shutil, "which", return_value="/usr/bin/codesign"),
            mock.patch.object(DAEMON_SWITCH, "run", return_value=failed_verification),
        ):
            self.assertFalse(DAEMON_SWITCH.macos_binary_has_development_signature(daemon))


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


if __name__ == "__main__":
    unittest.main()
