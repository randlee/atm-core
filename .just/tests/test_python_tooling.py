from __future__ import annotations

from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
REQUIREMENTS = REPO_ROOT / ".github" / "python-tooling-requirements.txt"
ACTION = REPO_ROOT / ".github" / "actions" / "setup-python-release-tools" / "action.yml"
SC_COMPOSE_ACTION = REPO_ROOT / ".github" / "actions" / "setup-sc-compose-cli" / "action.yml"
SC_COMPOSE_DEPENDENCY = REPO_ROOT / ".claude" / "lib" / "sc_compose_dependency.py"
WORKFLOWS = (
    REPO_ROOT / ".github" / "workflows" / "ci.yml",
    REPO_ROOT / ".github" / "workflows" / "release-preflight.yml",
    REPO_ROOT / ".github" / "workflows" / "hermes-atm-pypi-publish.yml",
)
RUST_WORKFLOWS = (
    REPO_ROOT / ".github" / "workflows" / "ci.yml",
    REPO_ROOT / ".github" / "workflows" / "release.yml",
    REPO_ROOT / ".github" / "workflows" / "release-preflight.yml",
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
                "uses: ./.github/actions/setup-python-release-tools",
                workflow,
                msg=f"{workflow_path.name} must use the canonical Python installer",
            )
            for inline_install in inline_installs:
                self.assertNotIn(inline_install, workflow, msg=f"{workflow_path.name}: {inline_install}")

    def test_sc_compose_cli_action_uses_the_repository_pinned_revision(self) -> None:
        dependency = SC_COMPOSE_DEPENDENCY.read_text(encoding="utf-8")
        action = SC_COMPOSE_ACTION.read_text(encoding="utf-8")
        revision = next(
            line.split('"')[1]
            for line in dependency.splitlines()
            if line.startswith("SC_COMPOSE_SOURCE_REV")
        )

        self.assertIn(f"--rev {revision}", action)
        self.assertIn("--locked --bin sc-compose", action)

    def test_rust_toolchain_action_is_consistent_across_ci_and_release_workflows(self) -> None:
        for workflow_path in RUST_WORKFLOWS:
            workflow = workflow_path.read_text(encoding="utf-8")
            self.assertNotIn("dtolnay/rust-toolchain@master", workflow)
            self.assertIn("dtolnay/rust-toolchain@stable", workflow)


if __name__ == "__main__":
    unittest.main()
