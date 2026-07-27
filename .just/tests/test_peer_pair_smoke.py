from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
RUNNER_PATH = ROOT / "scripts" / "smoke" / "run_peer_pair.py"


def load_runner():
    spec = importlib.util.spec_from_file_location("run_peer_pair", RUNNER_PATH)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


RUNNER = load_runner()
ATM = ["atm", "read", "--json"]
READ_OUTCOME = {
    "action": "read",
    "team": "test-team",
    "agent": "peer",
    "selection_mode": "all",
    "mutation_applied": True,
    "count": 1,
    "selected_message_id": "01TEST",
    "match_count": 1,
    "additional_match_count": 0,
    "bucket_counts": {"unread": 0, "pending_ack": 1, "history": 0},
    "message": {"message_id": "01TEST", "requires_ack": True},
}
REJECTION_EVENT = {
    "action": "request",
    "outcome": "rejected",
    "message": "HTTPS peer request was rejected before or during shared API routing",
    "fields": {"subsystem": "https_transport"},
}
ROUTING_EVENT = {"action": "peer_delivery", "outcome": "write_persisted"}


def assertion(path: str, value: object = "$message_ulid") -> dict[str, object]:
    return {"command": ATM, "json_path": path, "equals": value}


def sample_config(log: Path) -> dict[str, object]:
    cases = []
    paths = {
        "receiver_visible": "selected_message_id",
        "nudge_visible": "message.message_id",
        "ack_reply_visible": "selected_message_id",
        "single_record_retained": "match_count",
        "no_repeat_nudge": "additional_match_count",
        "no_ack_mutation": "message.acknowledgedAt",
        "no_prohibited_delivery_state": "count",
        "rejected_before_routing": "count",
        "ack_source_unchanged": "message.requires_ack",
        "no_remote_ack_state": "message.acknowledgedAt",
        "daemon_ready": "runtime_status.readiness",
    }
    values = {
        "single_record_retained": 1,
        "no_repeat_nudge": 0,
        "no_prohibited_delivery_state": 0,
        "rejected_before_routing": 0,
        "ack_source_unchanged": True,
        "daemon_ready": "ready",
    }
    for case_id in RUNNER.REQUIRED_CASES:
        assertions = {
            name: assertion(paths[name], values.get(name, "$message_ulid"))
            for name in RUNNER.REQUIRED_ASSERTIONS[case_id]
        }
        for name in {"no_ack_mutation", "no_remote_ack_state"}.intersection(assertions):
            assertions[name] = {"command": ATM, "json_path": paths[name], "absent": True}
        case: dict[str, object] = {
            "id": case_id,
            "expect": "typed_error" if case_id in {
                "unavailable_peer", "untrusted_or_allowlist_rejection", "failed_remote_ack",
            } else "success",
            "message_ulid": "01TEST",
            "command": ["atm", "send", "--json"],
            "verification": {"assertions": assertions},
        }
        if case["expect"] == "typed_error":
            case["typed_error_code"] = "ATM_REJECTED"
        if case_id == "untrusted_or_allowlist_rejection":
            case["verification"]["required_daemon_log_events"] = [REJECTION_EVENT]
            case["verification"]["forbidden_daemon_log_events"] = [ROUTING_EVENT]
        cases.append(case)
    return {
        "schema_version": 1,
        "role": "A",
        "commit": "test",
        "client_version_command": ["atm", "--version"],
        "peer_security": {"trust_id": "trust", "certificate_fingerprint": "fingerprint"},
        "daemon": {
            "endpoint": "127.0.0.1:1",
            "version_command": ["atm", "doctor", "--json"],
            "log_file": str(log),
        },
        "identities": {"sender": "a@t", "recipient": "b@t"},
        "cases": cases,
    }


class PeerPairSmokeTests(unittest.TestCase):
    def test_rejects_non_public_case_command(self):
        with tempfile.TemporaryDirectory() as temp:
            config = sample_config(Path(temp) / "daemon.log")
            config["cases"][0]["command"] = ["python3", "raw_socket_probe.py"]
            with self.assertRaisesRegex(RuntimeError, "public ATM client"):
                RUNNER.validate(config)

    def test_rejects_missing_duplicate_semantic_assertion(self):
        with tempfile.TemporaryDirectory() as temp:
            config = sample_config(Path(temp) / "daemon.log")
            duplicate = next(case for case in config["cases"] if case["id"] == "duplicate_ulid")
            del duplicate["verification"]["assertions"]["single_record_retained"]
            public_atm = Path(temp) / "atm"
            public_atm.touch()
            with patch.object(RUNNER.shutil, "which", return_value=str(public_atm)):
                with self.assertRaisesRegex(RuntimeError, "single_record_retained"):
                    RUNNER.validate(config)

    def test_semantic_verification_uses_real_read_outcome_paths(self):
        case = {
            "message_ulid": "01TEST",
            "verification": {"assertions": {
                "receiver_visible": assertion("selected_message_id"),
                "single_record_retained": assertion("match_count", 1),
                "ack_source_unchanged": assertion("message.requires_ack", True),
                "no_remote_ack_state": {"command": ATM, "json_path": "message.acknowledgedAt", "absent": True},
            }},
        }
        original = RUNNER.run_command
        RUNNER.run_command = lambda _command, _timeout: {
            "command": ATM, "exit_code": 0, "stdout": json.dumps(READ_OUTCOME), "stderr": "",
        }
        try:
            result = RUNNER.verify_semantics(case, 2, "", None)
        finally:
            RUNNER.run_command = original
        self.assertEqual(result["status"], "pass")

    def test_semantic_verification_rejects_missing_structured_rejection_event(self):
        with tempfile.TemporaryDirectory() as temp:
            log = Path(temp) / "daemon.log"
            log.write_text(
                "before\n"
                + json.dumps({"action": "peer_delivery", "outcome": "write_persisted"})
                + "\n",
                encoding="utf-8",
            )
            case = {
                "message_ulid": "01TEST",
                "verification": {
                    "assertions": {"rejected_before_routing": assertion("count", 0)},
                    "required_daemon_log_events": [REJECTION_EVENT],
                },
            }
            original = RUNNER.run_command
            RUNNER.run_command = lambda _command, _timeout: {
                "command": ATM, "exit_code": 0,
                "stdout": json.dumps({**READ_OUTCOME, "count": 0}), "stderr": "",
            }
            try:
                result = RUNNER.verify_semantics(case, 2, "before\n", str(log))
            finally:
                RUNNER.run_command = original
            self.assertEqual(result["status"], "fail")
            self.assertIn("required structured rejection event", result["failures"][-1])

    def test_teardown_never_deletes_runtime_without_matching_pid_marker(self):
        with tempfile.TemporaryDirectory() as temp:
            runtime = Path(temp) / "runtime"
            runtime.mkdir()
            owned = runtime / "owned.sock"
            owned.touch()
            process = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
            try:
                config = {"daemon": {
                    "endpoint": "127.0.0.1:1",
                    "runtime_dir": str(runtime),
                    "owned_runtime_paths": [str(owned)],
                    "launch_command": ["atm-daemon"],
                }}
                result = RUNNER.stop_owned(process, config)
                self.assertEqual(result["status"], "ownership_marker_missing")
                self.assertTrue(owned.exists())
            finally:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)


if __name__ == "__main__":
    unittest.main()
