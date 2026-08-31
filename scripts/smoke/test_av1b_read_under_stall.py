"""Decision-logic tests for the non-invasive AV.1b live proof runner."""
from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
from unittest import mock
import unittest


SCRIPT = Path(__file__).with_name("av1b_read_under_stall.py")


def load_runner():
    spec = importlib.util.spec_from_file_location("av1b_read_under_stall", SCRIPT)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


RUNNER = load_runner()


def ready_doctor(version: str = "1.4.6") -> dict[str, object]:
    return {
        "summary": {"status": "healthy"},
        "runtime_status": {"readiness": "ready"},
        "client_context": {"version": version},
        "daemon_context": {"version": version},
    }


class Av1bReadUnderStallTests(unittest.TestCase):
    def test_ready_doctor_requires_the_matched_pair(self):
        self.assertEqual(RUNNER.doctor_is_matched_ready(ready_doctor(), "1.4.6"), (True, "matched CLI/daemon pair is ready"))
        wrong_daemon = ready_doctor()
        wrong_daemon["daemon_context"] = {"version": "1.4.5"}
        self.assertEqual(RUNNER.doctor_is_matched_ready(wrong_daemon, "1.4.6"), (False, "daemon version does not match this checkout"))

    def test_read_command_is_only_a_public_read_invocation(self):
        args = SimpleNamespace(atm="/tmp/atm", team="proof-team", actor="proof-agent", message_id="01TEST", budget_ms=3000)
        command = RUNNER.read_command(args)
        self.assertEqual(command[:2], ["/tmp/atm", "read"])
        self.assertEqual(command[-1], "3")
        self.assertIn("--message-id", command)
        self.assertNotIn("daemon-switch", command)
        self.assertNotIn("start", command)
        self.assertNotIn("stop", command)

    def test_execute_fails_closed_before_read_when_doctor_is_wrong_version(self):
        args = SimpleNamespace(atm="atm", team="proof-team", actor="proof-agent", message_id=None, budget_ms=3000)
        mismatched = ready_doctor()
        mismatched["daemon_context"] = {"version": "1.4.5"}
        with mock.patch.object(RUNNER, "branch_version", return_value="1.4.6"), mock.patch.object(
            RUNNER, "run_command", return_value={"exit_code": 0, "stdout": json.dumps(mismatched), "stderr": ""}
        ) as run:
            record = RUNNER.execute(args)
        self.assertEqual(record["status"], "FAIL")
        self.assertIn("daemon version", record["failure"])
        self.assertEqual(run.call_count, 1, "a mismatched daemon must prevent any read invocation")

    def test_execute_records_pass_with_latency_and_log_excerpt(self):
        args = SimpleNamespace(atm="atm", team="proof-team", actor="proof-agent", message_id=None, budget_ms=3000)
        responses = [
            {"command": ["atm", "doctor"], "exit_code": 0, "stdout": json.dumps(ready_doctor()), "stderr": ""},
            {"command": ["atm", "read"], "exit_code": 0, "stdout": '{"messages": []}', "stderr": ""},
            {"command": ["atm", "doctor"], "exit_code": 0, "stdout": json.dumps(ready_doctor()), "stderr": ""},
            {"command": ["atm", "log"], "exit_code": 0, "stdout": '[]', "stderr": ""},
        ]
        with mock.patch.object(RUNNER, "branch_version", return_value="1.4.6"), mock.patch.object(
            RUNNER, "run_command", side_effect=responses
        ):
            record = RUNNER.execute(args)
        self.assertEqual(record["status"], "PASS")
        self.assertTrue(record["read"]["within_budget"])
        self.assertTrue(record["retained_log_excerpt"]["captured"])

    def test_main_writes_failure_evidence_when_read_exceeds_budget(self):
        with tempfile.TemporaryDirectory() as temporary:
            evidence = Path(temporary) / "proof.json"
            failing = {"status": "FAIL", "failure": "too slow"}
            with mock.patch.object(RUNNER, "execute", return_value=failing):
                self.assertEqual(
                    RUNNER.main(["--team", "proof-team", "--actor", "proof-agent", "--evidence-out", str(evidence)]),
                    1,
                )
            self.assertEqual(json.loads(evidence.read_text(encoding="utf-8"))["failure"], "too slow")

    def test_parse_args_rejects_a_budget_the_cli_cannot_enforce(self):
        with self.assertRaises(SystemExit):
            RUNNER.parse_args(
                ["--team", "proof-team", "--actor", "proof-agent", "--budget-ms", "2500", "--evidence-out", "proof.json"]
            )
