from __future__ import annotations

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "release_gate.sh"


def run(
    cmd: list[str], *, cwd: Path, env: dict[str, str]
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=cwd,
        env=env,
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )


@unittest.skipIf(sys.platform == "win32", "release_gate.sh is exercised on Unix-like runners")
class ReleaseGateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tempdir = tempfile.TemporaryDirectory()
        self.addCleanup(self.tempdir.cleanup)
        self.root = Path(self.tempdir.name)
        self.origin = self.root / "origin.git"
        self.seed = self.root / "seed"
        self.clone = self.root / "clone"
        home = self.root / "home"
        home.mkdir()
        git_config = self.root / "gitconfig"
        git_config.write_text(
            "[commit]\n"
            "\tgpgsign = false\n"
            "[tag]\n"
            "\tgpgSign = false\n"
            "[core]\n"
            "\thooksPath = /dev/null\n",
            encoding="utf-8",
        )
        self.git_env = {
            **os.environ,
            "HOME": str(home),
            "GIT_CONFIG_GLOBAL": str(git_config),
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_TERMINAL_PROMPT": "0",
        }

        run(["git", "init", "--bare", str(self.origin)], cwd=self.root, env=self.git_env)
        run(["git", "clone", str(self.origin), str(self.seed)], cwd=self.root, env=self.git_env)
        run(["git", "config", "user.name", "Test User"], cwd=self.seed, env=self.git_env)
        run(["git", "config", "user.email", "test@example.com"], cwd=self.seed, env=self.git_env)

        (self.seed / "README.md").write_text("seed\n", encoding="utf-8")
        artifacts = self.seed / "scripts" / "release_artifacts.py"
        artifacts.parent.mkdir(parents=True)
        artifacts.write_text("raise SystemExit(0)\n", encoding="utf-8")
        run(["git", "add", "README.md", "scripts/release_artifacts.py"], cwd=self.seed, env=self.git_env)
        run(["git", "commit", "-m", "seed"], cwd=self.seed, env=self.git_env)
        run(["git", "branch", "-M", "main"], cwd=self.seed, env=self.git_env)
        run(["git", "push", "-u", "origin", "main"], cwd=self.seed, env=self.git_env)
        run(["git", "checkout", "-b", "develop"], cwd=self.seed, env=self.git_env)
        run(["git", "push", "-u", "origin", "develop"], cwd=self.seed, env=self.git_env)
        run(["git", "checkout", "-b", "release/v1.2.3", "main"], cwd=self.seed, env=self.git_env)
        run(["git", "push", "-u", "origin", "release/v1.2.3"], cwd=self.seed, env=self.git_env)

        run(
            ["git", "clone", "--branch", "release/v1.2.3", str(self.origin), str(self.clone)],
            cwd=self.root,
            env=self.git_env,
        )

    def run_gate(self, version: str = "1.2.3") -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(SCRIPT), "origin/main", "origin/develop", version, "release/publish-artifacts.toml", f"v{version}", "production"],
            cwd=self.clone,
            env=self.git_env,
            check=False,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )

    def test_accepts_converged_main_and_develop_for_requested_version(self) -> None:
        completed = self.run_gate()

        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("PASS - release gate checks satisfied", completed.stdout)
        self.assertIn("version=1.2.3", completed.stdout)

    def test_rejects_develop_not_merged_to_main(self) -> None:
        run(["git", "checkout", "develop"], cwd=self.seed, env=self.git_env)
        (self.seed / "develop-only.txt").write_text("not released\n", encoding="utf-8")
        run(["git", "add", "develop-only.txt"], cwd=self.seed, env=self.git_env)
        run(["git", "commit", "-m", "develop only"], cwd=self.seed, env=self.git_env)
        run(["git", "push"], cwd=self.seed, env=self.git_env)

        completed = self.run_gate()

        self.assertNotEqual(completed.returncode, 0)
        self.assertIn(
            "origin/develop has commits not in origin/main",
            completed.stderr,
        )

    def test_rejects_existing_tag_that_points_away_from_main(self) -> None:
        run(["git", "checkout", "-b", "unrelated", "main"], cwd=self.seed, env=self.git_env)
        (self.seed / "unrelated.txt").write_text("not the release commit\n", encoding="utf-8")
        run(["git", "add", "unrelated.txt"], cwd=self.seed, env=self.git_env)
        run(["git", "commit", "-m", "unrelated tag target"], cwd=self.seed, env=self.git_env)
        run(["git", "tag", "-f", "v1.2.3"], cwd=self.seed, env=self.git_env)
        run(["git", "push", "--force", "origin", "refs/tags/v1.2.3"], cwd=self.seed, env=self.git_env)

        completed = self.run_gate()

        self.assertNotEqual(completed.returncode, 0)
        self.assertIn("tag v1.2.3 exists but points to", completed.stderr)
        self.assertIn("never retag", completed.stderr)


if __name__ == "__main__":
    unittest.main()
