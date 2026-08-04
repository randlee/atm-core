from __future__ import annotations

import argparse
import importlib.util
from pathlib import Path
import subprocess
import unittest
from unittest import mock


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / ".claude" / "skills" / "daemon-switch" / "scripts" / "daemon-switch.py"


def load_module():
    spec = importlib.util.spec_from_file_location("daemon_switch", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class DaemonSwitchTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = load_module()
        self.cli_link = Path("/selectors/atm")
        self.daemon_link = Path("/selectors/atm-daemon")
        self.old_cli = Path("/installed/bin/atm")
        self.old_daemon = Path("/installed/bin/atm-daemon")
        self.new_cli = Path("/candidate/bin/atm")
        self.new_daemon = Path("/candidate/bin/atm-daemon")
        self.args = argparse.Namespace(
            cli_link=None,
            daemon_link=None,
            yes=True,
            dry_run=False,
            service="atm-daemon",
            launch_agent_plist="/tmp/atm-daemon.plist",
            repair_orphan=False,
        )

    def patch_switch_inputs(self):
        return mock.patch.multiple(
            self.module,
            selected_links=mock.DEFAULT,
            validate_selectors=mock.DEFAULT,
            require_executable=mock.DEFAULT,
            save_default_pair=mock.DEFAULT,
            replace_link=mock.DEFAULT,
            run_service=mock.DEFAULT,
            live_pair_matches=mock.DEFAULT,
            require_stopped_daemon=mock.DEFAULT,
            require_macos_signed_daemon=mock.DEFAULT,
        )

    def test_switch_pair_stops_then_replaces_both_selectors_then_starts(self) -> None:
        with self.patch_switch_inputs() as patched:
            patched["selected_links"].return_value = (self.cli_link, self.daemon_link)
            patched["require_executable"].side_effect = [
                self.old_cli,
                self.old_daemon,
                self.new_cli,
                self.new_daemon,
            ]
            patched["live_pair_matches"].return_value = (True, "matched")

            self.module.switch_pair(self.args, self.new_cli, self.new_daemon)

        patched["validate_selectors"].assert_called_once_with(self.cli_link, self.daemon_link)
        patched["require_macos_signed_daemon"].assert_called_once_with(self.new_daemon)
        patched["save_default_pair"].assert_called_once_with(self.old_cli, self.old_daemon)
        self.assertEqual(
            patched["run_service"].call_args_list,
            [
                mock.call(self.args, "stop", allow_absent=True),
                mock.call(self.args, "start"),
            ],
        )
        patched["require_stopped_daemon"].assert_called_once_with(self.args, self.old_cli)
        self.assertEqual(
            patched["replace_link"].call_args_list,
            [
                mock.call(self.cli_link, self.new_cli),
                mock.call(self.daemon_link, self.new_daemon),
            ],
        )

    def test_switch_pair_rolls_back_both_selectors_and_restarts_after_replace_failure(self) -> None:
        with self.patch_switch_inputs() as patched:
            patched["selected_links"].return_value = (self.cli_link, self.daemon_link)
            patched["require_executable"].side_effect = [
                self.old_cli,
                self.old_daemon,
                self.new_cli,
                self.new_daemon,
            ]
            patched["live_pair_matches"].return_value = (True, "matched")
            patched["replace_link"].side_effect = [None, OSError("simulated replace failure"), None, None]

            with self.assertRaises(OSError):
                self.module.switch_pair(self.args, self.new_cli, self.new_daemon)

        self.assertEqual(
            patched["replace_link"].call_args_list,
            [
                mock.call(self.cli_link, self.new_cli),
                mock.call(self.daemon_link, self.new_daemon),
                mock.call(self.cli_link, self.old_cli),
                mock.call(self.daemon_link, self.old_daemon),
            ],
        )

    def test_switch_pair_repairs_dangling_selectors_only_with_explicit_repair(self) -> None:
        with self.patch_switch_inputs() as patched:
            patched["selected_links"].return_value = (self.cli_link, self.daemon_link)
            patched["require_executable"].side_effect = [
                self.module.SwitchError("selected atm CLI does not exist"),
                self.new_cli,
                self.new_daemon,
            ]
            patched["live_pair_matches"].return_value = (True, "matched")
            self.args.repair_orphan = True

            self.module.switch_pair(self.args, self.new_cli, self.new_daemon)

        patched["save_default_pair"].assert_not_called()
        patched["require_stopped_daemon"].assert_called_once_with(self.args, self.cli_link)
        self.assertEqual(
            patched["replace_link"].call_args_list,
            [
                mock.call(self.cli_link, self.new_cli),
                mock.call(self.daemon_link, self.new_daemon),
            ],
        )
        self.assertEqual(
            patched["run_service"].call_args_list,
            [
                mock.call(self.args, "stop", allow_absent=True),
                mock.call(self.args, "start"),
            ],
        )

    def test_invalid_selector_is_rejected_before_service_stop(self) -> None:
        with self.patch_switch_inputs() as patched:
            patched["selected_links"].return_value = (self.cli_link, self.daemon_link)
            patched["validate_selectors"].side_effect = self.module.SwitchError("not symlinks")

            with self.assertRaisesRegex(self.module.SwitchError, "not symlinks"):
                self.module.switch_pair(self.args, self.new_cli, self.new_daemon)

        patched["run_service"].assert_not_called()
        patched["replace_link"].assert_not_called()

    def test_invalid_macos_signature_is_rejected_before_service_stop(self) -> None:
        with self.patch_switch_inputs() as patched:
            patched["selected_links"].return_value = (self.cli_link, self.daemon_link)
            patched["require_executable"].side_effect = [
                self.old_cli,
                self.old_daemon,
                self.new_cli,
                self.new_daemon,
            ]
            patched["require_macos_signed_daemon"].side_effect = self.module.SwitchError(
                "target atm-daemon is unsigned"
            )

            with self.assertRaisesRegex(self.module.SwitchError, "unsigned"):
                self.module.switch_pair(self.args, self.new_cli, self.new_daemon)

        patched["run_service"].assert_not_called()
        patched["replace_link"].assert_not_called()

    def test_switch_pair_rolls_back_when_live_doctor_reports_the_old_daemon(self) -> None:
        with self.patch_switch_inputs() as patched:
            patched["selected_links"].return_value = (self.cli_link, self.daemon_link)
            patched["require_executable"].side_effect = [
                self.old_cli,
                self.old_daemon,
                self.new_cli,
                self.new_daemon,
            ]
            patched["live_pair_matches"].return_value = (False, "selected beta.29, daemon beta.24")

            with self.assertRaisesRegex(self.module.SwitchError, "split CLI/daemon pair"):
                self.module.switch_pair(self.args, self.new_cli, self.new_daemon)

        self.assertEqual(
            patched["replace_link"].call_args_list,
            [
                mock.call(self.cli_link, self.new_cli),
                mock.call(self.daemon_link, self.new_daemon),
                mock.call(self.cli_link, self.old_cli),
                mock.call(self.daemon_link, self.old_daemon),
            ],
        )

    def test_reachable_daemon_requires_explicit_orphan_repair(self) -> None:
        args = argparse.Namespace(repair_orphan=False)
        with (
            mock.patch.object(self.module, "macos_socket_owner_pids", return_value=[42]),
            mock.patch.object(self.module.platform, "system", return_value="Darwin"),
        ):
            with self.assertRaisesRegex(self.module.SwitchError, "refuse a split pair"):
                self.module.require_stopped_daemon(args, self.old_cli)

    def test_restart_requires_a_single_live_pair_after_controlled_stop(self) -> None:
        args = argparse.Namespace(yes=True)
        with (
            mock.patch.object(self.module, "selected_links", return_value=(self.old_cli, self.old_daemon)),
            mock.patch.object(self.module, "run_service") as run_service,
            mock.patch.object(self.module, "require_stopped_daemon") as stopped,
            mock.patch.object(self.module, "live_pair_matches", return_value=(True, "matched")),
        ):
            self.module.restart(args)

        stopped.assert_called_once_with(args, self.old_cli)
        self.assertEqual(
            run_service.call_args_list,
            [mock.call(args, "stop", allow_absent=True), mock.call(args, "start")],
        )

    def test_restore_prefers_homebrew_then_explicit_then_saved_state(self) -> None:
        explicit = argparse.Namespace(default_cli="/explicit/atm", default_daemon="/explicit/atm-daemon")
        with mock.patch.object(self.module, "homebrew_pair", return_value=(self.old_cli, self.old_daemon)):
            self.assertEqual(self.module.restore_pair(explicit), (self.old_cli, self.old_daemon))
        with mock.patch.object(self.module, "homebrew_pair", return_value=None):
            self.assertEqual(self.module.restore_pair(explicit), (Path("/explicit/atm"), Path("/explicit/atm-daemon")))
        saved_only = argparse.Namespace(default_cli=None, default_daemon=None)
        with (
            mock.patch.object(self.module, "homebrew_pair", return_value=None),
            mock.patch.object(self.module, "load_state", return_value={"default_cli": "/saved/atm", "default_daemon": "/saved/atm-daemon"}),
        ):
            self.assertEqual(self.module.restore_pair(saved_only), (Path("/saved/atm"), Path("/saved/atm-daemon")))

    def test_documented_post_subcommand_service_options_parse(self) -> None:
        parsed = self.module.parser().parse_args(
            [
                "switch",
                "--cli",
                "/candidate/bin/atm",
                "--daemon",
                "/candidate/bin/atm-daemon",
                "--yes",
                "--service",
                "atm-daemon",
                "--launch-agent-plist",
                "/tmp/atm-daemon.plist",
            ]
        )
        self.assertEqual(parsed.service, "atm-daemon")
        self.assertEqual(parsed.launch_agent_plist, "/tmp/atm-daemon.plist")

    def test_macos_start_retries_bootstrap_after_unload_race(self) -> None:
        args = argparse.Namespace(service="atm-daemon", launch_agent_plist="/tmp/atm-daemon.plist")
        results = [
            subprocess.CompletedProcess([], 5, "", "Bootstrap failed: 5"),
            subprocess.CompletedProcess([], 1, "", "not loaded"),
            subprocess.CompletedProcess([], 0, "", ""),
        ]
        with (
            mock.patch.object(self.module.platform, "system", return_value="Darwin"),
            mock.patch.object(self.module.os, "getuid", return_value=501, create=True),
            mock.patch.object(self.module, "run", side_effect=results),
            mock.patch.object(self.module.time, "sleep"),
        ):
            self.module.run_service(args, "start")

    def test_macos_stop_repairs_one_verified_orphan_after_bootout(self) -> None:
        args = argparse.Namespace(
            service="atm-daemon",
            launch_agent_plist="/tmp/atm-daemon.plist",
            repair_orphan=True,
        )
        loaded = subprocess.CompletedProcess([], 0, "", "")
        unloaded = subprocess.CompletedProcess([], 1, "", "not loaded")
        with (
            mock.patch.object(self.module.platform, "system", return_value="Darwin"),
            mock.patch.object(self.module.os, "getuid", return_value=501, create=True),
            mock.patch.object(
                self.module,
                "run",
                side_effect=[subprocess.CompletedProcess([], 0, "", ""), *([loaded] * 20), unloaded],
            ),
            mock.patch.object(self.module, "macos_socket_owner_pids", return_value=[42]),
            mock.patch.object(self.module, "repair_macos_orphan") as repair,
            mock.patch.object(self.module.time, "sleep"),
        ):
            self.module.run_service(args, "stop", allow_absent=True)

        repair.assert_called_once_with([42])

    def test_live_pair_doctor_uses_a_home_directory_not_the_calling_worktree(self) -> None:
        report = {
            "client_context": {"version": "1.3.2-beta.29"},
            "daemon_context": {"version": "1.3.2-beta.29"},
        }
        with (
            mock.patch.object(self.module, "selected_release_version", return_value="1.3.2-beta.29"),
            mock.patch.object(
                self.module,
                "run",
                return_value=subprocess.CompletedProcess([], 0, __import__("json").dumps(report), ""),
            ) as run,
        ):
            matched, _detail = self.module.live_pair_matches(self.new_cli)

        self.assertTrue(matched)
        self.assertEqual(run.call_args.kwargs["cwd"], Path.home())


class DaemonSwitchSigningTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = load_module()

    def test_macos_switch_rejects_unsigned_daemon_before_service_control(self) -> None:
        unsigned = subprocess.CompletedProcess(["codesign"], 1, stdout="", stderr="code object is not signed")
        with (
            mock.patch.object(self.module.platform, "system", return_value="Darwin"),
            mock.patch.object(self.module, "run", return_value=unsigned),
        ):
            with self.assertRaisesRegex(
                self.module.SwitchError,
                "unsigned or has an invalid macOS signature.*sign_daemon_dev",
            ):
                self.module.require_macos_signed_daemon(Path("target/release/atm-daemon"))

    def test_macos_switch_requires_the_development_signing_authority(self) -> None:
        verified = subprocess.CompletedProcess(["codesign"], 0, stdout="", stderr="")
        wrong_identity = subprocess.CompletedProcess(
            ["codesign"], 0, stdout="", stderr="Authority=other-development-identity"
        )
        with (
            mock.patch.object(self.module.platform, "system", return_value="Darwin"),
            mock.patch.object(self.module, "run", side_effect=[verified, wrong_identity]),
        ):
            with self.assertRaisesRegex(self.module.SwitchError, "must be signed with `atm-daemon-dev`"):
                self.module.require_macos_signed_daemon(Path("target/release/atm-daemon"))

    def test_macos_switch_accepts_verified_development_signature(self) -> None:
        verified = subprocess.CompletedProcess(["codesign"], 0, stdout="", stderr="")
        described = subprocess.CompletedProcess(
            ["codesign"], 0, stdout="", stderr="Authority=atm-daemon-dev"
        )
        with (
            mock.patch.object(self.module.platform, "system", return_value="Darwin"),
            mock.patch.object(self.module, "run", side_effect=[verified, described]),
        ):
            self.module.require_macos_signed_daemon(Path("target/release/atm-daemon"))

    def test_non_macos_switch_skips_codesign_preflight(self) -> None:
        with (
            mock.patch.object(self.module.platform, "system", return_value="Linux"),
            mock.patch.object(self.module, "run") as run,
        ):
            self.module.require_macos_signed_daemon(Path("target/release/atm-daemon"))
        run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
