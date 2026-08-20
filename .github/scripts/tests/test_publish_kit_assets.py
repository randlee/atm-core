"""Package-native assertions for the vendored Claude publishing assets."""

from __future__ import annotations

import unittest
from pathlib import Path


PACKAGE_ROOT = next(path for path in Path(__file__).resolve().parents if (path / "install.py").is_file())
AGENTS = PACKAGE_ROOT / ".claude" / "agents"
PUBLISHING = PACKAGE_ROOT / ".claude" / "skills" / "publishing"


class PublishingAssetTests(unittest.TestCase):
    def test_bespoke_hermes_pypi_pipeline_is_retired_in_favor_of_the_canonical_channel(self) -> None:
        root = PACKAGE_ROOT
        self.assertFalse((root / ".github" / "workflows" / "hermes-atm-pypi-publish.yml").exists())
        self.assertFalse((root / "scripts" / "prepare_hermes_atm_publish_artifacts.py").exists())

    def test_channel_agents_are_background_workers_at_the_current_package_version(self) -> None:
        for name in (
            "crates-io-publisher",
            "github-release-publisher",
            "pypi-publisher",
            "homebrew-publisher",
            "scoop-publisher",
            "winget-publisher",
        ):
            text = (AGENTS / f"{name}.md").read_text(encoding="utf-8")
            self.assertIn("version: 0.1.0", text)
            self.assertIn("spawn_policy: background_agent_required", text)
            self.assertIn("publisher-channel-protocol.md", text)

    def test_publisher_is_the_only_named_orchestrator(self) -> None:
        text = (AGENTS / "publisher.md").read_text(encoding="utf-8")
        self.assertIn("spawn_policy: named_teammate_required", text)
        self.assertIn("role-specific background workers", text)
        self.assertIn("Never ask whether a token exists", text)

    def test_task_templates_require_a_recipient(self) -> None:
        for name in ("preflight.xml.j2", "publish.xml.j2"):
            text = (PUBLISHING / name).read_text(encoding="utf-8")
            self.assertIn("version: 0.1.0", text)
            self.assertIn("- recipient", text)
            self.assertIn("<recipient>{{ recipient }}</recipient>", text)

    def test_shared_publishing_documents_use_the_packaged_release_artifacts_script(self) -> None:
        """Prompt commands must match the path the installer copies to consumers."""
        canonical_script = PACKAGE_ROOT / ".github" / "scripts" / "release_artifacts.py"
        self.assertTrue(canonical_script.is_file())
        canonical_gate = PACKAGE_ROOT / ".github" / "scripts" / "release_gate.sh"
        self.assertTrue(canonical_gate.is_file())

        shared_documents = (
            AGENTS / "publisher.md",
            PUBLISHING / "SKILL.md",
            PUBLISHING / "ref" / "channel-contracts.md",
            PACKAGE_ROOT / ".cursor" / "agents" / "publisher.md",
            PACKAGE_ROOT / ".cursor" / "commands" / "cursor-publish.md",
            PACKAGE_ROOT / ".cursor" / "skills" / "cursor-publish" / "SKILL.md",
        )
        for document in shared_documents:
            text = document.read_text(encoding="utf-8")
            self.assertIn(".github/scripts/release_artifacts.py", text, document)
            self.assertNotRegex(
                text,
                r"(?<!\.github/)scripts/release_artifacts\.py",
                document,
            )
            self.assertNotRegex(text, r"(?<!\.github/)scripts/release_gate\.sh", document)


if __name__ == "__main__":
    unittest.main()
