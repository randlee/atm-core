from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = Path(__file__).with_name("run_hermes_graft_live.py")


def load_module():
    spec = importlib.util.spec_from_file_location("run_hermes_graft_live", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class HermesGraftLiveSmokeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = load_module()

    def test_graft_report_and_envelope_carry_source_revision_and_procedure(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            with mock.patch.object(self.module.feature_smoke, "ROOT", root), \
                    mock.patch.object(self.module.feature_smoke, "source_revision", return_value="a" * 40), \
                    mock.patch.object(self.module.feature_smoke, "platform") as platform, \
                    mock.patch.object(self.module.feature_smoke, "os") as os_module, \
                    mock.patch.object(self.module.feature_smoke, "compose"), \
                    mock.patch.object(self.module.feature_smoke, "update_master_report_index"):
                platform.system.return_value = "Darwin"
                platform.node.return_value = "m5"
                os_module.environ = {"ATM_SMOKE_RUN_ID": "run-1"}
                os_module.getpid.return_value = 1
                report = self.module.feature_smoke.write_report("graft-hermes", [{"name": "doctor", "status": "PASS", "detail": "ready", "origin": "m5", "destination": "m5"}])
            self.assertEqual(json.loads(report.read_text())["procedure"], "graft-hermes")
            self.assertEqual(json.loads((report.parent / "smoke.envelope.json").read_text())["procedure"], "graft-hermes")
            self.assertEqual(json.loads(report.read_text())["source_revision"], "a" * 40)

    def test_graft_missing_git_writes_null_source_revision(self) -> None:
        with mock.patch.object(self.module.feature_smoke.subprocess, "run", return_value=mock.Mock(returncode=1, stdout="")):
            self.assertIsNone(self.module.feature_smoke.source_revision())

    def test_ready_pair_runs_backend_and_registers_graft_report(self) -> None:
        doctor = {"summary": {"status": "healthy"}, "runtime_status": {"readiness": "ready"}, "client_context": {"version": "1.4.1-beta-ai-1"}, "daemon_context": {"version": "1.4.1-beta-ai-1"}}
        completed = mock.Mock(returncode=0, stdout="Hermes graft smoke test: PASS\n", stderr="")
        with (
            mock.patch.object(self.module.feature_smoke, "require_environment", return_value=("atm", "sender", "hermes")),
            mock.patch.object(self.module.feature_smoke, "command", return_value={"exit_code": 0, "stdout": "{}", "stderr": ""}) as command,
            mock.patch.object(self.module.feature_smoke, "parse_json", return_value=doctor),
            mock.patch.object(self.module.feature_smoke, "branch_version", return_value="1.4.1-beta-ai-1"),
            mock.patch.object(self.module.feature_smoke, "doctor_ready", return_value=True),
            mock.patch.object(self.module.subprocess, "run", return_value=completed) as run,
            mock.patch.object(self.module.feature_smoke, "write_report", return_value=Path("report.json")) as write_report,
        ):
            self.assertEqual(self.module.run_live(["--sender", "sender"]), 0)

        command.assert_called_once_with(["atm", "doctor", "--json"])
        run.assert_called_once_with(
            [sys.executable, str(self.module.BACKEND), "--sender", "sender"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        feature, cases = write_report.call_args.args
        self.assertEqual(feature, "graft-hermes")
        self.assertEqual([case["status"] for case in cases], ["PASS", "PASS"])
        self.assertEqual(cases[1]["name"], "graft outbound durable write and receiver round trip")

    def test_unready_pair_does_not_run_backend_and_writes_failure_report(self) -> None:
        doctor = {"summary": {"status": "healthy"}, "runtime_status": {"readiness": "starting"}}
        with (
            mock.patch.object(self.module.feature_smoke, "require_environment", return_value=("atm", "sender", "hermes")),
            mock.patch.object(self.module.feature_smoke, "command", return_value={"exit_code": 0, "stdout": "{}", "stderr": ""}),
            mock.patch.object(self.module.feature_smoke, "parse_json", return_value=doctor),
            mock.patch.object(self.module.feature_smoke, "branch_version", return_value="1.4.1-beta-ai-1"),
            mock.patch.object(self.module.feature_smoke, "doctor_ready", return_value=False),
            mock.patch.object(self.module.subprocess, "run") as run,
            mock.patch.object(self.module.feature_smoke, "write_report", return_value=Path("report.json")) as write_report,
        ):
            self.assertEqual(self.module.run_live([]), 1)

        run.assert_not_called()
        _feature, cases = write_report.call_args.args
        self.assertEqual([case["status"] for case in cases], ["FAIL"])


if __name__ == "__main__":
    unittest.main()
