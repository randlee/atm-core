"""Unit tests for the progressive feature smoke dispatcher."""
from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import sys
import tempfile
from unittest import mock
import unittest


def load_runner():
    path = Path(__file__).with_name("run_feature_smoke.py")
    sys.path.insert(0, str(path.parent))
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

    def test_report_writes_browser_frame_for_xhtml_pane(self):
        with tempfile.TemporaryDirectory() as temp:
            with mock.patch.dict(os.environ, {"ATM_SMOKE_RUN_ID": "smoke-42"}, clear=False):
                with mock.patch.object(RUNNER, "ROOT", Path(temp)):
                    with mock.patch.object(RUNNER, "compose") as compose:
                        report = RUNNER.write_report("localhost", [{"name": "doctor", "status": "PASS", "detail": "ready"}])
        self.assertEqual(compose.call_count, 3)
        self.assertEqual(compose.call_args_list[1].args[2], report.with_suffix(".html"))
        self.assertEqual(compose.call_args_list[2].args[2], report.parent / "index.html")

    def test_feature_pane_renders_each_executed_case(self):
        pane = RUNNER.render_feature_pane(
            "localhost",
            [
                {"name": "doctor", "status": "PASS", "detail": "status: healthy\nreadiness: ready"},
                {"name": "localhost send/read", "status": "PASS", "detail": "01TEST"},
            ],
        )
        self.assertIn("localhost send/read", pane)
        self.assertIn("Doctor passed", pane)
        self.assertIn("healthy<br />readiness", pane)
        self.assertNotIn("<td>doctor</td>", pane)

    def test_artifact_segment_rejects_path_traversal(self):
        with self.assertRaisesRegex(RUNNER.SmokeError, "ATM_SMOKE_RUN_ID"):
            RUNNER.artifact_segment("../other-run", "ATM_SMOKE_RUN_ID")


if __name__ == "__main__":
    unittest.main()
