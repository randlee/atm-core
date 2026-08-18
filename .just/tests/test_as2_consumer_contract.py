"""Regression coverage for ATM's explicit sc-publish consumer input."""

from __future__ import annotations

import json
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
INPUT = ROOT / "docs/plans/phase-as/evidence/AS.2-consumer-input.json"

EXPECTED_CRATES = (
    "atm-error",
    "atm-storage",
    "agent-team-mail-core",
    "atm-storage-rusqlite",
    "atm-http-runtime",
    "atm-daemon-client",
    "atm-runtime",
    "atm-template-sc-compose",
    "atm-daemon-bootstrap",
    "atm-daemon",
    "atm-graft",
    "agent-team-mail",
)


class As2ConsumerContractTests(unittest.TestCase):
    @staticmethod
    def values() -> dict[str, object]:
        return json.loads(INPUT.read_text(encoding="utf-8"))

    def test_declares_the_complete_publishable_crate_surface(self) -> None:
        crates = self.values()["artifacts"]["crates"]
        self.assertEqual(tuple(crate["package"] for crate in crates), EXPECTED_CRATES)
        self.assertEqual([crate["publish_order"] for crate in crates], list(range(1, 13)))

        for crate in crates:
            cargo_toml = ROOT / crate["cargo_toml"]
            self.assertTrue(cargo_toml.is_file(), cargo_toml)
            with cargo_toml.open("rb") as file:
                cargo = tomllib.load(file)
            self.assertEqual(cargo["package"]["name"], crate["package"])
            self.assertIsInstance(crate["required"], bool)
            self.assertTrue(crate["publish"])
            self.assertEqual(crate["preflight_check"], "locked")
            self.assertIsInstance(crate["wait_after_publish_seconds"], int)
            self.assertGreaterEqual(crate["wait_after_publish_seconds"], 0)
            self.assertIsInstance(crate["verify_install"], bool)

    def test_declares_python_binaries_and_all_channel_states_explicitly(self) -> None:
        values = self.values()
        artifacts = values["artifacts"]
        self.assertEqual(
            artifacts["wheels"],
            [
                {"package": "atm-graft", "python_package": "atm_graft"},
                {"package": "atm-query", "python_package": "atm_query"},
                {"package": "hermes-atm", "python_package": "hermes_atm"},
            ],
        )
        self.assertEqual(artifacts["binaries"], ["atm", "atm-daemon"])
        self.assertEqual(
            set(values["channels"]),
            {"github_release", "crates_io", "pypi", "homebrew", "scoop", "winget"},
        )
        self.assertTrue(all(not channel["enabled"] for channel in values["channels"].values()))
        self.assertEqual(values["channels"]["pypi"]["workflow"], "pypi-publish.yml")


if __name__ == "__main__":
    unittest.main()
