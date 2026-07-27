from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import textwrap
import tomllib
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
REPO_ROOT = JUST_DIR.parent
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_boundaries import collect_boundary_violations
from lint_boundaries import collect_io_forbidden_source_violations
from lint_boundaries import boundary_doc_section_lines
from lint_boundaries import IO_FORBIDDEN_SOURCE_PATTERNS
from lint_boundaries import parse_boundary_records
from lint_boundaries import parse_simple_yaml_document


ROOT_MANIFEST = """\
[workspace]
members = ["crates/atm-core", "crates/atm-storage-rusqlite", "crates/atm", "crates/atm-daemon"]
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
toml_glob = "boundaries/*/*.toml"

[[boundaries.global_dependency_ownership]]
dependency = "rusqlite"
allowed_manifest_paths = ["crates/atm-storage-rusqlite/Cargo.toml"]
allowed_source_roots = ["crates/atm-storage-rusqlite/src"]
manifest_message = "only crates/atm-storage-rusqlite may depend on rusqlite"
source_message = "only crates/atm-storage-rusqlite source may import rusqlite directly"

[[boundaries.manifest_section_rules]]
owner_manifest_path = "crates/atm-core/Cargo.toml"
dependency_package = "atm-storage-rusqlite"
allowed_sections = ["dev-dependencies"]
message = "atm-core may reference atm-storage-rusqlite only in dev-dependencies"

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
dependency_package = "atm-storage-rusqlite"
allowed_sections = []
message = "atm-daemon must not depend on atm-storage-rusqlite"
"""

BASE_BOUNDARY_DOC = """\
# Example Boundaries

## SqliteMailStoreAdapter

```yaml
boundary_id: BOUNDARY-MailStore-Sqlite
owner_package: atm-storage-rusqlite
owner_crate_path: atm_storage_rusqlite
name: SqliteMailStoreAdapter

public:
  trait: MailStore
  facade: null

implementation:
  type: SqliteMailStore
  module: atm_storage_rusqlite::mail_store
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
    - atm -> atm-storage-rusqlite
    - atm-graft -> atm-storage-rusqlite

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

BASE_BOUNDARY_TOML = """\
boundary_id = "BOUNDARY-MailStore-Sqlite"
owner_package = "atm-storage-rusqlite"
owner_crate_path = "atm_storage_rusqlite"
name = "SqliteMailStoreAdapter"

[public]
trait = "MailStore"

[implementation]
type = "SqliteMailStore"
module = "atm_storage_rusqlite::mail_store"
visibility = "private"
constructor = "private"

[composition]
roots = ["atm_daemon::bootstrap"]

[ownership]
io_owns = ["sqlite"]
io_forbidden = ["socket_io"]

[dependencies]
allowed_dependents = ["atm-daemon"]
allowed_dependencies = ["atm-core", "rusqlite"]
forbidden_edges = ["atm -> atm-storage-rusqlite", "atm-graft -> atm-storage-rusqlite"]

[references]
scope = "outside_owner_crate"
forbidden = ["SqliteMailStore", "SqliteMailStore::open", "rusqlite::Connection"]

[contracts]
request_types = ["MailStore inputs"]
response_types = ["MailStore outputs"]
error_types = ["AtmError"]

[testing]
allowed_test_double_paths = ["atm_core::test_support::InMemoryMailStore"]
forbidden_test_bypasses = ["rusqlite::Connection"]

[enforcement]
lint_rules = ["LINT-BOUNDARY-MAILSTORE-SQLITE-EDGES"]
review_gates = ["no_public_impl"]

[status]
state = "planned"
notes = []
"""


class LintBoundariesTests(unittest.TestCase):
    def test_graft_shared_client_has_no_direct_interprocess_dependency(self) -> None:
        manifest = tomllib.loads(
            (REPO_ROOT / "crates/atm-graft/Cargo.toml").read_text(encoding="utf-8")
        )
        self.assertNotIn("interprocess", manifest["dependencies"])
        rendered = [
            violation.render()
            for violation in collect_boundary_violations(REPO_ROOT)
        ]
        self.assertNotIn(
            "crates/atm-graft/Cargo.toml [dependencies]: "
            "BOUNDARY-GraftSharedClientConsumer forbids edge atm-graft -> interprocess",
            rendered,
        )

    def write_repo(self, repo_root: Path) -> None:
        (repo_root / "Cargo.toml").write_text(ROOT_MANIFEST, encoding="utf-8")
        (repo_root / ".just").mkdir()
        (repo_root / ".just/lint-config.toml").write_text(LINT_CONFIG, encoding="utf-8")
        for crate_name in ("atm-core", "atm-storage-rusqlite", "atm", "atm-daemon"):
            crate_dir = repo_root / "crates" / crate_name
            crate_dir.mkdir(parents=True)
            (crate_dir / "src").mkdir()
            (crate_dir / "src/lib.rs").write_text("pub fn example() {}\n", encoding="utf-8")
        (repo_root / "crates/atm/src/commands").mkdir(parents=True, exist_ok=True)
        (repo_root / "crates/atm-core/src/team_admin").mkdir(parents=True, exist_ok=True)
        for doc_name in ("atm-core", "atm-storage-rusqlite", "atm", "atm-daemon"):
            (repo_root / "docs" / doc_name).mkdir(parents=True)
        self.write_scb_config_support(repo_root)
        self.write_scb_retained_support(repo_root)
        self.write_scb_workspace_support(repo_root)
        self.write_scb_singleton_support(repo_root)
        self.write_scb_observability_support(repo_root)

    def write_manifests(
        self,
        repo_root: Path,
        *,
        atm_dependencies: str = 'atm-core = { package = "agent-team-mail-core", path = "../atm-core", version = "1.1.2" }\n',
        atm_storage_rusqlite_dependencies: str = 'rusqlite = "0.37"\natm-core = { package = "agent-team-mail-core", path = "../atm-core", version = "1.1.2" }\n',
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
atm-storage-rusqlite = { path = "../atm-storage-rusqlite", version = "1.1.2" }
""",
            encoding="utf-8",
        )
        (repo_root / "crates/atm-storage-rusqlite/Cargo.toml").write_text(
            f"""\
[package]
name = "atm-storage-rusqlite"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true

[lib]
name = "atm_storage_rusqlite"

[dependencies]
{atm_storage_rusqlite_dependencies}""",
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
            self.write_doc(repo_root, "atm-storage-rusqlite")

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
            self.write_doc(repo_root, "atm-storage-rusqlite")
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

    def test_collect_boundary_violations_allows_daemon_rusqlite_edge(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(
                repo_root,
                atm_daemon_dependencies='atm-core = { package = "agent-team-mail-core", path = "../atm-core", version = "1.1.2" }\natm-storage-rusqlite = { path = "../atm-storage-rusqlite", version = "1.1.2" }\n',
            )
            self.write_doc(repo_root, "atm-storage-rusqlite")

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertNotIn(
                "crates/atm-daemon/Cargo.toml [dependencies]: atm-daemon may depend on atm-storage-rusqlite only through the documented runtime-owned SQLite boundaries",
                rendered,
            )

    def write_doc(self, repo_root: Path, crate_name: str, text: str = BASE_BOUNDARY_DOC) -> None:
        (repo_root / "docs" / crate_name / "boundaries.md").write_text(text, encoding="utf-8")

    def write_toml_record(self, repo_root: Path, owner_package: str, file_name: str = "mail-store.toml", text: str = BASE_BOUNDARY_TOML) -> None:
        target = repo_root / "boundaries" / owner_package
        target.mkdir(parents=True, exist_ok=True)
        (target / file_name).write_text(text, encoding="utf-8")

    def write_scb_config_support(self, repo_root: Path) -> None:
        (repo_root / ".just/allowlists").mkdir(parents=True, exist_ok=True)
        (repo_root / ".just/fixtures").mkdir(parents=True, exist_ok=True)
        (repo_root / ".just/allowlists/scb_config_allowlist.toml").write_text(
            """\
[[allow]]
rule = "SCB-CONFIG-001"
path = "crates/atm-core/src/boundary_support.rs"
symbol = "hydrate_roster_from_team_config_once_at_startup_if_empty"
why = "temporary startup-only roster hydration until watcher-owned ingest lands in Z.8"
sunset_sprint = "Z.8"
""",
            encoding="utf-8",
        )
        (repo_root / ".just/fixtures/scb_config_known_bad.rs").write_text(
            """\
use crate::config;

fn load_workspace_config(team_dir: &std::path::Path) {
    let _ = config::load_team_config(team_dir);
}

fn send_bad(team_dir: &std::path::Path) {
    let _ = load_team_config(team_dir);
}
""",
            encoding="utf-8",
        )

    def write_scb_retained_support(self, repo_root: Path) -> None:
        (repo_root / ".just/allowlists").mkdir(parents=True, exist_ok=True)
        (repo_root / ".just/fixtures").mkdir(parents=True, exist_ok=True)
        (repo_root / "crates/atm/src/commands").mkdir(parents=True, exist_ok=True)
        (repo_root / ".just/allowlists/scb_retained_allowlist.toml").write_text(
            "# no retained-runtime allowlist survivors expected on accepted branches\n",
            encoding="utf-8",
        )
        (repo_root / ".just/fixtures/scb_runtime_known_bad.rs").write_text(
            """\
use crate::service_runtime_store;

fn run_bad() {
    let _ = service_runtime_store::default_runtime();
}
""",
            encoding="utf-8",
        )

    def write_scb_workspace_support(self, repo_root: Path) -> None:
        (repo_root / ".just/allowlists").mkdir(parents=True, exist_ok=True)
        (repo_root / ".just/fixtures").mkdir(parents=True, exist_ok=True)
        (repo_root / "crates/atm/src/commands").mkdir(parents=True, exist_ok=True)
        (repo_root / ".just/allowlists/scb_workspace_allowlist.toml").write_text(
            "# no workspace-config allowlist survivors expected on accepted branches\n",
            encoding="utf-8",
        )

    def write_scb_singleton_support(self, repo_root: Path) -> None:
        (repo_root / ".just/allowlists").mkdir(parents=True, exist_ok=True)
        (repo_root / ".just/fixtures").mkdir(parents=True, exist_ok=True)
        (repo_root / "crates/atm-core/src").mkdir(parents=True, exist_ok=True)
        (repo_root / ".just/allowlists/scb_singleton_allowlist.toml").write_text(
            "# no singleton allowlist survivors expected on accepted branches\n",
            encoding="utf-8",
        )
        (repo_root / ".just/fixtures/scb_singleton_known_bad.rs").write_text(
            """\
pub use crate::service_runtime_store::install_default_runtime_factory;
""",
            encoding="utf-8",
        )
        (repo_root / ".just/fixtures/scb_workspace_known_bad.rs").write_text(
            """\
use crate::config::load_config;

fn run_bad(current_dir: &std::path::Path) {
    let _ = load_config(current_dir);
}
""",
            encoding="utf-8",
        )

    def write_scb_observability_support(self, repo_root: Path) -> None:
        (repo_root / ".just/allowlists").mkdir(parents=True, exist_ok=True)
        (repo_root / ".just/fixtures").mkdir(parents=True, exist_ok=True)
        (repo_root / "crates/atm-daemon/src").mkdir(parents=True, exist_ok=True)
        (repo_root / ".just/allowlists/scb_observability_allowlist.toml").write_text(
            """\
[[allow]]
rule = "SCB-OBSERVABILITY-001"
path = "crates/atm-daemon/src/daemon_runtime_observability.rs"
symbol = "__module__"
why = "sanctioned daemon adapter module"
sunset_sprint = "AD.26"
""",
            encoding="utf-8",
        )
        (repo_root / ".just/fixtures/scb_observability_known_bad.rs").write_text(
            """\
type ActionName = sc_observability_types::ActionName;
type OutcomeLabel = sc_observability_types::OutcomeLabel;
""",
            encoding="utf-8",
        )

    def test_parse_simple_yaml_document_reads_nested_lists(self) -> None:
        document = textwrap.dedent(
            """\
            boundary_id: BOUNDARY-Test
            dependencies:
              forbidden_edges:
                - atm -> atm-storage-rusqlite
            status:
              state: planned
            """
        )
        parsed = parse_simple_yaml_document(document)
        self.assertEqual(parsed["boundary_id"], "BOUNDARY-Test")
        self.assertEqual(parsed["dependencies"]["forbidden_edges"], ["atm -> atm-storage-rusqlite"])
        self.assertEqual(parsed["status"]["state"], "planned")

    def test_collect_boundary_violations_accepts_allowlisted_startup_helper(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-storage-rusqlite")
            self.write_scb_config_support(repo_root)
            self.write_scb_retained_support(repo_root)
            self.write_scb_workspace_support(repo_root)
            self.write_scb_singleton_support(repo_root)
            self.write_scb_observability_support(repo_root)
            (repo_root / "crates/atm-core/src/boundary_support.rs").write_text(
                """\
use crate::config;

fn hydrate_roster_from_team_config_once_at_startup_if_empty(team_dir: &std::path::Path) {
    let _ = config::load_team_config(team_dir);
}
""",
                encoding="utf-8",
            )

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertEqual(rendered, [])

    def test_collect_boundary_violations_rejects_scb_config_rule_family(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-storage-rusqlite")
            self.write_scb_config_support(repo_root)
            self.write_scb_retained_support(repo_root)
            self.write_scb_workspace_support(repo_root)
            self.write_scb_singleton_support(repo_root)
            self.write_scb_observability_support(repo_root)
            (repo_root / "crates/atm-core/src/direct_boundaries.rs").write_text(
                "fn load_workspace_config(team_dir: &std::path::Path) { let _ = team_dir; }\n",
                encoding="utf-8",
            )
            send_dir = repo_root / "crates/atm-core/src/send"
            send_dir.mkdir(parents=True, exist_ok=True)
            (send_dir / "mod.rs").write_text(
                """\
use crate::config;

fn send_bad(team_dir: &std::path::Path) {
    let _ = config::load_team_config(team_dir);
}
""",
                encoding="utf-8",
            )

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertTrue(any(item.startswith("SCB-CONFIG-001 ") for item in rendered), rendered)
            self.assertTrue(any(item.startswith("SCB-CONFIG-002 ") for item in rendered), rendered)
            self.assertTrue(any(item.startswith("SCB-CONFIG-003 ") for item in rendered), rendered)

    def test_collect_boundary_violations_rejects_scb_observability_rule_family(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-storage-rusqlite")
            self.write_scb_config_support(repo_root)
            self.write_scb_retained_support(repo_root)
            self.write_scb_workspace_support(repo_root)
            self.write_scb_singleton_support(repo_root)
            self.write_scb_observability_support(repo_root)
            (repo_root / "crates/atm-daemon/src/runtime_sqlite_observer.rs").write_text(
                "type ActionName = sc_observability_types::ActionName;\n",
                encoding="utf-8",
            )

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertTrue(
                any(item.startswith("SCB-OBSERVABILITY-001 ") for item in rendered), rendered
            )

    def test_collect_boundary_violations_rejects_scb_retained_rule_family(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-storage-rusqlite")
            self.write_scb_retained_support(repo_root)
            self.write_scb_workspace_support(repo_root)
            self.write_scb_singleton_support(repo_root)
            (repo_root / "crates/atm-core/src/team_admin/member_mutation.rs").write_text(
                """\
use crate::service_runtime_store;

fn run_bad() {
    let _ = service_runtime_store::default_runtime();
}
""",
                encoding="utf-8",
            )

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertTrue(any(item.startswith("SCB-RETAINED-001 ") for item in rendered), rendered)

    def test_collect_boundary_violations_rejects_scb_workspace_rule_family(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-storage-rusqlite")
            self.write_scb_workspace_support(repo_root)
            (repo_root / "crates/atm-core/src/team_admin/member_mutation.rs").write_text(
                """\
use crate::config::load_config;

fn run_bad(current_dir: &std::path::Path) {
    let _ = load_config(current_dir);
}
""",
                encoding="utf-8",
            )

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertTrue(any(item.startswith("SCB-WORKSPACE-001 ") for item in rendered), rendered)

    def test_collect_boundary_violations_rejects_scb_singleton_rule_family(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-storage-rusqlite")
            self.write_scb_singleton_support(repo_root)
            (repo_root / "crates/atm-core/src/lib.rs").write_text(
                """\
pub use crate::service_runtime_store::install_default_runtime_factory;
""",
                encoding="utf-8",
            )

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertTrue(any(item.startswith("SCB-SINGLETON-001 ") for item in rendered), rendered)

    def test_parse_boundary_records_accepts_planned_schema(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-storage-rusqlite")

            records, violations = parse_boundary_records(repo_root)
            self.assertEqual(violations, [])
            self.assertEqual(len(records), 1)
            self.assertEqual(records[0].boundary_id, "BOUNDARY-MailStore-Sqlite")
            self.assertFalse(records[0].is_active)

    def test_parse_boundary_records_accepts_toml_schema(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_toml_record(repo_root, "atm-storage-rusqlite")

            records, violations = parse_boundary_records(repo_root)
            self.assertEqual(violations, [])
            self.assertEqual(len(records), 1)
            self.assertEqual(records[0].boundary_id, "BOUNDARY-MailStore-Sqlite")
            self.assertEqual(records[0].source_path.as_posix(), "boundaries/atm-storage-rusqlite/mail-store.toml")
            self.assertFalse(records[0].is_active)

    def test_parse_boundary_records_markdown_and_toml_have_parity(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-storage-rusqlite")

            markdown_records, markdown_violations = parse_boundary_records(repo_root)
            self.assertEqual(markdown_violations, [])

            (repo_root / "docs" / "atm-storage-rusqlite" / "boundaries.md").unlink()
            self.write_toml_record(repo_root, "atm-storage-rusqlite")

            toml_records, toml_violations = parse_boundary_records(repo_root)
            self.assertEqual(toml_violations, [])

            self.assertEqual(len(markdown_records), 1)
            self.assertEqual(len(toml_records), 1)
            markdown_record = markdown_records[0]
            toml_record = toml_records[0]
            self.assertEqual(markdown_record.boundary_id, toml_record.boundary_id)
            self.assertEqual(markdown_record.owner_package, toml_record.owner_package)
            self.assertEqual(markdown_record.owner_crate_path, toml_record.owner_crate_path)
            self.assertEqual(markdown_record.name, toml_record.name)
            self.assertEqual(markdown_record.public_trait, toml_record.public_trait)
            self.assertEqual(markdown_record.implementation_type, toml_record.implementation_type)
            self.assertEqual(markdown_record.implementation_module, toml_record.implementation_module)
            self.assertEqual(markdown_record.composition_roots, toml_record.composition_roots)
            self.assertEqual(markdown_record.allowed_dependents, toml_record.allowed_dependents)
            self.assertEqual(markdown_record.allowed_dependencies, toml_record.allowed_dependencies)
            self.assertEqual(markdown_record.forbidden_edges, toml_record.forbidden_edges)
            self.assertEqual(markdown_record.forbidden_references, toml_record.forbidden_references)
            self.assertEqual(markdown_record.allowed_test_double_paths, toml_record.allowed_test_double_paths)
            self.assertEqual(markdown_record.forbidden_test_bypasses, toml_record.forbidden_test_bypasses)
            self.assertEqual(markdown_record.lint_rules, toml_record.lint_rules)
            self.assertEqual(markdown_record.review_gates, toml_record.review_gates)
            self.assertEqual(markdown_record.status_state, toml_record.status_state)

    def test_boundary_doc_section_lines_reports_per_doc_counts(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-storage-rusqlite")
            self.write_doc(
                repo_root,
                "atm-core",
                BASE_BOUNDARY_DOC.replace("owner_package: atm-storage-rusqlite", "owner_package: atm-core").replace(
                    "owner_crate_path: atm_storage_rusqlite", "owner_crate_path: atm_core"
                ),
            )

            records, violations = parse_boundary_records(repo_root)
            self.assertEqual(violations, [])

            lines = boundary_doc_section_lines(repo_root, records)
            joined = "\n".join(lines)
            self.assertIn("boundary docs analyzed:", joined)
            self.assertIn("docs/atm-core/boundaries.md", joined)
            self.assertIn("docs/atm-storage-rusqlite/boundaries.md", joined)
            self.assertIn("boundary doc count: 2", joined)
            self.assertIn("boundary records validated: 2", joined)

    def test_boundary_doc_section_lines_reports_toml_sources(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-core", BASE_BOUNDARY_DOC.replace("owner_package: atm-storage-rusqlite", "owner_package: atm-core").replace(
                "owner_crate_path: atm_storage_rusqlite", "owner_crate_path: atm_core"
            ))
            self.write_toml_record(
                repo_root,
                "atm-storage-rusqlite",
                text=BASE_BOUNDARY_TOML,
            )

            records, violations = parse_boundary_records(repo_root)
            self.assertEqual(violations, [])

            lines = boundary_doc_section_lines(repo_root, records)
            joined = "\n".join(lines)
            self.assertIn("docs/atm-core/boundaries.md", joined)
            self.assertIn("boundaries/atm-storage-rusqlite/mail-store.toml", joined)
            self.assertIn("boundary doc count: 2", joined)
            self.assertIn("boundary records validated: 2", joined)

    def test_parse_boundary_records_flags_missing_required_field(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            broken_doc = BASE_BOUNDARY_DOC.replace("owner_crate_path: atm_storage_rusqlite\n", "")
            self.write_doc(repo_root, "atm-storage-rusqlite", broken_doc)

            _records, violations = parse_boundary_records(repo_root)
            rendered = [violation.render() for violation in violations]
            self.assertTrue(
                any("missing required field: owner_crate_path" in item for item in rendered),
                rendered,
            )

    def test_parse_boundary_records_requires_declared_io_forbidden_policy(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            broken_doc = BASE_BOUNDARY_DOC.replace(
                "ownership:\n  io_owns:\n    - sqlite\n  io_forbidden:\n    - socket_io\n",
                "ownership:\n  io_owns:\n    - sqlite\n",
            )
            self.write_doc(repo_root, "atm-storage-rusqlite", broken_doc)

            _records, violations = parse_boundary_records(repo_root)
            rendered = [violation.render() for violation in violations]
            self.assertTrue(
                any("missing required field: ownership.io_forbidden" in item for item in rendered),
                rendered,
            )

    def test_parse_boundary_records_rejects_non_list_io_forbidden_policy(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            broken_doc = BASE_BOUNDARY_DOC.replace(
                "  io_forbidden:\n    - socket_io\n",
                "  io_forbidden: socket_io\n",
            )
            self.write_doc(repo_root, "atm-storage-rusqlite", broken_doc)

            _records, violations = parse_boundary_records(repo_root)
            rendered = [violation.render() for violation in violations]
            self.assertTrue(
                any("ownership.io_forbidden must be a list of strings" in item for item in rendered),
                rendered,
            )

    def test_parse_boundary_records_flags_invalid_edge_syntax(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            broken_doc = BASE_BOUNDARY_DOC.replace("- atm -> atm-storage-rusqlite", "- atm => atm-storage-rusqlite")
            self.write_doc(repo_root, "atm-storage-rusqlite", broken_doc)

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

    def test_parse_boundary_records_flags_toml_owner_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_toml_record(repo_root, "atm", text=BASE_BOUNDARY_TOML)

            _records, violations = parse_boundary_records(repo_root)
            rendered = [violation.render() for violation in violations]
            self.assertTrue(any("document path owner mismatch: boundaries/atm/mail-store.toml" in item for item in rendered), rendered)

    def test_collect_boundary_violations_accepts_allowed_layout(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-storage-rusqlite")
            (repo_root / "crates/atm-storage-rusqlite/src/lib.rs").write_text("use rusqlite::Connection;\n", encoding="utf-8")

            self.assertEqual(collect_boundary_violations(repo_root), [])

    def test_collect_boundary_violations_flags_duplicate_boundary_ids(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-storage-rusqlite")
            self.write_doc(repo_root, "atm-daemon", BASE_BOUNDARY_DOC.replace("owner_package: atm-storage-rusqlite", "owner_package: atm-daemon").replace("owner_crate_path: atm_storage_rusqlite", "owner_crate_path: atm_daemon"))

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertTrue(any("duplicate boundary_id" in item for item in rendered), rendered)

    def test_collect_boundary_violations_flags_duplicate_boundary_ids_across_markdown_and_toml(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-storage-rusqlite")
            self.write_toml_record(repo_root, "atm-storage-rusqlite")

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertTrue(any("duplicate boundary_id" in item for item in rendered), rendered)

    def test_collect_boundary_violations_flags_owner_crate_path_mismatch_for_existing_crate(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            broken_doc = BASE_BOUNDARY_DOC.replace("owner_crate_path: atm_storage_rusqlite", "owner_crate_path: bad_path")
            self.write_doc(repo_root, "atm-storage-rusqlite", broken_doc)

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertTrue(any("does not match workspace crate path" in item for item in rendered), rendered)

    def test_collect_boundary_violations_flags_unexpected_allowed_dependent(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root, atm_dependencies='atm-storage-rusqlite = { path = "../atm-storage-rusqlite", version = "1.1.2" }\n')
            self.write_doc(repo_root, "atm-storage-rusqlite")

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertTrue(any("found unexpected dependent" in item for item in rendered), rendered)

    def test_collect_boundary_violations_flags_stale_allowed_dependent_without_live_edge(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_toml_record(
                repo_root,
                "atm-storage-rusqlite",
                text=BASE_BOUNDARY_TOML.replace('state = "planned"', 'state = "active"'),
            )

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertTrue(any("stale allowed dependent" in item for item in rendered), rendered)

    def test_collect_boundary_violations_flags_forbidden_edge_even_when_planned(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root, atm_dependencies='atm-storage-rusqlite = { path = "../atm-storage-rusqlite", version = "1.1.2" }\n')
            self.write_doc(repo_root, "atm-storage-rusqlite")

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertIn(
                "crates/atm/Cargo.toml [dependencies]: BOUNDARY-MailStore-Sqlite forbids edge atm -> atm-storage-rusqlite",
                rendered,
            )

    def test_collect_boundary_violations_flags_direct_rusqlite_dependency(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-storage-rusqlite")
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
                "crates/atm-core/Cargo.toml [dependencies]: only crates/atm-storage-rusqlite may depend on rusqlite",
                rendered,
            )

    def test_collect_boundary_violations_flags_rusqlite_source_import_outside_store(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-storage-rusqlite")
            (repo_root / "crates/atm/src/lib.rs").write_text("use rusqlite::Connection;\n", encoding="utf-8")

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertIn(
                "crates/atm/src/lib.rs:1: only crates/atm-storage-rusqlite source may import rusqlite directly",
                rendered,
            )

    def test_collect_boundary_violations_flags_non_dev_atm_core_edge(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-storage-rusqlite")
            (repo_root / "crates/atm-core/Cargo.toml").write_text(
                """\
[package]
name = "agent-team-mail-core"

[dependencies]
atm-storage-rusqlite = { path = "../atm-storage-rusqlite", version = "1.1.2" }
""",
                encoding="utf-8",
            )

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertIn(
                "crates/atm-core/Cargo.toml [dependencies]: atm-core may reference atm-storage-rusqlite only in dev-dependencies",
                rendered,
            )

    def test_collect_boundary_violations_flags_forbidden_reference_outside_owner(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-storage-rusqlite")
            (repo_root / "crates/atm-core/src/lib.rs").write_text("pub fn demo() { let _ = SqliteMailStore::open(); }\n", encoding="utf-8")

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertTrue(any("forbids external reference 'SqliteMailStore::open'" in item for item in rendered), rendered)

    def test_collect_boundary_violations_flags_forbidden_reference_inside_owner(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_toml_record(
                repo_root,
                "atm-storage-rusqlite",
                text=BASE_BOUNDARY_TOML.replace(
                    "scope = \"outside_owner_crate\"", "scope = \"inside_owner_crate\""
                ),
            )
            (repo_root / "crates/atm-storage-rusqlite/src/lib.rs").write_text(
                "pub fn demo() { let _ = SqliteMailStore::open(); }\n", encoding="utf-8"
            )

            rendered = [violation.render() for violation in collect_boundary_violations(repo_root)]
            self.assertTrue(
                any("forbids owner-crate reference 'SqliteMailStore::open'" in item for item in rendered),
                rendered,
            )

    def test_collect_boundary_violations_allows_composition_root_reference(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_doc(repo_root, "atm-storage-rusqlite")
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
            self.write_doc(repo_root, "atm-storage-rusqlite", active_doc)
            (repo_root / "crates/atm-storage-rusqlite/src/lib.rs").write_text(
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

    def test_every_declared_io_forbidden_tag_has_source_pattern_mapping(self) -> None:
        declared: set[str] = set()
        for path in Path(__file__).resolve().parents[2].glob("boundaries/*/*.toml"):
            data = tomllib.loads(path.read_text(encoding="utf-8"))
            declared.update(data.get("ownership", {}).get("io_forbidden", []))

        self.assertTrue(declared)
        self.assertEqual(declared - set(IO_FORBIDDEN_SOURCE_PATTERNS), set())
        self.assertTrue(all(patterns for patterns in IO_FORBIDDEN_SOURCE_PATTERNS.values()))

    def test_io_forbidden_mapping_catches_temporary_source_violation(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_manifests(repo_root)
            self.write_toml_record(
                repo_root,
                "atm-storage-rusqlite",
                text=BASE_BOUNDARY_TOML.replace('state = "planned"', 'state = "active"'),
            )
            source_path = repo_root / "crates/atm-storage-rusqlite/src/mail_store.rs"
            source_path.write_text(
                "pub fn temporary_violation() {\n"
                "    let _ = std::net::TcpStream::connect(\"127.0.0.1:1\");\n"
                "}\n",
                encoding="utf-8",
            )

            records, parse_violations = parse_boundary_records(repo_root)
            self.assertEqual(parse_violations, [])
            rendered = [
                violation.render()
                for violation in collect_io_forbidden_source_violations(repo_root, records)
            ]
            self.assertTrue(
                any(
                    "BOUNDARY-MailStore-Sqlite forbids io 'socket_io'" in item
                    for item in rendered
                ),
                rendered,
            )


if __name__ == "__main__":
    unittest.main()
