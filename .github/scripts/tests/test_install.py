"""Unit tests for the repository-neutral publish-kit installer."""

from __future__ import annotations

import importlib.util
import json
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


INSTALLER = Path(__file__).resolve().parents[3] / "install.py"
SPEC = importlib.util.spec_from_file_location("sc_publish_install", INSTALLER)
assert SPEC is not None and SPEC.loader is not None
INSTALL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INSTALL)


class InstallValuesTests(unittest.TestCase):
    @staticmethod
    def valid_values() -> dict[str, object]:
        return {
            "release": {"version_source": "Cargo.toml", "tag_prefix": "v"},
            "artifacts": {
                "crates": [
                    {"name": "example-core", "publish_order": 1},
                    {"name": "example-cli", "publish_order": 2},
                ],
                "wheels": [{"package": "example-python", "python_package": "example_python"}],
                "binaries": ["example"],
            },
            "channels": {
                "github_release": {"enabled": True},
                "crates_io": {"enabled": True},
                "pypi": {"enabled": False, "workflow": "pypi-publish.yml"},
                "homebrew": {"enabled": False},
                "scoop": {"enabled": False},
                "winget": {"enabled": False},
            },
        }

    def test_help_explains_the_explicit_install_workflow(self) -> None:
        result = subprocess.run(
            [sys.executable, "-S", str(INSTALLER), "--help"],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--example-json [INSTALL.json]", result.stdout)
        self.assertIn("workflow:", result.stdout)
        self.assertIn("caller-owned JSON input", result.stdout)

    def test_example_json_writes_a_complete_reviewable_contract(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            destination = root / "install.json"
            result = subprocess.run(
                [
                    sys.executable,
                    "-S",
                    str(INSTALLER),
                    "--example-json",
                    str(destination),
                    str(root),
                ],
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            contract = json.loads(destination.read_text(encoding="utf-8"))
        self.assertEqual(set(contract["channels"]), set(INSTALL.CHANNEL_NAMES))
        self.assertEqual(contract["artifacts"], {"crates": [], "wheels": [], "binaries": []})
        self.assertIn("wrote reviewable install input", result.stdout)

    def test_example_json_refuses_to_overwrite_a_reviewed_contract(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            destination = root / "install.json"
            destination.write_text('{"reviewed": true}\n', encoding="utf-8")
            result = subprocess.run(
                [
                    sys.executable,
                    "-S",
                    str(INSTALLER),
                    "--example-json",
                    str(destination),
                    str(root),
                ],
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(destination.read_text(encoding="utf-8"), '{"reviewed": true}\n')
        self.assertIn("refusing to overwrite", result.stderr)

    def test_input_and_example_modes_are_mutually_exclusive(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            input_path = root / "input.json"
            input_path.write_text("{}", encoding="utf-8")
            result = subprocess.run(
                [
                    sys.executable,
                    "-S",
                    str(INSTALLER),
                    "--input",
                    str(input_path),
                    "--example-json",
                    str(root),
                ],
                check=False,
                capture_output=True,
                text=True,
            )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not allowed with argument", result.stderr)

    def test_example_json_enables_only_supported_channels(self) -> None:
        with (
            patch.object(INSTALL, "discover_crates", return_value=["example-core"]),
            patch.object(INSTALL, "discover_wheels", return_value=[]),
            patch.object(INSTALL, "discover_binaries", return_value=[]),
        ):
            values = INSTALL.example_values(Path("consumer"))
        self.assertTrue(values["channels"]["crates_io"]["enabled"])
        self.assertFalse(values["channels"]["pypi"]["enabled"])
        self.assertFalse(values["channels"]["homebrew"]["enabled"])
        self.assertFalse(values["channels"]["winget"]["enabled"])

    def test_load_install_values_requires_explicit_artifacts_and_channels(self) -> None:
        values = self.valid_values()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input.json"
            path.write_text(json.dumps(values), encoding="utf-8")
            loaded = INSTALL.load_install_values(path)
        self.assertEqual(
            loaded["artifacts"]["crates"],
            [
                {"name": "example-core", "publish_order": 1},
                {"name": "example-cli", "publish_order": 2},
            ],
        )
        self.assertEqual(
            loaded["artifacts"]["wheels"],
            [{"package": "example-python", "python_package": "example_python"}],
        )
        self.assertEqual(loaded["artifacts"]["binaries"], ["example"])
        self.assertTrue(loaded["channels"]["crates_io"]["enabled"])
        self.assertFalse(loaded["channels"]["scoop"]["enabled"])

    def test_load_install_values_rejects_invalid_publish_orders(self) -> None:
        cases = {
            "boolean": (True, "positive integer"),
            "zero": (0, "positive integer"),
            "negative": (-1, "positive integer"),
            "duplicate": (1, "unique"),
        }
        for name, (publish_order, message) in cases.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                values = self.valid_values()
                values["artifacts"]["crates"][1]["publish_order"] = publish_order
                path = Path(directory) / "input.json"
                path.write_text(json.dumps(values), encoding="utf-8")
                with self.assertRaisesRegex(Exception, message):
                    INSTALL.load_install_values(path)

    def test_load_install_values_rejects_missing_channel_activation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input.json"
            path.write_text("{}", encoding="utf-8")
            with self.assertRaisesRegex(Exception, "release must be an object"):
                INSTALL.load_install_values(path)

    def test_install_places_executable_release_helpers_at_every_workflow_path(self) -> None:
        """The byte-exact overlay keeps helpers under .github/scripts/.

        This exercises a real install into a temporary consumer. It prevents a
        parity-only check from accepting workflows that still call an obsolete
        consumer-local scripts/ path.
        """

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            consumer = root / "consumer"
            consumer.mkdir()
            input_path = root / "install.json"
            input_path.write_text(json.dumps(self.valid_values()), encoding="utf-8")

            def render_empty_toml(_template: Path, _values: dict[str, object], output: Path) -> None:
                output.write_text("schema_version = 1\n", encoding="utf-8")

            with (
                patch.object(INSTALL, "render_template", side_effect=render_empty_toml),
                patch.object(sys, "argv", [str(INSTALLER), "--input", str(input_path), str(consumer)]),
            ):
                self.assertEqual(INSTALL.main(), 0)

            artifacts = consumer / ".github" / "scripts" / "release_artifacts.py"
            gate = consumer / ".github" / "scripts" / "release_gate.sh"
            self.assertTrue(artifacts.is_file())
            self.assertTrue(gate.is_file())

            workflows = (
                "release.yml",
                "release-preflight.yml",
                "pypi-publish.yml",
                "homebrew-publish.yml",
                "scoop-publish.yml",
                "winget-publish.yml",
            )
            for name in workflows:
                text = (consumer / ".github" / "workflows" / name).read_text(encoding="utf-8")
                self.assertIn(".github/scripts/release_artifacts.py", text, name)
                self.assertNotIn("python3 scripts/release_artifacts.py", text, name)
            release = (consumer / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
            preflight = (consumer / ".github" / "workflows" / "release-preflight.yml").read_text(
                encoding="utf-8"
            )
            self.assertIn(".github/scripts/release_gate.sh", release)
            self.assertNotIn("run: scripts/release_gate.sh", release)
            self.assertIn(".github/scripts/release_artifacts.py validate-publish-order", preflight)
            self.assertNotIn("scripts/ci/validate_publish_order.sh", preflight)
            self.assertNotIn("docs/publishing-agent.md", preflight)
            packaged_runtime_files = (
                list((consumer / ".github" / "workflows").glob("*.yml"))
                + list((consumer / ".github" / "scripts").glob("*.py"))
                + list((consumer / ".github" / "scripts").glob("*.sh"))
                + list((consumer / ".github" / "actions").rglob("action.yml"))
            )
            for path in packaged_runtime_files:
                self.assertNotRegex(
                    path.read_text(encoding="utf-8"),
                    r"(?<![./\\w])scripts/",
                    path.relative_to(consumer).as_posix(),
                )

            pending = list((consumer / ".github" / "workflows").glob("*.yml"))
            seen: set[Path] = set()
            while pending:
                source = pending.pop()
                if source in seen:
                    continue
                seen.add(source)
                for action_name in re.findall(
                    r"uses:\s+\./\.github/actions/([^\s]+)", source.read_text(encoding="utf-8")
                ):
                    action = consumer / ".github" / "actions" / action_name / "action.yml"
                    self.assertTrue(action.is_file(), action.relative_to(consumer).as_posix())
                    pending.append(action)

            helper = subprocess.run(
                [sys.executable, str(artifacts), "validate-publish-order", "--help"],
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(helper.returncode, 0, helper.stderr)
            (consumer / "Cargo.toml").write_text(
                '[workspace]\nmembers = ["crates/base", "crates/leaf"]\n', encoding="utf-8"
            )
            base = consumer / "crates" / "base"
            leaf = consumer / "crates" / "leaf"
            base.mkdir(parents=True)
            leaf.mkdir(parents=True)
            (base / "Cargo.toml").write_text(
                '[package]\nname = "base"\nversion = "1.0.0"\n', encoding="utf-8"
            )
            (leaf / "Cargo.toml").write_text(
                '[package]\nname = "leaf"\nversion = "1.0.0"\n\n'
                '[dependencies]\nbase = { path = "../base" }\n',
                encoding="utf-8",
            )
            manifest = consumer / "release" / "publish-artifacts.toml"
            manifest.write_text(
                "\n".join(
                    (
                        "schema_version = 1",
                        "",
                        "[[crates]]",
                        'artifact = "base"',
                        'package = "base"',
                        'cargo_toml = "crates/base/Cargo.toml"',
                        "publish = true",
                        "publish_order = 1",
                        "wait_after_publish_seconds = 0",
                        "",
                        "[[crates]]",
                        'artifact = "leaf"',
                        'package = "leaf"',
                        'cargo_toml = "crates/leaf/Cargo.toml"',
                        "publish = true",
                        "publish_order = 2",
                        "wait_after_publish_seconds = 0",
                        "",
                    )
                ),
                encoding="utf-8",
            )
            order = subprocess.run(
                [
                    sys.executable,
                    str(artifacts),
                    "validate-publish-order",
                    "--manifest",
                    str(manifest),
                    "--workspace-toml",
                    str(consumer / "Cargo.toml"),
                ],
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(order.returncode, 0, order.stderr)
            self.assertIn("matches the workspace dependency graph", order.stdout)
            gate_check = subprocess.run(
                ["bash", "-n", str(gate)],
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(gate_check.returncode, 0, gate_check.stderr)


if __name__ == "__main__":
    unittest.main()
