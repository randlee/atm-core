"""Package-native assertions for the vendored Claude publishing assets."""

from __future__ import annotations

import unittest
from pathlib import Path


PACKAGE_ROOT = Path(__file__).resolve().parents[3]
AGENTS = PACKAGE_ROOT / ".claude" / "agents"
PUBLISHING = PACKAGE_ROOT / ".claude" / "skills" / "publishing"


class PublishingAssetTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
