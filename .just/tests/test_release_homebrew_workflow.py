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

    def test_stable_release_selects_only_stable_formulas(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")
        stable_update = text[text.index("- name: Update stable Homebrew formulas") : text.index(
            "- name: Update opt-in Homebrew prerelease formula", text.index("- name: Update stable Homebrew formulas")
        )]
        self.assertIn("outputs.prerelease == 'false'", stable_update)
        self.assertIn("--formula homebrew-tap/Formula/agent-team-mail.rb", stable_update)
        self.assertIn("--formula homebrew-tap/Formula/atm.rb", stable_update)
        self.assertNotIn("atm-beta.rb", stable_update)

    def test_prerelease_release_selects_only_atm_beta(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")
        start = text.index("- name: Update opt-in Homebrew prerelease formula")
        end = text.index("- name: Validate stable Homebrew formulas", start)
        prerelease_update = text[start:end]
        self.assertIn("outputs.prerelease == 'true'", prerelease_update)
        self.assertIn("--formula homebrew-tap/Formula/atm-beta.rb", prerelease_update)
        self.assertNotIn("Formula/atm.rb", prerelease_update)
        self.assertNotIn("Formula/agent-team-mail.rb", prerelease_update)


if __name__ == "__main__":
    unittest.main()
