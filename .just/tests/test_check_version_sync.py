from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import tomllib
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from check_version_sync import validate_crate_versions
from check_version_sync import validate_lockfile
from check_version_sync import validate_winget_manifests
from check_version_sync import success_message
from check_version_sync import validate_python_release_versions


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

    def write_python_release_manifests(self, repo_root: Path, version: str) -> tuple[tuple[Path, str], ...]:
        manifests = (
            (Path("crates/atm-graft-python/Cargo.toml"), "package"),
            (Path("crates/atm-graft-python/pyproject.toml"), "project"),
            (Path("crates/atm-query-python/Cargo.toml"), "package"),
            (Path("crates/atm-query-python/pyproject.toml"), "project"),
            (Path("crates/hermes-atm/pyproject.toml"), "project"),
        )
        for manifest_path, table_name in manifests:
            path = repo_root / manifest_path
            path.parent.mkdir(parents=True, exist_ok=True)
            name = path.parent.name
            path.write_text(
                f'[{table_name}]\nname = "{name}"\nversion = "{version}"\n',
                encoding="utf-8",
            )
        return manifests

    def test_python_release_versions_accept_final_workspace_version(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            manifests = self.write_python_release_manifests(repo_root, "1.4.1")

            validate_python_release_versions(repo_root, "1.4.1", manifests)

    def test_python_release_versions_strip_prerelease_workspace_qualifier(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            manifests = self.write_python_release_manifests(repo_root, "1.4.1")

            validate_python_release_versions(repo_root, "1.4.1-beta-ai-15", manifests)
            validate_python_release_versions(repo_root, "1.4.1-beta-aj", manifests)

    def test_python_release_versions_strip_combined_workspace_qualifiers(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            manifests = self.write_python_release_manifests(repo_root, "1.4.1")

            validate_python_release_versions(repo_root, "1.4.1-beta-ai-15+build.7", manifests)

    def test_python_release_versions_accept_dynamic_maturin_version_from_cargo(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            manifests = self.write_python_release_manifests(repo_root, "1.4.1")
            (repo_root / "crates/atm-graft-python/pyproject.toml").write_text(
                '[project]\nname = "atm-graft"\ndynamic = ["version"]\n',
                encoding="utf-8",
            )

            validate_python_release_versions(repo_root, "1.4.1", manifests)

    def test_python_release_versions_reject_wrong_dynamic_maturin_cargo_version(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            manifests = self.write_python_release_manifests(repo_root, "1.4.1")
            (repo_root / "crates/atm-graft-python/pyproject.toml").write_text(
                '[project]\nname = "atm-graft"\ndynamic = ["version"]\n',
                encoding="utf-8",
            )
            cargo_path = repo_root / "crates/atm-graft-python/Cargo.toml"
            cargo_path.write_text(
                cargo_path.read_text(encoding="utf-8").replace('version = "1.4.1"', 'version = "1.4.2"'),
                encoding="utf-8",
            )

            with self.assertRaises(SystemExit) as error:
                validate_python_release_versions(repo_root, "1.4.1", manifests)

            self.assertIn("crates/atm-graft-python/Cargo.toml", str(error.exception))
            self.assertIn("1.4.2", str(error.exception))

    def test_python_release_versions_reject_wrong_patch(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            manifests = self.write_python_release_manifests(repo_root, "1.4.2")

            with self.assertRaises(SystemExit) as error:
                validate_python_release_versions(repo_root, "1.4.1-beta-ai-15", manifests)

            message = str(error.exception)
            self.assertIn("crates/atm-graft-python/Cargo.toml", message)
            self.assertIn("1.4.2", message)
            self.assertIn("1.4.1", message)

    def test_python_release_versions_reject_python_prerelease(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            manifests = self.write_python_release_manifests(repo_root, "1.4.1-beta-ai")

            with self.assertRaises(SystemExit) as error:
                validate_python_release_versions(repo_root, "1.4.1-beta-ai-15", manifests)

            message = str(error.exception)
            self.assertIn("crates/atm-graft-python/Cargo.toml", message)
            self.assertIn("1.4.1-beta-ai", message)
            self.assertIn("1.4.1", message)

    def test_python_release_versions_reject_python_build_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            manifests = self.write_python_release_manifests(repo_root, "1.4.1+local")

            with self.assertRaises(SystemExit) as error:
                validate_python_release_versions(repo_root, "1.4.1-beta-ai-15+build.7", manifests)

            message = str(error.exception)
            self.assertIn("crates/atm-graft-python/Cargo.toml", message)
            self.assertIn("1.4.1+local", message)
            self.assertIn("1.4.1", message)

    def test_real_repository_python_release_versions_are_numeric_workspace_base(self) -> None:
        repo_root = Path(__file__).resolve().parents[2]
        workspace_version = tomllib.loads(
            (repo_root / "Cargo.toml").read_text(encoding="utf-8")
        )["workspace"]["package"]["version"]

        validate_python_release_versions(repo_root, workspace_version)

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


if __name__ == "__main__":
    unittest.main()
