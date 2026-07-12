from __future__ import annotations

from pathlib import Path
import subprocess
import sys
import tempfile
import textwrap
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "release_artifacts.py"


class ReleaseArtifactsTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tempdir = tempfile.TemporaryDirectory()
        self.root = Path(self.tempdir.name)

    def tearDown(self) -> None:
        self.tempdir.cleanup()

    def write_workspace(self, *, crate_package_block: str) -> tuple[Path, Path]:
        workspace_toml = self.root / "Cargo.toml"
        manifest_toml = self.root / "release" / "publish-artifacts.toml"
        crate_toml = self.root / "crates" / "demo-crate" / "Cargo.toml"

        manifest_toml.parent.mkdir(parents=True, exist_ok=True)
        crate_toml.parent.mkdir(parents=True, exist_ok=True)

        workspace_toml.write_text(
            textwrap.dedent(
                """
                [workspace]
                members = ["crates/demo-crate"]
                resolver = "2"

                [workspace.package]
                version = "1.2.3"
                edition = "2024"
                rust-version = "1.94.1"
                description = "Workspace inherited description."
                license = "MIT OR Apache-2.0"
                """
            ).strip()
            + "\n",
            encoding="utf-8",
        )

        manifest_toml.write_text(
            textwrap.dedent(
                """
                schema_version = 1

                [[crates]]
                artifact = "demo-crate"
                package = "demo-crate"
                cargo_toml = "crates/demo-crate/Cargo.toml"
                required = true
                publish = true
                publish_order = 1
                preflight_check = "full"
                wait_after_publish_seconds = 0
                verify_install = false

                [[release_binaries]]
                name = "atm"

                [installed_docs]
                source_root = "docs/user-documents"
                install_root = "share/doc/atm"
                entrypoint = "share/doc/atm/README.md"
                """
            ).strip()
            + "\n",
            encoding="utf-8",
        )

        crate_toml.write_text(
            textwrap.dedent(crate_package_block).strip() + "\n",
            encoding="utf-8",
        )
        (self.root / "docs" / "user-documents").mkdir(parents=True, exist_ok=True)
        (self.root / "docs" / "user-documents" / "README.md").write_text("# Docs\n", encoding="utf-8")
        return workspace_toml, manifest_toml

    def run_validate_manifest(self, *, workspace_toml: Path, manifest_toml: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "validate-manifest",
                "--manifest",
                str(manifest_toml),
                "--workspace-toml",
                str(workspace_toml),
            ],
            cwd=self.root,
            check=False,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )

    def test_validate_manifest_accepts_workspace_inherited_publish_metadata(self) -> None:
        workspace_toml, manifest_toml = self.write_workspace(
            crate_package_block="""
            [package]
            name = "demo-crate"
            version.workspace = true
            edition.workspace = true
            rust-version.workspace = true
            description.workspace = true
            license.workspace = true
            """
        )

        completed = self.run_validate_manifest(workspace_toml=workspace_toml, manifest_toml=manifest_toml)

        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("ok: all publishable workspace crates are present in the manifest", completed.stdout)
        self.assertIn("ok: all publishable manifest crates define required publish metadata", completed.stdout)

    def test_validate_manifest_rejects_missing_publish_metadata(self) -> None:
        workspace_toml, manifest_toml = self.write_workspace(
            crate_package_block="""
            [package]
            name = "demo-crate"
            version.workspace = true
            edition.workspace = true
            rust-version.workspace = true
            """
        )

        completed = self.run_validate_manifest(workspace_toml=workspace_toml, manifest_toml=manifest_toml)

        self.assertNotEqual(completed.returncode, 0)
        self.assertIn("publish metadata violation(s):", completed.stdout)
        self.assertIn(
            "demo-crate: missing required publish metadata field(s): description, license or license-file",
            completed.stdout,
        )


if __name__ == "__main__":
    unittest.main()
