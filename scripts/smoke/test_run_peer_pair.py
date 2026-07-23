"""Regression tests for AI.13 peer-pair smoke contract enforcement."""
from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


def load_runner():
    path = Path(__file__).with_name("run_peer_pair.py")
    spec = importlib.util.spec_from_file_location("run_peer_pair", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


RUNNER = load_runner()
ATM = ["atm", "peek", "--json"]


def assertion(value: object = "$message_ulid") -> dict[str, object]:
    return {"command": ATM, "json_path": "message_id", "equals": value}


def sample_config(log: Path) -> dict[str, object]:
    cases = []
    for case_id in RUNNER.REQUIRED_CASES:
        assertions = {name: assertion() for name in RUNNER.REQUIRED_ASSERTIONS[case_id]}
        case: dict[str, object] = {
            "id": case_id,
            "expect": "typed_error" if case_id in {
                "unavailable_peer", "untrusted_or_allowlist_rejection", "failed_remote_ack",
            } else "success",
            "message_ulid": "01TEST",
            "command": ATM,
            "verification": {"assertions": assertions},
        }
        if case["expect"] == "typed_error":
            case["typed_error_code"] = "ATM_REJECTED"
        if case_id == "untrusted_or_allowlist_rejection":
            case["verification"]["forbidden_daemon_log_entries"] = ["peer delivery"]
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


class PeerPairContractTests(unittest.TestCase):
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
            with self.assertRaisesRegex(RuntimeError, "single_record_retained"):
                RUNNER.validate(config)

    def test_semantic_verification_rejects_routing_after_rejection(self):
        with tempfile.TemporaryDirectory() as temp:
            log = Path(temp) / "daemon.log"
            log.write_text("before\npeer delivery\n", encoding="utf-8")
            case = {
                "message_ulid": "01TEST",
                "verification": {
                    "assertions": {"rejected_before_routing": assertion()},
                    "forbidden_daemon_log_entries": ["peer delivery"],
                },
            }
            original = RUNNER.run_command
            RUNNER.run_command = lambda _command, _timeout: {
                "command": ATM, "exit_code": 0,
                "stdout": json.dumps({"message_id": "01TEST"}), "stderr": "",
            }
            try:
                result = RUNNER.verify_semantics(case, 2, "before\n", str(log))
            finally:
                RUNNER.run_command = original
            self.assertEqual(result["status"], "fail")
            self.assertIn("forbidden post-rejection", result["failures"][0])

    def test_teardown_never_deletes_runtime_without_matching_pid_marker(self):
        with tempfile.TemporaryDirectory() as temp:
            runtime = Path(temp) / "runtime"
            runtime.mkdir()
            owned = runtime / "owned.sock"
            owned.touch()
            process = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
            config = {"daemon": {
                "endpoint": "127.0.0.1:1",
                "runtime_dir": str(runtime),
                "owned_runtime_paths": [str(owned)],
                "launch_command": ["atm-daemon"],
            }}
            result = RUNNER.stop_owned(process, config)
            self.assertEqual(result["status"], "ownership_marker_missing")
            self.assertTrue(owned.exists())


if __name__ == "__main__":
    unittest.main()
