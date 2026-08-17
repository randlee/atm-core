from __future__ import annotations

import json
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release.yml"
PUBLISHER = REPO_ROOT / ".claude" / "agents" / "publisher.md"
MANIFEST = REPO_ROOT / "release" / "publish-artifacts.toml"


class ReleaseWorkflowTests(unittest.TestCase):
    def test_root_release_workflow_has_no_destination_specific_publishing(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertNotIn("homebrew-tap", text)
        self.assertNotIn("winget-releaser", text)
        self.assertIn("release-target-matrix", text)
        self.assertIn("channel-dispatch-plan", PUBLISHER.read_text(encoding="utf-8"))

    def test_python_build_matrix_supports_each_manifest_declared_build_system(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("matrix.build_system == 'maturin'", text)
        self.assertIn("python -m build --wheel", text)
        self.assertIn("python -m build --sdist", text)

    def test_release_archives_are_verified_against_the_manifest_before_upload(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("scripts/verify_release_archive.py", text)
        self.assertIn('"--archive",', text)

    def test_actual_homebrew_template_installs_every_declared_binary(self) -> None:
        formulas = tomllib.loads(MANIFEST.read_text(encoding="utf-8"))["channels"]["homebrew"][
            "formulas"
        ]
        rendered_by_class = {}
        with tempfile.TemporaryDirectory() as directory:
            tempdir = Path(directory)
            for formula in formulas:
                variables = {
                    "formula_class": formula["class"],
                    "description": "ATM release formula",
                    "homepage": "https://github.com/randlee/atm-core",
                    "version": "1.4.3",
                    "license": "MIT",
                    "macos_arm_url": "https://example.test/atm-macos-arm.tar.gz",
                    "macos_arm_sha256": "a" * 64,
                    "macos_intel_url": "https://example.test/atm-macos-intel.tar.gz",
                    "macos_intel_sha256": "b" * 64,
                    "linux_url": "https://example.test/atm-linux.tar.gz",
                    "linux_sha256": "c" * 64,
                    "binary_paths": [f"bin/{binary}" for binary in formula["binaries"]],
                    "bundled_paths": [],
                    "test_binary": formula["binaries"][0],
                    "test_command": formula["test_command"],
                    "test_output": formula["test_output"],
                }
                var_file = tempdir / f"{formula['class']}.json"
                output = tempdir / f"{formula['class']}.rb"
                var_file.write_text(json.dumps(variables), encoding="utf-8")
                completed = subprocess.run(
                    [
                        "sc-compose",
                        "render",
                        "--root",
                        str(REPO_ROOT),
                        "--file",
                        str(REPO_ROOT / formula["template"]),
                        "--var-file",
                        str(var_file),
                        "--output",
                        str(output),
                    ],
                    check=False,
                    capture_output=True,
                    text=True,
                    encoding="utf-8",
                )
                self.assertEqual(completed.returncode, 0, completed.stderr)
                rendered = output.read_text(encoding="utf-8")
                install_block = rendered.split("  def install\n", 1)[1].split("\n  test do", 1)[0]
                rendered_by_class[formula["class"]] = install_block

        for formula_class in ("AgentTeamMail", "Atm"):
            with self.subTest(formula_class=formula_class):
                self.assertIn('bin.install "bin/atm"', rendered_by_class[formula_class])
                self.assertIn('bin.install "bin/atm-daemon"', rendered_by_class[formula_class])


if __name__ == "__main__":
    unittest.main()
