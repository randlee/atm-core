from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_boundaries import collect_boundary_violations


ROOT_MANIFEST = """\
[workspace]
members = ["crates/atm-core", "crates/atm-rusqlite", "crates/atm"]
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


class LintBoundariesTests(unittest.TestCase):
    def write_repo(self, repo_root: Path) -> None:
        (repo_root / "Cargo.toml").write_text(ROOT_MANIFEST, encoding="utf-8")
        for crate_name in ("atm-core", "atm-rusqlite", "atm"):
            crate_dir = repo_root / "crates" / crate_name
            crate_dir.mkdir(parents=True)
            (crate_dir / "src").mkdir()
            (crate_dir / "src/lib.rs").write_text("pub fn example() {}\n", encoding="utf-8")

    def test_collect_boundary_violations_accepts_allowed_layout(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/Cargo.toml").write_text(
                """\
[package]
name = "agent-team-mail-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true

[dev-dependencies]
atm-rusqlite = { path = "../atm-rusqlite", version = "1.1.2" }
""",
                encoding="utf-8",
            )
            (repo_root / "crates/atm-rusqlite/Cargo.toml").write_text(
                """\
[package]
name = "atm-rusqlite"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true

[dependencies]
rusqlite = "0.37"
atm-core = { path = "../atm-core", version = "1.1.2" }
""",
                encoding="utf-8",
            )
            (repo_root / "crates/atm/Cargo.toml").write_text(
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
atm-core = { path = "../atm-core", version = "1.1.2" }
""",
                encoding="utf-8",
            )
            (repo_root / "crates/atm-rusqlite/src/lib.rs").write_text("use rusqlite::Connection;\n", encoding="utf-8")

            self.assertEqual(collect_boundary_violations(repo_root), [])

    def test_collect_boundary_violations_flags_direct_rusqlite_dependency(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/Cargo.toml").write_text(
                """\
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
rusqlite = "0.37"
""",
                encoding="utf-8",
            )
            (repo_root / "crates/atm-rusqlite/Cargo.toml").write_text("[package]\nname = \"atm-rusqlite\"\n", encoding="utf-8")
            (repo_root / "crates/atm/Cargo.toml").write_text("[package]\nname = \"agent-team-mail\"\n", encoding="utf-8")

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertIn(
                "crates/atm-core/Cargo.toml [dependencies]: only crates/atm-rusqlite may depend on rusqlite",
                rendered,
            )

    def test_collect_boundary_violations_flags_rusqlite_source_import_outside_store(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/Cargo.toml").write_text("[package]\nname = \"agent-team-mail-core\"\n", encoding="utf-8")
            (repo_root / "crates/atm-rusqlite/Cargo.toml").write_text("[package]\nname = \"atm-rusqlite\"\n", encoding="utf-8")
            (repo_root / "crates/atm/Cargo.toml").write_text("[package]\nname = \"agent-team-mail\"\n", encoding="utf-8")
            (repo_root / "crates/atm/src/lib.rs").write_text("use rusqlite::Connection;\n", encoding="utf-8")

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertIn(
                "crates/atm/src/lib.rs:1: only crates/atm-rusqlite source may import rusqlite directly",
                rendered,
            )

    def test_collect_boundary_violations_flags_non_dev_atm_core_edge(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/Cargo.toml").write_text(
                """\
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
atm-rusqlite = { path = "../atm-rusqlite", version = "1.1.2" }
""",
                encoding="utf-8",
            )
            (repo_root / "crates/atm-rusqlite/Cargo.toml").write_text("[package]\nname = \"atm-rusqlite\"\n", encoding="utf-8")
            (repo_root / "crates/atm/Cargo.toml").write_text("[package]\nname = \"agent-team-mail\"\n", encoding="utf-8")

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertIn(
                "crates/atm-core/Cargo.toml [dependencies]: atm-core may reference atm-rusqlite only in dev-dependencies",
                rendered,
            )


if __name__ == "__main__":
    unittest.main()
