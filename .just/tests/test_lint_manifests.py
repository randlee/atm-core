from __future__ import annotations

from pathlib import Path
from pathlib import PureWindowsPath
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_manifests import collect_manifest_violations
from lint_manifests import relative_manifest_display


ROOT_MANIFEST = """\
[workspace]
members = ["crates/atm-core", "crates/atm"]
resolver = "2"

[workspace.package]
version = "1.1.2"
edition = "2024"
rust-version = "1.94.1"
authors = ["atm-core contributors"]
license = "MIT OR Apache-2.0"
repository = "https://example.invalid/repo"
homepage = "https://example.invalid/repo"
"""


GOOD_MEMBER = """\
[package]
name = "agent-team-mail-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true

[dependencies]
serde = "1"
"""


class LintManifestsTests(unittest.TestCase):
    def test_relative_manifest_display_uses_posix_separators(self) -> None:
        repo_root = PureWindowsPath(r"D:\a\atm-core\atm-core")
        manifest_path = PureWindowsPath(r"D:\a\atm-core\atm-core\crates\atm-core\Cargo.toml")

        self.assertEqual(
            relative_manifest_display(manifest_path, repo_root),
            "crates/atm-core/Cargo.toml",
        )

    def write_repo(self, repo_root: Path) -> None:
        (repo_root / "Cargo.toml").write_text(ROOT_MANIFEST, encoding="utf-8")
        crate_dir = repo_root / "crates/atm-core"
        crate_dir.mkdir(parents=True)
        (crate_dir / "Cargo.toml").write_text(GOOD_MEMBER, encoding="utf-8")

    def test_collect_manifest_violations_accepts_workspace_inheritance(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.assertEqual(collect_manifest_violations(repo_root), [])

    def test_collect_manifest_violations_flags_missing_workspace_field(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            manifest_path = repo_root / "crates/atm-core/Cargo.toml"
            manifest_path.write_text(GOOD_MEMBER.replace("homepage.workspace = true\n", ""), encoding="utf-8")

            violations = collect_manifest_violations(repo_root)
            rendered = [violation.render() for violation in violations]
            self.assertIn("crates/atm-core/Cargo.toml: set [package].homepage.workspace = true", rendered)

    def test_collect_manifest_violations_flags_mismatched_internal_path_version(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / "Cargo.toml").write_text(ROOT_MANIFEST.replace('"crates/atm"]', '"crates/atm", "crates/atm-core"]'), encoding="utf-8")
            atm_core_dir = repo_root / "crates/atm-core"
            atm_core_dir.mkdir(parents=True)
            (atm_core_dir / "Cargo.toml").write_text(GOOD_MEMBER, encoding="utf-8")
            atm_dir = repo_root / "crates/atm"
            atm_dir.mkdir(parents=True)
            (atm_dir / "Cargo.toml").write_text(
                """\
[package]
name = "agent-team-mail"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true

[dependencies]
atm-core = { path = "../atm-core", version = "9.9.9" }
""",
                encoding="utf-8",
            )

            violations = collect_manifest_violations(repo_root)
            rendered = [violation.render() for violation in violations]
            self.assertIn(
                'crates/atm/Cargo.toml [dependencies.atm-core]: path dependency version must match target crate version "1.1.2"',
                rendered,
            )

    def test_collect_manifest_violations_accepts_explicit_tool_crate_version(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / "Cargo.toml").write_text(
                ROOT_MANIFEST.replace('"crates/atm"]', '"crates/atm", "crates/sc-lint-attributes"]'),
                encoding="utf-8",
            )
            self.write_repo(repo_root)
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

            self.assertEqual(collect_manifest_violations(repo_root), [])


if __name__ == "__main__":
    unittest.main()
