from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest


ROOT_DIR = Path(__file__).resolve().parents[2]
JUST_DIR = ROOT_DIR / ".just"
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_same_host_portability import collect_forbidden_todo_markers
from lint_same_host_portability import collect_non_unix_same_host_stubs


class SameHostPortabilityLintTests(unittest.TestCase):
    BOOTSTRAP_PATH = "crates/atm-daemon-bootstrap/src/lib.rs"

    def write_bootstrap(self, repo_root: Path, source: str) -> None:
        path = repo_root / self.BOOTSTRAP_PATH
        path.parent.mkdir(parents=True)
        path.write_text(source, encoding="utf-8")

    def test_rejects_non_unix_daemon_unavailable_stub_in_live_bootstrap(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            repo_root = Path(temporary)
            self.write_bootstrap(
                repo_root,
                """
#[cfg(not(unix))]
fn local_transport() -> Result<(), AtmError> {
    Err(AtmError::daemon_unavailable("unsupported"))
}
""",
            )
            findings = collect_non_unix_same_host_stubs(repo_root)
        self.assertEqual(len(findings), 1)
        self.assertEqual(findings[0].path, self.BOOTSTRAP_PATH)

    def test_rejects_retired_portability_todo_in_live_bootstrap(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            repo_root = Path(temporary)
            self.write_bootstrap(repo_root, "// TODO(S.2/ADR-007): restore Windows later\n")
            findings = collect_forbidden_todo_markers(repo_root)
        self.assertEqual(len(findings), 1)
        self.assertEqual(findings[0].path, self.BOOTSTRAP_PATH)


if __name__ == "__main__":
    unittest.main()
