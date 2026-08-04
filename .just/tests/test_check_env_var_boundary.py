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
from check_env_var_boundary import load_forbidden_env_vars
from check_env_var_boundary import load_restricted_crate_roots


LINT_CONFIG = """\
[env_var_boundary]
forbidden_env_vars = ["ATM_TEAM", "ATM_IDENTITY", "ATM_CHAT_ID", "ATM_SESSION_ID", "ATM_PID"]
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
            forbidden_env_vars=load_forbidden_env_vars(repo_root),
            restricted_crate_roots=load_restricted_crate_roots(repo_root),
            boundary_reader_functions=load_boundary_reader_functions(repo_root),
            allowlist=load_allowlist(repo_root),
        )

    def test_load_config_reads_forbidden_env_vars(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)

            self.assertEqual(
                load_forbidden_env_vars(repo_root),
                ("ATM_TEAM", "ATM_IDENTITY", "ATM_CHAT_ID", "ATM_SESSION_ID", "ATM_PID"),
            )
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

    def test_flags_direct_env_var_literal_in_atm_core(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/example.rs").write_text(
                """\
use std::env;

pub fn resolve_team_from_env() -> Option<String> {
    env::var("ATM_TEAM").ok()
}
""",
                encoding="utf-8",
            )

            violations = self.collect(repo_root)

            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].path, "crates/atm-core/src/example.rs")
            self.assertEqual(violations[0].symbol, "resolve_team_from_env")
            self.assertEqual(violations[0].kind, "direct_literal_env_read")

    def test_flags_direct_env_var_os_literal_in_atm_daemon(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-daemon/src/example.rs").write_text(
                """\
pub fn resolve_identity_from_env() -> Option<std::ffi::OsString> {
    std::env::var_os("ATM_IDENTITY")
}
""",
                encoding="utf-8",
            )

            violations = self.collect(repo_root)

            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].path, "crates/atm-daemon/src/example.rs")
            self.assertEqual(violations[0].symbol, "resolve_identity_from_env")
            self.assertEqual(violations[0].kind, "direct_literal_env_read")

    def test_flags_direct_activity_metadata_env_reads_outside_approved_readers(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/example.rs").write_text(
                """\
use std::env;

pub fn bypass_activity_reader() -> Option<std::ffi::OsString> {
    env::var_os("ATM_SESSION_ID").or_else(|| env::var_os("ATM_PID"))
}
""",
                encoding="utf-8",
            )

            violations = self.collect(repo_root)

            self.assertEqual(len(violations), 2)
            self.assertEqual(
                {violation.kind for violation in violations}, {"direct_literal_env_read"}
            )

    def test_allows_the_two_approved_activity_metadata_readers(self) -> None:
        """The session and PID readers are independently exercised positives.

        This mirrors the real caller-context choke points, including their
        exact reviewed allowlist lines, so adding either reader to the config
        without keeping its reviewed implementation would not satisfy the
        boundary suite.
        """
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            caller_context = repo_root / "crates/atm-core/src/caller_context.rs"
            caller_context.write_text(
                """\
pub fn read_cli_session_id_from_env() -> Option<std::ffi::OsString> {
    let value = std::env::var_os("ATM_SESSION_ID")?;
    Some(value)
}

pub fn read_cli_pid_from_env() -> Option<std::ffi::OsString> {
    std::env::var_os("ATM_PID")
}
""",
                encoding="utf-8",
            )
            allowlist_dir = repo_root / ".just/allowlists"
            allowlist_dir.mkdir(parents=True)
            (allowlist_dir / "env_var_boundary_allowlist.toml").write_text(
                """\
[[allow]]
rule = "ATM-ENV-BOUNDARY-001"
path = "crates/atm-core/src/caller_context.rs"
symbol = "read_cli_session_id_from_env"
line = 'let value = std::env::var_os("ATM_SESSION_ID")?;'
why = "approved session telemetry reader"
sunset_sprint = "n/a"

[[allow]]
rule = "ATM-ENV-BOUNDARY-001"
path = "crates/atm-core/src/caller_context.rs"
symbol = "read_cli_pid_from_env"
line = 'std::env::var_os("ATM_PID")'
why = "approved PID telemetry reader"
sunset_sprint = "n/a"
""",
                encoding="utf-8",
            )

            self.assertEqual(self.collect(repo_root), [])

    def test_flags_literal_forwarded_through_same_file_helper(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/example.rs").write_text(
                """\
use std::env;

pub fn read_cli_team_from_env_example() -> Option<String> {
    read_env_raw("ATM_TEAM")
}

fn read_env_raw(key: &str) -> Option<String> {
    env::var(key).ok()
}
""",
                encoding="utf-8",
            )

            violations = self.collect(repo_root)

            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].kind, "env_var_via_forwarding_function")
            self.assertEqual(violations[0].symbol, "read_cli_team_from_env_example")

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
                    "crates/atm-core/src/caller_context.rs",
                    "read_cli_team_from_env",
                    "direct_literal_env_read",
                ),
                kinds,
            )
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
                    "crates/atm-core/src/caller_context.rs",
                    "read_cli_team_from_env",
                    "direct_literal_env_read",
                ),
                kinds,
            )
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

    def test_allows_legitimate_cli_reads_outside_restricted_crate_roots(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm/src/main.rs").write_text(
                """\
use std::env;

pub fn resolve_team_from_env() -> Option<String> {
    env::var("ATM_TEAM").ok()
}
""",
                encoding="utf-8",
            )

            violations = self.collect(repo_root)

            self.assertEqual(violations, [])

    def test_does_not_flag_test_scope_env_manipulation(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/example.rs").write_text(
                """\
pub fn production() {}

#[cfg(test)]
mod tests {
    use std::env;

    #[test]
    fn example() {
        let _ = env::var_os("ATM_TEAM");
    }
}
""",
                encoding="utf-8",
            )

            violations = self.collect(repo_root)

            self.assertEqual(violations, [])

    def test_allowlisted_call_site_is_suppressed(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/example.rs").write_text(
                """\
use std::env;

pub fn resolve_team_from_env() -> Option<String> {
    env::var("ATM_TEAM").ok()
}
""",
                encoding="utf-8",
            )
            (repo_root / ".just/allowlists").mkdir(parents=True)
            (repo_root / ".just/allowlists/env_var_boundary_allowlist.toml").write_text(
                """\
[[allow]]
rule = "ATM-ENV-BOUNDARY-001"
path = "crates/atm-core/src/example.rs"
symbol = "resolve_team_from_env"
line = 'env::var("ATM_TEAM").ok()'
why = "test fixture"
sunset_sprint = "n/a"
""",
                encoding="utf-8",
            )

            violations = self.collect(repo_root)

            self.assertEqual(violations, [])

    def test_allowlist_does_not_suppress_a_new_violation_in_the_same_function(self) -> None:
        """A second, distinct env-read line added to an already-allowlisted
        function must still be reported -- the allowlist entry only covers
        the exact call site it was written for, not the whole enclosing
        function body."""
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-core/src/example.rs").write_text(
                """\
use std::env;

pub fn resolve_team_from_env() -> Option<String> {
    let _ = env::var("ATM_TEAM").ok();
    env::var("ATM_IDENTITY").ok()
}
""",
                encoding="utf-8",
            )
            (repo_root / ".just/allowlists").mkdir(parents=True)
            (repo_root / ".just/allowlists/env_var_boundary_allowlist.toml").write_text(
                """\
[[allow]]
rule = "ATM-ENV-BOUNDARY-001"
path = "crates/atm-core/src/example.rs"
symbol = "resolve_team_from_env"
line = 'let _ = env::var("ATM_TEAM").ok();'
why = "test fixture: only the ATM_TEAM read is reviewed/allowlisted"
sunset_sprint = "n/a"
""",
                encoding="utf-8",
            )

            violations = self.collect(repo_root)

            # The allowlisted ATM_TEAM line is suppressed, but the new
            # ATM_IDENTITY read in the same function must still be flagged.
            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].path, "crates/atm-core/src/example.rs")
            self.assertEqual(violations[0].symbol, "resolve_team_from_env")
            self.assertEqual(violations[0].line, 'env::var("ATM_IDENTITY").ok()')


if __name__ == "__main__":
    unittest.main()
