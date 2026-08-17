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
        self.assertNotIn("randlee/atm-core", text)

    def test_release_preflight_installs_canonical_python_tools(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        install_python = text.index("- name: Install canonical Python release tools")
        validate = text.index("- id: manifest")
        self.assertLess(install_python, validate)
        self.assertIn("uses: ./.github/actions/setup-python-release-tools", text)
        self.assertNotIn("python -m pip install codespell", text)


if __name__ == "__main__":
    unittest.main()
