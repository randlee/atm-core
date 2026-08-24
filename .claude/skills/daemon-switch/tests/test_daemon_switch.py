"""Regression tests for the daemon-switch singleton cleanup rules."""

from __future__ import annotations

import importlib.util
import io
import argparse
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
import plistlib
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


class MacosDevelopmentSigningTests(unittest.TestCase):
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
                mock.call(cli, DAEMON_SWITCH.CLI_IDENTIFIER, identity.team_identifier),
                mock.call(daemon, DAEMON_SWITCH.DAEMON_IDENTIFIER, identity.team_identifier),
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
                mock.call(cli, DAEMON_SWITCH.CLI_IDENTIFIER, identity.team_identifier),
                mock.call(daemon, DAEMON_SWITCH.DAEMON_IDENTIFIER, identity.team_identifier),
            ],
        )

    def test_signature_check_uses_shared_stable_identifier_verifier(self) -> None:
        daemon = Path("/candidate/atm-daemon")
        with (
            mock.patch.object(DAEMON_SWITCH, "verify_apple_signature", return_value=True) as verify,
        ):
            self.assertTrue(
                DAEMON_SWITCH.macos_binary_has_development_signature(
                    daemon, DAEMON_SWITCH.DAEMON_IDENTIFIER, "4869P2ZYC6"
                )
            )
        verify.assert_called_once_with(str(daemon), DAEMON_SWITCH.DAEMON_IDENTIFIER, "4869P2ZYC6")

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


class WindowsTemporaryLaunchAdapterTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.cli = self.root / "atm.exe"
        self.daemon = self.root / "atm-daemon.exe"
        for binary in (self.cli, self.daemon):
            binary.write_bytes(binary.name.encode("utf-8"))
            binary.chmod(0o700)
        self.args = argparse.Namespace(
            yes=True,
            service="atm-daemon-test",
            peer_wire_security=DAEMON_SWITCH.PeerWireSecurity.PLAINTEXT_TEST,
            repair_orphan=False,
        )
        self.current = DAEMON_SWITCH.quote_windows_command_line(
            [str(self.daemon), "--log-format", "json"]
        )
        self.commands: list[list[str]] = []
        self.before_config: object | None = None
        self.adapter = DAEMON_SWITCH.WindowsScmAdapter(self.run_sc)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def run_sc(self, command: object, _timeout: float) -> object:
        values = list(command)
        self.commands.append(values)
        if values[1] == "qc":
            return subprocess.CompletedProcess(values, 0, f"BINARY_PATH_NAME   : {self.current}\n", "")
        if values[1] == "config":
            if callable(self.before_config):
                self.before_config()
            self.current = values[4]
            return subprocess.CompletedProcess(values, 0, "SUCCESS\n", "")
        raise AssertionError(f"unexpected SCM command: {values}")

    def captured_session(self) -> object:
        captured = self.adapter.capture(
            self.args,
            self.cli,
            self.daemon,
            self.args.peer_wire_security,
        )
        return DAEMON_SWITCH.TemporaryLaunchSession.captured(
            peer_wire_security=self.args.peer_wire_security,
            platform="Windows",
            account_id="user:benchmark",
            service=self.args.service,
            cli_path=self.cli,
            cli_digest=DAEMON_SWITCH.sha256_file(self.cli),
            daemon_path=self.daemon,
            daemon_digest=DAEMON_SWITCH.sha256_file(self.daemon),
            launch_spec=captured,
        )

    def test_windows_argv_codec_round_trips_quoted_arguments(self) -> None:
        argv = [
            r"C:\\Program Files\\ATM\\atm-daemon.exe",
            "--log-format",
            "json",
            "--label",
            'quote " and trailing slash\\',
            "",
        ]

        self.assertEqual(DAEMON_SWITCH.parse_windows_command_line(DAEMON_SWITCH.quote_windows_command_line(argv)), argv)

    def test_overlay_is_journaled_before_exact_scm_mutation_then_restored(self) -> None:
        session = self.captured_session().transition(DAEMON_SWITCH.TemporaryLaunchPhase.STOPPED)
        overlay_spec = self.adapter.apply_overlay(self.args, session)
        session = session.with_overlay(overlay_spec)
        original = self.current

        self.assertEqual(self.current, original)
        self.adapter.activate_overlay(self.args, session)
        self.assertEqual(self.current, overlay_spec.overlay_reference)
        self.assertEqual(
            DAEMON_SWITCH.parse_windows_command_line(self.current)[-2:],
            ["--peer-wire-security", "plaintext-test"],
        )

        self.adapter.restore_exact(self.args, session)
        self.adapter.restore_exact(self.args, session)

        self.assertEqual(self.current, original)

    def test_begin_persists_overlay_before_scm_config_at_fake_service_boundary(self) -> None:
        journal = DAEMON_SWITCH.TemporaryLaunchJournal(self.root / "state" / "temporary-launch.json")

        def assert_durable_overlay() -> None:
            active = journal.load()
            assert active is not None
            self.assertEqual(active.phase, DAEMON_SWITCH.TemporaryLaunchPhase.OVERLAY_APPLIED)
            self.assertIsNotNone(active.overlay_reference)
            self.assertIsNotNone(active.overlay_digest)

        self.before_config = assert_durable_overlay
        with (
            mock.patch.object(DAEMON_SWITCH, "temporary_launch_journal", return_value=journal),
            mock.patch.object(DAEMON_SWITCH, "selected_matched_pair", return_value=(self.cli, self.daemon)),
            mock.patch.object(DAEMON_SWITCH, "temporary_launch_adapter", return_value=self.adapter),
            mock.patch.object(DAEMON_SWITCH, "account_identifier", return_value="user:benchmark"),
            mock.patch.object(DAEMON_SWITCH.platform, "system", return_value="Windows"),
            mock.patch.object(DAEMON_SWITCH, "run_service"),
            mock.patch.object(DAEMON_SWITCH, "require_stopped_daemon"),
            mock.patch.object(DAEMON_SWITCH, "wait_for_temporary_launch", return_value=(True, "ready")),
            redirect_stdout(io.StringIO()),
        ):
            DAEMON_SWITCH.begin_temporary_launch(self.args)

        active = journal.load()
        assert active is not None
        self.assertEqual(active.phase, DAEMON_SWITCH.TemporaryLaunchPhase.OVERLAY_STARTED)

    def test_capture_rejects_preexisting_peer_wire_security(self) -> None:
        self.current = DAEMON_SWITCH.quote_windows_command_line(
            [str(self.daemon), "--peer-wire-security", "mutual-tls"]
        )

        with self.assertRaisesRegex(DAEMON_SWITCH.TemporaryLaunchError, "already selects"):
            self.adapter.capture(self.args, self.cli, self.daemon, self.args.peer_wire_security)

    def test_restore_refuses_operator_changed_binary_path(self) -> None:
        session = self.captured_session().transition(DAEMON_SWITCH.TemporaryLaunchPhase.STOPPED)
        session = session.with_overlay(self.adapter.apply_overlay(self.args, session))
        self.adapter.activate_overlay(self.args, session)
        self.current = DAEMON_SWITCH.quote_windows_command_line([str(self.daemon), "--operator-change"])

        with self.assertRaisesRegex(DAEMON_SWITCH.TemporaryLaunchError, "changed; refusing restore"):
            self.adapter.restore_exact(self.args, session)


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


if __name__ == "__main__":
    unittest.main()
