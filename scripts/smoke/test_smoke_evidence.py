"""Tests for public-only live smoke evidence metadata."""
from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import unittest


def load_module():
    path = Path(__file__).with_name("smoke_evidence.py")
    sys.path.insert(0, str(path.parent))
    spec = importlib.util.spec_from_file_location("smoke_evidence", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


EVIDENCE = load_module()


class SmokeEvidenceTests(unittest.TestCase):
    def test_metadata_records_public_candidate_and_tls_facts_without_key_reference(self):
        def command(argv: list[str]):
            if argv[:2] == ["git", "-C"]:
                return {"exit_code": 0, "stdout": "a" * 40, "stderr": ""}
            if argv[1:4] == ["peer", "interface", "list"]:
                return {
                    "exit_code": 0,
                    "stdout": json.dumps([
                        {"advertise_host": "rand-m4.local", "enabled": True},
                        {"advertise_host": "disabled.local", "enabled": False},
                    ]),
                    "stderr": "",
                }
            if argv[1:4] == ["peer", "certificate", "show"]:
                return {
                    "exit_code": 0,
                    "stdout": json.dumps({"fingerprint": "local-public", "private_key_ref": "/secret/key.pem"}),
                    "stderr": "",
                }
            if argv[1:4] == ["peer", "trust", "list"]:
                return {
                    "exit_code": 0,
                    "stdout": json.dumps([
                        {"host": "rand-m5.local", "fingerprint": "peer-public", "enabled": True},
                    ]),
                    "stderr": "",
                }
            raise AssertionError(argv)

        metadata = EVIDENCE.collect_live_evidence_metadata(
            command=command,
            repo_root=Path("/repo"),
            atm="atm",
            feature="crosshost-ack",
            version="1.4.3",
            operating_system="macos",
            architecture="arm64",
            cases=[{"origin": "rand-m4.local", "destination": "rand-m5.local"}],
        )

        self.assertEqual(metadata["candidate"], {"git_sha": "a" * 40, "version": "1.4.3"})
        self.assertEqual(metadata["registered_hostnames"], ["rand-m4.local", "rand-m5.local"])
        self.assertEqual(metadata["public_tls_fingerprints"]["local"], "local-public")
        self.assertEqual(metadata["public_tls_fingerprints"]["trusted_peers"][0]["fingerprint"], "peer-public")
        self.assertNotIn("private_key_ref", json.dumps(metadata))

    def test_metadata_keeps_optional_tls_configuration_absent_without_persisting_command_errors(self):
        def command(argv: list[str]):
            if argv[:2] == ["git", "-C"]:
                return {"exit_code": 0, "stdout": "b" * 40, "stderr": ""}
            return {"exit_code": 1, "stdout": "private_key=/secret", "stderr": "private_key=/secret"}

        metadata = EVIDENCE.collect_live_evidence_metadata(
            command=command,
            repo_root=Path("/repo"),
            atm="atm",
            feature="localhost",
            version="1.4.3",
            operating_system="windows",
            architecture="x86_64",
            cases=[],
        )

        self.assertEqual(metadata["public_tls_fingerprints"], {"local": None, "trusted_peers": []})
        self.assertNotIn("private_key", json.dumps(metadata))


if __name__ == "__main__":
    unittest.main()
