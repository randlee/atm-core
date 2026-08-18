from __future__ import annotations

from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release-preflight.yml"


class ReleasePreflightWorkflowTests(unittest.TestCase):
    def test_release_preflight_reads_the_manifest_derived_credential_plan(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("preflight-secret-plan --manifest", text)
        self.assertIn("channel-preflight-results", text)

    def test_release_preflight_uses_contract_derived_registry_checks(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("public-registry-check-plan", text)

    def test_release_preflight_has_environment_metadata_and_crates_io_access(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("deployments: read", text)
        self.assertEqual(
            text.count('User-Agent: atm-core-release-preflight (https://github.com/randlee/atm-core)'),
            2,
        )

    def test_release_preflight_installs_canonical_python_tools(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        install_python = text.index("- name: Install canonical Python release tools")
        validate = text.index("- id: manifest")
        self.assertLess(install_python, validate)
        self.assertIn("uses: ./.github/actions/setup-python-release-tools", text)
        self.assertNotIn("python -m pip install codespell", text)

    def test_release_preflight_installs_sc_compose_before_workspace_tests(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        install_sc_compose = text.index("uses: ./.github/actions/setup-sc-compose-cli")
        workspace_tests = text.index("- id: workspace_tests")
        self.assertLess(install_sc_compose, workspace_tests)


if __name__ == "__main__":
    unittest.main()
