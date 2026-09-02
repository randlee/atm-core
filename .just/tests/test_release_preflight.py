"""Temporary consumer-contract coverage retained pending the first kit release."""

from __future__ import annotations

from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release-preflight.yml"
CI_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "ci.yml"


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

    def test_ci_bootstraps_and_verifies_every_ecosystem_tool(self) -> None:
        # AQ6: the kit-owned release-preflight workflow (ADR-050) no longer runs
        # `just bootstrap`; the consumer-owned CI workflow carries the pinned
        # ecosystem bootstrap and the sc-compose / wyvern presence checks.
        workflow = CI_WORKFLOW.read_text(encoding="utf-8")
        bootstrap_manifest = (REPO_ROOT / "tools" / "bootstrap.toml").read_text(encoding="utf-8")

        self.assertIn("run: just bootstrap", workflow)
        self.assertIn("sc-compose --version", workflow)
        self.assertIn("wyvern --version", workflow)
        self.assertIn('[sc-compose]\nversion = "1.6.1"', bootstrap_manifest)
        self.assertIn('[wyvern]\nversion = "0.5.0"', bootstrap_manifest)


if __name__ == "__main__":
    unittest.main()
