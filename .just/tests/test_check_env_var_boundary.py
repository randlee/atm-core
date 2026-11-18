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
