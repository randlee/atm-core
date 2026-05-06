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
from lint_boundaries import boundary_doc_section_lines
from lint_boundaries import parse_boundary_records
from lint_boundaries import parse_simple_yaml_document


ROOT_MANIFEST = """\
[workspace]
members = ["crates/atm-core", "crates/atm-rusqlite", "crates/atm", "crates/atm-daemon"]
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

LINT_CONFIG = """\
[boundaries]
doc_glob = "docs/*/boundaries.md"

[[boundaries.global_dependency_ownership]]
dependency = "rusqlite"
allowed_manifest_paths = ["crates/atm-rusqlite/Cargo.toml"]
allowed_source_roots = ["crates/atm-rusqlite/src"]
manifest_message = "only crates/atm-rusqlite may depend on rusqlite"
source_message = "only crates/atm-rusqlite source may import rusqlite directly"

[[boundaries.manifest_section_rules]]
owner_manifest_path = "crates/atm-core/Cargo.toml"
dependency_package = "atm-rusqlite"
allowed_sections = ["dev-dependencies"]
message = "atm-core may reference atm-rusqlite only in dev-dependencies"

[[boundaries.manifest_section_rules]]
owner_manifest_path = "crates/atm/Cargo.toml"
dependency_package = "atm-daemon"
allowed_sections = []
message = "atm must not depend on atm-daemon"

[[boundaries.manifest_section_rules]]
owner_manifest_path = "crates/atm-core/Cargo.toml"
dependency_package = "atm-daemon"
allowed_sections = []
message = "atm-core must not depend on atm-daemon"

[[boundaries.manifest_section_rules]]
owner_manifest_path = "crates/atm-daemon/Cargo.toml"
dependency_package = "atm-rusqlite"
allowed_sections = []
message = "atm-daemon must not depend on atm-rusqlite"
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
  roots:
    - atm_daemon::bootstrap

ownership:
  io_owns:
    - sqlite
  io_forbidden:
    - socket_io

dependencies:
  allowed_dependents:
    - atm-daemon
  allowed_dependencies:
    - atm-core
    - rusqlite
  forbidden_edges:
    - atm -> atm-rusqlite
    - atm-graft -> atm-rusqlite

references:
  scope: outside_owner_crate
  forbidden:
    - SqliteMailStore
    - SqliteMailStore::open
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
        (repo_root / ".just").mkdir()
        (repo_root / ".just/lint-config.toml").write_text(LINT_CONFIG, encoding="utf-8")
        for crate_name in ("atm-core", "atm-rusqlite", "atm", "atm-daemon"):
            crate_dir = repo_root / "crates" / crate_name
            crate_dir.mkdir(parents=True)
            (crate_dir / "src").mkdir()
            (crate_dir / "src/lib.rs").write_text("pub fn example() {}\n", encoding="utf-8")
        for doc_name in ("atm-core", "atm-rusqlite", "atm", "atm-daemon"):
            (repo_root / "docs" / doc_name).mkdir(parents=True)

    def write_manifests(
        self,
        repo_root: Path,
        *,
        atm_dependencies: str = 'atm-core = { package = "agent-team-mail-core", path = "../atm-core", version = "1.1.2" }\n',
        atm_rusqlite_dependencies: str = 'rusqlite = "0.37"\natm-core = { package = "agent-team-mail-core", path = "../atm-core", version = "1.1.2" }\n',
        atm_daemon_dependencies: str = 'atm-core = { package = "agent-team-mail-core", path = "../atm-core", version = "1.1.2" }\n',
    ) -> None:
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

[lib]
name = "atm_core"

[dev-dependencies]
atm-rusqlite = { path = "../atm-rusqlite", version = "1.1.2" }
""",
            encoding="utf-8",
        )
        (repo_root / "crates/atm-rusqlite/Cargo.toml").write_text(
            f"""\
[package]
name = "atm-rusqlite"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true

[lib]
name = "atm_rusqlite"

[dependencies]
{atm_rusqlite_dependencies}""",
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

[lib]
name = "atm"

[dependencies]
{atm_dependencies}""",
            encoding="utf-8",
        )
        (repo_root / "crates/atm-daemon/Cargo.toml").write_text(
            """\
[package]
name = "atm-daemon"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true

[lib]
name = "atm_daemon"

[dependencies]
"""
            + atm_daemon_dependencies,
            encoding="utf-8",
        )

    def test_collect_boundary_violations_flags_cli_daemon_edge(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(
                repo_root,
                atm_dependencies='atm-daemon = { path = "../atm-daemon", version = "1.1.2" }\n',
            )
            self.write_doc(repo_root, "atm-rusqlite")

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertIn(
                "crates/atm/Cargo.toml [dependencies]: atm must not depend on atm-daemon",
                rendered,
            )

    def test_collect_boundary_violations_flags_core_daemon_edge(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-rusqlite")
            (repo_root / "crates/atm-core/Cargo.toml").write_text(
                """\
[package]
name = "agent-team-mail-core"

[dependencies]
atm-daemon = { path = "../atm-daemon", version = "1.1.2" }
""",
                encoding="utf-8",
            )

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertIn(
                "crates/atm-core/Cargo.toml [dependencies]: atm-core must not depend on atm-daemon",
                rendered,
            )

    def test_collect_boundary_violations_flags_daemon_rusqlite_edge(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(
                repo_root,
                atm_daemon_dependencies='atm-core = { package = "agent-team-mail-core", path = "../atm-core", version = "1.1.2" }\natm-rusqlite = { path = "../atm-rusqlite", version = "1.1.2" }\n',
            )
            self.write_doc(repo_root, "atm-rusqlite")

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertIn(
                "crates/atm-daemon/Cargo.toml [dependencies]: atm-daemon must not depend on atm-rusqlite",
                rendered,
            )

    def write_doc(self, repo_root: Path, crate_name: str, text: str = BASE_BOUNDARY_DOC) -> None:
        (repo_root / "docs" / crate_name / "boundaries.md").write_text(text, encoding="utf-8")

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
            self.write_doc(repo_root, "atm-rusqlite")

            records, violations = parse_boundary_records(repo_root)
            self.assertEqual(violations, [])
            self.assertEqual(len(records), 1)
            self.assertEqual(records[0].boundary_id, "BOUNDARY-MailStore-Sqlite")
            self.assertFalse(records[0].is_active)

    def test_boundary_doc_section_lines_reports_per_doc_counts(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-rusqlite")
            self.write_doc(
                repo_root,
                "atm-core",
                BASE_BOUNDARY_DOC.replace("owner_package: atm-rusqlite", "owner_package: atm-core").replace(
                    "owner_crate_path: atm_rusqlite", "owner_crate_path: atm_core"
                ),
            )

            records, violations = parse_boundary_records(repo_root)
            self.assertEqual(violations, [])

            lines = boundary_doc_section_lines(repo_root, records)
            joined = "\n".join(lines)
            self.assertIn("boundary docs analyzed:", joined)
            self.assertIn("docs/atm-core/boundaries.md", joined)
            self.assertIn("docs/atm-rusqlite/boundaries.md", joined)
            self.assertIn("boundary doc count: 2", joined)
            self.assertIn("boundary records validated: 2", joined)

    def test_parse_boundary_records_flags_missing_required_field(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            broken_doc = BASE_BOUNDARY_DOC.replace("owner_crate_path: atm_rusqlite\n", "")
            self.write_doc(repo_root, "atm-rusqlite", broken_doc)

            _records, violations = parse_boundary_records(repo_root)
            rendered = [violation.render() for violation in violations]
            self.assertTrue(
                any("missing required field: owner_crate_path" in item for item in rendered),
                rendered,
            )

    def test_parse_boundary_records_flags_invalid_edge_syntax(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            broken_doc = BASE_BOUNDARY_DOC.replace("- atm -> atm-rusqlite", "- atm => atm-rusqlite")
            self.write_doc(repo_root, "atm-rusqlite", broken_doc)

            _records, violations = parse_boundary_records(repo_root)
            rendered = [violation.render() for violation in violations]
            self.assertTrue(
                any("invalid dependencies.forbidden_edges entry" in item for item in rendered),
                rendered,
            )

    def test_parse_boundary_records_flags_doc_owner_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm", BASE_BOUNDARY_DOC)

            _records, violations = parse_boundary_records(repo_root)
            rendered = [violation.render() for violation in violations]
            self.assertTrue(any("document path owner mismatch" in item for item in rendered), rendered)

    def test_collect_boundary_violations_accepts_allowed_layout(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-rusqlite")
            (repo_root / "crates/atm-rusqlite/src/lib.rs").write_text("use rusqlite::Connection;\n", encoding="utf-8")

            self.assertEqual(collect_boundary_violations(repo_root), [])

    def test_collect_boundary_violations_flags_duplicate_boundary_ids(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-rusqlite")
            self.write_doc(repo_root, "atm-daemon", BASE_BOUNDARY_DOC.replace("owner_package: atm-rusqlite", "owner_package: atm-daemon").replace("owner_crate_path: atm_rusqlite", "owner_crate_path: atm_daemon"))

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertTrue(any("duplicate boundary_id" in item for item in rendered), rendered)

    def test_collect_boundary_violations_flags_owner_crate_path_mismatch_for_existing_crate(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            broken_doc = BASE_BOUNDARY_DOC.replace("owner_crate_path: atm_rusqlite", "owner_crate_path: bad_path")
            self.write_doc(repo_root, "atm-rusqlite", broken_doc)

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertTrue(any("does not match workspace crate path" in item for item in rendered), rendered)

    def test_collect_boundary_violations_flags_unexpected_allowed_dependent(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root, atm_dependencies='atm-rusqlite = { path = "../atm-rusqlite", version = "1.1.2" }\n')
            self.write_doc(repo_root, "atm-rusqlite")

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertTrue(any("found unexpected dependent" in item for item in rendered), rendered)

    def test_collect_boundary_violations_flags_forbidden_edge_even_when_planned(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root, atm_dependencies='atm-rusqlite = { path = "../atm-rusqlite", version = "1.1.2" }\n')
            self.write_doc(repo_root, "atm-rusqlite")

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertIn(
                "crates/atm/Cargo.toml [dependencies]: BOUNDARY-MailStore-Sqlite forbids edge atm -> atm-rusqlite",
                rendered,
            )

    def test_collect_boundary_violations_flags_direct_rusqlite_dependency(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-rusqlite")
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
            self.write_doc(repo_root, "atm-rusqlite")
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
            self.write_doc(repo_root, "atm-rusqlite")
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

    def test_collect_boundary_violations_flags_forbidden_reference_outside_owner(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-rusqlite")
            (repo_root / "crates/atm-core/src/lib.rs").write_text("pub fn demo() { let _ = SqliteMailStore::open(); }\n", encoding="utf-8")

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertTrue(any("forbids external reference 'SqliteMailStore::open'" in item for item in rendered), rendered)

    def test_collect_boundary_violations_allows_composition_root_reference(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-rusqlite")
            (repo_root / "crates/atm-daemon/src/bootstrap.rs").write_text(
                "pub fn build() { let _ = SqliteMailStore::open(); }\n",
                encoding="utf-8",
            )

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertFalse(any("SqliteMailStore::open" in item for item in rendered), rendered)

    def test_collect_boundary_violations_flags_active_public_impl_and_constructor(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            active_doc = BASE_BOUNDARY_DOC.replace("state: planned", "state: active")
            self.write_doc(repo_root, "atm-rusqlite", active_doc)
            (repo_root / "crates/atm-rusqlite/src/lib.rs").write_text(
                textwrap.dedent(
                    """\
                    pub struct SqliteMailStore;
                    pub use self::SqliteMailStore as ExportedStore;

                    impl SqliteMailStore {
                        pub fn open() -> Self {
                            Self
                        }
                    }
                    """
                ),
                encoding="utf-8",
            )

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertTrue(any("requires private implementation.type" in item for item in rendered), rendered)
            self.assertTrue(any("forbids public re-export" in item for item in rendered), rendered)
            self.assertTrue(any("forbids public constructor/helper methods" in item for item in rendered), rendered)


if __name__ == "__main__":
    unittest.main()
