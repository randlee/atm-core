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
        patched["save_default_pair"].assert_called_once_with(self.old_cli, self.old_daemon)
        self.assertEqual(
            patched["run_service"].call_args_list,
            [
                mock.call(self.args, "stop", allow_absent=True),
                mock.call(self.args, "start"),
            ],
        )
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


if __name__ == "__main__":
    unittest.main()
