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


INSTALLER = next(path / "install.py" for path in Path(__file__).resolve().parents if (path / "install.py").is_file())
SPEC = importlib.util.spec_from_file_location("sc_publish_install", INSTALLER)
assert SPEC is not None and SPEC.loader is not None
INSTALL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INSTALL)


class InstallValuesTests(unittest.TestCase):
    @staticmethod
    def valid_values() -> dict[str, object]:
        return {
            "schema_version": 1,
            "project": {
                "name": "example",
                "archive_prefix": "example",
                "description": "Example release package",
                "homepage": "https://example.test/example",
                "license": "MIT",
                "readme_dependency_crate": "example-core",
                "renderer_archive_path": "bin/example",
            },
            "release_targets": [
                {"target": "x86_64-unknown-linux-gnu", "os": "ubuntu-latest", "archive": "tar.gz"}
            ],
            "crates": [
                {
                    "artifact": "example-core",
                    "package": "example-core",
                    "cargo_toml": "crates/example-core/Cargo.toml",
                    "required": True,
                    "publish": True,
                    "publish_order": 1,
                    "preflight_check": "locked",
                    "wait_after_publish_seconds": 0,
                    "verify_install": False,
                },
                {
                    "artifact": "example-python",
                    "package": "example-python",
                    "cargo_toml": "crates/example-python/Cargo.toml",
                    "required": True,
                    "publish": False,
                    "publish_order": 0,
                    "preflight_check": "locked",
                    "wait_after_publish_seconds": 0,
                    "verify_install": True,
                },
            ],
            "release_binaries": [
                {
                    "name": "example",
                    "bundled_paths": [
                        {
                            "source": "docs",
                            "destination": "share/doc/example",
                            "homebrew_destination_components": ["pkgshare"],
                        }
                    ],
                }
            ],
            "installed_docs": {
                "source_root": "docs",
                "install_root": "share/doc/example",
                "entrypoint": "share/doc/example/README.md",
            },
            "python_packages": [
                {
                    "artifact": "example-wheel",
                    "package": "example",
                    "manifest": "python/pyproject.toml",
                    "module": "example",
                    "publish": "pypi",
                }
            ],
            "python_distributions": [
                {
                    "name": "example",
                    "source": "python",
                    "cargo_manifest": "crates/example-python/Cargo.toml",
                    "module_path": "python/example",
                    "sdist": True,
                    "wheels": ["ubuntu-latest", "macos-latest", "windows-latest"],
                },
                {
                    "name": "example-plugin",
                    "source": "plugin",
                    "build_system": "setuptools",
                    "module_path": "plugin/src/example_plugin",
                    "sdist": True,
                    "wheels": ["ubuntu-latest"],
                },
            ],
            "channels": {
                "pypi": {
                    "workflow": "pypi-publish.yml",
                    "dispatch_inputs": {"target": "production"},
                    "credential_rehearsal_inputs": {"target": "testpypi"},
                    "test_repository": "testpypi",
                    "production_repository": "pypi",
                },
                "homebrew": {
                    "workflow": "homebrew-publish.yml",
                    "dispatch_inputs": {},
                    "tap_repository": "example/tap",
                    "renderer_target": "x86_64-unknown-linux-gnu",
                    "formulas": [
                        {
                            "path": "Formula/example.rb",
                            "template": "release/homebrew/formula.rb.j2",
                            "class": "Example",
                            "binaries": ["example"],
                            "test_command": "--help",
                            "test_output": "Example release package",
                            "release_track": "stable",
                        }
                    ],
                    "assets": [{"key": "linux", "target": "x86_64-unknown-linux-gnu"}],
                },
                "winget": {
                    "workflow": "winget-publish.yml",
                    "dispatch_inputs": {},
                    "identifier": "example.example",
                    "installer_target": "x86_64-pc-windows-msvc",
                },
                "scoop": {
                    "workflow": "scoop-publish.yml",
                    "dispatch_inputs": {},
                    "bucket_repository": "example/scoop-bucket",
                    "manifest_path": "bucket/example.json",
                    "manifest_template": "release/scoop/manifest.json.j2",
                    "installer_target": "x86_64-pc-windows-msvc",
                    "binary": "bin/example.exe",
                    "renderer_target": "x86_64-unknown-linux-gnu",
                },
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
        self.assertIn("--input INSTALL.json", result.stdout)
        self.assertIn("workflow:", result.stdout)
        self.assertIn("caller-owned complete JSON input", result.stdout)

    def test_load_install_values_accepts_the_complete_manifest_contract(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "input.json"
            values = self.valid_values()
            path.write_text(json.dumps(values), encoding="utf-8")
            loaded = INSTALL.load_install_values(path)
        self.assertEqual(loaded, values)
        self.assertEqual(set(loaded["channels"]), set(INSTALL.CHANNEL_NAMES))
        self.assertEqual(loaded["python_distributions"][1]["build_system"], "setuptools")

    def test_load_install_values_rejects_invalid_publish_orders(self) -> None:
        cases = {
            "boolean": (True, "integer"),
            "zero": (0, "positive when publish is true"),
            "negative": (-1, "non-negative integer"),
        }
        for name, (publish_order, message) in cases.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                values = self.valid_values()
                values["crates"][0]["publish_order"] = publish_order
                path = Path(directory) / "input.json"
                path.write_text(json.dumps(values), encoding="utf-8")
                with self.assertRaisesRegex(Exception, message):
                    INSTALL.load_install_values(path)
        with tempfile.TemporaryDirectory() as directory:
            values = self.valid_values()
            values["crates"][1].update(publish=True, publish_order=1)
            path = Path(directory) / "input.json"
            path.write_text(json.dumps(values), encoding="utf-8")
            with self.assertRaisesRegex(Exception, "unique"):
                INSTALL.load_install_values(path)

    def test_load_install_values_rejects_missing_complete_contract_fields(self) -> None:
        values = self.valid_values()
        del values["project"]["license"]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input.json"
            path.write_text(json.dumps(values), encoding="utf-8")
            with self.assertRaisesRegex(Exception, "project.license"):
                INSTALL.load_install_values(path)

    def test_load_install_values_rejects_ambiguous_python_distribution(self) -> None:
        values = self.valid_values()
        values["python_distributions"][0]["build_system"] = "setuptools"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input.json"
            path.write_text(json.dumps(values), encoding="utf-8")
            with self.assertRaisesRegex(Exception, "must not set both"):
                INSTALL.load_install_values(path)

    def test_install_places_executable_release_helpers_at_every_workflow_path(self) -> None:
        """The byte-exact overlay keeps helpers under .github/scripts/.

        This exercises a real install into a temporary consumer. It prevents a
        parity-only check from accepting workflows that still call an obsolete
        consumer-local scripts/ path.
        """

        if not (INSTALLER.parent / ".sc-publish-source-root").is_file():
            self.skipTest("installed consumers do not re-run the package source installer")

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
