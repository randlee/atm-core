from __future__ import annotations

import importlib.util
from importlib.machinery import SourceFileLoader
from pathlib import Path
import subprocess
import unittest
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "hermes_gateway"


def load_module():
    spec = importlib.util.spec_from_loader(
        "hermes_gateway", SourceFileLoader("hermes_gateway", str(SCRIPT))
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class HermesGatewayTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = load_module()
        self.profiles = [{"name": "skillrx"}]

    def test_status_requires_one_known_profile(self) -> None:
        with mock.patch.object(self.module, "list_profiles", return_value=self.profiles):
            self.assertEqual(self.module.main(["missing"]), 1)
            self.assertEqual(self.module.main(["all", "--reset"]), 1)

    def test_empty_invocation_lists_profile_state(self) -> None:
        with mock.patch.object(self.module, "list_status") as list_status:
            self.assertEqual(self.module.main(["--list"]), 0)
        list_status.assert_called_once_with()

    def test_dead_loaded_service_uses_kickstart_not_bootstrap(self) -> None:
        with (
            mock.patch.object(self.module.os.path, "isfile", return_value=True),
            mock.patch.object(self.module, "_parse_launchctl", side_effect=[{"last_exit": 1}, {"pid": 9, "running": True}]),
            mock.patch.object(self.module.subprocess, "run") as run,
            mock.patch.object(self.module.time, "sleep"),
        ):
            self.assertTrue(self.module.restart("skillrx"))

        run.assert_called_once_with(
            ["launchctl", "kickstart", "-k", f"gui/{self.module.os.getuid()}/ai.hermes.gateway-skillrx"],
            check=True,
        )

    def test_reset_prints_status_before_restarting(self) -> None:
        with (
            mock.patch.object(self.module, "known_profile", return_value=True),
            mock.patch.object(self.module, "show_status") as status,
            mock.patch.object(self.module, "restart", return_value=True) as restart,
        ):
            self.assertEqual(self.module.main(["skillrx", "--reset"]), 0)

        status.assert_called_once_with("skillrx")
        restart.assert_called_once_with("skillrx")

    def test_reset_supports_multiple_explicit_profiles(self) -> None:
        profiles = [{"name": "skillrx"}, {"name": "alpha-prime"}]
        with (
            mock.patch.object(self.module, "list_profiles", return_value=profiles),
            mock.patch.object(self.module, "show_status") as status,
            mock.patch.object(self.module, "restart", return_value=True) as restart,
        ):
            self.assertEqual(self.module.main(["skillrx", "alpha-prime", "--reset"]), 0)

        self.assertEqual(status.call_args_list, [mock.call("skillrx"), mock.call("alpha-prime")])
        self.assertEqual(restart.call_args_list, [mock.call("skillrx"), mock.call("alpha-prime")])

    def test_restart_command_failure_is_reported_as_a_failure(self) -> None:
        with (
            mock.patch.object(self.module.os.path, "isfile", return_value=True),
            mock.patch.object(self.module, "_parse_launchctl", return_value={"pid": 9, "running": True}),
            mock.patch.object(
                self.module.subprocess,
                "run",
                side_effect=subprocess.CalledProcessError(1, ["launchctl"]),
            ),
        ):
            self.assertFalse(self.module.restart("skillrx"))


if __name__ == "__main__":
    unittest.main()
