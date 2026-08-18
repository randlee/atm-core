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

EXPECTED_NON_CRATES_IO_INVENTORY = ("atm-graft-python", "atm-query-python")


class As2ConsumerContractTests(unittest.TestCase):
    @staticmethod
    def values() -> dict[str, object]:
        return json.loads(INPUT.read_text(encoding="utf-8"))

    def test_declares_the_complete_publishable_crate_surface(self) -> None:
        crates = self.values()["artifacts"]["crates"]
        self.assertEqual(
            tuple(crate["package"] for crate in crates),
            EXPECTED_NON_CRATES_IO_INVENTORY + EXPECTED_CRATES,
        )
        self.assertEqual(
            [crate["publish_order"] for crate in crates if crate["publish"]], list(range(1, 13))
        )

        for crate in crates:
            cargo_toml = ROOT / crate["cargo_toml"]
            self.assertTrue(cargo_toml.is_file(), cargo_toml)
            with cargo_toml.open("rb") as file:
                cargo = tomllib.load(file)
            self.assertEqual(cargo["package"]["name"], crate["package"])
            self.assertIsInstance(crate["required"], bool)
            self.assertEqual(crate["preflight_check"], "locked")
            self.assertIsInstance(crate["wait_after_publish_seconds"], int)
            self.assertGreaterEqual(crate["wait_after_publish_seconds"], 0)
            self.assertIsInstance(crate["verify_install"], bool)

        self.assertEqual(
            [crate["package"] for crate in crates if not crate["publish"]],
            list(EXPECTED_NON_CRATES_IO_INVENTORY),
        )

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

    def test_release_manifest_uses_the_validator_schema(self) -> None:
        with (ROOT / "release/publish-artifacts.toml").open("rb") as file:
            manifest = tomllib.load(file)
        self.assertIn("crates", manifest)
        self.assertNotIn("artifacts", manifest)
        self.assertEqual(
            [crate["package"] for crate in manifest["crates"] if crate["publish"]],
            list(EXPECTED_CRATES),
        )
        self.assertEqual(
            [crate["package"] for crate in manifest["crates"] if not crate["publish"]],
            list(EXPECTED_NON_CRATES_IO_INVENTORY),
        )
        self.assertEqual(
            [package["package"] for package in manifest["python_packages"]],
            ["atm-graft", "atm-query", "hermes-atm"],
        )
        self.assertEqual(
            [distribution["name"] for distribution in manifest["python_distributions"]],
            ["atm-graft", "atm-query", "hermes-atm"],
        )

    def test_atm_query_declares_the_same_abi3_release_contract_as_atm_graft(self) -> None:
        query_cargo = (ROOT / "crates/atm-query-python/Cargo.toml").read_text(encoding="utf-8")
        query_pyproject = (ROOT / "crates/atm-query-python/pyproject.toml").read_text(
            encoding="utf-8"
        )

        self.assertIn('default = ["abi3"]', query_cargo)
        self.assertIn('abi3 = ["pyo3/abi3-py311"]', query_cargo)
        self.assertIn('requires-python = ">=3.11,<3.15"', query_pyproject)
        self.assertIn('features = ["abi3", "extension-module"]', query_pyproject)


if __name__ == "__main__":
    unittest.main()
