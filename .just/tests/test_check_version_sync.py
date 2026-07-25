from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from check_version_sync import validate_crate_versions
from check_version_sync import validate_lockfile
from check_version_sync import validate_winget_manifests
from check_version_sync import success_message


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
    def write_repo(self, repo_root: Path) -> None:
        (repo_root / "Cargo.toml").write_text(ROOT_MANIFEST, encoding="utf-8")
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

    def test_validate_crate_versions_accepts_explicit_tool_crate_versions(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / "Cargo.toml").write_text(
                """\
[workspace]
members = ["crates/atm-core", "crates/sc-lint-attributes"]
resolver = "2"

[workspace.package]
version = "1.1.2"
""",
                encoding="utf-8",
            )
            atm_core_dir = repo_root / "crates/atm-core"
            atm_core_dir.mkdir(parents=True)
            (atm_core_dir / "Cargo.toml").write_text(
                crate_manifest(
                    "agent-team-mail-core",
                    extra='\n[dependencies]\nsc-lint-attributes = { path = "../sc-lint-attributes", version = "0.1.0" }\n',
                ),
                encoding="utf-8",
            )
            tool_dir = repo_root / "crates/sc-lint-attributes"
            tool_dir.mkdir(parents=True)
            (tool_dir / "Cargo.toml").write_text(
                """\
[package]
name = "sc-lint-attributes"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true
""",
                encoding="utf-8",
            )

            validate_crate_versions(repo_root, "1.1.2")

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


if __name__ == "__main__":
    unittest.main()
