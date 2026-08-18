from __future__ import annotations

from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release-preflight.yml"


class ReleasePreflightWorkflowTests(unittest.TestCase):
    def test_release_preflight_derives_channel_checks_from_the_manifest(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("preflight-secret-plan --manifest", text)
        self.assertIn("public-registry-check-plan", text)
        self.assertIn("channel-preflight-results", text)
        self.assertNotIn("stage-install-docs", text)
        self.assertNotIn("validate_release.py", text)

    def test_release_preflight_uses_the_shared_lint_toolchain_setup(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        setup_python = text.index("- name: Set up Python\n")
        setup_lint_toolchain = text.index("- name: Set up lint toolchain")
        formatting = text.index("- id: formatting")
        self.assertLess(setup_python, setup_lint_toolchain)
        self.assertLess(setup_lint_toolchain, formatting)
        self.assertIn("uses: ./.github/actions/setup-lint-toolchain", text)

    def test_release_preflight_runs_manifest_and_package_checks(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("validate-manifest", text)
        self.assertIn("validate-publish-order", text)
        self.assertIn("cargo package -p", text)


if __name__ == "__main__":
    unittest.main()
