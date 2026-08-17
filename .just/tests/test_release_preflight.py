from __future__ import annotations

from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release-preflight.yml"


class ReleasePreflightWorkflowTests(unittest.TestCase):
    def test_release_preflight_stages_installed_docs_before_validation(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("python3 scripts/release_artifacts.py stage-install-docs", text)
        self.assertIn("--output-root \"${STAGED_INSTALL_ROOT}\"", text)

    def test_release_preflight_passes_staged_install_root_to_validator(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("python3 scripts/validate_release.py all \\", text)
        self.assertIn("--staged-install-root \"${STAGED_INSTALL_ROOT}\"", text)

    def test_release_preflight_installs_canonical_python_tools_before_validation(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        install_python = text.index("- name: Install canonical Python tools")
        validate = text.index("- name: Run canonical retained release validation suite")
        self.assertLess(install_python, validate)
        self.assertIn("uses: ./.github/actions/setup-atm-python-tools", text)
        self.assertNotIn("python -m pip install codespell", text)


if __name__ == "__main__":
    unittest.main()
