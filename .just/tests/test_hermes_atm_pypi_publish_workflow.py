"""Temporary legacy PyPI coverage retained pending production-kit evidence."""

from __future__ import annotations

from pathlib import Path
import unittest


WORKFLOW = Path(__file__).resolve().parents[2] / ".github" / "workflows" / "hermes-atm-pypi-publish.yml"


class HermesAtmPyPiPublishWorkflowTests(unittest.TestCase):
    def test_publish_workflow_is_manual_with_explicit_target_environments(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("workflow_dispatch:", workflow)
        self.assertNotIn("pull_request:", workflow)
        self.assertNotIn("push:", workflow)
        self.assertIn("- testpypi", workflow)
        self.assertIn("- pypi", workflow)
        self.assertIn("environment: testpypi", workflow)
        self.assertIn("environment: pypi", workflow)
        self.assertIn("inputs.target == 'testpypi'", workflow)
        self.assertIn("inputs.target == 'pypi'", workflow)

    def test_publish_workflow_builds_and_verifies_manifest_declared_artifacts(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")

        self.assertNotIn("uses: ./.github/workflows/ci.yml", workflow)
        self.assertIn("python-wheel-matrix --manifest", workflow)
        self.assertIn("python-sdist-matrix --manifest", workflow)
        self.assertNotIn("hermes-atm-wheels-", workflow)
        self.assertIn("prepare_hermes_atm_publish_artifacts.py", workflow)
        self.assertIn("--manifest \"$RELEASE_ARTIFACT_MANIFEST\"", workflow)
        self.assertIn("twine check publish-dist/*", workflow)

    def test_publish_workflow_uses_target_scoped_twine_tokens(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("secrets.TEST_PYPI_API_TOKEN", workflow)
        self.assertIn("secrets.PYPI_API_TOKEN", workflow)
        self.assertNotIn("secrets.TEST_PYPI_TOKEN", workflow)
        self.assertNotIn("secrets.PYPI_TOKEN", workflow)
        self.assertIn("https://test.pypi.org/legacy/", workflow)
        self.assertIn("twine upload --non-interactive publish-dist/*", workflow)


if __name__ == "__main__":
    unittest.main()
