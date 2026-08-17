"""Regression tests for peer-pair semantic verification."""
from __future__ import annotations

import importlib.util
import json
import tempfile
from pathlib import Path
import unittest
from unittest.mock import MagicMock, patch


def load_runner():
    path = Path(__file__).with_name("run_peer_pair.py")
    spec = importlib.util.spec_from_file_location("run_peer_pair", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


RUNNER = load_runner()
SUCCESS = ["atm", "success"]
REJECTED = ["atm", "reject"]
VERIFY_MESSAGE = [
    "python3",
    "-c",
    "import json; print(json.dumps({'message': {'message_id': '01TEST'}}))",
]
REJECTION_EVENT = {
    "action": "request",
    "outcome": "rejected",
    "message": "HTTPS peer request was rejected before or during shared API routing",
    "fields": {"subsystem": "https_transport"},
}
ROUTING_EVENT = {"action": "peer_delivery", "outcome": "write_persisted"}


def encoded_event() -> str:
    return json.dumps(
        {
            "action": "request",
            "outcome": "rejected",
            "message": "HTTPS peer request was rejected before or during shared API routing",
            "fields": {"subsystem": "https_transport", "error_code": "ATM_REJECTED"},
        }
    )


def public_atm_command(command, _timeout):
    if command[1:] == ["reject"]:
        return {"command": command, "exit_code": 2, "stdout": "", "stderr": "ATM_REJECTED"}
    if command[1:] == ["verify"]:
        return {
            "command": command,
            "exit_code": 0,
            "stdout": (
                '{"runtime_status":{"readiness":"ready"},'
                '"message":{"message_id":"01TEST","requires_ack":true},'
                '"match_count":1,"additional_match_count":0,"count":0}'
            ),
            "stderr": "",
        }
    return {"command": command, "exit_code": 0, "stdout": "", "stderr": ""}


class PeerPairSemanticVerificationTests(unittest.TestCase):
    def test_readiness_poll_retries_public_client_until_success(self):
        process = MagicMock()
        process.poll.return_value = None
        attempts = [
            {"command": ["atm", "doctor", "--json"], "exit_code": 1, "stdout": "", "stderr": "starting"},
            {"command": ["atm", "doctor", "--json"], "exit_code": 0, "stdout": "ready", "stderr": ""},
        ]
        with patch.object(RUNNER, "run_command", side_effect=attempts) as run_command:
            result = RUNNER.await_daemon_ready(process, ["atm", "doctor", "--json"], 2)
        self.assertEqual(result["stdout"], "ready")
        self.assertEqual(run_command.call_count, 2)

    def test_execute_requires_public_state_verification_for_every_case(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            log = root / "daemon.log"
            log.write_text("ready\n", encoding="utf-8")
            config = sample_config(log)

            with patch.object(RUNNER.shutil, "which", return_value=str(Path(__file__).resolve())):
                def command_with_rejection_event(command, timeout):
                    result = public_atm_command(command, timeout)
                    if command[1:] == ["reject"]:
                        with log.open("a", encoding="utf-8") as stream:
                            stream.write(f"{encoded_event()}\n")
                    return result

                with patch.object(RUNNER, "run_command", side_effect=command_with_rejection_event):
                    self.assertEqual(RUNNER.execute(config, root / "evidence", 2), 0)
            evidence = (root / "evidence" / "peer-smoke-evidence.json").read_text(
                encoding="utf-8"
            )
            self.assertIn('"semantic_verification"', evidence)
            self.assertIn('"status": "passed"', evidence)

    def test_required_rejection_event_is_verified_from_structured_log(self):
        with tempfile.TemporaryDirectory() as temp:
            log = Path(temp) / "daemon.log"
            log.write_text(f"before\n{encoded_event()}\n", encoding="utf-8")
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
                    "required_daemon_log_events": [REJECTION_EVENT],
                    "forbidden_daemon_log_events": [ROUTING_EVENT],
                },
            }

            result = RUNNER.verify_semantics(case, 2, "before\n", str(log))

            self.assertEqual(result["status"], "pass")
            self.assertEqual(result["daemon_log_event_count"], 1)

    def test_synthetic_token_does_not_satisfy_required_rejection_event(self):
        with tempfile.TemporaryDirectory() as temp:
            log = Path(temp) / "daemon.log"
            log.write_text("before\nlegacy-token\n", encoding="utf-8")
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
                    "required_daemon_log_events": [REJECTION_EVENT],
                },
            }

            result = RUNNER.verify_semantics(case, 2, "before\n", str(log))

            self.assertEqual(result["status"], "fail")
            self.assertIn("required structured rejection event", result["failures"][-1])

    def test_forbidden_structured_event_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            log = Path(temp) / "daemon.log"
            log.write_text(
                f"before\n{encoded_event()}\n{json.dumps(ROUTING_EVENT)}\n",
                encoding="utf-8",
            )
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
                    "forbidden_daemon_log_events": [ROUTING_EVENT],
                },
            }

            result = RUNNER.verify_semantics(case, 2, "before\n", str(log))

            self.assertEqual(result["status"], "fail")
            self.assertIn("forbidden structured event", result["failures"][-1])

    def test_rejection_fails_closed_when_daemon_log_rotates(self):
        with tempfile.TemporaryDirectory() as temp:
            log = Path(temp) / "daemon.log"
            log.write_text("new log after rotation\n", encoding="utf-8")
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
                    "required_daemon_log_events": [REJECTION_EVENT],
                },
            }

            result = RUNNER.verify_semantics(case, 2, "old log before rotation\n", str(log))

            self.assertEqual(result["status"], "fail")
            self.assertIn("rotated or truncated", result["failures"][0])

    def test_rejection_fails_closed_when_log_delta_is_not_jsonl(self):
        with tempfile.TemporaryDirectory() as temp:
            log = Path(temp) / "daemon.log"
            log.write_text("before\nnot a structured event\n", encoding="utf-8")
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
                    "required_daemon_log_events": [REJECTION_EVENT],
                },
            }

            result = RUNNER.verify_semantics(case, 2, "before\n", str(log))

            self.assertEqual(result["status"], "fail")
            self.assertIn("non-JSON line", result["failures"][0])

    def test_validation_rejects_constant_message_assertion(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            log = root / "daemon.log"
            log.write_text("ready\n", encoding="utf-8")
            config = sample_config(log)
            config["cases"][1]["verification"]["assertions"]["receiver_visible"]["equals"] = "01TEST"

            with patch.object(RUNNER.shutil, "which", return_value=str(Path(__file__).resolve())):
                with self.assertRaisesRegex(RuntimeError, "must bind to `\\$message_ulid`"):
                    RUNNER.validate(config)

    def test_validation_rejects_legacy_synthetic_log_entry_field(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            log = root / "daemon.log"
            log.write_text("ready\n", encoding="utf-8")
            config = sample_config(log)
            rejection = next(
                case for case in config["cases"] if case["id"] == "untrusted_or_allowlist_rejection"
            )
            rejection["verification"]["forbidden_daemon_log_entries"] = ["legacy-token"]

            with patch.object(RUNNER.shutil, "which", return_value=str(Path(__file__).resolve())):
                with self.assertRaisesRegex(RuntimeError, "was removed"):
                    RUNNER.validate(config)

    def test_validation_requires_structured_rejection_event(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            log = root / "daemon.log"
            log.write_text("ready\n", encoding="utf-8")
            config = sample_config(log)
            rejection = next(
                case for case in config["cases"] if case["id"] == "untrusted_or_allowlist_rejection"
            )
            del rejection["verification"]["required_daemon_log_events"]

            with patch.object(RUNNER.shutil, "which", return_value=str(Path(__file__).resolve())):
                with self.assertRaisesRegex(RuntimeError, "requires required_daemon_log_events"):
                    RUNNER.validate(config)

    def test_validation_rejects_path_shadow_when_install_root_is_selected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            log = root / "daemon.log"
            log.write_text("ready\n", encoding="utf-8")
            install_client = root / "install" / "bin" / "atm"
            install_client.parent.mkdir(parents=True)
            install_client.write_text("installed", encoding="utf-8")
            shadow_client = root / "shadow" / "atm"
            shadow_client.parent.mkdir()
            shadow_client.write_text("shadow", encoding="utf-8")
            config = sample_config(log)

            with patch.dict(RUNNER.os.environ, {"ATM_SMOKE_INSTALL_ROOT": str(root / "install")}, clear=False):
                with patch.object(RUNNER.shutil, "which", return_value=str(shadow_client)):
                    with self.assertRaisesRegex(RuntimeError, "does not resolve"):
                        RUNNER.validate(config)


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
                    assertion_name: assertion_definition(assertion_name)
                    for assertion_name in RUNNER.REQUIRED_ASSERTIONS[case_id]
                },
            },
        }
        if typed_error:
            case["typed_error_code"] = "ATM_REJECTED"
        if case_id == "untrusted_or_allowlist_rejection":
            case["verification"]["required_daemon_log_events"] = [REJECTION_EVENT]
            case["verification"]["forbidden_daemon_log_events"] = [ROUTING_EVENT]
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


def assertion_definition(name: str):
    command = ["atm", "verify"]
    if name == "daemon_ready":
        return {"command": command, "json_path": "runtime_status.readiness", "equals": "ready"}
    if name in {"receiver_visible", "nudge_visible", "ack_reply_visible"}:
        return {"command": command, "json_path": "message.message_id", "equals": "$message_ulid"}
    if name == "single_record_retained":
        return {"command": command, "json_path": "match_count", "equals": 1}
    if name == "no_repeat_nudge":
        return {"command": command, "json_path": "additional_match_count", "equals": 0}
    if name in {"no_ack_mutation", "no_remote_ack_state"}:
        return {"command": command, "json_path": "message.acknowledgedAt", "absent": True}
    if name in {"no_prohibited_delivery_state", "rejected_before_routing"}:
        return {"command": command, "json_path": "count", "equals": 0}
    if name == "ack_source_unchanged":
        return {"command": command, "json_path": "message.requires_ack", "equals": True}
    raise AssertionError(f"missing fixture for {name}")


if __name__ == "__main__":
    unittest.main()
