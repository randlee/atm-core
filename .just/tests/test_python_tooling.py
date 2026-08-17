from __future__ import annotations

from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
REQUIREMENTS = REPO_ROOT / ".github" / "python-tooling-requirements.txt"
ACTION = REPO_ROOT / ".github" / "actions" / "setup-atm-python-tools" / "action.yml"
WORKFLOWS = (
    REPO_ROOT / ".github" / "workflows" / "ci.yml",
    REPO_ROOT / ".github" / "workflows" / "release-preflight.yml",
    REPO_ROOT / ".github" / "workflows" / "hermes-atm-pypi-publish.yml",
)


class PythonToolingTests(unittest.TestCase):
    def test_requirements_pin_every_shared_python_tool(self) -> None:
        requirements = REQUIREMENTS.read_text(encoding="utf-8")

        self.assertIn("codespell==2.4.3", requirements)
        self.assertIn("maturin==1.11.5", requirements)
        self.assertIn("pydantic==2.12.5", requirements)
        self.assertIn("twine==6.1.0", requirements)
        for line in requirements.splitlines():
            if line and not line.startswith("#"):
                self.assertIn("==", line, msg=f"shared tool must be exactly pinned: {line}")

    def test_shared_installer_uses_the_single_requirements_manifest(self) -> None:
        action = ACTION.read_text(encoding="utf-8")

        self.assertIn("actions/setup-python@v5", action)
        self.assertIn("python -m pip install --requirement .github/python-tooling-requirements.txt", action)
        self.assertIn("python -m pip check", action)

    def test_relevant_workflows_use_shared_installer_without_inline_shared_tools(self) -> None:
        inline_installs = (
            "pip install codespell",
            "pip install maturin",
            "pip install twine",
            "pip install 'pydantic",
            'pip install "pydantic',
        )
        for workflow_path in WORKFLOWS:
            workflow = workflow_path.read_text(encoding="utf-8")
            self.assertIn(
                "uses: ./.github/actions/setup-atm-python-tools",
                workflow,
                msg=f"{workflow_path.name} must use the canonical Python installer",
            )
            for inline_install in inline_installs:
                self.assertNotIn(inline_install, workflow, msg=f"{workflow_path.name}: {inline_install}")


if __name__ == "__main__":
    unittest.main()
