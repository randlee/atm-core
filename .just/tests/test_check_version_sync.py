from __future__ import annotations

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from check_version_sync import validate_crate_versions
from check_version_sync import validate_lockfile
from check_version_sync import validate_winget_manifests
from check_version_sync import success_message
from check_version_sync import validate_release_version_lockstep
from check_version_sync import KIT_RELEASE_ARTIFACTS
from check_version_sync import replace_version_occurrences
from prerelease_tag import copy_tracked_files
from prerelease_tag import sync_python_version


ROOT_MANIFEST = """\
[workspace]
members = ["crates/atm", "crates/atm-core", "crates/atm-daemon", "crates/atm-rusqlite"]
resolver = "2"

[workspace.package]
version = "1.1.2"
"""


def crate_manifest(name: str, extra: str = "") -> str:
    return f"""\
[package]
name = "{name}"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true
{extra}
"""


class CheckVersionSyncTests(unittest.TestCase):
    def write_repo(self, repo_root: Path, workspace_version: str = "1.1.2") -> None:
        root_manifest = ROOT_MANIFEST.replace('version = "1.1.2"', f'version = "{workspace_version}"')
        (repo_root / "Cargo.toml").write_text(root_manifest, encoding="utf-8")
        crates_dir = repo_root / "crates"
        for crate_name, package_name in (
            ("atm", "agent-team-mail"),
            ("atm-core", "agent-team-mail-core"),
            ("atm-daemon", "agent-team-mail-daemon"),
            ("atm-rusqlite", "agent-team-mail-rusqlite"),
        ):
            crate_dir = crates_dir / crate_name
            crate_dir.mkdir(parents=True)
            extra = ""
            if crate_name == "atm":
                extra = '\n[dependencies]\natm-core = { package = "agent-team-mail-core", path = "../atm-core", version = "1.1.2" }\n'
            (crate_dir / "Cargo.toml").write_text(crate_manifest(package_name, extra=extra), encoding="utf-8")

    @mock.patch("check_version_sync.subprocess.run")
    def test_release_version_lockstep_delegates_to_installed_kit(self, run: mock.Mock) -> None:
        run.return_value = subprocess.CompletedProcess(args=[], returncode=0, stdout="ok", stderr="")
        repo_root = Path("/tmp/atm")

        validate_release_version_lockstep(repo_root)

        expected_kit_script = str(repo_root / KIT_RELEASE_ARTIFACTS)
        self.assertEqual(
            run.call_args.args[0],
            [
                sys.executable,
                expected_kit_script,
                "verify-version-lockstep",
                "--manifest",
                "release/publish-artifacts.toml",
                "--workspace-toml",
                "Cargo.toml",
            ],
        )

    @mock.patch("check_version_sync.subprocess.run")
    def test_release_version_lockstep_propagates_kit_failure(self, run: mock.Mock) -> None:
        run.return_value = subprocess.CompletedProcess(args=[], returncode=1, stdout="", stderr="version mismatch")

        with self.assertRaisesRegex(SystemExit, "version mismatch"):
            validate_release_version_lockstep(Path("/tmp/atm"))

    def test_validate_crate_versions_checks_all_member_manifests(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            manifest = repo_root / "crates/atm-rusqlite/Cargo.toml"
            manifest.write_text(
                manifest.read_text(encoding="utf-8").replace("version.workspace = true\n", ""),
                encoding="utf-8",
            )

            with self.assertRaises(SystemExit) as error:
                validate_crate_versions(repo_root, "1.1.2")

            message = str(error.exception).replace("\\", "/")
            self.assertIn(
                "crates/atm-rusqlite/Cargo.toml must define [package].version either as a non-empty string or version.workspace = true",
                message,
            )

    def test_validate_lockfile_checks_all_workspace_packages(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "Cargo.lock").write_text(
                """\
version = 3

[[package]]
name = "agent-team-mail"
version = "1.1.2"

[[package]]
name = "agent-team-mail-core"
version = "1.1.2"

[[package]]
name = "agent-team-mail-daemon"
version = "1.1.2"
""",
                encoding="utf-8",
            )

            with self.assertRaises(SystemExit) as error:
                validate_lockfile(repo_root, "1.1.2")

            self.assertIn("agent-team-mail-rusqlite missing from Cargo.lock", str(error.exception))

    def test_validate_crate_versions_requires_internal_path_dep_pin(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            manifest = repo_root / "crates/atm/Cargo.toml"
            manifest.write_text(
                manifest.read_text(encoding="utf-8").replace(', version = "1.1.2"', ""),
                encoding="utf-8",
            )

            with self.assertRaises(SystemExit) as error:
                validate_crate_versions(repo_root, "1.1.2")

            self.assertIn(
                'crates/atm/Cargo.toml [dependencies.atm-core]: internal path dependency version must match target crate version "1.1.2"',
                str(error.exception),
            )

    def test_validate_crate_versions_requires_exact_prerelease_path_dep_pin(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root, workspace_version="1.1.2-beta-am.1")

            with self.assertRaises(SystemExit) as error:
                validate_crate_versions(repo_root, "1.1.2-beta-am.1")

            self.assertIn(
                'crates/atm/Cargo.toml [dependencies.atm-core]: internal path dependency version must match target crate version "1.1.2-beta-am.1"',
                str(error.exception),
            )

    def test_success_message_includes_workspace_version(self) -> None:
        self.assertEqual(
            success_message("1.1.2", ["workspace member versions", "internal path deps", "Cargo.lock"]),
            "version sync check passed: workspace_version=1.1.2; workspace member versions, internal path deps, Cargo.lock are aligned.",
        )

    def test_validate_winget_manifests_reads_installer_url_from_installers_array(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / ".winget").mkdir(parents=True)
            (repo_root / ".winget/randlee.agent-team-mail.yaml").write_text(
                """\
PackageIdentifier: randlee.agent-team-mail
PackageVersion: 1.1.2
Installers:
  - Architecture: x64
    InstallerType: zip
    InstallerUrl: https://github.com/randlee/atm-core/releases/download/v1.1.2/atm_1.1.2_x86_64-pc-windows-msvc.zip
ManifestType: installer
ManifestVersion: 1.1.2
""",
                encoding="utf-8",
            )
            config = {
                "winget": {
                    "enabled": True,
                    "manifest_glob": ".winget/*.yaml",
                    "package_version_field": "PackageVersion",
                    "manifest_version_field": "ManifestVersion",
                    "installer_url_field": "InstallerUrl",
                }
            }

            self.assertTrue(validate_winget_manifests(repo_root, "1.1.2", config))
            manifest = repo_root / ".winget/randlee.agent-team-mail.yaml"
            manifest.write_text(
                manifest.read_text(encoding="utf-8").replace("atm_1.1.2_", "atm_1.1.3_"),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(SystemExit, "versions"):
                validate_winget_manifests(repo_root, "1.1.2", config)

    def test_validate_winget_manifests_accepts_prerelease_versions(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / ".winget").mkdir(parents=True)
            (repo_root / ".winget/randlee.agent-team-mail.yaml").write_text(
                """\\
PackageIdentifier: randlee.agent-team-mail
PackageVersion: 1.3.2-beta-21-pre
Installers:
  - Architecture: x64
    InstallerType: zip
    InstallerUrl: https://github.com/randlee/atm-core/releases/download/v1.3.2-beta-21-pre/atm_1.3.2-beta-21-pre_x86_64-pc-windows-msvc.zip
ManifestType: installer
ManifestVersion: 1.3.2-beta-21-pre
""",
                encoding="utf-8",
            )
            config = {
                "winget": {
                    "enabled": True,
                    "manifest_glob": ".winget/*.yaml",
                    "package_version_field": "PackageVersion",
                    "manifest_version_field": "ManifestVersion",
                    "installer_url_field": "InstallerUrl",
                }
            }
            self.assertTrue(validate_winget_manifests(repo_root, "1.3.2-beta-21-pre", config))

    def test_replace_version_occurrences_rejects_adjacent_version_component(self) -> None:
        self.assertEqual(
            replace_version_occurrences(
                "https://example.invalid/atm_1.1.2.0_x86_64.zip", "1.1.2", "1.1.3"
            ),
            "https://example.invalid/atm_1.1.2.0_x86_64.zip",
        )

    @mock.patch("prerelease_tag.subprocess.run")
    def test_sync_python_version_propagates_owner_failure(self, run: mock.Mock) -> None:
        run.return_value = subprocess.CompletedProcess(
            args=[], returncode=1, stdout="", stderr="owner failed"
        )
        with self.assertRaisesRegex(SystemExit, "owner failed"):
            sync_python_version(Path("/tmp/atm"), Path("/tmp/atm/crates/example/pyproject.toml"))

    @unittest.skipUnless(hasattr(os, "symlink"), "symlink support is required")
    def test_copy_tracked_files_excludes_untracked_content_and_preserves_symlinks(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            source = Path(tempdir) / "source"
            destination = Path(tempdir) / "destination"
            source.mkdir()
            (source / "tracked.txt").write_text("tracked", encoding="utf-8")
            (source / "target.txt").write_text("target", encoding="utf-8")
            (source / "link.txt").symlink_to("target.txt")
            (source / "secret.txt").write_text("untracked", encoding="utf-8")
            subprocess.run(["git", "init", "-q"], cwd=source, check=True)
            subprocess.run(["git", "add", "tracked.txt", "target.txt", "link.txt"], cwd=source, check=True)
            copy_tracked_files(source, destination)
            self.assertTrue((destination / "tracked.txt").is_file())
            self.assertFalse((destination / "secret.txt").exists())
            self.assertTrue((destination / "link.txt").is_symlink())
            self.assertEqual(os.readlink(destination / "link.txt"), "target.txt")


if __name__ == "__main__":
    unittest.main()
