"""Regression tests for peer-pair semantic verification."""
from __future__ import annotations

import importlib.util
import tempfile
from pathlib import Path
import unittest


def load_runner():
    path = Path(__file__).with_name("run_peer_pair.py")
    spec = importlib.util.spec_from_file_location("run_peer_pair", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


RUNNER = load_runner()
SUCCESS = ["python3", "-c", "raise SystemExit(0)"]
REJECTED = [
    "python3",
    "-c",
    "import sys; print('ATM_REJECTED', file=sys.stderr); raise SystemExit(2)",
]
VERIFY_MESSAGE = [
    "python3",
    "-c",
    "import json; print(json.dumps({'message': {'message_id': '01TEST'}}))",
]


class PeerPairSemanticVerificationTests(unittest.TestCase):
    def test_execute_requires_public_state_verification_for_every_case(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            log = root / "daemon.log"
            log.write_text("ready\n", encoding="utf-8")
            config = sample_config(log)

            self.assertEqual(RUNNER.execute(config, root / "evidence", 2), 0)
            evidence = (root / "evidence" / "peer-smoke-evidence.json").read_text(
                encoding="utf-8"
            )
            self.assertIn('"semantic_verification"', evidence)
            self.assertIn('"status": "passed"', evidence)

    def test_rejection_fails_when_new_log_window_shows_delivery(self):
        with tempfile.TemporaryDirectory() as temp:
            log = Path(temp) / "daemon.log"
            log.write_text("before\npeer delivery\n", encoding="utf-8")
            case = {
                "message_ulid": "01TEST",
                "verification": {
                    "assertions": {
                        "receiver_visible": {
                            "command": VERIFY_MESSAGE,
                            "json_path": "message.message_id",
                            "equals": "$message_ulid",
                        }
                    },
                    "forbidden_daemon_log_entries": ["peer delivery"],
                },
            }

            result = RUNNER.verify_semantics(case, 2, "before\n", str(log))

            self.assertEqual(result["status"], "fail")
            self.assertIn("forbidden post-rejection", result["failures"][0])


def sample_config(log: Path):
    cases = []
    typed_error_cases = {
        "unavailable_peer",
        "untrusted_or_allowlist_rejection",
        "failed_remote_ack",
    }
    for case_id in RUNNER.REQUIRED_CASES:
        typed_error = case_id in typed_error_cases
        case = {
            "id": case_id,
            "expect": "typed_error" if typed_error else "success",
            "message_ulid": "01TEST",
            "command": REJECTED if typed_error else SUCCESS,
            "verification": {
                "assertions": {
                    "receiver_visible": {
                        "command": VERIFY_MESSAGE,
                        "json_path": "message.message_id",
                        "equals": "$message_ulid",
                    }
                },
            },
        }
        if typed_error:
            case["typed_error_code"] = "ATM_REJECTED"
        if case_id == "untrusted_or_allowlist_rejection":
            case["verification"]["forbidden_daemon_log_entries"] = ["peer delivery"]
        cases.append(case)
    return {
        "schema_version": 1,
        "role": "A",
        "commit": "test",
        "client_version_command": SUCCESS,
        "peer_security": {"trust_id": "trust", "certificate_fingerprint": "fingerprint"},
        "daemon": {
            "endpoint": "127.0.0.1:1",
            "version_command": SUCCESS,
            "log_file": str(log),
        },
        "identities": {"sender": "a@t", "recipient": "b@t"},
        "cases": cases,
    }


if __name__ == "__main__":
    unittest.main()
