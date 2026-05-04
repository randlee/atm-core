from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import textwrap
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_boundaries import collect_boundary_violations
from lint_boundaries import parse_boundary_records
from lint_boundaries import parse_simple_yaml_document


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

BASE_BOUNDARY_DOC = """\
# Example Boundaries

## SqliteMailStoreAdapter

```yaml
boundary_id: BOUNDARY-MailStore-Sqlite
owner_package: atm-rusqlite
owner_crate_path: atm_rusqlite
name: SqliteMailStoreAdapter

public:
  trait: MailStore
  facade: null

implementation:
  type: SqliteMailStore
  module: atm_rusqlite::mail_store
  visibility: private
  constructor: private

composition:
  roots: []

ownership:
  io_owns:
    - sqlite
  io_forbidden:
    - socket_io

dependencies:
  allowed_dependents: []
  allowed_dependencies:
    - atm-core
    - rusqlite
  forbidden_edges:
    - atm -> atm-rusqlite

references:
  scope: outside_owner_crate
  forbidden:
    - SqliteMailStore
    - rusqlite::Connection

contracts:
  request_types:
    - MailStore inputs
  response_types:
    - MailStore outputs
  error_types:
    - AtmError

testing:
  allowed_test_double_paths:
    - atm_core::test_support::InMemoryMailStore
  forbidden_test_bypasses:
    - rusqlite::Connection

enforcement:
  lint_rules:
    - LINT-BOUNDARY-MAILSTORE-SQLITE-EDGES
  review_gates:
    - no_public_impl

status:
  state: planned
  notes: []
```
"""


class LintBoundariesTests(unittest.TestCase):
    def write_repo(self, repo_root: Path) -> None:
        (repo_root / "Cargo.toml").write_text(ROOT_MANIFEST, encoding="utf-8")
        for crate_name in ("atm-core", "atm-rusqlite", "atm"):
            crate_dir = repo_root / "crates" / crate_name
            crate_dir.mkdir(parents=True)
            (crate_dir / "src").mkdir()
            (crate_dir / "src/lib.rs").write_text("pub fn example() {}\n", encoding="utf-8")
        (repo_root / "docs" / "atm-rusqlite").mkdir(parents=True)

    def write_manifests(self, repo_root: Path, *, atm_dependencies: str = "atm-core = { path = \"../atm-core\", version = \"1.1.2\" }\n") -> None:
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
            f"""\
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
{atm_dependencies}""",
            encoding="utf-8",
        )

    def test_parse_simple_yaml_document_reads_nested_lists(self) -> None:
        document = textwrap.dedent(
            """\
            boundary_id: BOUNDARY-Test
            dependencies:
              forbidden_edges:
                - atm -> atm-rusqlite
            status:
              state: planned
            """
        )
        parsed = parse_simple_yaml_document(document)
        self.assertEqual(parsed["boundary_id"], "BOUNDARY-Test")
        self.assertEqual(parsed["dependencies"]["forbidden_edges"], ["atm -> atm-rusqlite"])
        self.assertEqual(parsed["status"]["state"], "planned")

    def test_parse_boundary_records_accepts_planned_schema(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            (repo_root / "docs/atm-rusqlite/boundaries.md").write_text(BASE_BOUNDARY_DOC, encoding="utf-8")

            records, violations = parse_boundary_records(repo_root)
            self.assertEqual(violations, [])
            self.assertEqual(len(records), 1)
            self.assertEqual(records[0].boundary_id, "BOUNDARY-MailStore-Sqlite")
            self.assertFalse(records[0].is_enforced)

    def test_parse_boundary_records_flags_missing_required_field(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            broken_doc = BASE_BOUNDARY_DOC.replace("owner_crate_path: atm_rusqlite\n", "")
            (repo_root / "docs/atm-rusqlite/boundaries.md").write_text(broken_doc, encoding="utf-8")

            _records, violations = parse_boundary_records(repo_root)
            rendered = [violation.render() for violation in violations]
            self.assertTrue(
                any(item.endswith("missing required field: owner_crate_path") for item in rendered),
                rendered,
            )

    def test_collect_boundary_violations_accepts_allowed_layout(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            (repo_root / "docs/atm-rusqlite/boundaries.md").write_text(BASE_BOUNDARY_DOC, encoding="utf-8")
            (repo_root / "crates/atm-rusqlite/src/lib.rs").write_text("use rusqlite::Connection;\n", encoding="utf-8")

            self.assertEqual(collect_boundary_violations(repo_root), [])

    def test_collect_boundary_violations_flags_direct_rusqlite_dependency(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            (repo_root / "docs/atm-rusqlite/boundaries.md").write_text(BASE_BOUNDARY_DOC, encoding="utf-8")
            (repo_root / "crates/atm-core/Cargo.toml").write_text(
                """\
[package]
name = "agent-team-mail-core"

[dependencies]
rusqlite = "0.37"
""",
                encoding="utf-8",
            )

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertIn(
                "crates/atm-core/Cargo.toml [dependencies]: only crates/atm-rusqlite may depend on rusqlite",
                rendered,
            )

    def test_collect_boundary_violations_flags_rusqlite_source_import_outside_store(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            (repo_root / "docs/atm-rusqlite/boundaries.md").write_text(BASE_BOUNDARY_DOC, encoding="utf-8")
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
            self.write_manifests(repo_root)
            (repo_root / "docs/atm-rusqlite/boundaries.md").write_text(BASE_BOUNDARY_DOC, encoding="utf-8")
            (repo_root / "crates/atm-core/Cargo.toml").write_text(
                """\
[package]
name = "agent-team-mail-core"

[dependencies]
atm-rusqlite = { path = "../atm-rusqlite", version = "1.1.2" }
""",
                encoding="utf-8",
            )

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertIn(
                "crates/atm-core/Cargo.toml [dependencies]: atm-core may reference atm-rusqlite only in dev-dependencies",
                rendered,
            )

    def test_collect_boundary_violations_ignores_planned_forbidden_edges(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(
                repo_root,
                atm_dependencies="atm-rusqlite = { path = \"../atm-rusqlite\", version = \"1.1.2\" }\n",
            )
            (repo_root / "docs/atm-rusqlite/boundaries.md").write_text(BASE_BOUNDARY_DOC, encoding="utf-8")

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertNotIn(
                "crates/atm/Cargo.toml [dependencies]: BOUNDARY-MailStore-Sqlite forbids edge atm -> atm-rusqlite",
                rendered,
            )

    def test_collect_boundary_violations_flags_active_forbidden_edges(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(
                repo_root,
                atm_dependencies="atm-rusqlite = { path = \"../atm-rusqlite\", version = \"1.1.2\" }\n",
            )
            active_doc = BASE_BOUNDARY_DOC.replace("state: planned", "state: active")
            (repo_root / "docs/atm-rusqlite/boundaries.md").write_text(active_doc, encoding="utf-8")

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertIn(
                "crates/atm/Cargo.toml [dependencies]: BOUNDARY-MailStore-Sqlite forbids edge atm -> atm-rusqlite",
                rendered,
            )


if __name__ == "__main__":
    unittest.main()
