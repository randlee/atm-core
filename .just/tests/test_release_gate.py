"""Temporary legacy gate coverage retained until kit release evidence exists."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "release_gate.sh"


def run(cmd: list[str], *, cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=cwd,
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )


@unittest.skipIf(sys.platform == "win32", "release_gate.sh is exercised on Unix-like runners")
class ReleaseGateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tempdir = tempfile.TemporaryDirectory()
        self.root = Path(self.tempdir.name)
        self.origin = self.root / "origin.git"
        self.seed = self.root / "seed"
        self.clone = self.root / "clone"

        run(["git", "init", "--bare", str(self.origin)], cwd=self.root)
        run(["git", "clone", str(self.origin), str(self.seed)], cwd=self.root)
        run(["git", "config", "user.name", "Test User"], cwd=self.seed)
        run(["git", "config", "user.email", "test@example.com"], cwd=self.seed)

        (self.seed / "README.md").write_text("seed\n", encoding="utf-8")
        run(["git", "add", "README.md"], cwd=self.seed)
        run(["git", "commit", "-m", "seed"], cwd=self.seed)
        run(["git", "branch", "-M", "main"], cwd=self.seed)
        run(["git", "push", "-u", "origin", "main"], cwd=self.seed)
        run(["git", "checkout", "-b", "develop"], cwd=self.seed)
        run(["git", "push", "-u", "origin", "develop"], cwd=self.seed)
        run(["git", "checkout", "-b", "release/v1.2.3", "main"], cwd=self.seed)
        run(["git", "push", "-u", "origin", "release/v1.2.3"], cwd=self.seed)

        run(["git", "clone", "--branch", "release/v1.2.3", str(self.origin), str(self.clone)], cwd=self.root)

    def tearDown(self) -> None:
        self.tempdir.cleanup()

    def run_gate(self, trigger_ref: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(SCRIPT), "origin/main", "origin/develop", trigger_ref],
            cwd=self.clone,
            check=False,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )

    def test_accepts_release_branch_trigger(self) -> None:
        completed = self.run_gate("refs/heads/release/v1.2.3")

        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("PASS - release gate checks satisfied", completed.stdout)
        self.assertIn("trigger_ref=refs/heads/release/v1.2.3", completed.stdout)

    def test_rejects_non_release_branch_trigger(self) -> None:
        completed = self.run_gate("refs/heads/develop")

        self.assertNotEqual(completed.returncode, 0)
        self.assertIn(
            "triggering ref must match refs/heads/release/vX.Y.Z",
            completed.stderr,
        )


if __name__ == "__main__":
    unittest.main()
