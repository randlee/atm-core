from __future__ import annotations

from pathlib import Path
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
        (crate_dir / "Cargo.toml").write_text('[package]\nname="agent-team-mail"\nversion="1.1.2"\n', encoding="utf-8")

    def write_test(self, repo_root: Path, relative_path: str, source: str) -> None:
        path = repo_root / relative_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(source, encoding="utf-8")

    def test_collect_violations_detects_named_categories(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_workspace(repo_root)
            self.write_test(
                repo_root,
                "crates/atm/tests/send.rs",
                """
use std::process::Command;
use std::thread;
use std::time::Duration;

struct DaemonGuard;

fn spawn_test_daemon() {
    let _ = Command::new("atm-daemon");
    thread::sleep(Duration::from_millis(25));
}

fn warm_daemon() {}

fn set_bin() {
    let _ = "ATM_DAEMON_BIN";
}
""",
            )

            violations = collect_violations(repo_root)
            categories = {violation.category for violation in violations}
            self.assertIn("spawn_test_daemon", categories)
            self.assertIn("warm_daemon", categories)
            self.assertIn("daemon_guard", categories)
            self.assertIn("atm_daemon_bin", categories)
            self.assertIn("direct_atm_daemon_command", categories)
            self.assertIn("timing_warmup_shortcut", categories)

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
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_workspace(repo_root)
            self.write_test(
                repo_root,
                "crates/atm-core/src/error.rs",
                """
pub const RECOVERY: &str = "Set ATM_DAEMON_BIN before retrying.";

#[cfg(test)]
mod tests {
    fn warm_daemon() {}
}
""",
            )

            violations = collect_violations(repo_root)
            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].category, "warm_daemon")
            self.assertEqual(violations[0].line_number, 6)

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

            violations = collect_violations(repo_root)
            categories = {violation.category for violation in violations}
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

    def test_has_unix_gating_detects_common_forms(self) -> None:
        self.assertTrue(has_unix_gating("#[cfg(unix)]\nfn x() {}"))
        self.assertTrue(has_unix_gating("if cfg!(unix) { }"))
        self.assertFalse(has_unix_gating("fn x() {}"))


if __name__ == "__main__":
    unittest.main()
