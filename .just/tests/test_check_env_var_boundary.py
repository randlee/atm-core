from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from check_env_var_boundary import collect_env_var_boundary_violations
from check_env_var_boundary import load_allowlist
from check_env_var_boundary import load_boundary_reader_functions
from check_env_var_boundary import load_restricted_crate_roots


LINT_CONFIG = """\
[env_var_boundary]
restricted_crate_roots = ["crates/atm-core/src", "crates/atm-daemon/src"]
boundary_reader_functions = [
  "read_cli_identity_from_env",
  "read_cli_team_from_env",
  "read_cli_chat_id_from_env",
  "read_cli_session_id_from_env",
  "read_cli_pid_from_env",
]
"""

ROOT_MANIFEST = """\
[workspace]
members = ["crates/atm-core", "crates/atm-daemon", "crates/atm"]
resolver = "2"
"""


class CheckEnvVarBoundaryTests(unittest.TestCase):
    def write_repo(self, repo_root: Path) -> None:
        (repo_root / ".just").mkdir()
        (repo_root / ".just/lint-config.toml").write_text(LINT_CONFIG, encoding="utf-8")
        (repo_root / "Cargo.toml").write_text(ROOT_MANIFEST, encoding="utf-8")
        (repo_root / "sc-lint.toml").write_text('''[[tool.sc-lint.analyzers.boundary.env_var_boundary]]
forbidden_env_vars = ["ATM_TEAM", "ATM_IDENTITY", "ATM_CHAT_ID", "ATM_SESSION_ID", "ATM_PID", "ATM_TEMP"]
''', encoding="utf-8")
        for crate, lib_name in (
            ("atm-core", "atm_core"),
            ("atm-daemon", "atm_daemon"),
            ("atm", "atm"),
        ):
            crate_dir = repo_root / "crates" / crate
            (crate_dir / "src").mkdir(parents=True)
            (crate_dir / "Cargo.toml").write_text(
                f"""\
[package]
name = "{crate}"
version = "0.1.0"

[lib]
name = "{lib_name}"
""",
                encoding="utf-8",
            )

    def collect(self, repo_root: Path):
        return collect_env_var_boundary_violations(
            repo_root,
            restricted_crate_roots=load_restricted_crate_roots(repo_root),
            boundary_reader_functions=load_boundary_reader_functions(repo_root),
            allowlist=load_allowlist(repo_root),
        )

    def test_load_config_reads_reader_policy(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)

            self.assertEqual(
                load_restricted_crate_roots(repo_root),
                (Path("crates/atm-core/src"), Path("crates/atm-daemon/src")),
            )
            self.assertEqual(
                load_boundary_reader_functions(repo_root),
                (
                    "read_cli_identity_from_env",
                    "read_cli_team_from_env",
                    "read_cli_chat_id_from_env",
                    "read_cli_session_id_from_env",
                    "read_cli_pid_from_env",
                ),
            )






    def test_static_initializer_and_function_read_are_detected_with_approved_sites_allowed(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            source_root = JUST_DIR.parent
            (repo_root / "crates/atm-core/src/caller_context.rs").write_text(
                (source_root / "crates/atm-core/src/caller_context.rs").read_text(encoding="utf-8"),
                encoding="utf-8",
            )
            allow_dir = repo_root / ".just/allowlists"
            allow_dir.mkdir()
            (allow_dir / "env_var_boundary_allowlist.toml").write_text(
                (source_root / ".just/allowlists/env_var_boundary_allowlist.toml").read_text(encoding="utf-8"),
                encoding="utf-8",
            )
            (repo_root / "crates/atm-core/src/config.rs").write_text(
                'static TEAM: std::sync::LazyLock<Option<String>> = std::sync::LazyLock::new(|| std::env::var("ATM_TEAM").ok());\n'
                'fn positive_control() { let _ = std::env::var("ATM_IDENTITY"); }\n', encoding="utf-8"
            )
            violations = self.collect(repo_root)
            self.assertEqual([(v.path, v.line_number, v.kind) for v in violations], [
                ("crates/atm-core/src/config.rs", 1, "direct_literal_env_read"),
                ("crates/atm-core/src/config.rs", 2, "direct_literal_env_read"),
            ])

    def test_exempt_reader_rejects_extra_read_and_allows_exact_approved_site(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/caller_context.rs").write_text('''
pub fn read_cli_pid_from_env() -> Option<String> {
    let _ = std::env::var("ATM_TEAM");
    std::env::var_os("ATM_PID")
}
fn positive_control() {
    let _ = std::env::var("ATM_TEAM");
}
''', encoding="utf-8")
            allow_dir = repo_root / ".just/allowlists"
            allow_dir.mkdir()
            (allow_dir / "env_var_boundary_allowlist.toml").write_text('''
[[allow]]
rule = "ATM-ENV-BOUNDARY-001"
path = "crates/atm-core/src/caller_context.rs"
symbol = "read_cli_pid_from_env"
line = 'std::env::var_os("ATM_PID")'
why = "approved PID read"
sunset_sprint = "sc-lint#983"
''', encoding="utf-8")
            violations = self.collect(repo_root)
            self.assertEqual([(v.symbol, v.kind) for v in violations], [
                ("read_cli_pid_from_env", "direct_literal_env_read"),
                ("positive_control", "direct_literal_env_read"),
            ])

    def test_extra_forwarded_read_inside_reader_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/caller_context.rs").write_text('''
fn read_env_raw(key: &str) -> Option<String> {
    std::env::var(key).ok()
}
pub fn read_cli_pid_from_env() {
    read_env_raw("ATM_TEAM");
}
''', encoding="utf-8")
            violations = self.collect(repo_root)
            self.assertEqual([(v.symbol, v.kind) for v in violations], [
                ("read_cli_pid_from_env", "env_var_via_forwarding_function"),
            ])

    def test_flags_calls_to_configured_boundary_reader_functions_from_other_files(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/caller_context.rs").write_text(
                """\
use std::env;

pub fn read_cli_team_from_env() -> Option<String> {
    env::var("ATM_TEAM").ok()
}
""",
                encoding="utf-8",
            )
            (repo_root / "crates/atm-core/src/config.rs").write_text(
                """\
use crate::caller_context::read_cli_team_from_env;

pub fn resolve_team() -> Option<String> {
    read_cli_team_from_env()
}
""",
                encoding="utf-8",
            )

            violations = self.collect(repo_root)

            kinds = {(violation.path, violation.symbol, violation.kind) for violation in violations}
            self.assertIn(
                (
                    "crates/atm-core/src/config.rs",
                    "resolve_team",
                    "boundary_reader_function_call",
                ),
                kinds,
            )

    def test_flags_calls_to_same_file_wrapper_boundary_reader_functions(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/caller_context.rs").write_text(
                """\
use std::env;

pub fn read_cli_team_from_env() -> Option<String> {
    env::var("ATM_TEAM").ok()
}

pub fn read_cli_team_from_env_or_warn() -> Option<String> {
    read_cli_team_from_env()
}
""",
                encoding="utf-8",
            )
            (repo_root / "crates/atm-core/src/health.rs").write_text(
                """\
use crate::caller_context::read_cli_team_from_env_or_warn;

pub fn environment_visibility() -> Option<String> {
    read_cli_team_from_env_or_warn()
}
""",
                encoding="utf-8",
            )

            violations = self.collect(repo_root)

            kinds = {(violation.path, violation.symbol, violation.kind) for violation in violations}
            self.assertIn(
                (
                    "crates/atm-core/src/health.rs",
                    "environment_visibility",
                    "boundary_reader_function_call",
                ),
                kinds,
            )

    def test_does_not_flag_internal_calls_within_the_defining_file(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/caller_context.rs").write_text(
                """\
use std::env;

pub fn read_cli_team_from_env() -> Option<String> {
    env::var("ATM_TEAM").ok()
}

fn resolve_team_component() -> Option<String> {
    read_cli_team_from_env()
}
""",
                encoding="utf-8",
            )

            violations = self.collect(repo_root)

            symbols = {violation.symbol for violation in violations}
            self.assertNotIn("resolve_team_component", symbols)






if __name__ == "__main__":
    unittest.main()
