from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import textwrap
import tomllib
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
        docs_readme = self.root / "docs" / "user-documents" / "README.md"

        manifest_toml.parent.mkdir(parents=True, exist_ok=True)
        crate_toml.parent.mkdir(parents=True, exist_ok=True)
        docs_readme.parent.mkdir(parents=True, exist_ok=True)

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
        docs_readme.write_text("# Demo ATM Docs\n", encoding="utf-8")
        return workspace_toml, manifest_toml

    def write_homebrew_fixture(self) -> tuple[Path, Path]:
        release_dir = self.root / "release"
        formula_path = self.root / "homebrew-tap" / "Formula" / "atm.rb"
        release_dir.mkdir(parents=True, exist_ok=True)
        formula_path.parent.mkdir(parents=True, exist_ok=True)
        (release_dir / "checksums.txt").write_text(
            textwrap.dedent(
                """
                linuxsha  atm_1.3.0_x86_64-unknown-linux-gnu.tar.gz
                intelmacsha  atm_1.3.0_x86_64-apple-darwin.tar.gz
                armmacsha  atm_1.3.0_aarch64-apple-darwin.tar.gz
                """
            ).strip()
            + "\n",
            encoding="utf-8",
        )
        formula_path.write_text(
            textwrap.dedent(
                """
                # typed: false
                # frozen_string_literal: true

                class Atm < Formula
                  desc "CLI for mail-like messaging with Claude agent teams"
                  homepage "https://github.com/randlee/atm-core"
                  version "1.2.3"
                  license "MIT"

                  on_macos do
                    on_intel do
                      url "https://github.com/randlee/atm-core/releases/download/v1.2.3/atm_1.2.3_aarch64-apple-darwin.tar.gz"
                      sha256 "wrong-old-macos-intel"

                      def install
                        bin.install "atm"
                      end
                    end
                    on_arm do
                      url "https://github.com/randlee/atm-core/releases/download/v1.2.3/atm_1.2.3_aarch64-apple-darwin.tar.gz"
                      sha256 "wrong-old-macos-arm"

                      def install
                        bin.install "atm"
                      end
                    end
                  end

                  on_linux do
                    on_intel do
                      if Hardware::CPU.is_64_bit?
                        url "https://github.com/randlee/atm-core/releases/download/v1.2.3/atm_1.2.3_aarch64-apple-darwin.tar.gz"
                        sha256 "wrong-old-linux"

                        def install
                          bin.install "atm"
                        end
                      end
                    end
                  end

                  test do
                    system "#{bin}/atm", "--version"
                  end
                end
                """
            ).strip()
            + "\n",
            encoding="utf-8",
        )
        return release_dir, formula_path

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

    def run_release_artifacts(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), *args],
            cwd=self.root,
            check=False,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )

    def run_repository_release_artifacts(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), *args],
            cwd=REPO_ROOT,
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

    def test_publish_channel_plans_are_derived_from_the_manifest(self) -> None:
        manifest_path = REPO_ROOT / "release" / "publish-artifacts.toml"
        manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
        preflight = self.run_repository_release_artifacts(
            "preflight-secret-plan", "--manifest", str(manifest_path)
        )
        dispatch = self.run_repository_release_artifacts(
            "channel-dispatch-plan", "--manifest", str(manifest_path), "--tag", "v1.4.3"
        )

        self.assertEqual(preflight.returncode, 0, preflight.stderr)
        self.assertEqual(dispatch.returncode, 0, dispatch.stderr)
        self.assertEqual(
            {channel["name"] for channel in json.loads(dispatch.stdout)["channels"]},
            set(manifest["channels"]),
        )
        self.assertEqual(
            {channel["name"] for channel in json.loads(preflight.stdout)["root_channels"]},
            {"crates_io", "github_release"},
        )

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

    def test_validate_manifest_rejects_non_publishable_runtime_path_dependency(self) -> None:
        workspace_toml, manifest_toml = self.write_workspace(
            crate_package_block="""
            [package]
            name = "demo-crate"
            version.workspace = true
            edition.workspace = true
            rust-version.workspace = true
            description.workspace = true
            license.workspace = true

            [dependencies]
            private-crate = { path = "../private-crate" }
            """
        )
        workspace_toml.write_text(
            workspace_toml.read_text(encoding="utf-8").replace(
                'members = ["crates/demo-crate"]',
                'members = ["crates/demo-crate", "crates/private-crate"]',
            ),
            encoding="utf-8",
        )
        private_manifest = self.root / "crates" / "private-crate" / "Cargo.toml"
        private_manifest.parent.mkdir(parents=True)
        private_manifest.write_text(
            textwrap.dedent(
                """
                [package]
                name = "private-crate"
                version.workspace = true
                edition.workspace = true
                rust-version.workspace = true
                description.workspace = true
                license.workspace = true
                publish = false
                """
            ).strip()
            + "\n",
            encoding="utf-8",
        )

        completed = self.run_validate_manifest(
            workspace_toml=workspace_toml,
            manifest_toml=manifest_toml,
        )

        self.assertNotEqual(completed.returncode, 0)
        self.assertIn("publish dependency violation(s):", completed.stdout)
        self.assertIn(
            "demo-crate has runtime/build path dependency private-crate whose Cargo.toml sets publish = false",
            completed.stdout,
        )

    def test_update_homebrew_formulas_rewrites_each_platform_block_to_its_own_asset(self) -> None:
        release_dir, formula_path = self.write_homebrew_fixture()

        completed = self.run_release_artifacts(
            "update-homebrew-formulas",
            "--release-dir",
            str(release_dir),
            "--version",
            "1.3.0",
            "--tag",
            "v1.3.0",
            "--formula",
            str(formula_path),
        )

        self.assertEqual(completed.returncode, 0, completed.stderr)
        text = formula_path.read_text(encoding="utf-8")
        self.assertIn('version "1.3.0"', text)
        self.assertIn(
            'url "https://github.com/randlee/atm-core/releases/download/v1.3.0/atm_1.3.0_x86_64-apple-darwin.tar.gz"',
            text,
        )
        self.assertIn(
            'url "https://github.com/randlee/atm-core/releases/download/v1.3.0/atm_1.3.0_aarch64-apple-darwin.tar.gz"',
            text,
        )
        self.assertIn(
            'url "https://github.com/randlee/atm-core/releases/download/v1.3.0/atm_1.3.0_x86_64-unknown-linux-gnu.tar.gz"',
            text,
        )
        self.assertIn('sha256 "intelmacsha"', text)
        self.assertIn('sha256 "armmacsha"', text)
        self.assertIn('sha256 "linuxsha"', text)

    def test_validate_homebrew_formulas_rejects_cross_platform_asset_reuse(self) -> None:
        release_dir, formula_path = self.write_homebrew_fixture()

        completed = self.run_release_artifacts(
            "validate-homebrew-formulas",
            "--release-dir",
            str(release_dir),
            "--version",
            "1.3.0",
            "--tag",
            "v1.3.0",
            "--formula",
            str(formula_path),
        )

        self.assertNotEqual(completed.returncode, 0)
        self.assertIn("homebrew formula validation failed:", completed.stdout)
        self.assertIn("on_macos/on_intel url mismatch", completed.stdout)
        self.assertIn("on_linux/on_intel url mismatch", completed.stdout)

    def test_validate_homebrew_formulas_passes_after_rewrite(self) -> None:
        release_dir, formula_path = self.write_homebrew_fixture()

        updated = self.run_release_artifacts(
            "update-homebrew-formulas",
            "--release-dir",
            str(release_dir),
            "--version",
            "1.3.0",
            "--tag",
            "v1.3.0",
            "--formula",
            str(formula_path),
        )
        self.assertEqual(updated.returncode, 0, updated.stderr)

        completed = self.run_release_artifacts(
            "validate-homebrew-formulas",
            "--release-dir",
            str(release_dir),
            "--version",
            "1.3.0",
            "--tag",
            "v1.3.0",
            "--formula",
            str(formula_path),
        )

        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("ok: Homebrew formulas match expected platform assets and checksums", completed.stdout)


if __name__ == "__main__":
    unittest.main()
