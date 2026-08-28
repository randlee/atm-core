"""ATM-owned manifest-data assertions for the kit-rendered Homebrew workflow."""

from __future__ import annotations

from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "homebrew-publish.yml"


class ReleaseHomebrewWorkflowTests(unittest.TestCase):
    def test_homebrew_job_uses_the_manifest_and_published_release(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("Read Homebrew configuration from release manifest", text)
        self.assertIn("channel-config --manifest release/publish-artifacts.toml --channel homebrew", text)
        self.assertIn("./.github/actions/verify-published-release", text)

    def test_homebrew_job_uses_manifest_declared_formula_values(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn('for index, formula in enumerate(channel["formulas"]):', text)
        self.assertIn('output = Path("homebrew-tap") / formula["path"]', text)
        self.assertIn('formula["template"]', text)

    def test_homebrew_job_requires_credential_and_published_renderer(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("HOMEBREW_TAP_TOKEN is required", text)
        self.assertIn("./.github/actions/setup-renderer", text)
        self.assertIn("Render manifest-selected formulas with the published renderer", text)


if __name__ == "__main__":
    unittest.main()
