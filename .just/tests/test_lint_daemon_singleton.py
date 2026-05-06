from __future__ import annotations

from pathlib import Path
import shutil
import sys
import tempfile
import unittest


ROOT_DIR = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = ROOT_DIR / "scripts"
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

from lint_daemon_singleton import apply_allow_entries
from lint_daemon_singleton import collect_violations
from lint_daemon_singleton import has_unix_gating
from lint_daemon_singleton import load_allow_entries


class LintDaemonSingletonTests(unittest.TestCase):
    def write_workspace(self, repo_root: Path) -> None:
        (repo_root / "Cargo.toml").write_text(
            '[workspace]\nmembers=["crates/atm"]\nresolver="2"\n',
            encoding="utf-8",
        )
        crate_dir = repo_root / "crates" / "atm"
        crate_dir.mkdir(parents=True)
        (crate_dir / "Cargo.toml").write_text(
            '[package]\nname="agent-team-mail"\nversion="1.1.2"\n',
            encoding="utf-8",
        )

    def write_test(self, repo_root: Path, relative_path: str, source: str) -> None:
        path = repo_root / relative_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(source, encoding="utf-8")

    def collect_categories(self, repo_root: Path) -> set[str]:
        return {violation.category for violation in collect_violations(repo_root)}

    def write_single_category_fixture(self, category_source: str, *, relative_path: str = "crates/atm/tests/send.rs") -> Path:
        repo_root = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: shutil.rmtree(repo_root, ignore_errors=True))
        self.write_workspace(repo_root)
        self.write_test(repo_root, relative_path, category_source)
        return repo_root

    def assert_category_present(self, source: str, category: str, *, relative_path: str = "crates/atm/tests/send.rs") -> None:
        repo_root = self.write_single_category_fixture(source, relative_path=relative_path)
        self.assertIn(category, self.collect_categories(repo_root))

    def assert_category_absent(self, source: str, category: str, *, relative_path: str = "crates/atm/tests/send.rs") -> None:
        repo_root = self.write_single_category_fixture(source, relative_path=relative_path)
        self.assertNotIn(category, self.collect_categories(repo_root))

    def test_spawn_test_daemon_positive(self) -> None:
        self.assert_category_present("fn spawn_test_daemon() {}\n", "spawn_test_daemon")

    def test_spawn_test_daemon_negative(self) -> None:
        self.assert_category_absent("fn spawn_loopback_transport() {}\n", "spawn_test_daemon")

    def test_warm_daemon_positive(self) -> None:
        self.assert_category_present("fn warm_daemon() {}\n", "warm_daemon")

    def test_warm_daemon_negative(self) -> None:
        self.assert_category_absent("fn warm_transport() {}\n", "warm_daemon")

    def test_daemon_guard_positive(self) -> None:
        self.assert_category_present("struct DaemonGuard;\n", "daemon_guard")

    def test_daemon_guard_negative(self) -> None:
        self.assert_category_absent("struct TransportGuard;\n", "daemon_guard")

    def test_atm_daemon_bin_positive(self) -> None:
        self.assert_category_present('const BIN: &str = "ATM_DAEMON_BIN";\n', "atm_daemon_bin")

    def test_atm_daemon_bin_negative(self) -> None:
        self.assert_category_absent('const BIN: &str = "ATM_HOME";\n', "atm_daemon_bin")

    def test_daemon_socket_path_positive(self) -> None:
        self.assert_category_present('let _ = "atm-daemon.sock";\n', "daemon_socket_path")

    def test_daemon_socket_path_negative(self) -> None:
        self.assert_category_absent('let _ = "atm-loopback.sock";\n', "daemon_socket_path")

    def test_direct_atm_daemon_command_positive(self) -> None:
        self.assert_category_present(
            """
use std::process::Command;

fn launch() {
    let _ = Command::new("atm-daemon");
}
""",
            "direct_atm_daemon_command",
        )

    def test_direct_atm_daemon_command_negative(self) -> None:
        self.assert_category_absent(
            """
use std::process::Command;

fn launch() {
    let _ = Command::new("atm");
}
""",
            "direct_atm_daemon_command",
        )

    def test_timing_warmup_shortcut_positive(self) -> None:
        self.assert_category_present(
            """
use std::thread;
use std::time::Duration;

fn wait_for_daemon() {
    warm_daemon();
    thread::sleep(Duration::from_millis(25));
}
""",
            "timing_warmup_shortcut",
        )

    def test_timing_warmup_shortcut_negative(self) -> None:
        self.assert_category_absent(
            """
use std::thread;
use std::time::Duration;

fn wait_for_work() {
    thread::sleep(Duration::from_millis(25));
}
""",
            "timing_warmup_shortcut",
        )

    def test_collect_violations_ignores_non_test_code(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_workspace(repo_root)
            self.write_test(
                repo_root,
                "crates/atm-core/src/error.rs",
                'const HELP: &str = "Ensure ATM_DAEMON_BIN is set";\n',
            )

            violations = collect_violations(repo_root)
            self.assertEqual(violations, [])

    def test_collect_violations_ignores_production_lines_in_cfg_test_source_file(self) -> None:
        source = """
pub const RECOVERY: &str = "Set ATM_DAEMON_BIN before retrying.";

#[cfg(test)]
mod tests {
    fn warm_daemon() {}
}
"""
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_workspace(repo_root)
            self.write_test(repo_root, "crates/atm-core/src/error.rs", source)

            violations = collect_violations(repo_root)
            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].category, "warm_daemon")
            self.assertGreater(violations[0].line_number, source.splitlines().index("pub const RECOVERY: &str = \"Set ATM_DAEMON_BIN before retrying.\";") + 1)

    def test_collect_violations_detects_cfg_test_scoped_direct_spawn(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_workspace(repo_root)
            self.write_test(
                repo_root,
                "crates/atm/src/runtime.rs",
                """
#[cfg(test)]
mod tests {
    use std::process::Command;

    fn launch() {
        let _ = Command::new("atm-daemon");
    }
}
""",
            )

            categories = self.collect_categories(repo_root)
            self.assertIn("direct_atm_daemon_command", categories)

    def test_allow_entries_suppress_only_matching_categories(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_workspace(repo_root)
            self.write_test(
                repo_root,
                "crates/atm/tests/send.rs",
                """
fn spawn_test_daemon() {}
fn warm_daemon() {}
""",
            )
            config_path = repo_root / "scripts" / "lint_daemon_singleton.toml"
            config_path.parent.mkdir(parents=True, exist_ok=True)
            config_path.write_text(
                """
[daemon_singleton]
[[daemon_singleton.allow]]
path = "crates/atm/tests/send.rs"
categories = ["spawn_test_daemon"]
reason = "temporary tier 3 exception"
""",
                encoding="utf-8",
            )

            allow_entries = load_allow_entries(repo_root, Path("scripts/lint_daemon_singleton.toml"))
            violations = collect_violations(repo_root)
            remaining, allowed = apply_allow_entries(repo_root, violations, allow_entries)

            self.assertEqual(len(allowed), 1)
            self.assertTrue(any(violation.category == "warm_daemon" for violation in remaining))
            self.assertFalse(any(violation.category == "spawn_test_daemon" for violation in remaining))

    def test_allow_entry_can_require_unix_gating(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_workspace(repo_root)
            self.write_test(
                repo_root,
                "crates/atm/tests/send.rs",
                """
fn spawn_test_daemon() {}
""",
            )
            config_path = repo_root / "scripts" / "lint_daemon_singleton.toml"
            config_path.parent.mkdir(parents=True, exist_ok=True)
            config_path.write_text(
                """
[daemon_singleton]
[[daemon_singleton.allow]]
path = "crates/atm/tests/send.rs"
categories = ["spawn_test_daemon"]
reason = "temporary tier 3 exception"
require_unix_gating = true
""",
                encoding="utf-8",
            )

            allow_entries = load_allow_entries(repo_root, Path("scripts/lint_daemon_singleton.toml"))
            violations = collect_violations(repo_root)
            remaining, _allowed = apply_allow_entries(repo_root, violations, allow_entries)
            self.assertEqual(len(remaining), 1)
            self.assertIn("requires explicit #[cfg(unix)] gating", remaining[0].detail)

    def test_allow_entry_with_local_unix_gating_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_workspace(repo_root)
            self.write_test(
                repo_root,
                "crates/atm/tests/send.rs",
                """
#[cfg(unix)]
fn spawn_test_daemon() {}
""",
            )
            config_path = repo_root / "scripts" / "lint_daemon_singleton.toml"
            config_path.parent.mkdir(parents=True, exist_ok=True)
            config_path.write_text(
                """
[daemon_singleton]
[[daemon_singleton.allow]]
path = "crates/atm/tests/send.rs"
categories = ["spawn_test_daemon"]
reason = "temporary tier 3 exception"
require_unix_gating = true
""",
                encoding="utf-8",
            )

            allow_entries = load_allow_entries(repo_root, Path("scripts/lint_daemon_singleton.toml"))
            violations = collect_violations(repo_root)
            remaining, allowed = apply_allow_entries(repo_root, violations, allow_entries)
            self.assertEqual(remaining, [])
            self.assertEqual(len(allowed), 1)

    def test_allow_entry_with_unrelated_unix_gating_still_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_workspace(repo_root)
            self.write_test(
                repo_root,
                "crates/atm/tests/send.rs",
                """
#[cfg(unix)]
fn ok_transport_test() {}

fn spawn_test_daemon() {}
""",
            )
            config_path = repo_root / "scripts" / "lint_daemon_singleton.toml"
            config_path.parent.mkdir(parents=True, exist_ok=True)
            config_path.write_text(
                """
[daemon_singleton]
[[daemon_singleton.allow]]
path = "crates/atm/tests/send.rs"
categories = ["spawn_test_daemon"]
reason = "temporary tier 3 exception"
require_unix_gating = true
""",
                encoding="utf-8",
            )

            allow_entries = load_allow_entries(repo_root, Path("scripts/lint_daemon_singleton.toml"))
            violations = collect_violations(repo_root)
            remaining, _allowed = apply_allow_entries(repo_root, violations, allow_entries)
            self.assertEqual(len(remaining), 1)
            self.assertIn("requires explicit #[cfg(unix)] gating", remaining[0].detail)

    def test_duplicate_allow_entry_for_same_path_and_category_errors(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_workspace(repo_root)
            config_path = repo_root / "scripts" / "lint_daemon_singleton.toml"
            config_path.parent.mkdir(parents=True, exist_ok=True)
            config_path.write_text(
                """
[daemon_singleton]
[[daemon_singleton.allow]]
path = "crates/atm/tests/send.rs"
categories = ["spawn_test_daemon"]
reason = "one"

[[daemon_singleton.allow]]
path = "crates/atm/tests/send.rs"
categories = ["spawn_test_daemon"]
reason = "two"
""",
                encoding="utf-8",
            )

            with self.assertRaises(ValueError):
                load_allow_entries(repo_root, Path("scripts/lint_daemon_singleton.toml"))

    def test_has_unix_gating_detects_common_forms(self) -> None:
        self.assertTrue(has_unix_gating("#[cfg(unix)]\nfn x() {}"))
        self.assertTrue(has_unix_gating("if cfg!(unix) { }"))
        self.assertFalse(has_unix_gating("fn x() {}"))


if __name__ == "__main__":
    unittest.main()
