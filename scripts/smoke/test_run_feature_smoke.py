"""Unit tests for the progressive feature smoke dispatcher."""
from __future__ import annotations

import importlib.util
import os
from pathlib import Path
from unittest import mock
import unittest


def load_runner():
    path = Path(__file__).with_name("run_feature_smoke.py")
    spec = importlib.util.spec_from_file_location("run_feature_smoke", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


RUNNER = load_runner()


class FeatureSmokeTests(unittest.TestCase):
    def test_local_ip_alias_is_supported(self):
        with mock.patch.object(RUNNER, "run_live", return_value=0) as run_live:
            with mock.patch.object(RUNNER.sys, "argv", ["smoke", "local-up"]):
                self.assertEqual(RUNNER.main(), 0)
        run_live.assert_called_once_with("local-ip", [])

    def test_crosshost_passes_all_hostnames_to_live_runner(self):
        with mock.patch.object(RUNNER, "run_live", return_value=0) as run_live:
            with mock.patch.object(RUNNER.sys, "argv", ["smoke", "crosshost", "m5", "fastpc4"]):
                self.assertEqual(RUNNER.main(), 0)
        run_live.assert_called_once_with("crosshost", ["m5", "fastpc4"])

    def test_fixture_level_retains_existing_runner(self):
        completed = mock.Mock(returncode=0)
        with mock.patch.object(RUNNER.subprocess, "run", return_value=completed) as run:
            with mock.patch.object(RUNNER.sys, "argv", ["smoke", "thorough"]):
                self.assertEqual(RUNNER.main(), 0)
        self.assertEqual(Path(run.call_args.args[0][1]).name, "run.py")

    def test_missing_identity_is_a_hard_failure(self):
        with mock.patch.dict(os.environ, {"ATM_IDENTITY": "", "ATM_TEAM": ""}, clear=False):
            with self.assertRaisesRegex(RUNNER.SmokeError, "ATM_IDENTITY"):
                RUNNER.require_environment()

    def test_branch_version_requires_one_shared_cli_daemon_version(self):
        metadata = {"packages": [{"name": "atm", "version": "1.3.2-beta.27"}, {"name": "atm-daemon", "version": "1.3.2-beta.27"}]}
        with mock.patch.object(RUNNER, "command", return_value={"exit_code": 0, "stdout": __import__("json").dumps(metadata), "stderr": ""}):
            self.assertEqual(RUNNER.branch_version(), "1.3.2-beta.27")

    def test_branch_version_rejects_divergent_cli_daemon_versions(self):
        metadata = {"packages": [{"name": "atm", "version": "1.3.2-beta.27"}, {"name": "atm-daemon", "version": "1.3.2-beta.28"}]}
        with mock.patch.object(RUNNER, "command", return_value={"exit_code": 0, "stdout": __import__("json").dumps(metadata), "stderr": ""}):
            with self.assertRaisesRegex(RUNNER.SmokeError, "shared"):
                RUNNER.branch_version()

    def test_remote_hosts_are_rejected_for_localhost_feature(self):
        with mock.patch.object(RUNNER.sys, "argv", ["smoke", "localhost", "m5"]):
            with self.assertRaisesRegex(RUNNER.SmokeError, "only valid"):
                RUNNER.main()


if __name__ == "__main__":
    unittest.main()
