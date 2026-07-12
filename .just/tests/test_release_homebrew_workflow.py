from __future__ import annotations

from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release.yml"


class ReleaseHomebrewWorkflowTests(unittest.TestCase):
    def test_update_homebrew_job_uses_scripted_formula_update_and_validation(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("update-homebrew:", text)
        self.assertIn("python3 scripts/release_artifacts.py update-homebrew-formulas \\", text)
        self.assertIn("python3 scripts/release_artifacts.py validate-homebrew-formulas \\", text)
        self.assertIn("--release-dir release \\", text)
        self.assertIn("--formula homebrew-tap/Formula/agent-team-mail.rb \\", text)
        self.assertIn("--formula homebrew-tap/Formula/atm.rb", text)


if __name__ == "__main__":
    unittest.main()
