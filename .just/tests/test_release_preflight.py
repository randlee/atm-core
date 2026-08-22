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

    def test_release_preflight_uses_the_shared_pinned_bootstrap_before_validation(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        install_python = text.index("- name: Install Python\n")
        bootstrap = text.index("- name: Bootstrap exact repository tools")
        validate = text.index("- name: Run canonical retained release validation suite")
        self.assertLess(install_python, bootstrap)
        self.assertLess(bootstrap, validate)
        self.assertIn('python-version: "3.14.7"', text)
        self.assertIn("tool: just@1.58.0", text)
        self.assertIn("run: just bootstrap", text)


if __name__ == "__main__":
    unittest.main()
