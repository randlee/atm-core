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
            manifest.write_text(manifest.read_text(encoding="utf-8").replace("version.workspace = true\n", ""), encoding="utf-8")

            with self.assertRaises(SystemExit) as error:
                validate_crate_versions(repo_root, "1.1.2")

            self.assertIn("crates/atm-rusqlite/Cargo.toml must use version.workspace = true", str(error.exception))

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


if __name__ == "__main__":
    unittest.main()
