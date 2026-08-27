from __future__ import annotations

from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release-preflight.yml"


class ReleasePreflightWorkflowTests(unittest.TestCase):
    def test_release_preflight_validates_the_manifest_and_publish_order(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn('RELEASE_ARTIFACT_MANIFEST: release/publish-artifacts.toml', text)
        self.assertIn("release_artifacts.py validate-manifest", text)
        self.assertIn("release_artifacts.py validate-publish-order", text)

    def test_release_preflight_reports_channel_results_from_the_manifest(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("preflight-secret-plan", text)
        self.assertIn("channel-preflight-results", text)
        self.assertIn("Deny release after complete preflight summary", text)

    def test_release_preflight_requires_release_candidate_provenance(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("Verify release-candidate provenance", text)
        self.assertIn("release_gate.sh readiness HEAD", text)
        self.assertIn('"release-candidate-${{ steps.meta.outputs.release_tag }}"', text)


if __name__ == "__main__":
    unittest.main()
