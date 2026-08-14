from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from check_test_identity_literals import collect_identity_violations
from check_test_identity_literals import load_forbidden_literals
from check_test_identity_literals import load_production_canonical_literals


LINT_CONFIG = """\
[identities]
forbidden_literals = [
  "team-lead",
  "arch-ctm",
  "quality-mgr",
]

[identities.production_canonical_literals]
"team-lead" = ["crates/atm-core/src/roles.rs"]
"""

ROOT_MANIFEST = """\
[workspace]
members = ["crates/atm-core"]
resolver = "2"
"""


class CheckTestIdentityLiteralsTests(unittest.TestCase):
    def write_repo(self, repo_root: Path) -> None:
        (repo_root / ".just").mkdir()
        (repo_root / ".just/lint-config.toml").write_text(LINT_CONFIG, encoding="utf-8")
        (repo_root / "Cargo.toml").write_text(ROOT_MANIFEST, encoding="utf-8")
        (repo_root / "crates/atm-core/src").mkdir(parents=True)
        (repo_root / "crates/atm-core/tests").mkdir(parents=True)
        (repo_root / "crates/atm-core/Cargo.toml").write_text(
            """\
[package]
name = "agent-team-mail-core"
version = "1.1.2"

[lib]
name = "atm_core"
""",
            encoding="utf-8",
        )

    def test_load_forbidden_literals_reads_config(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)

            literals = load_forbidden_literals(repo_root)

            self.assertEqual(literals, ("team-lead", "arch-ctm", "quality-mgr"))

    def test_collect_identity_violations_flags_test_scope_only(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/example.rs").write_text(
                """\
pub fn production() {
    let role = "other-role";
}

#[cfg(test)]
mod tests {
    #[test]
    fn example() {
        let role = "team-lead";
    }
}
""",
                encoding="utf-8",
            )

            violations = collect_identity_violations(
                repo_root,
                forbidden_literals=load_forbidden_literals(repo_root),
                production_canonical_literals=load_production_canonical_literals(repo_root),
            )

            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].line_number, 9)
            self.assertEqual(violations[0].kind, "test_scope_forbidden_literal")

    def test_collect_identity_violations_flags_production_literal_outside_canonical_source(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/roles.rs").write_text(
                'pub const ROLE_TEAM_LEAD: &str = "team-lead";\n',
                encoding="utf-8",
            )
            (repo_root / "crates/atm-core/src/example.rs").write_text(
                """\
pub fn production() {
    let role = "team-lead";
}
""",
                encoding="utf-8",
            )

            violations = collect_identity_violations(
                repo_root,
                forbidden_literals=load_forbidden_literals(repo_root),
                production_canonical_literals=load_production_canonical_literals(repo_root),
            )

            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].path, "crates/atm-core/src/example.rs")
            self.assertEqual(violations[0].kind, "production_scope_canonical_literal")

    def test_collect_identity_violations_flags_forbidden_production_literal_without_canonical_source(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/example.rs").write_text(
                'pub const TEST_ARCH_CTM: &str = "arch-ctm";\n',
                encoding="utf-8",
            )

            violations = collect_identity_violations(
                repo_root,
                forbidden_literals=load_forbidden_literals(repo_root),
                production_canonical_literals=load_production_canonical_literals(repo_root),
            )

            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].path, "crates/atm-core/src/example.rs")
            self.assertEqual(violations[0].kind, "production_scope_forbidden_literal")

    def test_collect_identity_violations_allows_non_semantic_prose_mentions(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/roles.rs").write_text(
                'pub const ROLE_TEAM_LEAD: &str = "team-lead";\n',
                encoding="utf-8",
            )
            (repo_root / "crates/atm-core/src/example.rs").write_text(
                """\
pub fn production() {
    let message = "failed to notify team-lead recipient";
}
""",
                encoding="utf-8",
            )

            violations = collect_identity_violations(
                repo_root,
                forbidden_literals=load_forbidden_literals(repo_root),
                production_canonical_literals=load_production_canonical_literals(repo_root),
            )

            self.assertEqual(violations, [])

    def test_collect_identity_violations_allows_canonical_production_source(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/roles.rs").write_text(
                'pub const ROLE_TEAM_LEAD: &str = "team-lead";\n',
                encoding="utf-8",
            )

            violations = collect_identity_violations(
                repo_root,
                forbidden_literals=load_forbidden_literals(repo_root),
                production_canonical_literals=load_production_canonical_literals(repo_root),
            )

            self.assertEqual(violations, [])

    def test_collect_identity_violations_reassembles_multiline_concat_fragments(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/example.rs").write_text(
                """\
#[cfg(test)]
mod tests {
    #[test]
    fn example() {
        let role = concat!(
            "team-",
            "lead",
        );
    }
}
""",
                encoding="utf-8",
            )

            violations = collect_identity_violations(
                repo_root,
                forbidden_literals=load_forbidden_literals(repo_root),
                production_canonical_literals=load_production_canonical_literals(repo_root),
            )

            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].line_number, 5)
            self.assertEqual(violations[0].kind, "test_scope_forbidden_literal")

    def test_collect_identity_violations_ignores_nonmatching_concat_fragments(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/example.rs").write_text(
                """\
#[cfg(test)]
mod tests {
    #[test]
    fn example() {
        let role = concat!(
            "team-",
            "member",
        );
    }
}
""",
                encoding="utf-8",
            )

            violations = collect_identity_violations(
                repo_root,
                forbidden_literals=load_forbidden_literals(repo_root),
                production_canonical_literals=load_production_canonical_literals(repo_root),
            )

            self.assertEqual(violations, [])


if __name__ == "__main__":
    unittest.main()
