"""Package-native unit tests for the vendored GitHub scripts."""

from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


PACKAGE_ROOT = Path(__file__).resolve().parents[3]
SCRIPTS = PACKAGE_ROOT / ".github" / "scripts"
sys.path.insert(0, str(SCRIPTS))
import release_manifest  # noqa: E402


class ReleaseManifestTests(unittest.TestCase):
    def test_channel_contracts_describe_all_six_workers(self) -> None:
        contracts = release_manifest.load_channel_contracts(
            PACKAGE_ROOT / "release" / "publish-channel-contracts.toml.j2"
        )
        self.assertEqual(
            {contract["agent"] for contract in contracts.values()},
            {
                "crates-io-publisher",
                "github-release-publisher",
                "pypi-publisher",
                "homebrew-publisher",
                "scoop-publisher",
                "winget-publisher",
            },
        )
        self.assertEqual(contracts["pypi"]["stage"], "post_release")
        self.assertEqual(contracts["crates_io"]["repository_secrets"], ["CARGO_REGISTRY_TOKEN"])

    def test_load_manifest_orders_crates_by_publish_order(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            manifest = Path(directory) / "publish-artifacts.toml"
            manifest.write_text(
                """[[crates]]
artifact = "late"
package = "late"
publish_order = 2

[[crates]]
artifact = "first"
package = "first"
publish_order = 1

[[release_binaries]]
name = "example"
""",
                encoding="utf-8",
            )
            loaded = release_manifest.load_manifest(manifest)
        self.assertEqual([crate["artifact"] for crate in loaded["crates"]], ["first", "late"])
        self.assertEqual(loaded["release_binaries"], [{"name": "example"}])


class ReleaseScriptTests(unittest.TestCase):
    def test_release_gate_has_valid_bash_syntax(self) -> None:
        result = subprocess.run(
            ["bash", "-n", str(SCRIPTS / "release_gate.sh")],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_release_artifacts_cli_exposes_read_only_inquiry(self) -> None:
        result = subprocess.run(
            [sys.executable, str(SCRIPTS / "release_artifacts.py"), "--help"],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("public-registry-inquiry-plan", result.stdout)
        self.assertIn("preflight-secret-plan", result.stdout)

    def test_bootstrap_pins_the_python_wheel_version(self) -> None:
        script = SCRIPTS / "bootstrap_sc_compose.py"
        text = script.read_text(encoding="utf-8")
        self.assertIn('SC_COMPOSE_VERSION = "1.4.1"', text)
        self.assertIn('"venv"', text)
        self.assertIn('f"sc-compose=={SC_COMPOSE_VERSION}"', text)
        self.assertIn("managed environment has incompatible sc-compose wheel", text)


if __name__ == "__main__":
    unittest.main()
