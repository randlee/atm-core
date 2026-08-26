from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from check_fixed_sleep_hygiene import collect_fixed_sleep_violations
from check_fixed_sleep_hygiene import load_allowed_paths


LINT_CONFIG = """\
[fixed_sleep]
allowed_paths = ["crates/atm-daemon/src/reconcile_runtime.rs"]
"""

ROOT_MANIFEST = """\
[workspace]
members = ["crates/atm-daemon"]
resolver = "2"
"""


class CheckFixedSleepHygieneTests(unittest.TestCase):
    def write_repo(self, repo_root: Path) -> None:
        (repo_root / ".just").mkdir()
        (repo_root / ".just/lint-config.toml").write_text(LINT_CONFIG, encoding="utf-8")
        (repo_root / "Cargo.toml").write_text(ROOT_MANIFEST, encoding="utf-8")
        (repo_root / "crates/atm-daemon/src").mkdir(parents=True)
        (repo_root / "crates/atm-daemon/Cargo.toml").write_text(
            """\
[package]
name = "atm-daemon"
version = "0.1.0"

[lib]
name = "atm_daemon"
""",
            encoding="utf-8",
        )

    def test_load_allowed_paths_reads_config(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)

            self.assertEqual(
                load_allowed_paths(repo_root),
                ("crates/atm-daemon/src/reconcile_runtime.rs",),
            )

    def test_flags_fixed_sleep_inside_cfg_test_scope(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-daemon/src/lib.rs").write_text(
                """\
#[cfg(test)]
mod tests {
    #[test]
    fn waits() {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
""",
                encoding="utf-8",
            )

            violations = collect_fixed_sleep_violations(
                repo_root,
                allowed_paths=load_allowed_paths(repo_root),
            )

            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].line_number, 5)

    def test_flags_tokio_sleep_inside_cfg_test_scope(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-daemon/src/lib.rs").write_text(
                """\
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn waits() {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}
""",
                encoding="utf-8",
            )
            violations = collect_fixed_sleep_violations(
                repo_root,
                allowed_paths=load_allowed_paths(repo_root),
            )
            self.assertEqual(len(violations), 1)
            self.assertEqual(violations[0].line_number, 5)

    def test_ignores_production_sleep(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-daemon/src/lib.rs").write_text(
                """\
pub fn backoff() {
    std::thread::sleep(std::time::Duration::from_millis(5));
}
""",
                encoding="utf-8",
            )

            violations = collect_fixed_sleep_violations(
                repo_root,
                allowed_paths=load_allowed_paths(repo_root),
            )

            self.assertEqual(violations, [])

    def test_allows_explicit_allowlisted_file(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm-daemon/src/reconcile_runtime.rs").write_text(
                """\
#[cfg(test)]
mod tests {
    fn waits() {
        thread::sleep(std::time::Duration::from_millis(5));
    }
}
""",
                encoding="utf-8",
            )

            violations = collect_fixed_sleep_violations(
                repo_root,
                allowed_paths=load_allowed_paths(repo_root),
            )

            self.assertEqual(violations, [])

if __name__ == "__main__":
    unittest.main()
