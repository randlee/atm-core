from __future__ import annotations

from pathlib import Path
import shutil
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "scripts"
sys.path.insert(0, str(SCRIPTS))

from lint_daemon_singleton import collect_violations


class DaemonSingletonLintTests(unittest.TestCase):
    def fixture(self) -> Path:
        root = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: shutil.rmtree(root, ignore_errors=True))
        (root / "crates" / "fixture" / "src").mkdir(parents=True)
        (root / "scripts").mkdir()
        (root / "scripts" / "lint_daemon_singleton.toml").write_text("[daemon_singleton]\n", encoding="utf-8")
        return root

    def write(self, root: Path, relative: str, source: str) -> None:
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(source, encoding="utf-8")

    def categories(self, root: Path) -> set[str]:
        return {finding.category for finding in collect_violations(root)}

    def test_reintroduced_runtime_home_override_fails(self) -> None:
        root = self.fixture()
        self.write(root, "crates/fixture/src/home.rs", 'let _ = std::env::var_os("ATM_TEST_RUNTIME_HOME");\n')
        self.assertIn("runtime-scope-override", self.categories(root))

    def test_ungated_popen_daemon_fixture_fails(self) -> None:
        root = self.fixture()
        self.write(root, "scripts/test_fixture.py", 'subprocess.Popen(["atm-daemon"])\n')
        self.assertIn("ungated-daemon-launch", self.categories(root))

    def test_clean_host_gated_launcher_is_accepted(self) -> None:
        root = self.fixture()
        self.write(
            root,
            "scripts/fixture.py",
            'def ambient_daemon_pids(): return []\nsubprocess.Popen(["atm-daemon"])\n',
        )
        self.assertNotIn("ungated-daemon-launch", self.categories(root))

    def test_new_test_environment_variable_fails(self) -> None:
        root = self.fixture()
        self.write(root, "crates/fixture/src/lib.rs", 'let _ = "ATM_TEST_NEW_SINGLETON_OVERRIDE";\n')
        self.assertIn("new-test-env", self.categories(root))

    def test_direct_peer_port_flag_fails(self) -> None:
        root = self.fixture()
        self.write(root, "scripts/fixture.py", 'command = ["atm-daemon", "--direct-peer-port", "43102"]\n')
        self.assertIn("endpoint-override", self.categories(root))

    def test_allowlist_entry_fails(self) -> None:
        root = self.fixture()
        self.write(root, "scripts/lint_daemon_singleton.toml", "[daemon_singleton]\n[[daemon_singleton.allow]]\n")
        self.assertIn("allowlist", self.categories(root))


if __name__ == "__main__":
    unittest.main()
