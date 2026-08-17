from __future__ import annotations

from pathlib import Path
import unittest


WORKFLOW = Path(__file__).resolve().parents[2] / ".github" / "workflows" / "hermes-atm-pypi-publish.yml"
CI_WORKFLOW = Path(__file__).resolve().parents[2] / ".github" / "workflows" / "ci.yml"


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

    def test_publish_workflow_reuses_ci_builds_and_validates_artifact_cardinality(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("uses: ./.github/workflows/ci.yml", workflow)
        for artifact in (
            "hermes-atm-wheels-linux-x86_64",
            "hermes-atm-wheels-linux-musl-x86_64",
            "hermes-atm-wheels-linux-aarch64",
            "hermes-atm-wheels-macos-aarch64",
            "hermes-atm-wheels-windows-x86_64",
            "atm-graft-sdist",
        ):
            self.assertIn(artifact, workflow)
        self.assertIn("prepare_hermes_atm_publish_artifacts.py", workflow)
        self.assertIn("twine check publish-dist/*", workflow)
        self.assertIn("workflow_call:", CI_WORKFLOW.read_text(encoding="utf-8"))

    def test_publish_workflow_uses_target_scoped_twine_tokens(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("secrets.TEST_PYPI_API_TOKEN", workflow)
        self.assertIn("secrets.PYPI_API_TOKEN", workflow)
        self.assertNotIn("secrets.TEST_PYPI_TOKEN", workflow)
        self.assertNotIn("secrets.PYPI_TOKEN", workflow)
        self.assertIn("https://test.pypi.org/legacy/", workflow)
        self.assertIn("twine upload --non-interactive publish-dist/*", workflow)

    def test_publish_workflow_uses_canonical_python_tool_installer(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")

        self.assertEqual(workflow.count("uses: ./.github/actions/setup-atm-python-tools"), 2)
        self.assertGreaterEqual(workflow.count("uses: actions/checkout@v4"), 3)
        self.assertNotIn("python -m pip install twine", workflow)


if __name__ == "__main__":
    unittest.main()
